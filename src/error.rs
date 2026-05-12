use thiserror::Error;

pub type Result<T, E = Error> = core::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("unexpected end of input")]
    UnexpectedEof,

    #[error("not a Fast Infoset stream (bad magic)")]
    BadMagic,

    #[error("unsupported FI version")]
    UnsupportedVersion,

    #[error("malformed encoding: {0}")]
    Malformed(&'static str),

    #[error("vocabulary table index {index} out of range (size {size})")]
    VocabIndexOutOfRange { index: u32, size: u32 },

    #[error("encoding algorithm {0} is not registered")]
    UnknownAlgorithm(u32),

    #[error("invalid UTF-8 in string table entry")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),

    #[error("writer received event that violates document structure: {0}")]
    InvalidEventOrder(&'static str),

    #[error("{0}")]
    Other(String),
}
