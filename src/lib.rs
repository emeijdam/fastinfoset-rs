//! Fast Infoset — binary encoding of the XML Information Set
//! (ITU-T X.891 / ISO/IEC 24824-1).
//!
//! Pure-Rust streaming reader and writer.

#![doc(html_root_url = "https://docs.rs/fastinfoset-rs")]

pub mod algorithm;
pub mod bits;
pub mod encode_helpers;
pub mod encoding;
pub mod error;
pub mod event;
pub mod header;
pub mod name;
pub mod reader;
pub mod state_tables;
pub mod vocabulary;
pub mod writer;

#[cfg(feature = "xml")]
pub mod xml;

pub use error::{Error, Result};
pub use event::Event;
pub use header::HeaderFlags;
pub use name::{NamespaceDecl, QualifiedName};
pub use reader::Reader;
pub use writer::Writer;

/// The four magic bytes that begin every Fast Infoset document: `E0 00 00 01`.
pub const MAGIC: [u8; 4] = [0xE0, 0x00, 0x00, 0x01];
