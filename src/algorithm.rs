//! Encoding algorithms (§ 10 of X.891).
//!
//! Built-in algorithms occupy IDs 1..=10. IDs 11..=31 are reserved.
//! IDs 32..=255 are for application-defined algorithms registered through
//! the document's encoding-algorithm table.

/// Numeric identifiers for the ten built-in algorithms.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuiltIn {
    HexBinary = 1,
    Base64 = 2,
    Short = 3,
    Int = 4,
    Long = 5,
    Boolean = 6,
    Float = 7,
    Double = 8,
    Uuid = 9,
    Cdata = 10,
}

/// First application-defined algorithm ID.
pub const APPLICATION_DEFINED_START: u32 = 32;

/// Raw octets carried under some algorithm ID, plus an optional URI for
/// application-defined algorithms.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AlgorithmData {
    pub id: u32,
    pub uri: Option<String>,
    pub octets: Vec<u8>,
}

impl AlgorithmData {
    pub fn builtin(id: BuiltIn, octets: Vec<u8>) -> Self {
        Self {
            id: id as u32,
            uri: None,
            octets,
        }
    }
}
