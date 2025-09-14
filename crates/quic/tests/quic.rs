use std::{time::Duration, vec};

use quiche::{Config, RecvInfo};
use zrquic::{Acceptor, Error, EventKind, Group, SimpleAddressValidator, Token};

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
fn test_multi_conn() {
    // pretty_env_logger::init_timed();

    let group = Group::new();

    let mut acceptor = Acceptor::new(
        mock_config(true),
        SimpleAddressValidator::new(Duration::from_secs(1)),
    );

    let mut client_config = mock_config(false);

    let laddr = "127.0.0.1:1".parse().unwrap();
    let raddr = "127.0.0.1:2".parse().unwrap();

    let mut client = group
        .connect(None, laddr, raddr, &mut client_config)
        .unwrap();

    let mut buf = vec![0; 1600];

    let mut i = 0;

    loop {
        let mut events = vec![];

        group.poll(&mut events, None).unwrap();

        for event in events {
            if event.kind == EventKind::Connected {
                i += 1;

                if i > 100 {
                    return;
                }

                client = group
                    .connect(None, laddr, raddr, &mut client_config)
                    .unwrap();
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

#[test]
fn test_stream_open() {
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

    assert_eq!(
        group
            .stream_open(client)
            .expect_err("stream_open before connected."),
        Error::Retry
    );

    let mut buf = vec![0; 1600];

    loop {
        let mut events = vec![];

        log::trace!("poll events");

        group.poll(&mut events, None).unwrap();

        log::trace!("poll events: {:?}", events);

        for event in events {
            if event.kind == EventKind::Connected {
                let stream_id = group.stream_open(client).unwrap();

                group
                    .stream_send(client, stream_id, b"hello", true)
                    .unwrap();
            }

            if event.kind == EventKind::StreamRecv || event.kind == EventKind::StreamAccept {
                let mut buf = vec![0; 1300];
                let (read_size, fin) = group
                    .stream_recv(event.token, event.stream_id, &mut buf)
                    .unwrap();

                assert_eq!(&buf[..read_size], b"hello");
                assert!(fin);
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
fn test_stream_open_limits() {
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

    assert_eq!(
        group
            .stream_open(client)
            .expect_err("stream_open before connected."),
        Error::Retry
    );

    let mut buf = vec![0; 1600];

    let mut i = 0;

    loop {
        let mut events = vec![];

        log::trace!("poll events");

        group.poll(&mut events, None).unwrap();

        log::trace!("poll events: {:?}", events);

        for event in events {
            if event.kind == EventKind::Connected {
                let stream_id = group.stream_open(client).unwrap();

                group
                    .stream_send(client, stream_id, b"hello", true)
                    .unwrap();
            }

            if (event.kind == EventKind::StreamRecv || event.kind == EventKind::StreamAccept)
                && event.token != client
            {
                i += 1;
                let mut buf = vec![0; 1300];
                let (read_size, fin) = group
                    .stream_recv(event.token, event.stream_id, &mut buf)
                    .unwrap();

                assert_eq!(&buf[..read_size], b"hello");
                assert!(fin);

                group
                    .stream_send(event.token, event.stream_id, b"", true)
                    .unwrap();

                if i > 1000 {
                    return;
                }

                let stream_id = match group.stream_open(client) {
                    Ok(stream_id) => stream_id,
                    Err(Error::Retry) => {
                        continue;
                    }
                    Err(err) => panic!("{}", err),
                };

                group
                    .stream_send(client, stream_id, b"hello", true)
                    .unwrap();
            }

            if event.kind == EventKind::StreamOpen && event.token == client {
                let stream_id = match group.stream_open(event.token) {
                    Ok(stream_id) => stream_id,
                    Err(Error::Retry) => {
                        continue;
                    }
                    Err(err) => panic!("{}", err),
                };

                group
                    .stream_send(event.token, stream_id, b"hello", true)
                    .unwrap();
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
fn test_server_side_stream_open() {
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

    assert_eq!(
        group
            .stream_open(client)
            .expect_err("stream_open before connected."),
        Error::Retry
    );

    let mut buf = vec![0; 1600];

    let mut i = 0;

    let mut server = Token(0);

    loop {
        let mut events = vec![];

        log::trace!("poll events");

        group.poll(&mut events, None).unwrap();

        log::trace!("poll events: {:?}", events);

        for event in events {
            if event.kind == EventKind::Accept {
                server = event.token;

                let stream_id = group.stream_open(event.token).unwrap();

                group
                    .stream_send(event.token, stream_id, b"hello", true)
                    .unwrap();
            }

            if (event.kind == EventKind::StreamRecv || event.kind == EventKind::StreamAccept)
                && event.token == client
            {
                i += 1;
                let mut buf = vec![0; 1300];
                let (read_size, fin) = group
                    .stream_recv(event.token, event.stream_id, &mut buf)
                    .unwrap();

                assert_eq!(&buf[..read_size], b"hello", "{}", i);
                assert!(fin);

                group
                    .stream_send(event.token, event.stream_id, b"", true)
                    .unwrap();

                if i > 1000 {
                    return;
                }

                let stream_id = match group.stream_open(server) {
                    Ok(stream_id) => stream_id,
                    Err(Error::Retry) => {
                        continue;
                    }
                    Err(err) => panic!("{}", err),
                };

                group
                    .stream_send(server, stream_id, b"hello", true)
                    .unwrap();
            }

            if event.kind == EventKind::StreamOpen && event.token == server {
                let stream_id = match group.stream_open(event.token) {
                    Ok(stream_id) => stream_id,
                    Err(Error::Retry) => {
                        continue;
                    }
                    Err(err) => panic!("{}", err),
                };

                group
                    .stream_send(server, stream_id, b"hello", true)
                    .unwrap();
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
