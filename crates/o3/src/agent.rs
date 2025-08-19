use std::{
    collections::{HashMap, VecDeque},
    io::{ErrorKind, Result},
    net::SocketAddr,
    time::Instant,
};

use mio::{
    Interest,
    net::{TcpListener, UdpSocket},
};

use quico::{Group, quiche};
use ringbuff::RingBuf;

use crate::pipe::TcpQuicPipe;

/// O3 forward agent, passes tcp traffic through quic tunnel.
#[allow(unused)]
pub struct Agent {
    /// capacity of ringbuf for quic conn send/recv ops.
    quic_conn_ring_buf: usize,
    /// capacity of ringbuf for pipe data copy.
    pipe_copy_ring_buf: usize,
    /// remote quic server addresses.
    raddrs: Vec<SocketAddr>,
    /// shared quiche configuration for client connection.
    config: quiche::Config,
    /// mio token generator.
    mio_token_next: usize,
    /// Network poll
    poll: mio::Poll,
    /// non-blocking tcp server-side socket.
    listener: TcpListener,
    /// quic packets send/recv via this socket.
    udp_socket: UdpSocket,
    /// Map tcp stream to pipe.
    tcp_streams: HashMap<mio::Token, u64>,
    /// quico I/O group.
    group: quico::Group,
    /// quic connection pool.
    quic_conns: HashMap<quico::Token, RingBuf>,
    /// udp data pending queue.
    udp_send_queue: VecDeque<(quico::Token, SocketAddr)>,
    /// Map quic stream to pipe.
    quic_streams: HashMap<(quico::Token, u64), u64>,
    /// Alive pipes.
    pipes: HashMap<u64, TcpQuicPipe>,
    /// Returns by `quico::Group::non_blocking_poll`
    next_release_time: Option<Instant>,
}

impl Agent {
    /// Create a new `Agent` instance.
    pub fn new(
        laddr: SocketAddr,
        raddrs: Vec<SocketAddr>,
        config: quiche::Config,
        quic_conn_ring_buf: usize,
        pipe_copy_ring_buf: usize,
    ) -> Result<Self> {
        let poll = mio::Poll::new()?;

        let mut listener = TcpListener::bind(laddr)?;

        poll.registry()
            .register(&mut listener, mio::Token(0), Interest::READABLE)?;

        let mut udp_socket = UdpSocket::bind("[::]:0".parse().unwrap())?;

        poll.registry().register(
            &mut udp_socket,
            mio::Token(1),
            Interest::READABLE.add(Interest::WRITABLE),
        )?;

        let group = Group::new();

        Ok(Self {
            quic_conn_ring_buf,
            pipe_copy_ring_buf,
            next_release_time: None,
            config,
            raddrs,
            mio_token_next: 1,
            poll,
            listener,
            udp_socket,
            tcp_streams: Default::default(),
            group,
            quic_conns: Default::default(),
            quic_streams: Default::default(),
            pipes: Default::default(),
            udp_send_queue: Default::default(),
        })
    }

    pub fn run(&mut self) -> Result<()> {
        loop {
            self.run_once()?;
        }
    }

    fn run_once(&mut self) -> Result<()> {
        self.quic_group_poll()?;

        todo!()
    }

    fn quic_group_poll(&mut self) -> Result<()> {
        loop {
            // first check group events.
            let mut events = vec![];

            self.next_release_time = self.group.non_blocking_poll(&mut events);

            if events.is_empty() {
                return Ok(());
            }

            for event in events {
                match event.kind {
                    quico::EventKind::Send => {
                        self.quic_conn_send(event.token);
                    }
                    quico::EventKind::Recv => todo!(),
                    quico::EventKind::Connected => todo!(),
                    quico::EventKind::Accept => todo!(),
                    quico::EventKind::Closed => todo!(),
                    quico::EventKind::StreamOpen => todo!(),
                    quico::EventKind::StreamSend => todo!(),
                    quico::EventKind::StreamRecv => todo!(),
                }
            }
        }
    }

    fn quic_conn_send(&mut self, token: quico::Token) {
        let buff = self
            .quic_conns
            .get_mut(&token)
            .expect("conn send/recv buffer.");

        match self.group.send(token, unsafe { buff.writable_buf() }) {
            Ok((send_size, send_info)) => {
                unsafe {
                    buff.writable_consume(send_size);
                };

                match self
                    .udp_socket
                    .send_to(unsafe { buff.readable_buf() }, send_info.to)
                {
                    Ok(send_size) => {
                        unsafe {
                            buff.readable_consume(send_size);
                        };

                        assert_eq!(buff.readable(), 0, "send whole datagram.");

                        log::trace!("send udp, to={}, len={}", send_info.to, send_size,);

                        return;
                    }
                    Err(err) if err.kind() == ErrorKind::WouldBlock => {
                        // add task to pending queue.
                        self.udp_send_queue.push_back((token, send_info.to));
                        return;
                    }
                    Err(err) => {
                        log::error!("send udp, to={}, err={}", send_info.to, err);
                    }
                }
            }
            Err(quico::Error::Busy) => {
                unreachable!("single thread mode.");
            }
            Err(quico::Error::Retry) => {
                return;
            }
            Err(_) => {
                // do nothing.
            }
        }
    }
}
