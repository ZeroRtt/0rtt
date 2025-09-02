use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    time::Instant,
};

use mio::{Interest, net::UdpSocket};
use quico::{Group, acceptor::Acceptor};

use crate::{buf::QuicSocket, errors::Result, router::Router, token::Token};

#[allow(unused)]
/// Redirect all inbound quic traffic to target server via tcp tunnels.
pub struct RProxy {
    /// mio token generator.
    mio_token_next: usize,
    /// Socket address of target server.
    target: SocketAddr,
    /// Quic connection group.
    group: Group,
    /// Polls for readiness events on all registered values.
    poll: mio::Poll,
    /// Acceptor for inbound quic connections.
    acceptor: Acceptor,
    /// Inbound QUIC streams that are not yet paired
    pairing_quic_streams: VecDeque<(quico::Token, u64)>,
    /// Underlying udp sockets for quic traffics.
    quic_sockets: HashMap<mio::Token, QuicSocket>,
    /// Map quic socket address to token.
    quic_socket_addrs: HashMap<SocketAddr, mio::Token>,
    /// router for quic traffic.
    router: Router,
}

impl RProxy {
    /// Create a new instance.
    pub fn new(laddrs: Vec<SocketAddr>, target: SocketAddr, acceptor: Acceptor) -> Result<Self> {
        let poll = mio::Poll::new()?;

        let mut quic_sockets = HashMap::new();
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

            quic_sockets.insert(token, QuicSocket::new(udp_socket, 1024));
            quic_socket_addrs.insert(laddr, token);
        }

        Ok(Self {
            mio_token_next,
            target,
            group: Group::new(),
            poll,
            acceptor,
            pairing_quic_streams: Default::default(),
            quic_sockets,
            quic_socket_addrs,
            router: Router::new(),
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
                quico::EventKind::Accept => unreachable!("quic accept"),
                quico::EventKind::Closed => self.on_quic_closed(event.token)?,
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
            let _ = event.token();
        }

        Ok(())
    }

    #[allow(unused)]
    fn next_mio_token(&mut self) -> mio::Token {
        loop {
            let token = mio::Token(self.mio_token_next);

            (self.mio_token_next, _) = self.mio_token_next.overflowing_add(1);

            if self.router.contains_port(token) || self.quic_sockets.contains_key(&token) {
                continue;
            }
        }
    }

    fn on_quic_send(&mut self, _: quico::Token) -> Result<()> {
        todo!()
    }

    fn on_quic_closed(&mut self, _: quico::Token) -> Result<()> {
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
}
