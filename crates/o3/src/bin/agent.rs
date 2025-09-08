use std::{io::Error, net::Ipv6Addr};

use clap::Parser;
use color_print::ceprintln;
use o3::{
    agent::Agent,
    cli::{Cli, Commands},
    errors::Result,
};
use quico::quiche;

fn main() {
    let cli = Cli::parse();

    if let Err(err) = run(cli) {
        ceprintln!("<s><r>error:</r></s> {}", err)
    }
}

fn run(cli: Cli) -> Result<()> {
    if cli.debug {
        pretty_env_logger::try_init_timed().map_err(Error::other)?;
    }

    #[allow(irrefutable_let_patterns)]
    if let Commands::Listen { on } = cli.commands {
        let on = on.unwrap_or_else(|| (Ipv6Addr::UNSPECIFIED, 0).into());

        let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();

        cli.quiche_config(&mut config)?;

        let agent = Agent::new(on, cli.parse_o3_server_addrs()?, config, 1024 * 1024)?;

        agent.run()?;
    }

    Ok(())
}
