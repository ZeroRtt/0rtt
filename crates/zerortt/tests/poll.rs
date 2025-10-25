use std::{net::SocketAddr, task::Poll, time::Duration};

use fixedbuf::ArrayBuf;
use zerortt::quiche::{self, Config, RecvInfo};
use zerortt::{
    Acceptor, Error, EventKind, Group, QuicClient, QuicPoll, QuicServerTransport, QuicTransport,
    Result, SimpleAddressValidator, StreamKind, Token, WouldBlock,
};

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

type QuicBuf = ArrayBuf<1600>;

#[inline]
fn make_acceptor() -> Acceptor {
    Acceptor::new(
        mock_config(true),
        SimpleAddressValidator::new(Duration::from_secs(1)),
    )
}

#[inline]
fn make_addr_pair() -> (SocketAddr, SocketAddr) {
    let laddr = "127.0.0.1:1".parse().unwrap();
    let raddr = "127.0.0.1:2".parse().unwrap();

    (laddr, raddr)
}

#[inline]
fn transfer(acceptor: &mut Acceptor, group: &Group, from: Token) -> Poll<Result<()>> {
    let mut buf = QuicBuf::new();

    let Poll::Ready((mut send_size, mut send_info)) =
        group.send(from, buf.writable_buf()).would_block()?
    else {
        return Poll::Pending;
    };

    buf.writable_consume(send_size);

    while send_size > 0 {
        (send_size, send_info) = group
            .recv_with_acceptor(
                acceptor,
                buf.writable_buf(),
                send_size,
                RecvInfo {
                    from: send_info.from,
                    to: send_info.to,
                },
                None,
            )
            .unwrap();
    }

    Poll::Ready(Ok(()))
}

#[test]
fn test_connected() {
    let group = Group::new();
    let mut acceptor = make_acceptor();
    let (laddr, raddr) = make_addr_pair();

    let mut client_config = mock_config(false);

    let _client = group
        .connect(None, laddr, raddr, &mut client_config)
        .unwrap();

    loop {
        let mut events = vec![];
        _ = group.poll(&mut events);

        for event in events {
            match event.kind {
                EventKind::Connected => {
                    return;
                }
                EventKind::Send => match transfer(&mut acceptor, &group, event.token) {
                    Poll::Ready(_) => {}
                    Poll::Pending => {
                        continue;
                    }
                },
                _ => {}
            }
        }
    }
}

#[test]
fn test_close() {
    // pretty_env_logger::init();

    let group = Group::new();
    let mut acceptor = make_acceptor();
    let (laddr, raddr) = make_addr_pair();

    let mut client_config = mock_config(false);

    let _client = group
        .connect(None, laddr, raddr, &mut client_config)
        .unwrap();

    loop {
        let mut events = vec![];
        _ = group.poll(&mut events);

        for event in events {
            log::trace!("{:?}", event);

            match event.kind {
                EventKind::Connected => {
                    group.close(event.token, true, 0x0, b"".into()).unwrap();
                }
                EventKind::Closed => {
                    return;
                }
                EventKind::Send => match transfer(&mut acceptor, &group, event.token) {
                    Poll::Ready(_) => {}
                    Poll::Pending => {
                        continue;
                    }
                },
                _ => {}
            }
        }
    }
}

#[test]
fn test_accept() {
    let group = Group::new();
    let mut acceptor = make_acceptor();
    let (laddr, raddr) = make_addr_pair();

    let mut client_config = mock_config(false);

    let _client = group
        .connect(None, laddr, raddr, &mut client_config)
        .unwrap();

    loop {
        let mut events = vec![];
        _ = group.poll(&mut events);

        for event in events {
            match event.kind {
                EventKind::Accept => {
                    return;
                }
                EventKind::Send => match transfer(&mut acceptor, &group, event.token) {
                    Poll::Ready(_) => {}
                    Poll::Pending => {
                        continue;
                    }
                },
                _ => {}
            }
        }
    }
}

#[test]
fn test_client_stream_open_bidi() {
    // pretty_env_logger::init();

    let group = Group::new();
    let mut acceptor = make_acceptor();
    let (laddr, raddr) = make_addr_pair();

    let mut client_config = mock_config(false);

    let _client = group
        .connect(None, laddr, raddr, &mut client_config)
        .unwrap();

    loop {
        let mut events = vec![];
        _ = group.poll(&mut events);

        for event in events {
            match event.kind {
                EventKind::Connected => {
                    let stream_id = group
                        .stream_open(event.token, StreamKind::Bidi, false)
                        .unwrap()
                        .unwrap();

                    group
                        .stream_send(event.token, stream_id, b"hello world", true)
                        .unwrap();
                }
                EventKind::Send => match transfer(&mut acceptor, &group, event.token) {
                    Poll::Ready(_) => {}
                    Poll::Pending => {
                        continue;
                    }
                },
                EventKind::StreamAccept => {
                    let mut buf = vec![0; 1300];
                    let (read_size, fin) = group
                        .stream_recv(event.token, event.stream_id, &mut buf)
                        .unwrap();

                    assert_eq!(fin, true);
                    assert_eq!(b"hello world", &buf[..read_size]);
                    return;
                }
                _ => {}
            }
        }
    }
}

#[test]
fn test_client_stream_open_uni() {
    // pretty_env_logger::init();

    let group = Group::new();
    let mut acceptor = make_acceptor();
    let (laddr, raddr) = make_addr_pair();

    let mut client_config = mock_config(false);

    let _client = group
        .connect(None, laddr, raddr, &mut client_config)
        .unwrap();

    loop {
        let mut events = vec![];
        _ = group.poll(&mut events);

        for event in events {
            match event.kind {
                EventKind::Connected => {
                    let stream_id = group
                        .stream_open(event.token, StreamKind::Uni, false)
                        .unwrap()
                        .unwrap();

                    group
                        .stream_send(event.token, stream_id, b"hello world", true)
                        .unwrap();
                }
                EventKind::Send => match transfer(&mut acceptor, &group, event.token) {
                    Poll::Ready(_) => {}
                    Poll::Pending => {
                        continue;
                    }
                },
                EventKind::StreamAccept => {
                    let mut buf = vec![0; 1300];
                    let (read_size, fin) = group
                        .stream_recv(event.token, event.stream_id, &mut buf)
                        .unwrap();

                    assert_eq!(fin, true);
                    assert_eq!(b"hello world", &buf[..read_size]);
                    return;
                }
                _ => {}
            }
        }
    }
}

#[test]
fn test_client_stream_open_limits_uni() {
    // pretty_env_logger::init();

    let group = Group::new();
    let mut acceptor = make_acceptor();
    let (laddr, raddr) = make_addr_pair();

    let mut client_config = mock_config(false);

    let _client = group
        .connect(None, laddr, raddr, &mut client_config)
        .unwrap();

    let mut counter = 0;

    loop {
        let mut events = vec![];
        _ = group.poll(&mut events);

        for event in events {
            match event.kind {
                EventKind::Connected => {
                    let stream_id = group
                        .stream_open(event.token, StreamKind::Uni, false)
                        .unwrap()
                        .unwrap();

                    group
                        .stream_send(event.token, stream_id, b"hello world", true)
                        .unwrap();
                }
                EventKind::Send => match transfer(&mut acceptor, &group, event.token) {
                    Poll::Ready(_) => {}
                    Poll::Pending => {
                        continue;
                    }
                },
                EventKind::StreamAccept => {
                    let mut buf = vec![0; 1300];
                    let (read_size, fin) = group
                        .stream_recv(event.token, event.stream_id, &mut buf)
                        .unwrap();

                    assert_eq!(fin, true);
                    assert_eq!(b"hello world", &buf[..read_size]);

                    assert_eq!(
                        group
                            .stream_open(_client, StreamKind::Uni, false)
                            .expect_err("Retry"),
                        Error::Retry
                    );
                }
                EventKind::StreamOpenUni => {
                    log::trace!("counter: {}", counter);

                    let stream_id = match group.stream_open(event.token, StreamKind::Uni, false) {
                        Ok(stream_id) => stream_id.unwrap(),
                        Err(Error::Retry) => {
                            continue;
                        }
                        Err(err) => panic!("{}", err),
                    };

                    group
                        .stream_send(event.token, stream_id, b"hello world", true)
                        .unwrap();

                    counter += 1;

                    if counter > 1000 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }
}

#[test]
fn test_client_stream_open_limits_bidi() {
    // pretty_env_logger::init();

    let group = Group::new();
    let mut acceptor = make_acceptor();
    let (laddr, raddr) = make_addr_pair();

    let mut client_config = mock_config(false);

    let _client = group
        .connect(None, laddr, raddr, &mut client_config)
        .unwrap();

    let mut counter = 0;

    loop {
        let mut events = vec![];
        _ = group.poll(&mut events);

        for event in events {
            match event.kind {
                EventKind::Connected => {
                    let stream_id = group
                        .stream_open(event.token, StreamKind::Bidi, false)
                        .unwrap()
                        .unwrap();

                    group
                        .stream_send(event.token, stream_id, b"hello world", true)
                        .unwrap();
                }
                EventKind::Send => match transfer(&mut acceptor, &group, event.token) {
                    Poll::Ready(_) => {}
                    Poll::Pending => {
                        continue;
                    }
                },
                EventKind::StreamAccept => {
                    let mut buf = vec![0; 1300];
                    let (read_size, fin) = group
                        .stream_recv(event.token, event.stream_id, &mut buf)
                        .unwrap();

                    assert_eq!(fin, true);
                    assert_eq!(b"hello world", &buf[..read_size]);

                    group
                        .stream_send(event.token, event.stream_id, b"", true)
                        .unwrap();

                    assert_eq!(
                        group
                            .stream_open(_client, StreamKind::Bidi, false)
                            .expect_err("Retry"),
                        Error::Retry
                    );
                }
                EventKind::StreamOpenBidi => {
                    log::trace!("counter: {}", counter);

                    let stream_id = match group.stream_open(event.token, StreamKind::Bidi, false) {
                        Ok(stream_id) => stream_id.unwrap(),
                        Err(Error::Retry) => {
                            continue;
                        }
                        Err(err) => panic!("{}", err),
                    };

                    group
                        .stream_send(event.token, stream_id, b"hello world", true)
                        .unwrap();

                    counter += 1;

                    if counter > 1000 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }
}

#[test]
fn test() {
    fn a<I>(_i: I)
    where
        i32: From<I>,
    {
    }

    a(1i32);
}
