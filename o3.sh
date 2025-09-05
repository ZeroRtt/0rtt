RUST_LOG=trace \
cargo run --release --no-default-features --features o3,cli \
--bin o3 -p o3 -- \
-d -p 1900 -c "./crates/quico/cert/server.crt" \
-k "./crates/quico/cert/server.key" \
redirect 127.0.0.1:12948
