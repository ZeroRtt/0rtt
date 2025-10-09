use std::{
    borrow::Cow,
    collections::HashMap,
    io::Result,
    net::{SocketAddr, ToSocketAddrs},
    task::Poll,
    time::{Duration, Instant},
};

use crossbeam_utils::sync::Parker;
use mio::{Events, Interest, Waker, net::UdpSocket};
use parking_lot::Mutex;
use quiche::RecvInfo;

use crate::{
    mio::{
        buf::QuicBuf,
        udp::{QuicSocket, QuicSocketError},
        would_block::WouldBlock,
    },
    poll::{
        Event, EventKind, StreamKind, Token,
        server::{Acceptor, ServerGroup},
        utils::min_of_some,
    },
};

struct PollState {
    /// server-side connection acceptor.
    acceptor: Option<Acceptor>,
    /// mio poller.
    poll: mio::Poll,
    /// `UDP` sockets bound to this group.
    sockets: Vec<QuicSocket>,
}

/// Facade to access `QUIC` group.
pub struct Group {
    /// A waker of `mio::Poll`.
    waker: mio::Waker,
    /// quic group.
    group: crate::poll::Group,
    /// local bound addresses
    laddrs: HashMap<SocketAddr, usize>,
    /// poll state.
    state: Mutex<PollState>,
}

impl Group {
    /// Create a new `Group` and bind to `laddrs`.
    pub fn bind<S>(laddrs: S, acceptor: Option<Acceptor>) -> Result<Self>
    where
        S: ToSocketAddrs,
    {
        let poll = mio::Poll::new()?;
        let group = crate::poll::Group::new();

        let mut sockets = vec![];
        let mut addrs = HashMap::new();

        for laddr in laddrs.to_socket_addrs()? {
            let mut socket = UdpSocket::bind(laddr)?;
            addrs.insert(socket.local_addr()?, sockets.len());

            poll.registry().register(
                &mut socket,
                mio::Token(sockets.len()),
                Interest::READABLE | Interest::WRITABLE,
            )?;

            sockets.push(QuicSocket::new(socket, 1024)?);
        }

        let waker = Waker::new(poll.registry(), mio::Token(sockets.len()))?;

        Ok(Group {
            waker,
            group,
            laddrs: addrs,
            state: Mutex::new(PollState {
                acceptor,
                poll,
                sockets,
            }),
        })
    }

    /// Wrap and register a new `quiche::Connection`.
    #[inline]
    pub fn register(&self, wrapped: quiche::Connection) -> Result<Token> {
        let token = self.group.register(wrapped)?;

        self.waker.wake()?;

        Ok(token)
    }

    /// Unwrap a bound `quiche::Connection`
    #[inline]
    pub fn deregister(&self, token: Token) -> Result<quiche::Connection> {
        let conn = self.group.deregister(token)?;

        self.waker.wake()?;

        Ok(conn)
    }

    /// Close one wrapped `quiche::Connection`
    #[inline]
    pub fn close(
        &self,
        token: Token,
        app: bool,
        err: u64,
        reason: Cow<'static, [u8]>,
    ) -> Result<()> {
        self.group.close(token, app, err, reason)?;

        self.waker.wake()?;

        Ok(())
    }

    /// Open a new outbound stream.
    pub fn stream_open(&self, token: Token, kind: StreamKind) -> Result<u64> {
        let stream_id = self.group.stream_open(token, kind)?;

        self.waker.wake()?;

        Ok(stream_id)
    }

    /// Shutdown a stream.
    #[inline]
    pub fn stream_shutdown(&self, token: Token, stream_id: u64, err: u64) -> Result<()> {
        self.group.stream_shutdown(token, stream_id, err)?;

        self.waker.wake()?;

        Ok(())
    }

    /// Writes data to a stream.
    #[inline]
    pub fn stream_send(
        &self,
        token: Token,
        stream_id: u64,
        buf: &[u8],
        fin: bool,
    ) -> Result<usize> {
        let send_size = self.group.stream_send(token, stream_id, buf, fin)?;

        self.waker.wake()?;

        Ok(send_size)
    }

    /// Reads contiguous data from a stream into the provided slice.
    #[inline]
    pub fn stream_recv(
        &self,
        token: Token,
        stream_id: u64,
        buf: &mut [u8],
    ) -> Result<(usize, bool)> {
        let r = self.group.stream_recv(token, stream_id, buf)?;

        self.waker.wake()?;

        Ok(r)
    }

    /// Waits for readiness events.
    pub fn poll(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> Result<()> {
        let deadline = timeout.map(|timeout| Instant::now() + timeout);

        let mut poll_state = self.state.lock();

        loop {
            let next_release_time = self.group.poll(events);

            // filter events: `Send` and `Recv`.
            for event in events.drain(..).collect::<Vec<_>>() {
                match event.kind {
                    EventKind::Send => {
                        self.on_quic_send(&mut poll_state, event.token)?;
                    }
                    EventKind::Recv => {
                        self.on_quic_recv(&mut poll_state, event.token)?;
                    }
                    _ => events.push(event),
                }
            }

            // Readiness `events` is not empty, returns immediately.
            if !events.is_empty() {
                return Ok(());
            }

            // check if timeout.
            if let Some(deadline) = deadline {
                if !(deadline > Instant::now()) {
                    return Ok(());
                }
            }

            self.mio_poll_once(&mut poll_state, min_of_some(deadline, next_release_time))?;
        }
    }

    fn mio_poll_once(&self, poll_state: &mut PollState, deadline: Option<Instant>) -> Result<()> {
        let timeout = if let Some(next_release_time) = deadline {
            next_release_time.checked_duration_since(Instant::now())
        } else {
            None
        };

        let mut events = Events::with_capacity(1024);

        poll_state
            .poll
            .poll(&mut events, timeout)
            .inspect_err(|err| log::error!("mio poll error: {}", err))?;

        for event in events.iter() {
            log::trace!("readiness, event={:?}", event);

            let token = event.token();

            // Event to wakeup this `Poll`, skip it!!
            if token.0 == poll_state.sockets.len() {
                continue;
            }

            if event.is_readable() {
                self.on_udp_recv(poll_state, token)?;
            }

            if event.is_writable() {
                self.on_udp_send(poll_state, token)?;
            }
        }

        Ok(())
    }

    fn on_quic_send(&self, poll_state: &mut PollState, token: Token) -> Result<()> {
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
            .laddrs
            .get(&send_info.from)
            .cloned()
            .expect("Quic socket");

        let quic_socket = poll_state.sockets.get_mut(index).expect("Quic socket");

        let len = match quic_socket.send_to(buf, send_info.to) {
            Ok(len) => len,
            Err(QuicSocketError::IsFull(_)) => {
                log::warn!("udp send queue is full, socket=Token({})", index);
                return Ok(());
            }
            Err(err) => return Err(err.into()),
        };

        log::trace!("quic socket sending fifo, len={}", len);

        Ok(())
    }

    #[inline]
    fn on_quic_recv(&self, _: &mut PollState, _: Token) -> Result<()> {
        Ok(())
    }

    fn on_udp_recv(&self, poll_state: &mut PollState, token: mio::Token) -> Result<()> {
        let quic_socket = poll_state.sockets.get_mut(token.0).expect("Quic socket");

        let parker = Parker::new();

        loop {
            let mut buf = QuicBuf::new();

            let Poll::Ready(from) = quic_socket.recv_from(&mut buf).would_block()? else {
                return Ok(());
            };

            let read_size = buf.readable();

            if let Some(acceptor) = &mut poll_state.acceptor {
                // for server-side dispatching.
                loop {
                    match self.group.server_dispatch(
                        acceptor,
                        buf.writable_buf(),
                        read_size,
                        RecvInfo {
                            from,
                            to: quic_socket.local_addr(),
                        },
                        Some(parker.unparker().clone()),
                    ) {
                        Ok((send_size, send_info)) => {
                            if send_size == 0 {
                                // handle next udp packet.
                                break;
                            }

                            buf.writable_consume(send_size);

                            match quic_socket.send_to(buf, send_info.to) {
                                Ok(_) => {}
                                Err(QuicSocketError::IsFull(_)) => {
                                    log::warn!(
                                        "`QuicSocket` sending queue is full, socket={}",
                                        token.0
                                    );
                                }
                                Err(err) => return Err(err.into()),
                            }
                        }
                        Err(crate::poll::Error::Busy) | Err(crate::poll::Error::Retry) => {
                            parker.park();
                            // try agian.
                            continue;
                        }
                        Err(_) => {}
                    }

                    break;
                }
            } else {
                let header =
                    quiche::Header::from_slice(buf.readable_buf_mut(), quiche::MAX_CONN_ID_LEN)
                        .map_err(crate::poll::Error::Quiche)?;

                // for client-side dispatching.
                loop {
                    match self.group.recv_(
                        &header.dcid,
                        buf.readable_buf_mut(),
                        RecvInfo {
                            from,
                            to: quic_socket.local_addr(),
                        },
                        Some(parker.unparker().clone()),
                    ) {
                        Ok(_) => {}
                        // Current connection is busy.
                        Err(crate::poll::Error::Busy) | Err(crate::poll::Error::Retry) => {
                            parker.park();
                            // try agian.
                            continue;
                        }
                        Err(_) => {}
                    }
                }
            }
        }
    }

    fn on_udp_send(&self, poll_state: &mut PollState, token: mio::Token) -> Result<()> {
        let socket = poll_state.sockets.get_mut(token.0).expect("Quic socket");

        // try flush pending packets.
        _ = socket.flush().would_block()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use quiche::Config;

    use crate::poll::server::SimpleAddressValidator;

    use super::*;

    #[allow(unused)]
    fn mock_config(is_server: bool) -> Config {
        use std::path::Path;

        let mut config = Config::new(quiche::PROTOCOL_VERSION).unwrap();

        config.set_initial_max_data(10_000_000);
        config.set_initial_max_stream_data_bidi_local(1024 * 1024);
        config.set_initial_max_stream_data_bidi_remote(1024 * 1024);
        config.set_initial_max_stream_data_uni(1024 * 1024);
        config.set_initial_max_streams_bidi(1);
        config.set_initial_max_streams_uni(1);

        config.verify_peer(true);

        // if is_server {
        let root_path = Path::new(env!("CARGO_MANIFEST_DIR"));

        log::debug!("test run dir {:?}", root_path);

        if is_server {
            config
                .load_cert_chain_from_pem_file(root_path.join("cert/server.crt").to_str().unwrap())
                .unwrap();

            config
                .load_priv_key_from_pem_file(root_path.join("cert/server.key").to_str().unwrap())
                .unwrap();
        } else {
            config
                .load_cert_chain_from_pem_file(root_path.join("cert/client.crt").to_str().unwrap())
                .unwrap();

            config
                .load_priv_key_from_pem_file(root_path.join("cert/client.key").to_str().unwrap())
                .unwrap();
        }

        config
            .load_verify_locations_from_file(root_path.join("cert/rasi_ca.pem").to_str().unwrap())
            .unwrap();

        config.set_application_protos(&[b"test"]).unwrap();

        config.set_max_idle_timeout(60000);

        config
    }

    #[test]
    fn test_poll_timeout() {
        let group = Group::bind(
            "127.0.0.1:0",
            Some(Acceptor::new(
                mock_config(true),
                SimpleAddressValidator::new(Duration::from_secs(10)),
            )),
        )
        .unwrap();

        let mut events = vec![];

        group
            .poll(&mut events, Some(Duration::from_millis(50)))
            .unwrap();

        assert_eq!(events.len(), 0);

        let event = Event {
            token: Token(0),
            kind: EventKind::Accept,
            is_server: false,
            is_error: false,
            stream_id: 1,
        };

        group
            .group
            .readiness(event, Some(Instant::now() + Duration::from_millis(100)));

        group
            .poll(&mut events, Some(Duration::from_millis(50)))
            .unwrap();

        assert_eq!(events.len(), 0);

        group
            .poll(&mut events, Some(Duration::from_millis(50)))
            .unwrap();

        assert_eq!(events[0], event);
    }
}
