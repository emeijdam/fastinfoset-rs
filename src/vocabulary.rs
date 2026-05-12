//! Vocabulary tables (§ 7.2.21 / Annex C).
//!
//! Fast Infoset maintains nine indexed string tables plus two qualified-name
//! tables. Each occurrence of a value in the document either inlines it (and
//! appends to the relevant table) or references an existing table entry by
//! index — the source of FI's compression.

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
}

impl Vocabulary {
    pub fn new() -> Self {
        Self::default()
    }
}
