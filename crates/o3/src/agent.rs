//! client-side tunnel protocol implementation

use std::{
    collections::HashMap,
    io::Result,
    net::{Ipv6Addr, SocketAddr},
    sync::Arc,
};

use parking_lot::{Mutex, RwLock};
use rand::seq::SliceRandom;
use tokio::{
    io::copy,
    net::{TcpListener, TcpStream},
};
use zrquic::{
    futures::{Group, QuicConn, QuicConnector, QuicStream},
    poll::{StreamKind, Token},
    quiche,
};

#[allow(unused)]
/// Forward agent, passes tcp traffic through quic tunnel.
pub struct Agent {
    /// Listen incoming tcp streams.
    listener: TcpListener,
    /// Pooled connector for `O3` servers.
    connector: Arc<PoolConnector>,
}

impl Agent {
    /// Create a new agent.
    pub async fn new(
        local: SocketAddr,
        remote: Vec<SocketAddr>,
        config: quiche::Config,
    ) -> Result<Self> {
        let group = Group::bind((Ipv6Addr::UNSPECIFIED, 0), None)?;
        let local_addr = group.local_addrs().copied().next().unwrap();

        Ok(Self {
            listener: TcpListener::bind(local).await?,
            connector: Arc::new(PoolConnector {
                config: Mutex::new(config),
                local_addr,
                remote_server_addrs: remote,
                connector: QuicConnector::from(group),
                active_conns: Default::default(),
            }),
        })
    }

    /// Run the main event loop of this `Agent`.
    pub async fn run(self) -> Result<()> {
        loop {
            let (stream, raddr) = self.listener.accept().await?;

            let connector = self.connector.clone();
            // Handle incoming TCP streams
            tokio::spawn(async move {
                log::info!("handle incoming, from={}", raddr);

                if let Err(err) = connector.handle_incoming_tcp_stream(stream, raddr).await {
                    log::error!("handle incoming, from={}, err={}", raddr, err);
                }
            });
        }
    }
}

struct PoolConnector {
    /// local address the `QUIC` socket bound to.
    local_addr: SocketAddr,
    /// `O3` server addresses.
    remote_server_addrs: Vec<SocketAddr>,
    /// Shared `QUIC` configuration.
    config: Mutex<quiche::Config>,
    /// Group of quic connections.
    connector: QuicConnector,
    /// Active `QUIC` connections.
    active_conns: RwLock<HashMap<Token, Arc<QuicConn>>>,
}

impl PoolConnector {
    async fn handle_incoming_tcp_stream(
        self: Arc<Self>,
        input: TcpStream,
        raddr: SocketAddr,
    ) -> Result<()> {
        let oread = Arc::new(self.open_stream().await?);
        let owrite = oread.clone();

        let (mut iread, mut iwrite) = input.into_split();

        tokio::spawn(async move {
            log::trace!("create pipeline, from={}, to={}", raddr, owrite);
            match copy(&mut iread, &mut owrite.as_ref()).await {
                Ok(data) => {
                    log::trace!(
                        "pipeline is closed, from={}, to={}, len={}",
                        raddr,
                        owrite,
                        data
                    );
                }
                Err(err) => {
                    log::trace!(
                        "pipeline is closed, from={}, to={}, err={}",
                        raddr,
                        owrite,
                        err
                    );
                }
            }
        });

        tokio::spawn(async move {
            log::trace!("create pipeline, from={}, to={}", oread, raddr);
            match copy(&mut oread.as_ref(), &mut iwrite).await {
                Ok(data) => {
                    log::trace!(
                        "pipeline is closed, from={}, to={}, len={}",
                        oread,
                        raddr,
                        data
                    );
                }
                Err(err) => {
                    log::error!(
                        "pipeline is closed, from={}, to={}, err={}",
                        oread,
                        raddr,
                        err
                    );
                }
            }
        });

        Ok(())
    }

    async fn open_stream(self: &Arc<Self>) -> Result<QuicStream> {
        loop {
            let mut invalid_conns = vec![];

            let mut stream = None;

            let active_conns = self
                .active_conns
                .read()
                .values()
                .cloned()
                .collect::<Vec<_>>();

            for conn in active_conns {
                match conn.open(StreamKind::Bidi, true).await {
                    Ok(opened) => {
                        stream = Some(opened);
                        break;
                    }
                    Err(_) => {
                        invalid_conns.push(conn.token());
                    }
                }
            }

            {
                let mut active_conns = self.active_conns.write();

                for token in invalid_conns {
                    log::info!("remove connection, {:?}", token);
                    active_conns.remove(&token);
                }
            }

            // Successfully opened a new outbound stream !!
            if let Some(stream) = stream {
                return Ok(stream);
            }

            let mut remote_server_addrs = self.remote_server_addrs.clone();
            remote_server_addrs.shuffle(&mut rand::rng());

            let conn = self.clone().connector.create(
                None,
                self.local_addr,
                remote_server_addrs[0],
                // blocking other `open_stream` ops.
                &mut self.config.lock(),
            )?;

            conn.is_established().await?;

            self.active_conns
                .write()
                .insert(conn.token(), Arc::new(conn));
        }
    }
}
