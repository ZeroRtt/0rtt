//! Passes tcp traffic through quic tunnel.

use std::{
    collections::{HashSet, VecDeque},
    net::SocketAddr,
    sync::Arc,
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
    buf::{QuicBuf, QuicSocket},
    errors::{Error, Result},
    port::{BufPort, QuicStreamPort, TcpStreamPort},
    router::Router,
    token::Token,
    would_block::WouldBlock,
};

static TCP_TOKEN: mio::Token = mio::Token(0);
static UDP_TOKEN: mio::Token = mio::Token(1);

/// Forward agent, passes tcp traffic through quic tunnel.
pub struct Agent {
    /// port receving buffer size.
    port_buffer_size: usize,
    /// remote o3 server addresses.
    raddrs: Vec<SocketAddr>,
    /// local mio token generator.
    mio_token_next: usize,
    /// mio poll.
    mio_poll: mio::Poll,
    /// Accept inbound tcp streams.
    tcp_listener: TcpListener,
    /// Local bound address for quic socket.
    quic_local_addr: SocketAddr,
    /// socket for `QUIC` traffic.
    quic_socket: QuicSocket,
    /// shared quic configuration.
    config: quiche::Config,
    /// quic group.
    group: Arc<quico::Group>,
    /// paring tcp streams.
    paring_tcp_streams: VecDeque<TcpStream>,
    /// active quic connections.
    quic_conns: HashSet<quico::Token>,
    /// router for tcp traffic.
    router: Router,
}

impl Agent {
    /// Create a new `Agent` instance.
    pub fn new(
        laddr: SocketAddr,
        raddrs: Vec<SocketAddr>,
        port_buffer_size: usize,
        config: quiche::Config,
    ) -> Result<Self> {
        let poll = mio::Poll::new()?;

        let mut tcp_listener = TcpListener::bind(laddr)?;

        poll.registry()
            .register(&mut tcp_listener, TCP_TOKEN, Interest::READABLE)?;

        let mut udp_socket = UdpSocket::bind("[::]:0".parse().unwrap())?;

        poll.registry().register(
            &mut udp_socket,
            UDP_TOKEN,
            Interest::READABLE | Interest::WRITABLE,
        )?;

        let group = quico::Group::new();

        Ok(Self {
            port_buffer_size,
            mio_token_next: 2,
            mio_poll: poll,
            tcp_listener,
            quic_local_addr: udp_socket.local_addr()?,
            quic_socket: QuicSocket::new(udp_socket, 1024),
            group: Arc::new(group),
            paring_tcp_streams: Default::default(),
            quic_conns: Default::default(),
            router: Router::new(),
            raddrs,
            config,
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
                quico::EventKind::Connected => self.on_quic_connected(event.token)?,
                quico::EventKind::Accept => unreachable!("quic accept"),
                quico::EventKind::Closed => self.on_quic_closed(event.token)?,
                quico::EventKind::StreamOpen => self.on_quic_stream_open(event.token)?,
                quico::EventKind::StreamSend => self.router_send((event.token, event.stream_id))?,
                quico::EventKind::StreamRecv => self.router_recv((event.token, event.stream_id))?,
            }
        }

        Ok(next_release_time)
    }

    fn mio_poll_once(&mut self, next_release_time: Option<Instant>) -> Result<()> {
        let mut events = Events::with_capacity(1024);

        let timeout = if let Some(next_release_time) = next_release_time {
            next_release_time.checked_duration_since(Instant::now())
        } else {
            None
        };

        self.mio_poll.poll(&mut events, timeout)?;

        for event in events.iter() {
            let token = event.token();

            if token == TCP_TOKEN {
                self.on_tcp_accept()?;
            } else if token == UDP_TOKEN {
                if event.is_readable() {
                    self.on_udp_recv()?;
                } else {
                    self.on_udp_send()?;
                }
            } else {
                if self.router.contains_port(token) {
                    if event.is_readable() {
                        self.router_recv(token)?;
                    } else {
                        self.router_send(token)?;
                    }
                }
            }
        }

        Ok(())
    }

    #[allow(unused)]
    fn next_mio_token(&mut self) -> mio::Token {
        loop {
            let token = mio::Token(self.mio_token_next);

            let overflow;
            (self.mio_token_next, overflow) = self.mio_token_next.overflowing_add(1);

            if overflow {
                self.mio_token_next = 1;
            }

            if self.router.contains_port(token) {
                continue;
            }
        }
    }

    fn on_quic_send(&mut self, token: quico::Token) -> Result<()> {
        _ = self.quic_socket.flush().would_block()?;

        if self.quic_socket.is_full() {
            log::warn!("Quic socket is full.");
            return Ok(());
        }

        let mut buf = QuicBuf::new();

        let Poll::Ready(Ok((send_size, send_info))) =
            self.group.send(token, buf.writable_buf()).would_block()
        else {
            return Ok(());
        };

        buf.truncate(send_size);

        self.quic_socket.send_to(buf, send_info.to)?;

        Ok(())
    }

    fn on_quic_connected(&mut self, token: quico::Token) -> Result<()> {
        self.quic_conns.insert(token);

        self.make_pipelines()
    }

    fn on_quic_closed(&mut self, token: quico::Token) -> Result<()> {
        self.quic_conns.remove(&token);
        Ok(())
    }

    fn on_quic_stream_open(&mut self, _: quico::Token) -> Result<()> {
        self.make_pipelines()
    }

    fn router_send<T>(&mut self, token: T) -> Result<()>
    where
        T: Into<Token>,
    {
        // ignore all errors.
        _ = self.router.send(token);

        Ok(())
    }

    fn router_recv<T>(&mut self, token: T) -> Result<()>
    where
        T: Into<Token>,
    {
        // ignore all errors.
        _ = self.router.recv(token);

        Ok(())
    }

    fn on_tcp_accept(&mut self) -> Result<()> {
        let Poll::Ready((stream, from)) = self.tcp_listener.accept().would_block()? else {
            return Ok(());
        };

        log::trace!("new inbound TCP stream, {}", from);

        self.paring_tcp_streams.push_back(stream);

        self.make_pipelines()
    }

    fn on_udp_send(&mut self) -> Result<()> {
        _ = self.quic_socket.flush().would_block()?;

        Ok(())
    }

    fn on_udp_recv(&mut self) -> Result<()> {
        let mut buf = QuicBuf::new();

        let Poll::Ready(from) = self.quic_socket.recv_from(&mut buf).would_block()? else {
            // would block.
            return Ok(());
        };

        // skip all errors.
        _ = self.group.recv(
            buf.as_mut(),
            RecvInfo {
                from,
                to: self.quic_local_addr,
            },
        );

        Ok(())
    }

    fn make_pipelines(&mut self) -> Result<()> {
        while !self.paring_tcp_streams.is_empty() {
            let Poll::Ready((conn_id, stream_id)) = self.quic_stream_open().would_block()? else {
                return Ok(());
            };

            let tcp_stream = self.paring_tcp_streams.pop_front().unwrap();

            let tcp_stream_token = self.next_mio_token();

            self.router.register(
                BufPort::new(
                    TcpStreamPort::new(tcp_stream, tcp_stream_token),
                    self.port_buffer_size,
                ),
                BufPort::new(
                    QuicStreamPort::new(self.group.clone(), conn_id, stream_id),
                    self.port_buffer_size,
                ),
            );
        }

        Ok(())
    }

    fn quic_stream_open(&mut self) -> Result<(quico::Token, u64)> {
        let mut conn_ids = self.quic_conns.iter().collect::<Vec<_>>();

        conn_ids.shuffle(&mut rand::rng());

        for conn_id in conn_ids {
            let Poll::Ready(Ok(stream_id)) = self.group.stream_open(*conn_id).would_block() else {
                continue;
            };

            return Ok((*conn_id, stream_id));
        }

        self.raddrs.shuffle(&mut rand::rng());

        // Create a new QUIC connection and issue a non-blocking connect to the specified address.
        self.group
            .connect(None, self.quic_local_addr, self.raddrs[0], &mut self.config)?;

        Err(Error::Retry)
    }
}
