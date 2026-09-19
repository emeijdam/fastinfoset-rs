//! Vocabulary tables (§ 7.2.21 / Annex C).
//!
//! Fast Infoset maintains nine indexed string tables plus two qualified-name
//! tables. Each occurrence of a value in the document either inlines it (and
//! appends to the relevant table) or references an existing table entry by
//! index — the source of FI's compression.
//!
//! In addition to those thirteen indexed tables, § 7.2.14 defines an
//! "encoding-algorithm identifier" table that maps application-defined
//! algorithm IDs (32..=255) to URIs. It is populated only from an initial
//! vocabulary (§ 12.6.1); IDs 1..=10 refer to built-in algorithms and need
//! no URI. See `EncodingAlgorithmTable`.

use crate::algorithm::APPLICATION_DEFINED_START;
use crate::name::QualifiedName;

/// Indexed string table. Indices in the wire format are 1-based; this stores
/// them 0-based and converts on the boundary.
#[derive(Debug, Clone, Default)]
pub struct StringTable {
    entries: Vec<String>,
}

impl StringTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn push(&mut self, value: String) -> u32 {
        let idx = self.entries.len() as u32;
        self.entries.push(value);
        idx
    }

    pub fn get(&self, index: u32) -> Option<&str> {
        self.entries.get(index as usize).map(String::as_str)
    }

    pub fn find(&self, needle: &str) -> Option<u32> {
        self.entries
            .iter()
            .position(|v| v == needle)
            .map(|i| i as u32)
    }
}

/// Indexed qualified-name table, used for element and attribute names.
#[derive(Debug, Clone, Default)]
pub struct QualifiedNameTable {
    entries: Vec<QualifiedName>,
}

impl QualifiedNameTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn push(&mut self, name: QualifiedName) -> u32 {
        let idx = self.entries.len() as u32;
        self.entries.push(name);
        idx
    }

    pub fn get(&self, index: u32) -> Option<&QualifiedName> {
        self.entries.get(index as usize)
    }
}

/// URIs for application-defined encoding algorithms (§ 7.2.14 / § 10.2).
///
/// Stored densely: `entries[i]` holds the URI for algorithm ID
/// `APPLICATION_DEFINED_START + i`. Populated only from an initial
/// vocabulary — the wire format never introduces a new URI mid-document.
/// Built-in algorithm IDs (1..=10) don't need a URI and aren't stored here.
#[derive(Debug, Clone, Default)]
pub struct EncodingAlgorithmTable {
    entries: Vec<String>,
}

impl EncodingAlgorithmTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Append a URI, returning the algorithm ID it was assigned.
    /// The first push returns `APPLICATION_DEFINED_START` (32), the next 33,
    /// and so on. Overflow past ID 255 is the caller's problem to enforce.
    pub fn push(&mut self, uri: String) -> u32 {
        let id = self.entries.len() as u32 + APPLICATION_DEFINED_START;
        self.entries.push(uri);
        id
    }

    /// Look up the URI for algorithm ID `id`. Returns `None` for built-in or
    /// reserved IDs (< 32) and for unregistered app-defined IDs.
    pub fn get(&self, id: u32) -> Option<&str> {
        let idx = id.checked_sub(APPLICATION_DEFINED_START)? as usize;
        self.entries.get(idx).map(String::as_str)
    }
}

/// The full set of tables a document maintains while it is being read
/// or written. Reader and writer keep their own instances; they are not
/// shared across documents (§ 7.2.21).
#[derive(Debug, Clone, Default)]
pub struct Vocabulary {
    pub prefixes: StringTable,
    pub namespace_names: StringTable,
    pub local_names: StringTable,
    pub other_ncnames: StringTable,
    pub other_uris: StringTable,
    pub attribute_values: StringTable,
    pub content_character_chunks: StringTable,
    pub other_strings: StringTable,
    pub element_names: QualifiedNameTable,
    pub attribute_names: QualifiedNameTable,
    pub encoding_algorithms: EncodingAlgorithmTable,
}

impl Vocabulary {
    pub fn new() -> Self {
        Self::default()
    }
}
