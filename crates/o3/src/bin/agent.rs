use std::{
    collections::HashMap,
    io::{Error, Result},
    net::{Ipv6Addr, SocketAddr},
    thread::sleep,
    time::Duration,
};

use clap::Parser;
use color_print::ceprintln;
use metricrs::global::set_global_registry;
use metricrs_protobuf::{
    fetch::Fetch,
    protos::memory::{Metadata, Query},
    registry::ProtoBufRegistry,
};
use o3::{
    agent::Agent,
    cli::{Cli, Commands},
};
use zrquic::quiche;

struct LoopFetch {
    fetch: Fetch,
    verson: u64,
    metadatas: HashMap<u64, Metadata>,
}

impl LoopFetch {
    pub fn connect(remote: SocketAddr) -> Result<Self> {
        Ok(Self {
            fetch: Fetch::connect(remote)?,
            verson: 0,
            metadatas: Default::default(),
        })
    }

    pub fn run(&mut self) -> Result<()> {
        loop {
            self.fetch_once()?;
            sleep(Duration::from_secs(10));
        }
    }

    fn fetch_once(&mut self) -> Result<()> {
        let query_result = self.fetch.query(Query {
            version: self.verson,
            ..Default::default()
        })?;

        if query_result.version > self.verson {
            self.verson = query_result.version;

            for metadata in query_result.metadatas {
                self.metadatas.clear();
                self.metadatas.insert(metadata.hash, metadata);
            }
        }

        for value in query_result.values {
            if let Some(metadata) = self.metadatas.get(&value.hash) {
                let labels = metadata
                    .labels
                    .iter()
                    .map(|label| format!("{}={}", label.key, label.value))
                    .collect::<Vec<_>>()
                    .join(",");

                match metadata.instrument.enum_value_or_default() {
                    metricrs_protobuf::protos::memory::Instrument::COUNTER => {
                        log::info!(target: "metrics", "{}, {}, value={}",  metadata.name,labels, value.value);
                    }
                    _ => {
                        log::info!(target: "metrics", "{}, {}, value={}",  metadata.name,labels, f64::from_bits(value.value));
                    }
                };
            } else {
                log::info!(target: "metrics", "UNKNOWN, hash={}, value={}", value.hash,  value.value);
            }
        }

        Ok(())
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let cli = Cli::parse();

    if let Err(err) = run(cli).await {
        ceprintln!("<s><r>error:</r></s> {}", err)
    }
}

async fn run(cli: Cli) -> Result<()> {
    if cli.debug {
        pretty_env_logger::try_init_timed().map_err(Error::other)?;
    }

    if let Some(laddr) = cli.metrics {
        let registry = ProtoBufRegistry::bind(laddr)?;
        let laddr = registry.local_addr();
        set_global_registry(registry).unwrap();

        let mut fetch = LoopFetch::connect(laddr)?;

        std::thread::spawn(move || {
            if let Err(err) = fetch.run() {
                log::error!("background metrics worker is stopped, {}", err);
            }
        });
    }

    #[allow(irrefutable_let_patterns)]
    if let Commands::Listen { on } = cli.commands {
        let on = on.unwrap_or_else(|| (Ipv6Addr::UNSPECIFIED, 0).into());

        let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();

        cli.quiche_config(&mut config)?;

        let agent = Agent::new(on, cli.parse_o3_server_addrs()?, config).await?;

        agent.run().await?;
    }

    Ok(())
}
