use std::{time::Duration, vec};

use quiche::{Config, RecvInfo};
use quico::{Error, EventKind, Group, acceptor::Acceptor, validation::SimpleAddressValidator};

#[allow(unused)]
fn mock_config(is_server: bool) -> Config {
    use std::path::Path;

    let mut config = Config::new(quiche::PROTOCOL_VERSION).unwrap();

    config.set_initial_max_data(10_000_000);
    config.set_initial_max_stream_data_bidi_local(1024 * 1024);
    config.set_initial_max_stream_data_bidi_remote(1024 * 1024);
    config.set_initial_max_streams_bidi(3);
    config.set_initial_max_streams_uni(100);

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
fn test_connected() {
    // pretty_env_logger::init_timed();

    let group = Group::new();

    let mut acceptor = Acceptor::new(
        mock_config(true),
        SimpleAddressValidator::new(Duration::from_secs(1)),
    );

    let mut client_config = mock_config(false);

    let laddr = "127.0.0.1:1".parse().unwrap();
    let raddr = "127.0.0.1:2".parse().unwrap();

    let client = group
        .connect(None, laddr, raddr, &mut client_config)
        .unwrap();

    let mut buf = vec![0; 1600];

    loop {
        let mut events = vec![];

        group.poll(&mut events, None).unwrap();

        for event in events {
            if event.kind == EventKind::Connected {
                return;
            }

            if event.kind == EventKind::Send {
                let (send_size, send_info) = match group.send(event.token, &mut buf) {
                    Ok(r) => r,
                    Err(Error::Retry) => {
                        continue;
                    }
                    Err(err) => panic!("{}", err),
                };

                if event.token == client {
                    let (send_size, send_info) = group
                        .accept_recv(
                            &mut acceptor,
                            &mut buf,
                            send_size,
                            RecvInfo {
                                from: send_info.from,
                                to: send_info.to,
                            },
                        )
                        .unwrap();

                    if send_size > 0 {
                        group
                            .recv(
                                &mut buf[..send_size],
                                RecvInfo {
                                    from: send_info.from,
                                    to: send_info.to,
                                },
                            )
                            .unwrap();
                    }
                } else {
                    group
                        .recv(
                            &mut buf[..send_size],
                            RecvInfo {
                                from: send_info.from,
                                to: send_info.to,
                            },
                        )
                        .unwrap();
                }
            }
        }
    }
}

#[test]
fn test_accept() {
    // pretty_env_logger::init_timed();

    let group = Group::new();

    let mut acceptor = Acceptor::new(
        mock_config(true),
        SimpleAddressValidator::new(Duration::from_secs(1)),
    );

    let mut client_config = mock_config(false);

    let laddr = "127.0.0.1:1".parse().unwrap();
    let raddr = "127.0.0.1:2".parse().unwrap();

    let client = group
        .connect(None, laddr, raddr, &mut client_config)
        .unwrap();

    let mut buf = vec![0; 1600];

    loop {
        let mut events = vec![];

        group.poll(&mut events, None).unwrap();

        for event in events {
            if event.kind == EventKind::Accept {
                return;
            }

            if event.kind == EventKind::Send {
                let (send_size, send_info) = match group.send(event.token, &mut buf) {
                    Ok(r) => r,
                    Err(Error::Retry) => {
                        continue;
                    }
                    Err(err) => panic!("{}", err),
                };

                if event.token == client {
                    let (send_size, send_info) = group
                        .accept_recv(
                            &mut acceptor,
                            &mut buf,
                            send_size,
                            RecvInfo {
                                from: send_info.from,
                                to: send_info.to,
                            },
                        )
                        .unwrap();

                    if send_size > 0 {
                        group
                            .recv(
                                &mut buf[..send_size],
                                RecvInfo {
                                    from: send_info.from,
                                    to: send_info.to,
                                },
                            )
                            .unwrap();
                    }
                } else {
                    group
                        .recv(
                            &mut buf[..send_size],
                            RecvInfo {
                                from: send_info.from,
                                to: send_info.to,
                            },
                        )
                        .unwrap();
                }
            }
        }
    }
}
