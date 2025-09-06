use std::{
    collections::{HashMap, VecDeque},
    io::ErrorKind,
    net::SocketAddr,
    sync::Arc,
    task::Poll,
    time::Instant,
};

use mio::{
    Interest,
    net::{TcpStream, UdpSocket},
};
use quico::{Group, acceptor::Acceptor, quiche::RecvInfo};

use crate::{
    buf::{QuicBuf, QuicSocket},
    errors::{Error, Result},
    port::{BufPort, QuicStreamPort, TcpStreamPort},
    router::Router,
    token::Token,
    would_block::WouldBlock,
};

/// Redirect all inbound quic traffic to target server via tcp tunnels.
pub struct Redirect {
    /// port receving buffer size.
    port_buffer_size: usize,
    /// mio token generator.
    mio_token_next: usize,
    /// Socket address of target server.
    target: SocketAddr,
    /// Quic connection group.
    group: Arc<Group>,
    /// Polls for readiness events on all registered values.
    poll: mio::Poll,
    /// Acceptor for inbound quic connections.
    acceptor: Acceptor,
    /// Inbound QUIC streams that are not yet paired
    pairing_quic_streams: VecDeque<(quico::Token, u64)>,
    /// Connecting outbound `TCP` stream.
    connecting_tcp_streams: HashMap<mio::Token, TcpStream>,
    /// Underlying udp sockets for quic traffics.
    quic_sockets: Vec<QuicSocket>,
    /// Map quic socket address to token.
    quic_socket_addrs: HashMap<SocketAddr, usize>,
    /// router for quic traffic.
    router: Router,
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

            quic_sockets.push(QuicSocket::new(udp_socket, laddr, 1024));
            quic_socket_addrs.insert(laddr, token.0);
        }

        Ok(Self {
            port_buffer_size,
            mio_token_next,
            target,
            group: Arc::new(Group::new()),
            poll,
            acceptor,
            pairing_quic_streams: Default::default(),
            quic_sockets,
            quic_socket_addrs,
            router: Router::new(),
            connecting_tcp_streams: Default::default(),
        })
    }

    /// Run agent server.
    pub fn run(mut self) -> Result<()> {
        loop {
            let next_release_time = self.quico_poll_once()?;

            self.mio_poll_once(next_release_time)?;
        }
    }

    fn quico_poll_once(&mut self) -> Result<Option<Instant>> {
        let mut events = vec![];

        let next_release_time = self.group.non_blocking_poll(&mut events);

        for event in events {
            match event.kind {
                quico::EventKind::Send => self.on_quic_send(event.token)?,
                quico::EventKind::Recv => unreachable!("Single thread mode."),
                quico::EventKind::Connected => unreachable!("Quic server side"),
                quico::EventKind::Accept => self.on_quic_accept(event.token)?,
                quico::EventKind::Closed => self.on_quic_closed(event.token)?,
                quico::EventKind::StreamAccept => {
                    self.on_quic_stream_accept(event.token, event.stream_id)?
                }
                quico::EventKind::StreamOpen => unreachable!("Quic server side."),
                quico::EventKind::StreamSend => self.router_send((event.token, event.stream_id))?,
                quico::EventKind::StreamRecv => self.router_recv((event.token, event.stream_id))?,
            }
        }

        Ok(next_release_time)
    }

    fn mio_poll_once(&mut self, next_release_time: Option<Instant>) -> Result<()> {
        let mut events = mio::Events::with_capacity(1024);

        let timeout = if let Some(next_release_time) = next_release_time {
            next_release_time.checked_duration_since(Instant::now())
        } else {
            None
        };

        self.poll.poll(&mut events, timeout)?;

        for event in events.iter() {
            log::trace!("raised event: {:?}", event);
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
                if event.is_readable() {
                    self.router_recv(token)?;
                }

                if event.is_writable() {
                    if self.connecting_tcp_streams.contains_key(&token) {
                        self.on_tcp_stream_connect(token)?;
                    } else {
                        self.router_send(token)?;
                    }
                }

                // for tcp streams.
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

            if self.router.contains_port(token) {
                continue;
            }

            return token;
        }
    }

    fn on_tcp_stream_connect(&mut self, token: mio::Token) -> Result<()> {
        let tcp_stream = self
            .connecting_tcp_streams
            .get(&token)
            .expect("TcpStream is not found.");

        match tcp_stream.take_error() {
            Ok(None) => {}
            Ok(Some(err)) | Err(err) => {
                self.connecting_tcp_streams.remove(&token);

                log::error!("TcpStream is failed to connect to server, {}", err);

                return Ok(());
            }
        }

        match tcp_stream.peer_addr() {
            Ok(_) => {
                let tcp_stream = self
                    .connecting_tcp_streams
                    .remove(&token)
                    .expect("TcpStream");

                let (quic_conn_id, quic_stream_id) = self
                    .pairing_quic_streams
                    .pop_front()
                    .expect("Expect one pairing quic stream.");

                self.router.register(
                    BufPort::new(
                        QuicStreamPort::new(self.group.clone(), quic_conn_id, quic_stream_id),
                        self.port_buffer_size,
                    ),
                    BufPort::new(TcpStreamPort::new(tcp_stream, token), self.port_buffer_size),
                );

                self.router_recv((quic_conn_id, quic_stream_id))
            }
            Err(err)
                if err.kind() == ErrorKind::NotConnected
                    || err.raw_os_error() == Some(libc::EINPROGRESS) =>
            {
                return Ok(());
            }
            Err(err) => {
                self.connecting_tcp_streams.remove(&token);

                log::error!("TcpStream is failed to connect to server, {}", err);

                return Ok(());
            }
        }
    }

    fn on_udp_recv(&mut self, token: mio::Token) -> Result<()> {
        let quic_socket = self
            .quic_sockets
            .get_mut(token.0)
            .expect("Quic sockets: out of range.");

        let mut buf = QuicBuf::new();

        let Poll::Ready(from) = quic_socket.recv_from(&mut buf).would_block()? else {
            return Ok(());
        };

        log::trace!("recv : {}", from);
        let recv_size = buf.len();

        // Skip returns errors.
        match self.group.accept_recv(
            &mut self.acceptor,
            buf.writable_buf(),
            recv_size,
            RecvInfo {
                from,
                to: quic_socket.local_addr(),
            },
        ) {
            Ok((send_size, send_info)) => {
                buf.truncate(send_size);
                match quic_socket.send_to(buf, send_info.to) {
                    Ok(_) => Ok(()),
                    Err(Error::IsFull(_)) => {
                        log::warn!("udp send queue is full, socket={}", token.0);
                        Ok(())
                    }
                    Err(err) => return Err(err),
                }
            }
            Err(_) => Ok(()),
        }
    }

    fn on_udp_send(&mut self, token: mio::Token) -> Result<()> {
        let quic_socket = self
            .quic_sockets
            .get_mut(token.0)
            .expect("Quic sockets: out of range.");

        _ = quic_socket.flush().would_block()?;

        Ok(())
    }

    fn on_quic_accept(&mut self, _: quico::Token) -> Result<()> {
        Ok(())
    }

    fn on_quic_stream_accept(&mut self, token: quico::Token, stream_id: u64) -> Result<()> {
        let token = (token, stream_id);

        log::trace!("create outbound tcp stream, target={}", self.target);

        self.pairing_quic_streams.push_back(token);

        let mut tcp_stream = TcpStream::connect(self.target)?;
        let token = self.next_mio_token();

        self.poll.registry().register(
            &mut tcp_stream,
            token,
            Interest::READABLE | Interest::WRITABLE,
        )?;

        self.connecting_tcp_streams.insert(token, tcp_stream);

        Ok(())
    }

    fn on_quic_send(&mut self, token: quico::Token) -> Result<()> {
        let mut buf = QuicBuf::new();

        let Poll::Ready(Ok((send_size, send_info))) =
            self.group.send(token, buf.writable_buf()).would_block()
        else {
            return Ok(());
        };

        buf.truncate(send_size);

        let token = self
            .quic_socket_addrs
            .get(&send_info.from)
            .copied()
            .expect("Quic send path.");

        match self
            .quic_sockets
            .get_mut(token)
            .expect("Quic socket path.")
            .send_to(buf, send_info.to)
        {
            Ok(_) => Ok(()),
            Err(Error::IsFull(_)) => {
                log::warn!("udp send queue is full, socket={}", token);
                Ok(())
            }
            Err(err) => return Err(err),
        }
    }

    fn on_quic_closed(&mut self, _: quico::Token) -> Result<()> {
        Ok(())
    }

    fn router_send<T>(&mut self, token: T) -> Result<()>
    where
        T: Into<Token>,
    {
        let token = token.into();

        if !self.router.contains_port(token) {
            log::warn!("Port is not found: {:?}", token);
            return Ok(());
        }

        // ignore all errors.
        _ = self.router.send(token);

        Ok(())
    }

    fn router_recv<T>(&mut self, token: T) -> Result<()>
    where
        T: Into<Token>,
    {
        let token = token.into();

        if !self.router.contains_port(token) {
            log::warn!("new inbound quic stream: {:?}", token);
            return Ok(());
        }

        // ignore all errors.
        _ = self.router.recv(token);

        Ok(())
    }
}
