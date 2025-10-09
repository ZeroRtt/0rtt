use std::{io::Result, net::ToSocketAddrs, sync::Arc};

use mio::{Interest, Token, Waker, net::UdpSocket};

use crate::{mio::udp::QuicSocket, poll::server::Acceptor};

/// Facade to access `QUIC` group.
#[derive(Clone)]
pub struct Group {
    /// A waker of `mio::Poll`.
    waker: Arc<mio::Waker>,
    /// quic group.
    group: Arc<crate::poll::Group>,
}

impl Group {
    /// Create a new `Group` and bind to `laddrs`.
    pub fn bind<S>(laddrs: S, acceptor: Option<Acceptor>) -> Result<(Self, GroupWorker)>
    where
        S: ToSocketAddrs,
    {
        let poll = mio::Poll::new()?;
        let group = Arc::new(crate::poll::Group::new());

        let mut sockets = vec![];

        for laddr in laddrs.to_socket_addrs()? {
            let mut socket = UdpSocket::bind(laddr)?;

            poll.registry().register(
                &mut socket,
                mio::Token(sockets.len()),
                Interest::READABLE | Interest::WRITABLE,
            )?;

            sockets.push(QuicSocket::new(socket, 1024)?);
        }

        let waker = Arc::new(Waker::new(poll.registry(), Token(sockets.len()))?);

        Ok((
            Group {
                waker,
                group: group.clone(),
            },
            GroupWorker {
                poll,
                sockets,
                group: group.clone(),
                acceptor,
            },
        ))
    }
}

/// Background worker for [`Group`].
pub struct GroupWorker {
    /// mio poller.
    poll: mio::Poll,
    /// `UDP` sockets bound to this group.
    sockets: Vec<QuicSocket>,
    /// quic group.
    group: Arc<crate::poll::Group>,
    /// server-side connection acceptor.
    acceptor: Option<Acceptor>,
}
