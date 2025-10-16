RUST_LOG=trace \
cargo run --manifest-path ../Cargo.toml --release --no-default-features --features agent,cli \
--bin o3-agent -p o3 -- \
-d -s 127.0.0.1 -p 1900 -c "../crates/quic/cert/client.crt" -k "../crates/quic/cert/client.key" listen 0.0.0.0:1024
