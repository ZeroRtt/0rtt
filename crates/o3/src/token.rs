//! A reference to port object.

/// a reference to port object.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub enum Token {
    Mio(usize),
    QuicStream(u32, u64),
}

impl From<mio::Token> for Token {
    fn from(value: mio::Token) -> Self {
        Token::Mio(value.0)
    }
}

impl From<(quico::Token, u64)> for Token {
    fn from(value: (quico::Token, u64)) -> Self {
        Token::QuicStream(value.0.0, value.1)
    }
}
