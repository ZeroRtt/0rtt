use std::{
    collections::HashMap,
    io::Result,
    net::{SocketAddr, ToSocketAddrs},
    sync::Arc,
    task::Waker,
    thread::{JoinHandle, spawn},
};

use mio::{Interest, net::UdpSocket};
use parking_lot::Mutex;
use zrquic::{Acceptor, Event, Group as Group_};

use crate::net::quic::udp::QuicSocket;

/// A quic group for asynchronous runtimes.
#[derive(Clone)]
#[allow(unused)]
pub struct Group {
    group: Arc<Group_>,
    wakers: Arc<Mutex<HashMap<Event, Waker>>>,
}

impl Group {
    /// Create a new `QUIC` group and bind it to `addrs`
    pub fn bind<S>(addrs: S, acceptor: Option<Acceptor>) -> Result<(Group, JoinHandle<Result<()>>)>
    where
        S: ToSocketAddrs,
    {
        let poll = mio::Poll::new()?;

        let mut laddrs = HashMap::new();
        let mut sockets = vec![];

        for (index, laddr) in addrs.to_socket_addrs()?.enumerate() {
            let mut socket = UdpSocket::bind(laddr)?;
            laddrs.insert(socket.local_addr()?, index);
            poll.registry().register(
                &mut socket,
                mio::Token(index),
                Interest::READABLE | Interest::WRITABLE,
            )?;
            sockets.push(QuicSocket::new(socket, 1024)?);
        }

        let group = Arc::new(Group_::new());
        let wakers = Default::default();

        let worker = GroupWorker {
            group,
            wakers,
            sockets,
            laddrs,
            poll,
            acceptor,
        };

        let group = Group {
            group: worker.group.clone(),
            wakers: worker.wakers.clone(),
        };

        let handle = spawn(|| {
            if let Err(err) = worker.run() {
                log::error!("Quic background: {}", err);
                Err(err)
            } else {
                Ok(())
            }
        });

        Ok((group, handle))
    }
}

#[allow(unused)]
struct GroupWorker {
    group: Arc<Group_>,
    wakers: Arc<Mutex<HashMap<Event, Waker>>>,
    sockets: Vec<QuicSocket>,
    laddrs: HashMap<SocketAddr, usize>,
    poll: mio::Poll,
    acceptor: Option<Acceptor>,
}

impl GroupWorker {
    fn run(mut self) -> Result<()> {
        loop {
            self.run_once()?;
        }
    }

    fn run_once(&mut self) -> Result<()> {
        todo!()
    }
}
