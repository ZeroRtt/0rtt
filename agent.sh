RUST_LOG=info \
cargo run --release --no-default-features --features agent,cli \
--bin o3-agent -p o3 -- \
-d -s 127.0.0.1 -p 1900 -c "./crates/quico/cert/client.crt" -k "./crates/quico/cert/client.key" listen 0.0.0.0:1024
