//! Asynchronous Runtime Binding for `QUIC`.

use std::{
    collections::{HashMap, VecDeque},
    io::Result,
    net::ToSocketAddrs,
    sync::Arc,
    task::Waker,
};

use parking_lot::Mutex;

use crate::poll::{StreamKind, Token, server::Acceptor};

/// Event types used by `Group` inner.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
enum Event {
    Accept,
    Connected,
    StreamAccept,
    StreamOpenBidi,
    StreamOpenUni,
    StreamSend(u64),
    StreamRecv(u64),
}

/// Group inner mutable state.
#[derive(Default)]
struct State {
    wakers: HashMap<Token, HashMap<Event, Waker>>,
    /// incoming connections first seen.
    incoming_conns: VecDeque<Token>,
    /// incoming streams first seen.
    incoming_streams: HashMap<Token, VecDeque<u64>>,
}

pub struct GroupWorker {
    group: crate::mio::Group,
    state: Mutex<State>,
}

/// Asynchronous Runtime Binding for `QUIC` group.
#[derive(Clone)]
pub struct Group(Arc<GroupWorker>);

impl Group {
    /// Create a new `Group` and bind to `laddrs`.
    pub fn bind<S>(laddrs: S, acceptor: Option<Acceptor>) -> Result<Self>
    where
        S: ToSocketAddrs,
    {
        Ok(Self(Arc::new(GroupWorker {
            group: crate::mio::Group::bind(laddrs, acceptor)?,
            state: Default::default(),
        })))
    }

    /// Consume self and execute `poll events` dispatch loop
    pub fn run(self) -> Result<()> {
        loop {
            self.run_once()?;
        }
    }

    fn run_once(&self) -> Result<()> {
        let mut events = vec![];

        self.0.group.poll(&mut events, None)?;

        let mut state = self.0.state.lock();

        for event in events {
            match event.kind {
                crate::poll::EventKind::Connected => self.on_connected(&mut state, event.token)?,
                crate::poll::EventKind::Accept => self.on_accept(&mut state, event.token)?,
                crate::poll::EventKind::Closed => self.on_closed(&mut state, event.token)?,
                crate::poll::EventKind::StreamOpenBidi => {
                    self.on_stream_open(&mut state, event.token, StreamKind::Bidi)?
                }
                crate::poll::EventKind::StreamOpenUni => {
                    self.on_stream_open(&mut state, event.token, StreamKind::Uni)?
                }
                crate::poll::EventKind::StreamAccept => {
                    self.on_stream_accept(&mut state, event.token, event.stream_id)?
                }
                crate::poll::EventKind::StreamSend => {
                    self.on_stream_send(&mut state, event.token, event.stream_id)?
                }
                crate::poll::EventKind::StreamRecv => {
                    self.on_stream_recv(&mut state, event.token, event.stream_id)?
                }
                _ => unreachable!("illegal event: {:?}", event),
            }
        }

        Ok(())
    }

    #[inline]
    fn wake(&self, state: &mut State, token: Token, event: Event) {
        if let Some(conn) = state.wakers.get_mut(&token) {
            if let Some(waker) = conn.remove(&event) {
                waker.wake();
            }
        }
    }

    #[inline]
    fn on_closed(&self, state: &mut State, token: Token) -> Result<()> {
        // wakeup all pending tasks.
        if let Some(conn) = state.wakers.remove(&token) {
            for waker in conn.into_values() {
                waker.wake();
            }
        }

        // remove `Stream` incomoing pipeline.
        state.incoming_streams.remove(&token);

        Ok(())
    }

    #[inline]
    fn on_connected(&self, state: &mut State, token: Token) -> Result<()> {
        self.wake(state, token, Event::Connected);
        Ok(())
    }

    #[inline]
    fn on_accept(&self, state: &mut State, token: Token) -> Result<()> {
        state.incoming_conns.push_back(token);
        self.wake(state, token, Event::Accept);

        Ok(())
    }

    #[inline]
    fn on_stream_open(&self, state: &mut State, token: Token, kind: StreamKind) -> Result<()> {
        self.wake(
            state,
            token,
            if kind == StreamKind::Bidi {
                Event::StreamOpenBidi
            } else {
                Event::StreamOpenUni
            },
        );

        Ok(())
    }

    #[inline]
    fn on_stream_accept(&self, state: &mut State, token: Token, stream_id: u64) -> Result<()> {
        state
            .incoming_streams
            .entry(token)
            .or_insert_with(|| {
                let mut incoming = VecDeque::new();

                incoming.push_back(stream_id);

                incoming
            })
            .push_back(stream_id);

        self.wake(state, token, Event::StreamAccept);

        Ok(())
    }

    #[inline]
    fn on_stream_send(&self, state: &mut State, token: Token, stream_id: u64) -> Result<()> {
        self.wake(state, token, Event::StreamSend(stream_id));
        Ok(())
    }

    #[inline]
    fn on_stream_recv(&self, state: &mut State, token: Token, stream_id: u64) -> Result<()> {
        self.wake(state, token, Event::StreamRecv(stream_id));
        Ok(())
    }
}
