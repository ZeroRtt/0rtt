//! Passes tcp traffic through quic tunnel.

use std::{
    collections::VecDeque,
    io::ErrorKind,
    net::{Ipv6Addr, SocketAddr},
    sync::Arc,
    task::Poll,
    time::Instant,
};

use mio::{
    Events, Interest,
    net::{TcpListener, TcpStream, UdpSocket},
};
use zrquic::quiche::{self, RecvInfo};

use crate::{
    buf::QuicBuf,
    connector::QuicConnector,
    errors::{Error, Result},
    mapping::Mapping,
    metrics::Metrics,
    port::{BufPort, QuicStreamPort, TcpStreamPort},
    token::Token,
    udp::QuicSocket,
    would_block::WouldBlock,
};

static TCP_LISTENER_TOKEN: mio::Token = mio::Token(0);
static UDP_SOCKET_TOKEN: mio::Token = mio::Token(1);

/// Forward agent, passes tcp traffic through quic tunnel.
pub struct Agent {
    /// max pairing tcp streams.
    max_pairing_tcp_streams: usize,
    /// Buffered port buffer size.
    port_buffer_size: usize,
    /// local token generator.
    mio_token_next: usize,
    /// readiness events poll.
    poll: mio::Poll,
    /// Inbound tcp stream listener.
    tcp_listener: TcpListener,
    /// Udp socket for quic traffic.
    quic_socket: QuicSocket,
    /// Pairing inbound tcp streams.
    pairing_tcp_streams: VecDeque<TcpStream>,
    /// Group for quic sockets.
    group: Arc<zrquic::Group>,
    /// connector for quic connections.
    quic_connector: QuicConnector,
    /// bidirectional data pipelines.
    mapping: Mapping,
    /// Metrics collector.
    meterics: Metrics,
}

impl Agent {
    /// Create a new `Agent` instance.
    pub fn new(
        local_addr: SocketAddr,
        o3_server_addrs: Vec<SocketAddr>,
        config: quiche::Config,
        port_buffer_size: usize,
        max_pairing_tcp_streams: usize,
    ) -> Result<Self> {
        let poll = mio::Poll::new()?;

        let mut tcp_listener = TcpListener::bind(local_addr)?;

        poll.registry()
            .register(&mut tcp_listener, TCP_LISTENER_TOKEN, Interest::READABLE)?;

        let mut udp_socket = UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0).into())?;

        let local_addr = udp_socket.local_addr()?;

        log::trace!("quic socket bind to: {}", local_addr);

        poll.registry().register(
            &mut udp_socket,
            UDP_SOCKET_TOKEN,
            Interest::READABLE | Interest::WRITABLE,
        )?;

        let group = zrquic::Group::new();

        Ok(Self {
            max_pairing_tcp_streams,
            port_buffer_size,
            mio_token_next: 2,
            poll,
            tcp_listener,
            quic_socket: QuicSocket::new(udp_socket, 1024)?,
            pairing_tcp_streams: Default::default(),
            group: Arc::new(group),
            quic_connector: QuicConnector::new(local_addr, o3_server_addrs, config, 1),
            mapping: Default::default(),
            meterics: Default::default(),
        })
    }

    /// Run agent server.
    pub fn run(mut self) -> Result<()> {
        loop {
            let next_release_time = self.quico_poll_once()?;

            self.mio_poll_once(next_release_time)?;

            self.meterics.report();
        }
    }
}

impl Agent {
    fn quico_poll_once(&mut self) -> Result<Option<Instant>> {
        loop {
            let mut events = vec![];

            let next_release_time = self.group.non_blocking_poll(&mut events);

            log::trace!(
                "quico readiness, raised={}, next_release_time={:?}",
                events.len(),
                next_release_time
            );

            self.meterics.qucio_poll_add(events.len());

            if events.is_empty() {
                return Ok(next_release_time);
            }

            for event in events {
                match event.kind {
                    zrquic::EventKind::Send => self.on_quic_send(event.token)?,
                    zrquic::EventKind::Recv => unreachable!("Single thread mode."),
                    zrquic::EventKind::Connected => self.on_quic_connected(event.token)?,
                    zrquic::EventKind::Accept => unreachable!("quic accept"),
                    zrquic::EventKind::Closed => self.on_quic_closed(event.token)?,
                    zrquic::EventKind::StreamOpen => self.on_quic_stream_open(event.token)?,
                    zrquic::EventKind::StreamSend => {
                        self.on_quic_stream_send(event.token, event.stream_id)?
                    }
                    zrquic::EventKind::StreamRecv => {
                        self.on_quic_stream_recv(event.token, event.stream_id)?
                    }
                    zrquic::EventKind::StreamAccept => unreachable!("quic stream accept"),
                }
            }
        }
    }

    fn mio_poll_once(&mut self, next_release_time: Option<Instant>) -> Result<()> {
        let mut events = Events::with_capacity(1024);

        let timeout = if let Some(next_release_time) = next_release_time {
            next_release_time.checked_duration_since(Instant::now())
        } else {
            None
        };

        self.poll
            .poll(&mut events, timeout)
            .inspect_err(|err| log::error!("mio poll error: {}", err))?;

        log::trace!(
            "mio readiness, raised={}, timeout={:?}",
            events.iter().count(),
            timeout,
        );

        let mut count = 0;

        for event in events.iter() {
            log::trace!("readiness, event={:?}", event);

            count += 1;

            let token = event.token();

            if token == TCP_LISTENER_TOKEN {
                self.on_tcp_accept()?;
            } else if token == UDP_SOCKET_TOKEN {
                if event.is_readable() {
                    self.on_udp_recv()?;
                }

                if event.is_writable() {
                    self.on_udp_send()?;
                }
            } else {
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

        self.meterics.mio_poll_add(count);

        Ok(())
    }
}

impl Agent {
    fn make_port_mapping(&mut self) -> Result<()> {
        while !self.pairing_tcp_streams.is_empty() {
            let Poll::Ready(Ok((conn_id, stream_id))) =
                self.quic_connector.stream_open(&self.group).would_block()
            else {
                return Ok(());
            };

            let mut tcp_stream = self.pairing_tcp_streams.pop_front().unwrap();
            let token = self.next_mio_token();

            match self.poll.registry().register(
                &mut tcp_stream,
                token,
                Interest::READABLE | Interest::WRITABLE,
            ) {
                Ok(_) => {}
                Err(err) => {
                    log::error!("register tcp stream, err={}", err);
                    continue;
                }
            }

            self.mapping.register(
                BufPort::new(TcpStreamPort::new(tcp_stream, token), self.port_buffer_size),
                BufPort::new(
                    QuicStreamPort::new(self.group.clone(), conn_id, stream_id),
                    self.port_buffer_size,
                ),
            );

            self.on_transfer_from(token)?;
            self.on_transfer_to(token)?;
        }

        Ok(())
    }

    fn next_mio_token(&mut self) -> mio::Token {
        loop {
            let token = mio::Token(self.mio_token_next);

            let overflow;
            (self.mio_token_next, overflow) = self.mio_token_next.overflowing_add(1);

            if overflow {
                self.mio_token_next = 2;
            }

            if self.mapping.contains_port(token) {
                continue;
            }

            return token;
        }
    }
}

impl Agent {
    fn on_quic_send(&mut self, token: zrquic::Token) -> Result<()> {
        let mut buf = QuicBuf::new();

        let Poll::Ready(Ok((send_size, send_info))) =
            self.group.send(token, buf.writable_buf()).would_block()
        else {
            // skip all other errors.
            return Ok(());
        };

        buf.writable_consume(send_size);

        let len = match self.quic_socket.send_to(buf, send_info.to) {
            Ok(len) => len,
            Err(Error::IsFull(_)) => {
                log::warn!("udp send queue is full, socket={}", token.0);
                return Ok(());
            }
            Err(err) => return Err(err),
        };

        log::trace!("quic socket sending fifo, len={}", len);

        Ok(())
    }

    fn on_quic_connected(&mut self, token: zrquic::Token) -> Result<()> {
        self.quic_connector.connected(token);
        self.make_port_mapping()
    }

    fn on_quic_closed(&mut self, token: zrquic::Token) -> Result<()> {
        self.quic_connector.closed(token);
        self.mapping.on_quic_closed(token);
        Ok(())
    }

    fn on_quic_stream_open(&mut self, _: zrquic::Token) -> Result<()> {
        log::info!("on_quic_stream_open");

        self.make_port_mapping()
    }

    fn on_quic_stream_send(&mut self, conn_id: zrquic::Token, stream_id: u64) -> Result<()> {
        self.on_transfer_to((conn_id, stream_id))
    }

    fn on_quic_stream_recv(&mut self, conn_id: zrquic::Token, stream_id: u64) -> Result<()> {
        self.on_transfer_from((conn_id, stream_id))
    }

    fn on_tcp_accept(&mut self) -> Result<()> {
        loop {
            let Poll::Ready(Ok((tcp_stream, from))) = self
                .tcp_listener
                .accept()
                .inspect_err(|err| {
                    if err.kind() != ErrorKind::WouldBlock {
                        log::error!(
                            "accept tcp stream, pairings={}, err={}",
                            self.pairing_tcp_streams.len(),
                            err
                        );
                    }
                })
                .would_block()
            else {
                break;
            };

            if self.pairing_tcp_streams.len() == self.max_pairing_tcp_streams {
                self.pairing_tcp_streams.pop_front();
            }

            self.pairing_tcp_streams.push_back(tcp_stream);

            log::info!(
                "Accept new tcp stream, from={}, pairings={}",
                from,
                self.pairing_tcp_streams.len()
            );
        }

        self.make_port_mapping()
    }

    fn on_udp_recv(&mut self) -> Result<()> {
        loop {
            let mut buf = QuicBuf::new();

            let Poll::Ready(from) = self.quic_socket.recv_from(&mut buf).would_block()? else {
                return Ok(());
            };

            // skip all returned errors.
            _ = self.group.recv(
                buf.readable_buf_mut(),
                RecvInfo {
                    from,
                    to: self.quic_connector.local_addr(),
                },
            );
        }
    }

    fn on_udp_send(&mut self) -> Result<()> {
        // try flush pending packets.
        _ = self.quic_socket.flush().would_block()?;

        Ok(())
    }

    fn on_transfer_from<T>(&mut self, token: T) -> Result<()>
    where
        T: Into<Token>,
    {
        _ = self.mapping.transfer_from(token, &mut self.meterics);

        Ok(())
    }

    fn on_transfer_to<T>(&mut self, token: T) -> Result<()>
    where
        T: Into<Token>,
    {
        _ = self.mapping.transfer_to(token, &mut self.meterics);

        Ok(())
    }
}
