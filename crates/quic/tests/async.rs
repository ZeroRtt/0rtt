use std::time::Duration;

use quiche::Config;
use zrquic::{
    futures::{QuicConn, QuicListener},
    poll::{
        StreamKind,
        server::{Acceptor, SimpleAddressValidator},
    },
};

#[allow(unused)]
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

#[inline]
#[allow(unused)]
fn make_acceptor() -> Acceptor {
    Acceptor::new(
        mock_config(true),
        SimpleAddressValidator::new(Duration::from_secs(1)),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn connect() {
    // pretty_env_logger::init_timed();

    let server = QuicListener::bind("127.0.0.1:0", make_acceptor()).unwrap();
    let remote_addr = server.local_addrs().copied().next().unwrap();

    let _ = QuicConn::connect(
        None,
        "127.0.0.1:0".parse().unwrap(),
        remote_addr,
        &mut mock_config(false),
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn accept() {
    let server = QuicListener::bind("127.0.0.1:0", make_acceptor()).unwrap();
    let remote_addr = server.local_addrs().copied().next().unwrap();

    tokio::spawn(async move {
        let _ = QuicConn::connect(
            None,
            "127.0.0.1:0".parse().unwrap(),
            remote_addr,
            &mut mock_config(false),
        )
        .await
        .unwrap();
    });

    server.accept().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn stream_bidi() {
    // _ = pretty_env_logger::try_init_timed();
    let server = QuicListener::bind("127.0.0.1:0", make_acceptor()).unwrap();
    let remote_addr = server.local_addrs().copied().next().unwrap();

    let client_conn = QuicConn::connect(
        None,
        "127.0.0.1:0".parse().unwrap(),
        remote_addr,
        &mut mock_config(false),
    )
    .await
    .unwrap();

    let server_conn = server.accept().await.unwrap();

    tokio::spawn(async move {
        loop {
            let stream = server_conn.accept().await.unwrap();

            let mut buf = vec![0; 100];

            let (read_size, fin) = stream.recv(&mut buf).await.unwrap();
            assert_eq!(fin, true);
            stream.send(&buf[..read_size], true).await.unwrap();
        }
    });

    for i in 0..100 {
        let stream = client_conn.open(StreamKind::Bidi).await.unwrap();

        let msg = format!("Send {}", i);

        let len = stream.send(msg.as_bytes(), true).await.unwrap();

        log::trace!("send({}): {}", i, len);

        let mut buf = vec![0; 100];

        let (read_size, fin) = stream.recv(&mut buf).await.unwrap();
        log::trace!("recv({}): {}", i, read_size);
        assert_eq!(fin, true);
        assert_eq!(&buf[..read_size], msg.as_bytes());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 10)]
async fn stream_uni() {
    // _ = pretty_env_logger::try_init_timed();
    let server = QuicListener::bind("127.0.0.1:0", make_acceptor()).unwrap();
    let remote_addr = server.local_addrs().copied().next().unwrap();

    let client_conn = QuicConn::connect(
        None,
        "127.0.0.1:0".parse().unwrap(),
        remote_addr,
        &mut mock_config(false),
    )
    .await
    .unwrap();

    let server_conn = server.accept().await.unwrap();

    tokio::spawn(async move {
        loop {
            let stream = server_conn.accept().await.unwrap();

            let mut buf = vec![0; 100];

            let (_, fin) = stream.recv(&mut buf).await.unwrap();
            assert_eq!(fin, true);
        }
    });

    for i in 0..100 {
        let stream = client_conn.open(StreamKind::Uni).await.unwrap();

        let msg = format!("Send {}", i);

        let len = stream.send(msg.as_bytes(), true).await.unwrap();

        log::trace!("send({}): {}", i, len);
    }
}
