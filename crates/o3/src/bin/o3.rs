use std::{io::Error, time::Duration};

use clap::Parser;
use color_print::ceprintln;
use o3::{
    cli::{Cli, Commands},
    errors::Result,
    redirect::Redirect,
};
use zrquic::{Acceptor, SimpleAddressValidator, quiche};

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
    if let Commands::Redirect { target } = cli.commands {
        let mut config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();

        cli.quiche_config(&mut config)?;

        let rproxy = Redirect::new(
            cli.parse_redirect_listen_addrs()?,
            target,
            Acceptor::new(config, SimpleAddressValidator::new(Duration::from_secs(60))),
            cli.ring_buffer_size,
        )?;

        rproxy.run()?;
    }

    Ok(())
}
