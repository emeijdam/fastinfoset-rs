//! Streaming, pull-style reader.
//!
//! The reader yields `Event`s in document order. Internally it walks the
//! productions described in X.891 Appendix C, dispatching off the byte
//! tables in `state_tables`.
//!
//! Scope of the current implementation:
//!   * literal qualified-name forms 0 and 1 (no prefix; optional namespace);
//!   * indexed qualified-name references (small/medium/large);
//!   * NISTRING UTF-8 attribute values + indexed attribute-value references;
//!   * CII UTF-8 character chunks + indexed CII references;
//!   * single and double terminators.
//!
//! Not yet supported (returns `Error::Malformed`):
//!   * EII namespaces (`0x38` / `0x78`) — namespace-declaration loop;
//!   * UTF-16 strings, restricted alphabets, encoding algorithms;
//!   * processing instructions, comments, DTD, unexpanded entity refs;
//!   * literal qualified names with prefixes (form 3).

use std::collections::VecDeque;
use std::io::Read;

use crate::encoding::{
    CHARACTER_CHUNK_ADD_TO_TABLE_FLAG, ELEMENT_ATTRIBUTE_FLAG, INT2_LARGE_MASK,
    INT2_MEDIUM_LIMIT, INT2_MEDIUM_MASK, INT2_SMALL_LIMIT, INT2_SMALL_MASK,
    LEN2_MEDIUM_LIMIT, LEN2_SMALL_LIMIT, LEN5_MEDIUM_LIMIT, LEN5_SMALL_LIMIT, LEN5_SMALL_MASK,
    LEN7_MEDIUM_LIMIT, LEN7_SMALL_LIMIT, LITERAL_QNAME_PREFIX_NAMESPACE_NAME_MASK,
    NISTRING_ADD_TO_TABLE_FLAG,
};
use crate::error::{Error, Result};
use crate::event::{Attribute, CharacterChunk, Event};
use crate::header::{read_header, HeaderFlags};
use crate::name::QualifiedName;
use crate::state_tables::{aii, eii, istring, nistring, Aii, Eii, Istring, Nistring};
use crate::vocabulary::Vocabulary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    Document,
    Element,
}

pub struct Reader<R: Read> {
    inner: R,
    octets_read: u64,
    voc: Vocabulary,
    flags: HeaderFlags,
    stack: Vec<Frame>,
    pending: VecDeque<Event>,
    started: bool,
    finished: bool,
}

impl<R: Read> Reader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            octets_read: 0,
            voc: Vocabulary::new(),
            flags: HeaderFlags::default(),
            stack: Vec::new(),
            pending: VecDeque::new(),
            started: false,
            finished: false,
        }
    }

    pub fn vocabulary(&self) -> &Vocabulary {
        &self.voc
    }

    pub fn header_flags(&self) -> HeaderFlags {
        self.flags
    }

    pub fn octet_position(&self) -> u64 {
        self.octets_read
    }

    pub fn next_event(&mut self) -> Result<Option<Event>> {
        if let Some(ev) = self.pending.pop_front() {
            return Ok(Some(ev));
        }
        if self.finished {
            return Ok(None);
        }
        if !self.started {
            self.flags = read_header(&mut self.inner)?;
            self.octets_read = 5;
            if self.flags.any() {
                return Err(Error::Malformed(
                    "DII optional components not yet supported",
                ));
            }
            self.started = true;
            self.stack.push(Frame::Document);
            return Ok(Some(Event::StartDocument));
        }
        self.step()
    }

    pub fn into_inner(self) -> R {
        self.inner
    }

    // ----- production driver -----------------------------------------------

    fn step(&mut self) -> Result<Option<Event>> {
        loop {
            let frame = match self.stack.last() {
                Some(f) => *f,
                None => {
                    self.finished = true;
                    return Ok(None);
                }
            };
            let b = self.read_byte()?;
            match frame {
                Frame::Document => match self.dispatch_child(b, true)? {
                    Dispatch::Event => return self.pop_pending(),
                    Dispatch::Terminate(double) => {
                        self.stack.pop();
                        self.pending.push_back(Event::EndDocument);
                        if double {
                            return Err(Error::Malformed(
                                "double terminator at document level",
                            ));
                        }
                        self.finished = true;
                        return self.pop_pending();
                    }
                },
                Frame::Element => match self.dispatch_child(b, false)? {
                    Dispatch::Event => return self.pop_pending(),
                    Dispatch::Terminate(double) => {
                        self.stack.pop();
                        self.pending.push_back(Event::EndElement);
                        if double {
                            if let Some(parent) = self.stack.pop() {
                                self.pending.push_back(match parent {
                                    Frame::Element => Event::EndElement,
                                    Frame::Document => {
                                        self.finished = true;
                                        Event::EndDocument
                                    }
                                });
                            } else {
                                return Err(Error::Malformed(
                                    "double terminator with no parent frame",
                                ));
                            }
                        }
                        return self.pop_pending();
                    }
                },
            }
        }
    }

    fn dispatch_child(&mut self, b: u8, at_document: bool) -> Result<Dispatch> {
        match eii(b) {
            Eii::Literal => {
                let has_attrs = (b & ELEMENT_ATTRIBUTE_FLAG) != 0;
                let qname_state = b & LITERAL_QNAME_PREFIX_NAMESPACE_NAME_MASK;
                let name = self.decode_literal_element_qname(qname_state)?;
                self.emit_element(name, has_attrs)?;
                Ok(Dispatch::Event)
            }
            Eii::NoAiisIndexSmall | Eii::AiisIndexSmall => {
                let idx = u32::from(b & 0x1F);
                let has_attrs = matches!(eii(b), Eii::AiisIndexSmall);
                let name = self.lookup_element_name(idx)?;
                self.emit_element(name, has_attrs)?;
                Ok(Dispatch::Event)
            }
            Eii::IndexMedium => {
                let b2 = self.read_byte()?;
                let idx = ((u32::from(b) & 0x0F) << 8) | u32::from(b2);
                // INTEGER_3RD_BIT_SMALL_LIMIT = 32 (range starts at 32)
                let idx = idx + 32;
                let has_attrs = (b & ELEMENT_ATTRIBUTE_FLAG) != 0;
                let name = self.lookup_element_name(idx)?;
                self.emit_element(name, has_attrs)?;
                Ok(Dispatch::Event)
            }
            Eii::IndexLarge => Err(Error::Malformed(
                "EII large index not yet implemented",
            )),
            Eii::Namespaces => Err(Error::Malformed(
                "EII with namespace declarations not yet implemented",
            )),
            Eii::CiiUtf8SmallLength
            | Eii::CiiUtf8MediumLength
            | Eii::CiiUtf8LargeLength => {
                if at_document {
                    return Err(Error::Malformed(
                        "character chunk at document level",
                    ));
                }
                let add_to_table = (b & CHARACTER_CHUNK_ADD_TO_TABLE_FLAG) != 0;
                let len = match eii(b) {
                    Eii::CiiUtf8SmallLength => {
                        u32::from(b & 0x01) + 1
                    }
                    Eii::CiiUtf8MediumLength => {
                        u32::from(self.read_byte()?) + LEN7_SMALL_LIMIT
                    }
                    Eii::CiiUtf8LargeLength => self.read_u32()? + LEN7_MEDIUM_LIMIT,
                    _ => unreachable!(),
                };
                let s = self.read_utf8_string(len)?;
                if add_to_table {
                    self.voc.content_character_chunks.push(s.clone());
                }
                self.pending
                    .push_back(Event::Characters(CharacterChunk::Text(s)));
                Ok(Dispatch::Event)
            }
            Eii::CiiIndexSmall => {
                if at_document {
                    return Err(Error::Malformed(
                        "character chunk at document level",
                    ));
                }
                let idx = u32::from(b & 0x0F);
                let s = self.lookup_character_chunk(idx)?;
                self.pending
                    .push_back(Event::Characters(CharacterChunk::Text(s)));
                Ok(Dispatch::Event)
            }
            Eii::CiiIndexMedium => {
                if at_document {
                    return Err(Error::Malformed(
                        "character chunk at document level",
                    ));
                }
                let b2 = self.read_byte()?;
                // INTEGER_4TH_BIT_SMALL_LIMIT = 16
                let idx = (((u32::from(b) & 0x03) << 8) | u32::from(b2)) + 16;
                let s = self.lookup_character_chunk(idx)?;
                self.pending
                    .push_back(Event::Characters(CharacterChunk::Text(s)));
                Ok(Dispatch::Event)
            }
            Eii::CiiIndexLarge | Eii::CiiIndexLargeLarge => Err(Error::Malformed(
                "CII large index not yet implemented",
            )),
            Eii::CiiUtf16SmallLength
            | Eii::CiiUtf16MediumLength
            | Eii::CiiUtf16LargeLength => Err(Error::Malformed(
                "UTF-16 character chunks not yet implemented",
            )),
            Eii::CiiRa | Eii::CiiEa => Err(Error::Malformed(
                "restricted alphabet / encoding algorithm CII not yet implemented",
            )),
            Eii::UnexpandedEntityRef
            | Eii::ProcessingInstruction
            | Eii::Comment => Err(Error::Malformed(
                "PI / comment / unexpanded entity not yet implemented",
            )),
            Eii::TerminatorSingle => Ok(Dispatch::Terminate(false)),
            Eii::TerminatorDouble => Ok(Dispatch::Terminate(true)),
            Eii::Illegal => Err(Error::Malformed("illegal EII identification byte")),
        }
    }

    /// Emit a StartElement event and, depending on attribute-list termination,
    /// decide whether the element will have children to read.
    fn emit_element(&mut self, name: QualifiedName, has_attrs: bool) -> Result<()> {
        let (attributes, term) = if has_attrs {
            self.decode_attributes()?
        } else {
            (Vec::new(), AttrTerm::Single)
        };
        self.pending.push_back(Event::StartElement {
            name,
            namespaces: Vec::new(),
            attributes,
        });
        match term {
            AttrTerm::Single => {
                // Children follow until a child-list terminator.
                self.stack.push(Frame::Element);
            }
            AttrTerm::Double => {
                // Element has no children — close it right away.
                self.pending.push_back(Event::EndElement);
            }
        }
        Ok(())
    }

    // ----- name decoding ---------------------------------------------------

    fn decode_literal_element_qname(&mut self, state: u8) -> Result<QualifiedName> {
        match state {
            0 => {
                let local = self.decode_istring(StringTableKind::LocalName)?;
                let qn = QualifiedName::local(local);
                self.voc.element_names.push(qn.clone());
                Ok(qn)
            }
            1 => Err(Error::Malformed(
                "literal qname form 1 (namespace, no prefix) not yet implemented",
            )),
            2 => Err(Error::Malformed("literal qname form 2 is invalid")),
            3 => Err(Error::Malformed(
                "literal qname form 3 (prefix+namespace) not yet implemented",
            )),
            _ => unreachable!(),
        }
    }

    fn lookup_element_name(&self, idx: u32) -> Result<QualifiedName> {
        self.voc
            .element_names
            .get(idx)
            .cloned()
            .ok_or(Error::VocabIndexOutOfRange {
                index: idx,
                size: self.voc.element_names.len() as u32,
            })
    }

    fn lookup_character_chunk(&self, idx: u32) -> Result<String> {
        self.voc
            .content_character_chunks
            .get(idx)
            .map(str::to_owned)
            .ok_or(Error::VocabIndexOutOfRange {
                index: idx,
                size: self.voc.content_character_chunks.len() as u32,
            })
    }

    /// Decode an "identifying non-empty string on the first bit" — either a
    /// literal (added to the named table) or an index into that table.
    fn decode_istring(&mut self, kind: StringTableKind) -> Result<String> {
        let b = self.read_byte()?;
        match istring(b) {
            Istring::SmallLength => {
                let len = u32::from(b) + 1;
                let s = self.read_utf8_string(len)?;
                self.push_string_table(kind, &s);
                Ok(s)
            }
            Istring::MediumLength => {
                let len = u32::from(self.read_byte()?) + LEN2_SMALL_LIMIT;
                let s = self.read_utf8_string(len)?;
                self.push_string_table(kind, &s);
                Ok(s)
            }
            Istring::LargeLength => {
                let len = self.read_u32()? + LEN2_MEDIUM_LIMIT;
                let s = self.read_utf8_string(len)?;
                self.push_string_table(kind, &s);
                Ok(s)
            }
            Istring::IndexSmall => {
                let idx = u32::from(b & INT2_SMALL_MASK);
                self.lookup_string_table(kind, idx)
            }
            Istring::IndexMedium => {
                let b2 = self.read_byte()?;
                let idx = ((u32::from(b) & u32::from(INT2_MEDIUM_MASK)) << 8) | u32::from(b2);
                let idx = idx + INT2_SMALL_LIMIT;
                self.lookup_string_table(kind, idx)
            }
            Istring::IndexLarge => {
                let b2 = self.read_byte()?;
                let b3 = self.read_byte()?;
                let idx = ((u32::from(b) & u32::from(INT2_LARGE_MASK)) << 16)
                    | (u32::from(b2) << 8)
                    | u32::from(b3);
                let idx = idx + INT2_MEDIUM_LIMIT;
                self.lookup_string_table(kind, idx)
            }
        }
    }

    fn push_string_table(&mut self, kind: StringTableKind, s: &str) {
        let table = match kind {
            StringTableKind::LocalName => &mut self.voc.local_names,
            StringTableKind::Prefix => &mut self.voc.prefixes,
            StringTableKind::NamespaceName => &mut self.voc.namespace_names,
        };
        table.push(s.to_owned());
    }

    fn lookup_string_table(&self, kind: StringTableKind, idx: u32) -> Result<String> {
        let (table, size) = match kind {
            StringTableKind::LocalName => (
                self.voc.local_names.get(idx),
                self.voc.local_names.len() as u32,
            ),
            StringTableKind::Prefix => (
                self.voc.prefixes.get(idx),
                self.voc.prefixes.len() as u32,
            ),
            StringTableKind::NamespaceName => (
                self.voc.namespace_names.get(idx),
                self.voc.namespace_names.len() as u32,
            ),
        };
        table
            .map(str::to_owned)
            .ok_or(Error::VocabIndexOutOfRange { index: idx, size })
    }

    // ----- attributes ------------------------------------------------------

    fn decode_attributes(&mut self) -> Result<(Vec<Attribute>, AttrTerm)> {
        let mut out = Vec::new();
        loop {
            let b = self.read_byte()?;
            match aii(b) {
                Aii::Literal => {
                    let qname_state = b & LITERAL_QNAME_PREFIX_NAMESPACE_NAME_MASK;
                    let name = self.decode_literal_attribute_qname(qname_state)?;
                    let value = self.decode_nistring()?;
                    out.push(Attribute { name, value });
                }
                Aii::IndexSmall => {
                    let idx = u32::from(b & INT2_SMALL_MASK);
                    let name = self.lookup_attribute_name(idx)?;
                    let value = self.decode_nistring()?;
                    out.push(Attribute { name, value });
                }
                Aii::IndexMedium => {
                    let b2 = self.read_byte()?;
                    let idx = ((u32::from(b) & u32::from(INT2_MEDIUM_MASK)) << 8)
                        | u32::from(b2);
                    let idx = idx + INT2_SMALL_LIMIT;
                    let name = self.lookup_attribute_name(idx)?;
                    let value = self.decode_nistring()?;
                    out.push(Attribute { name, value });
                }
                Aii::IndexLarge => {
                    let b2 = self.read_byte()?;
                    let b3 = self.read_byte()?;
                    let idx = ((u32::from(b) & u32::from(INT2_LARGE_MASK)) << 16)
                        | (u32::from(b2) << 8)
                        | u32::from(b3);
                    let idx = idx + INT2_MEDIUM_LIMIT;
                    let name = self.lookup_attribute_name(idx)?;
                    let value = self.decode_nistring()?;
                    out.push(Attribute { name, value });
                }
                Aii::TerminatorSingle => return Ok((out, AttrTerm::Single)),
                Aii::TerminatorDouble => return Ok((out, AttrTerm::Double)),
                Aii::Illegal => {
                    return Err(Error::Malformed("illegal AII identification byte"))
                }
            }
        }
    }

    fn decode_literal_attribute_qname(&mut self, state: u8) -> Result<QualifiedName> {
        match state {
            0 => {
                let local = self.decode_istring(StringTableKind::LocalName)?;
                let qn = QualifiedName::local(local);
                self.voc.attribute_names.push(qn.clone());
                Ok(qn)
            }
            1 | 3 => Err(Error::Malformed(
                "literal AII with prefix/namespace not yet implemented",
            )),
            _ => Err(Error::Malformed("literal AII form 2 is invalid")),
        }
    }

    fn lookup_attribute_name(&self, idx: u32) -> Result<QualifiedName> {
        self.voc
            .attribute_names
            .get(idx)
            .cloned()
            .ok_or(Error::VocabIndexOutOfRange {
                index: idx,
                size: self.voc.attribute_names.len() as u32,
            })
    }

    // ----- NISTRING (attribute values, etc.) -------------------------------

    fn decode_nistring(&mut self) -> Result<CharacterChunk> {
        let b = self.read_byte()?;
        match nistring(b) {
            Nistring::Utf8SmallLength => {
                let add = (b & NISTRING_ADD_TO_TABLE_FLAG) != 0;
                let len = u32::from(b & LEN5_SMALL_MASK) + 1;
                let s = self.read_utf8_string(len)?;
                if add {
                    self.voc.attribute_values.push(s.clone());
                }
                Ok(CharacterChunk::Text(s))
            }
            Nistring::Utf8MediumLength => {
                let add = (b & NISTRING_ADD_TO_TABLE_FLAG) != 0;
                let len = u32::from(self.read_byte()?) + LEN5_SMALL_LIMIT;
                let s = self.read_utf8_string(len)?;
                if add {
                    self.voc.attribute_values.push(s.clone());
                }
                Ok(CharacterChunk::Text(s))
            }
            Nistring::Utf8LargeLength => {
                let add = (b & NISTRING_ADD_TO_TABLE_FLAG) != 0;
                let len = self.read_u32()? + LEN5_MEDIUM_LIMIT;
                let s = self.read_utf8_string(len)?;
                if add {
                    self.voc.attribute_values.push(s.clone());
                }
                Ok(CharacterChunk::Text(s))
            }
            Nistring::IndexSmall => {
                let idx = u32::from(b & INT2_SMALL_MASK);
                Ok(CharacterChunk::Text(self.lookup_attribute_value(idx)?))
            }
            Nistring::IndexMedium => {
                let b2 = self.read_byte()?;
                let idx = ((u32::from(b) & u32::from(INT2_MEDIUM_MASK)) << 8) | u32::from(b2);
                let idx = idx + INT2_SMALL_LIMIT;
                Ok(CharacterChunk::Text(self.lookup_attribute_value(idx)?))
            }
            Nistring::IndexLarge => {
                let b2 = self.read_byte()?;
                let b3 = self.read_byte()?;
                let idx = ((u32::from(b) & u32::from(INT2_LARGE_MASK)) << 16)
                    | (u32::from(b2) << 8)
                    | u32::from(b3);
                let idx = idx + INT2_MEDIUM_LIMIT;
                Ok(CharacterChunk::Text(self.lookup_attribute_value(idx)?))
            }
            Nistring::Empty => Ok(CharacterChunk::Text(String::new())),
            Nistring::Utf16SmallLength
            | Nistring::Utf16MediumLength
            | Nistring::Utf16LargeLength => Err(Error::Malformed(
                "UTF-16 NISTRING not yet implemented",
            )),
            Nistring::Ra | Nistring::Ea => Err(Error::Malformed(
                "restricted alphabet / encoding algorithm NISTRING not yet implemented",
            )),
            Nistring::Illegal => Err(Error::Malformed("illegal NISTRING identification byte")),
        }
    }

    fn lookup_attribute_value(&self, idx: u32) -> Result<String> {
        self.voc
            .attribute_values
            .get(idx)
            .map(str::to_owned)
            .ok_or(Error::VocabIndexOutOfRange {
                index: idx,
                size: self.voc.attribute_values.len() as u32,
            })
    }

    // ----- low-level helpers ----------------------------------------------

    fn read_byte(&mut self) -> Result<u8> {
        let mut buf = [0u8; 1];
        match self.inner.read(&mut buf)? {
            0 => Err(Error::UnexpectedEof),
            _ => {
                self.octets_read += 1;
                Ok(buf[0])
            }
        }
    }

    fn read_u32(&mut self) -> Result<u32> {
        let mut buf = [0u8; 4];
        self.inner.read_exact(&mut buf).map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                Error::UnexpectedEof
            } else {
                Error::Io(e)
            }
        })?;
        self.octets_read += 4;
        Ok(u32::from_be_bytes(buf))
    }

    fn read_utf8_string(&mut self, len: u32) -> Result<String> {
        let len = len as usize;
        let mut buf = vec![0u8; len];
        self.inner.read_exact(&mut buf).map_err(|e| {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                Error::UnexpectedEof
            } else {
                Error::Io(e)
            }
        })?;
        self.octets_read += len as u64;
        String::from_utf8(buf).map_err(Error::from)
    }

    fn pop_pending(&mut self) -> Result<Option<Event>> {
        Ok(self.pending.pop_front())
    }
}

enum Dispatch {
    Event,
    Terminate(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttrTerm {
    /// AII terminator `0xF0` — element continues to read its children.
    Single,
    /// AII terminator `0xFF` — element has no children, parent's children end too.
    Double,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Prefix and NamespaceName reserved for future namespace support.
enum StringTableKind {
    LocalName,
    Prefix,
    NamespaceName,
}
