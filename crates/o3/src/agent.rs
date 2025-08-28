use std::{
    collections::{HashSet, VecDeque},
    io::{Error, ErrorKind, Result},
    net::SocketAddr,
    rc::Rc,
    task::Poll,
    time::Instant,
};

use mio::{
    Events, Interest,
    net::{TcpListener, TcpStream, UdpSocket},
};
use quico::quiche::{self, RecvInfo};
use rand::seq::SliceRandom;

use crate::{
    io::{BufferedDatagram, QuicBuf, WouldBlock},
    switch::{QuicStreamPort, Switch, SwitchError, TcpStreamPort, Token},
};

static _TCP_LISTENER_TOKEN: mio::Token = mio::Token(0);
static _UDP_SOCKET_TOKEN: mio::Token = mio::Token(1);
static _QUIC_PACKET_LEN: usize = 1390;

#[allow(unused)]
/// Forward agent, passes tcp traffic through quic tunnel.
pub struct Agent {
    /// Locally mio token generator.
    mio_token_next: usize,
    /// poll for mio sockets.
    mio_poll: mio::Poll,
    /// thread-local quic `I/O` group.
    quic_group: Rc<quico::Group>,
    /// non-blocking tcp server socket.
    tcp_listener: TcpListener,
    /// non-blocking udp socket. which assumes the max `QUIC` packet length is `1392`
    udp_socket: BufferedDatagram<QuicBuf<_QUIC_PACKET_LEN>>,
    /// pending inbound tcp stream.
    pairing_tcp_streams: VecDeque<TcpStream>,
    /// active quic connections.
    quic_conns: HashSet<quico::Token>,
    /// o3 server address list.
    o3_server_addrs: Vec<SocketAddr>,
    /// shared quic config.
    quic_config: quiche::Config,
    /// In-memory/local-thread data switch.
    switch: Switch,
}

impl Agent {
    /// Create a new agent instance.
    pub fn new(
        laddr: SocketAddr,
        o3_server_addrs: Vec<SocketAddr>,
        quic_config: quiche::Config,
    ) -> Result<Self> {
        let mio_poll = mio::Poll::new()?;

        let mut tcp_listener = TcpListener::bind(laddr)?;

        mio_poll
            .registry()
            .register(&mut tcp_listener, _TCP_LISTENER_TOKEN, Interest::READABLE)?;

        let mut udp_socket = UdpSocket::bind("[::]:0".parse().unwrap())?;

        mio_poll.registry().register(
            &mut udp_socket,
            _UDP_SOCKET_TOKEN,
            Interest::READABLE | Interest::WRITABLE,
        )?;

        let quic_group = quico::Group::new();

        Ok(Self {
            o3_server_addrs,
            mio_token_next: 2,
            mio_poll,
            quic_group: Rc::new(quic_group),
            tcp_listener,
            udp_socket: BufferedDatagram::new(udp_socket, 1024),
            pairing_tcp_streams: Default::default(),
            quic_conns: Default::default(),
            quic_config,
            switch: Switch::default(),
        })
    }

    /// Run agent.
    pub fn run(mut self) -> Result<()> {
        loop {
            self.run_once()?;
        }
    }

    fn run_once(&mut self) -> Result<()> {
        let next_release_time = self.run_quic_poll_once()?;
        self.run_mio_poll_once(next_release_time)?;
        Ok(())
    }

    fn run_quic_poll_once(&mut self) -> Result<Option<Instant>> {
        let mut events = vec![];

        let next_release_instant = self.quic_group.non_blocking_poll(&mut events);

        for event in events {
            match event.kind {
                quico::EventKind::Send => {
                    self.quic_conn_send(event.token)?;
                }
                quico::EventKind::Recv => {
                    unreachable!("single thread mod.");
                }
                quico::EventKind::Connected => {
                    self.quic_connected(event.token)?;
                }
                quico::EventKind::Accept => {
                    unreachable!("client quic.");
                }
                quico::EventKind::Closed => {
                    self.quic_conn_closed(event.token)?;
                }
                quico::EventKind::StreamOpen => {
                    self.quic_stream_open(event.token, event.stream_id)?;
                }
                quico::EventKind::StreamSend => {
                    self.switch_send((event.token, event.stream_id))?;
                }
                quico::EventKind::StreamRecv => {
                    self.switch_recv((event.token, event.stream_id))?;
                }
            }
        }

        Ok(next_release_instant)
    }

    fn quic_conn_send(&mut self, token: quico::Token) -> Result<()> {
        if self.udp_socket.is_full() {
            log::warn!("udp socket sendbuf is full.");
            return Ok(());
        }

        let mut buf = QuicBuf::<_QUIC_PACKET_LEN>::new();

        let Ok((send_size, send_info)) = self.quic_group.send(token, buf.as_mut()) else {
            return Ok(());
        };

        buf.written_bytes(send_size);

        _ = self.udp_socket.send_to(buf, send_info.to).would_block()?;

        Ok(())
    }

    fn quic_connected(&mut self, token: quico::Token) -> Result<()> {
        self.quic_conns.insert(token);

        while !self.pairing_tcp_streams.is_empty() {
            let Poll::Ready((conn_id, stream_id)) = self.connect_to_server().would_block()? else {
                return Ok(());
            };

            let stream = self.pairing_tcp_streams.pop_front().unwrap();

            self.make_pipeline(conn_id, stream_id, stream)?;
        }

        Ok(())
    }

    fn quic_conn_closed(&mut self, token: quico::Token) -> Result<()> {
        self.quic_conns.remove(&token);
        Ok(())
    }

    fn quic_stream_open(&mut self, conn_id: quico::Token, stream_id: u64) -> Result<()> {
        if let Some(stream) = self.pairing_tcp_streams.pop_front() {
            self.make_pipeline(conn_id, stream_id, stream)?;
        }

        Ok(())
    }

    fn run_mio_poll_once(&mut self, next_release_time: Option<Instant>) -> Result<()> {
        let timeout = if let Some(next_release_time) = next_release_time {
            next_release_time.checked_duration_since(Instant::now())
        } else {
            None
        };

        let mut events = Events::with_capacity(1024);

        self.mio_poll.poll(&mut events, timeout)?;

        for event in events.iter() {
            let token = event.token();

            // indicates new inbound tcp stream.
            if token == _TCP_LISTENER_TOKEN {
                self.tcp_accept()?;
            } else if token == _UDP_SOCKET_TOKEN {
                if event.is_writable() {
                    self.udp_socket_send()?;
                }

                if event.is_readable() {
                    self.udp_socket_recv()?;
                }
            } else {
                // for tcp stream.
                if event.is_writable() {
                    self.switch_send(token)?;
                }

                if event.is_readable() {
                    self.switch_recv(token)?;
                }
            }
        }

        Ok(())
    }

    fn switch_send<T>(&mut self, token: T) -> Result<()>
    where
        crate::switch::Token: From<T>,
    {
        let token: Token = token.into();
        let Err(err) = self.switch.send(token) else {
            return Ok(());
        };

        match err {
            // pending.
            SwitchError::Source(_) | SwitchError::WouldBlock(_) | SwitchError::Sink(_) => {
                return Ok(());
            }
            _ => {
                if let Some(to) = self.switch.remove_rule(token) {
                    if let Ok(mut port) = self.switch.deregister(to) {
                        if let Err(err) = port.close() {
                            log::error!("close port({:?}), err={}", to, err);
                        }
                    }
                }

                if let Ok(mut port) = self.switch.deregister(token) {
                    if let Err(err) = port.close() {
                        log::error!("close port({:?}), err={}", token, err);
                    }
                }
            }
        }

        Ok(())
    }

    fn switch_recv<T>(&mut self, token: T) -> Result<()>
    where
        crate::switch::Token: From<T>,
    {
        let token: Token = token.into();

        let Err(err) = self.switch.recv(token) else {
            return Ok(());
        };

        match err {
            // pending.
            SwitchError::Source(_) | SwitchError::WouldBlock(_) | SwitchError::Sink(_) => {
                return Ok(());
            }
            _ => {
                if let Some(to) = self.switch.remove_rule(token) {
                    if let Ok(mut port) = self.switch.deregister(to) {
                        if let Err(err) = port.close() {
                            log::error!("close port({:?}), err={}", to, err);
                        }
                    }
                }

                if let Ok(mut port) = self.switch.deregister(token) {
                    if let Err(err) = port.close() {
                        log::error!("close port({:?}), err={}", token, err);
                    }
                }
            }
        }

        Ok(())
    }

    fn udp_socket_recv(&mut self) -> Result<()> {
        let mut buf = QuicBuf::<_QUIC_PACKET_LEN>::new();

        let Poll::Ready((read_size, from)) = self
            .udp_socket
            .recv_from(buf.writable_buf())
            .would_block()?
        else {
            return Ok(());
        };

        buf.written_bytes(read_size);

        match self.quic_group.recv(
            buf.as_mut(),
            RecvInfo {
                from,
                to: self.udp_socket.local_addr(),
            },
        ) {
            Ok(_) => {}
            Err(quico::Error::Busy) | Err(quico::Error::Retry) => {
                unreachable!("single thread mod.");
            }
            Err(err) => {
                log::error!("quic recv, err={}", err)
            }
        }

        Ok(())
    }

    fn udp_socket_send(&mut self) -> Result<()> {
        _ = self.udp_socket.flush().would_block()?;

        Ok(())
    }

    fn tcp_accept(&mut self) -> Result<()> {
        let Poll::Ready((stream, raddr)) = self.tcp_listener.accept().would_block()? else {
            return Ok(());
        };

        log::trace!("inbound tcp stream, from={}", raddr);

        let Poll::Ready((conn_id, stream_id)) = self.connect_to_server().would_block()? else {
            log::trace!("pipeline creation suspended , from={}", raddr);
            self.pairing_tcp_streams.push_back(stream);
            return Ok(());
        };

        self.make_pipeline(conn_id, stream_id, stream)
    }

    fn make_pipeline(
        &mut self,
        conn_id: quico::Token,
        stream_id: u64,
        mut stream: TcpStream,
    ) -> Result<()> {
        let tcp_stream_token = self.next_mio_token();

        self.mio_poll.registry().register(
            &mut stream,
            tcp_stream_token,
            Interest::READABLE | Interest::WRITABLE,
        )?;

        self.switch
            .register(TcpStreamPort::new(tcp_stream_token, stream)?, 1024 * 1024)?;

        self.switch.register(
            QuicStreamPort::new(self.quic_group.clone(), conn_id, stream_id),
            1024 * 1024,
        )?;

        self.switch.add_rule(tcp_stream_token, (conn_id, stream_id));

        Ok(())
    }

    fn connect_to_server(&mut self) -> Result<(quico::Token, u64)> {
        let mut quic_conns = self.quic_conns.iter().copied().collect::<Vec<_>>();

        quic_conns.shuffle(&mut rand::rng());

        let mut busy = false;

        for conn_id in quic_conns {
            match self.quic_group.stream_open(conn_id) {
                Ok(stream_id) => return Ok((conn_id, stream_id)),
                Err(quico::Error::Retry) => {}
                Err(quico::Error::Busy) => busy = true,
                Err(err) => {
                    log::error!("quic stream open, on={:?}, err={}", conn_id, err);
                    self.quic_conns.remove(&conn_id);
                }
            }
        }

        if busy {
            return Err(Error::new(ErrorKind::WouldBlock, "connection is busy."));
        }

        self.o3_server_addrs.shuffle(&mut rand::rng());

        let conn_id = self.quic_group.connect(
            None,
            self.udp_socket.local_addr(),
            self.o3_server_addrs[0],
            &mut self.quic_config,
        )?;

        self.quic_conns.insert(conn_id);

        return Err(Error::new(
            ErrorKind::WouldBlock,
            "Create a new quic connection.",
        ));
    }

    fn next_mio_token(&mut self) -> mio::Token {
        loop {
            let token = mio::Token(self.mio_token_next);

            let overflowing;

            (self.mio_token_next, overflowing) = self.mio_token_next.overflowing_add(1);

            if overflowing {
                self.mio_token_next = 2;
            }

            if self.switch.contains_port(token) {
                continue;
            }

            return token;
        }
    }
}
