//! Streaming writer.
//!
//! Mirrors the reader: takes [`Event`]s in document order and emits a
//! Fast Infoset octet stream. Currently supports the same subset as the
//! reader (no namespaces, no UTF-16, no encoding algorithms, no
//! PI/comment/DTD).

use std::collections::HashMap;
use std::io::Write;

use crate::encode_helpers::{
    write_int_2nd_bit_b0, write_int_2nd_bit_b1, write_int_3rd_bit, write_int_4th_bit,
    write_length_2nd_bit, write_length_5th_bit, write_length_7th_bit,
};
use crate::encoding::{
    ATTRIBUTE_LITERAL_QNAME_FLAG, CHARACTER_CHUNK, CHARACTER_CHUNK_ADD_TO_TABLE_FLAG,
    CHARACTER_CHUNK_INDEX_PREFIX, ELEMENT_ATTRIBUTE_FLAG, ELEMENT_LITERAL_QNAME_FLAG,
    NISTRING_ADD_TO_TABLE_FLAG, TERMINATOR,
};
use crate::error::{Error, Result};
use crate::event::{Attribute, CharacterChunk, Event};
use crate::header::{write_header, HeaderFlags};
use crate::name::QualifiedName;

/// Per-stream vocabulary maintained while encoding.
#[derive(Default)]
struct WriteVocab {
    local_names: HashMap<String, u32>,
    attribute_values: HashMap<String, u32>,
    content_character_chunks: HashMap<String, u32>,
    element_names: HashMap<QName, u32>,
    attribute_names: HashMap<QName, u32>,
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct QName {
    prefix: Option<String>,
    namespace: Option<String>,
    local: String,
}

impl From<&QualifiedName> for QName {
    fn from(q: &QualifiedName) -> Self {
        Self {
            prefix: q.prefix.clone(),
            namespace: q.namespace_name.clone(),
            local: q.local_name.clone(),
        }
    }
}

pub struct Writer<W: Write> {
    inner: W,
    voc: WriteVocab,
    header_written: bool,
    finished: bool,
    depth: u32,
}

impl<W: Write> Writer<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            voc: WriteVocab::default(),
            header_written: false,
            finished: false,
            depth: 0,
        }
    }

    pub fn write_event(&mut self, event: &Event) -> Result<()> {
        if self.finished {
            return Err(Error::InvalidEventOrder("event after EndDocument"));
        }
        match event {
            Event::StartDocument => self.start_document(),
            Event::EndDocument => self.end_document(),
            Event::StartElement {
                name,
                namespaces,
                attributes,
            } => self.start_element(name, namespaces, attributes),
            Event::EndElement => self.end_element(),
            Event::Characters(chunk) => self.characters(chunk),
            Event::ProcessingInstruction { .. }
            | Event::Comment(_)
            | Event::DocumentTypeDeclaration { .. }
            | Event::UnexpandedEntityReference { .. } => Err(Error::Other(
                "writer: PI / comment / DTD / entity-ref not yet implemented".into(),
            )),
        }
    }

    pub fn finish(mut self) -> Result<W> {
        if !self.finished {
            return Err(Error::InvalidEventOrder("finish() before EndDocument"));
        }
        self.inner.flush().map_err(Error::Io)?;
        Ok(self.inner)
    }

    // ----- per-event handlers ---------------------------------------------

    fn start_document(&mut self) -> Result<()> {
        if self.header_written {
            return Err(Error::InvalidEventOrder("duplicate StartDocument"));
        }
        write_header(&mut self.inner, HeaderFlags::default())?;
        self.header_written = true;
        Ok(())
    }

    fn end_document(&mut self) -> Result<()> {
        if !self.header_written {
            return Err(Error::InvalidEventOrder("EndDocument before StartDocument"));
        }
        if self.depth != 0 {
            return Err(Error::InvalidEventOrder("EndDocument with open elements"));
        }
        // Terminate the document's child list.
        self.put(TERMINATOR)?;
        self.finished = true;
        Ok(())
    }

    fn start_element(
        &mut self,
        name: &QualifiedName,
        namespaces: &[crate::name::NamespaceDecl],
        attributes: &[Attribute],
    ) -> Result<()> {
        if !self.header_written {
            return Err(Error::InvalidEventOrder("element before StartDocument"));
        }
        if !namespaces.is_empty() {
            return Err(Error::Other(
                "writer: namespace declarations not yet implemented".into(),
            ));
        }
        if name.prefix.is_some() || name.namespace_name.is_some() {
            return Err(Error::Other(
                "writer: namespaced element names not yet implemented".into(),
            ));
        }

        let has_attrs = !attributes.is_empty();
        let attr_flag = if has_attrs { ELEMENT_ATTRIBUTE_FLAG } else { 0 };

        let qkey = QName::from(name);
        if let Some(&index) = self.voc.element_names.get(&qkey) {
            // Indexed EII — write_int_3rd_bit emits prefix-or'd byte + index.
            write_int_3rd_bit(&mut self.inner, attr_flag, index)?;
        } else {
            // Literal EII: header byte + local-name (literal or index ref).
            self.put(attr_flag | ELEMENT_LITERAL_QNAME_FLAG)?;
            self.write_local_name(&name.local_name)?;
            let next_idx = self.voc.element_names.len() as u32;
            self.voc.element_names.insert(qkey, next_idx);
        }

        if has_attrs {
            for attr in attributes {
                self.write_attribute(attr)?;
            }
            self.put(TERMINATOR)?;
        }

        self.depth += 1;
        Ok(())
    }

    fn end_element(&mut self) -> Result<()> {
        if self.depth == 0 {
            return Err(Error::InvalidEventOrder("EndElement with no open element"));
        }
        self.put(TERMINATOR)?;
        self.depth -= 1;
        Ok(())
    }

    fn characters(&mut self, chunk: &CharacterChunk) -> Result<()> {
        if self.depth == 0 {
            return Err(Error::InvalidEventOrder(
                "Characters outside of any element",
            ));
        }
        let s = match chunk {
            CharacterChunk::Text(s) => s,
            CharacterChunk::Encoded(_) => {
                return Err(Error::Other(
                    "writer: encoded character chunks not yet implemented".into(),
                ))
            }
        };
        if s.is_empty() {
            // FI doesn't have a "zero-length CII"; characters with empty text are
            // a no-op — drop silently.
            return Ok(());
        }

        if let Some(&index) = self.voc.content_character_chunks.get(s) {
            // Indexed CII.
            write_int_4th_bit(&mut self.inner, CHARACTER_CHUNK_INDEX_PREFIX, index)?;
        } else {
            // Literal CII with add-to-table flag.
            let bytes = s.as_bytes();
            let prefix = CHARACTER_CHUNK | CHARACTER_CHUNK_ADD_TO_TABLE_FLAG;
            write_length_7th_bit(&mut self.inner, prefix, bytes.len() as u32)?;
            self.inner.write_all(bytes).map_err(Error::Io)?;
            let next_idx = self.voc.content_character_chunks.len() as u32;
            self.voc.content_character_chunks.insert(s.clone(), next_idx);
        }
        Ok(())
    }

    // ----- name & attribute helpers ----------------------------------------

    fn write_attribute(&mut self, attr: &Attribute) -> Result<()> {
        if attr.name.prefix.is_some() || attr.name.namespace_name.is_some() {
            return Err(Error::Other(
                "writer: namespaced attribute names not yet implemented".into(),
            ));
        }
        let qkey = QName::from(&attr.name);
        if let Some(&index) = self.voc.attribute_names.get(&qkey) {
            // AII index — encoded as bit-2 integer with first bit = 0.
            write_int_2nd_bit_b0(&mut self.inner, index)?;
        } else {
            // Literal AII.
            self.put(ATTRIBUTE_LITERAL_QNAME_FLAG)?;
            self.write_local_name(&attr.name.local_name)?;
            let next_idx = self.voc.attribute_names.len() as u32;
            self.voc.attribute_names.insert(qkey, next_idx);
        }
        self.write_attribute_value(&attr.value)
    }

    fn write_local_name(&mut self, name: &str) -> Result<()> {
        if let Some(&index) = self.voc.local_names.get(name) {
            write_int_2nd_bit_b1(&mut self.inner, index)
        } else {
            let bytes = name.as_bytes();
            write_length_2nd_bit(&mut self.inner, bytes.len() as u32)?;
            self.inner.write_all(bytes).map_err(Error::Io)?;
            let next_idx = self.voc.local_names.len() as u32;
            self.voc.local_names.insert(name.to_owned(), next_idx);
            Ok(())
        }
    }

    fn write_attribute_value(&mut self, chunk: &CharacterChunk) -> Result<()> {
        let s = match chunk {
            CharacterChunk::Text(s) => s,
            CharacterChunk::Encoded(_) => {
                return Err(Error::Other(
                    "writer: encoded attribute values not yet implemented".into(),
                ))
            }
        };
        if s.is_empty() {
            // C.26 empty-string NISTRING shorthand
            return self.put(0xFF);
        }
        if let Some(&index) = self.voc.attribute_values.get(s) {
            write_int_2nd_bit_b1(&mut self.inner, index)
        } else {
            let bytes = s.as_bytes();
            let prefix = NISTRING_ADD_TO_TABLE_FLAG; // UTF-8 (0x00) + add-to-table
            write_length_5th_bit(&mut self.inner, prefix, bytes.len() as u32)?;
            self.inner.write_all(bytes).map_err(Error::Io)?;
            let next_idx = self.voc.attribute_values.len() as u32;
            self.voc.attribute_values.insert(s.clone(), next_idx);
            Ok(())
        }
    }

    fn put(&mut self, byte: u8) -> Result<()> {
        self.inner.write_all(&[byte]).map_err(Error::Io)
    }
}
