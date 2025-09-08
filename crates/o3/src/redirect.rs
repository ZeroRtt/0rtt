use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    sync::Arc,
    task::Poll,
    time::Instant,
};

use mio::{Interest, net::UdpSocket};
use quico::{Group, acceptor::Acceptor, quiche::RecvInfo};

use crate::{
    buf::QuicBuf,
    connector::TcpConnector,
    errors::{Error, Result},
    mapping::Mapping,
    poll::WouldBlock,
    port::{BufPort, QuicStreamPort, TcpStreamPort},
    token::Token,
    udp::QuicSocket,
};

/// Redirect all inbound quic traffic to target server via tcp tunnels.
pub struct Redirect {
    /// port receving buffer size.
    port_buffer_size: usize,
    /// mio token generator.
    mio_token_next: usize,
    /// Quic connection group.
    group: Arc<Group>,
    /// Underlying udp sockets for quic traffics.
    quic_sockets: Vec<QuicSocket>,
    /// Map quic socket address to token.
    quic_socket_addrs: HashMap<SocketAddr, usize>,
    /// Inbound QUIC streams that are not yet paired
    pairing_quic_streams: VecDeque<(quico::Token, u64)>,
    /// Acceptor for inbound quic connections.
    acceptor: Acceptor,
    /// Polls for readiness events on all registered values.
    poll: mio::Poll,
    /// A connector for outbound tcp stream.
    tcp_connector: TcpConnector,
    /// router for quic traffic.
    mapping: Mapping,
}

impl Redirect {
    /// Create a new instance.
    pub fn new(
        laddrs: Vec<SocketAddr>,
        target: SocketAddr,
        acceptor: Acceptor,
        port_buffer_size: usize,
    ) -> Result<Self> {
        let poll = mio::Poll::new()?;

        let mut quic_sockets = Vec::new();
        let mut quic_socket_addrs = HashMap::new();

        let mut mio_token_next = 0;

        for laddr in laddrs {
            let token = mio::Token(mio_token_next);
            mio_token_next += 1;

            let mut udp_socket = UdpSocket::bind(laddr)?;

            poll.registry().register(
                &mut udp_socket,
                token,
                Interest::READABLE | Interest::WRITABLE,
            )?;

            let laddr = udp_socket.local_addr()?;

            quic_sockets.push(QuicSocket::new(udp_socket, 1024)?);
            quic_socket_addrs.insert(laddr, token.0);
        }

        Ok(Self {
            port_buffer_size,
            mio_token_next,
            group: Arc::new(Group::new()),
            poll,
            acceptor,
            pairing_quic_streams: Default::default(),
            quic_sockets,
            quic_socket_addrs,
            mapping: Default::default(),
            tcp_connector: TcpConnector::new(vec![target]),
        })
    }

    /// Run agent server.
    pub fn run(mut self) -> Result<()> {
        loop {
            let next_release_time = self.quico_poll_once()?;

            self.mio_poll_once(next_release_time)?;
        }
    }
}

impl Redirect {
    fn quico_poll_once(&mut self) -> Result<Option<Instant>> {
        loop {
            let mut events = vec![];

            let next_release_time = self.group.non_blocking_poll(&mut events);

            if events.is_empty() {
                return Ok(next_release_time);
            }

            for event in events {
                match event.kind {
                    quico::EventKind::Send => self.on_quic_send(event.token)?,
                    quico::EventKind::Recv => unreachable!("Single thread mode."),
                    quico::EventKind::Connected => unreachable!("Redirect"),
                    quico::EventKind::Accept => {
                        log::info!("Accept new quic connection, {:?}", event.token);
                    }
                    quico::EventKind::Closed => {
                        log::info!("Quic connection closed, {:?}", event.token);
                    }
                    quico::EventKind::StreamOpen => unreachable!("Redirect"),
                    quico::EventKind::StreamSend => {
                        self.on_quic_stream_send(event.token, event.stream_id)?
                    }
                    quico::EventKind::StreamRecv => {
                        self.on_quic_stream_recv(event.token, event.stream_id)?
                    }
                    quico::EventKind::StreamAccept => {
                        self.on_quic_stream_accept(event.token, event.stream_id)?
                    }
                }
            }
        }
    }

    fn mio_poll_once(&mut self, next_release_time: Option<Instant>) -> Result<()> {
        let mut events = mio::Events::with_capacity(1024);

        let timeout = if let Some(next_release_time) = next_release_time {
            next_release_time.checked_duration_since(Instant::now())
        } else {
            None
        };

        log::trace!("mio readiness, timeout={:?}", timeout);

        self.poll.poll(&mut events, timeout)?;

        log::trace!("mio readiness, raised={}", events.iter().count());

        for event in events.iter() {
            let token = event.token();

            // for udp sockets.
            if token.0 < self.quic_sockets.len() {
                if event.is_readable() {
                    self.on_udp_recv(token)?;
                }

                if event.is_writable() {
                    self.on_udp_send(token)?;
                }
            } else {
                if self.tcp_connector.is_connecting(&token) {
                    self.make_port_mapping(token)?;
                }

                if !self.mapping.contains_port(token) {
                    // The only reason for this is that the mapping has been removed.
                    //
                    // TcpStream will not be registered to `Poll` until the pairing is successful
                    continue;
                }

                if event.is_readable() {
                    self.on_transfer_from(token)?;
                }

                if event.is_writable() {
                    self.on_transfer_to(token)?;
                }
            }
        }

        Ok(())
    }

    fn next_mio_token(&mut self) -> mio::Token {
        loop {
            let token = mio::Token(self.mio_token_next);

            let overflow;
            (self.mio_token_next, overflow) = self.mio_token_next.overflowing_add(1);

            if overflow {
                self.mio_token_next = self.quic_sockets.len();
            }

            if self.mapping.contains_port(token) {
                continue;
            }

            return token;
        }
    }

    fn make_port_mapping(&mut self, token: mio::Token) -> Result<()> {
        if self.pairing_quic_streams.is_empty() {
            return Ok(());
        }

        let Poll::Ready(Ok((tcp_stream, _))) = self.tcp_connector.is_ready(&token) else {
            // skip all errors.
            return Ok(());
        };

        let (conn_id, stream_id) = self.pairing_quic_streams.pop_front().unwrap();

        self.mapping.register(
            BufPort::new(
                QuicStreamPort::new(self.group.clone(), conn_id, stream_id),
                self.port_buffer_size,
            ),
            BufPort::new(TcpStreamPort::new(tcp_stream, token), self.port_buffer_size),
        );

        Ok(())
    }
}

impl Redirect {
    fn on_udp_recv(&mut self, token: mio::Token) -> Result<()> {
        let quic_socket = self.quic_sockets.get_mut(token.0).expect("Quic socket");

        loop {
            let mut buf = QuicBuf::new();

            let Poll::Ready(from) = quic_socket.recv_from(&mut buf).would_block()? else {
                return Ok(());
            };

            let read_size = buf.readable();

            // skip all returned errors.
            match self.group.accept_recv(
                &mut self.acceptor,
                buf.writable_buf(),
                read_size,
                RecvInfo {
                    from,
                    to: quic_socket.local_addr(),
                },
            ) {
                Ok((send_size, send_info)) => {
                    if send_size == 0 {
                        continue;
                    }

                    buf.writable_consume(send_size);
                    match quic_socket.send_to(buf, send_info.to) {
                        Ok(_) => {}
                        Err(Error::IsFull(_)) => {
                            log::warn!("udp send queue is full, socket={}", token.0);
                        }
                        Err(err) => return Err(err),
                    }
                }
                Err(_) => {}
            }
        }
    }

    fn on_udp_send(&mut self, token: mio::Token) -> Result<()> {
        let quic_socket = self.quic_sockets.get_mut(token.0).expect("Quic socket");

        // try flush pending packets.
        _ = quic_socket.flush().would_block()?;

        Ok(())
    }

    fn on_quic_send(&mut self, token: quico::Token) -> Result<()> {
        let mut buf = QuicBuf::new();

        let Poll::Ready(Ok((send_size, send_info))) =
            self.group.send(token, buf.writable_buf()).would_block()
        else {
            // skip all other errors.
            return Ok(());
        };

        assert!(send_size > 0);

        buf.writable_consume(send_size);

        let index = self
            .quic_socket_addrs
            .get(&send_info.from)
            .cloned()
            .expect("Quic socket");

        let quic_socket = self.quic_sockets.get_mut(index).expect("Quic socket");

        let len = quic_socket.send_to(buf, send_info.to)?;

        log::trace!("quic socket sending fifo, len={}", len);

        Ok(())
    }

    fn on_quic_stream_accept(&mut self, conn_id: quico::Token, stream_id: u64) -> Result<()> {
        log::trace!("on_quic_stream_accept");

        self.pairing_quic_streams.push_back((conn_id, stream_id));

        let token = self.next_mio_token();

        self.tcp_connector.connect(self.poll.registry(), token)
    }

    fn on_quic_stream_send(&mut self, conn_id: quico::Token, stream_id: u64) -> Result<()> {
        self.on_transfer_to((conn_id, stream_id))
    }

    fn on_quic_stream_recv(&mut self, conn_id: quico::Token, stream_id: u64) -> Result<()> {
        self.on_transfer_from((conn_id, stream_id))
    }

    fn on_transfer_from<T>(&mut self, token: T) -> Result<()>
    where
        T: Into<Token>,
    {
        _ = self.mapping.transfer_from(token);

        Ok(())
    }

    fn on_transfer_to<T>(&mut self, token: T) -> Result<()>
    where
        T: Into<Token>,
    {
        _ = self.mapping.transfer_to(token);

        Ok(())
    }
}
