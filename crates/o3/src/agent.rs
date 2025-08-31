//! Passes tcp traffic through quic tunnel.

use std::{
    collections::{HashSet, VecDeque},
    net::SocketAddr,
    time::Instant,
};

use mio::{
    Events, Interest,
    net::{TcpListener, TcpStream, UdpSocket},
};

use crate::{buf::QuicSocket, errors::Result, router::Router, token::Token};

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
    /// socket for `QUIC` traffic.
    quic_socket: QuicSocket,
    /// quic group.
    group: quico::Group,
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
            quic_socket: QuicSocket::new(udp_socket, 1024),
            group,
            paring_tcp_streams: Default::default(),
            quic_conns: Default::default(),
            router: Router::new(),
            raddrs,
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
                quico::EventKind::Send => self.quic_send(event.token)?,
                quico::EventKind::Recv => self.quic_recv(event.token)?,
                quico::EventKind::Connected => self.quic_connected(event.token)?,
                quico::EventKind::Accept => unreachable!("quic accept"),
                quico::EventKind::Closed => self.quic_closed(event.token)?,
                quico::EventKind::StreamOpen => {
                    self.quic_stream_open(event.token, event.stream_id)?
                }
                quico::EventKind::StreamSend => self.router_send((event.token, event.stream_id))?,
                quico::EventKind::StreamRecv => self.router_recv((event.token, event.stream_id))?,
            }
        }

        Ok(next_release_time)
    }

    fn quic_send(&mut self, _: quico::Token) -> Result<()> {
        todo!()
    }

    fn quic_recv(&mut self, _: quico::Token) -> Result<()> {
        todo!()
    }

    fn quic_connected(&mut self, _: quico::Token) -> Result<()> {
        todo!()
    }

    fn quic_closed(&mut self, _: quico::Token) -> Result<()> {
        todo!()
    }

    fn quic_stream_open(&mut self, _: quico::Token, _: u64) -> Result<()> {
        todo!()
    }

    fn router_send<T>(&mut self, _: T) -> Result<()>
    where
        T: Into<Token>,
    {
        todo!()
    }

    fn router_recv<T>(&mut self, _: T) -> Result<()>
    where
        T: Into<Token>,
    {
        todo!()
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
                self.tcp_accept()?;
            } else if token == UDP_TOKEN {
                if event.is_readable() {
                    self.udp_recv()?;
                } else {
                    self.udp_send()?;
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

    fn tcp_accept(&mut self) -> Result<()> {
        todo!()
    }

    fn udp_send(&mut self) -> Result<()> {
        todo!()
    }

    fn udp_recv(&mut self) -> Result<()> {
        todo!()
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
}
