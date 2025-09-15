/// Associates readiness events.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Token {
    USize(usize),
    U64(u64),
    U32(u32),
    QuicStream(u32, u64),
}
