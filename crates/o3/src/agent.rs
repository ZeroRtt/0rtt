use crate::switch::Switch;

#[allow(unused)]
/// Forward agent, passes tcp traffic through quic tunnel.
pub struct Agent {
    /// In-memory/local-thread data switch.
    switch: Switch,
}
