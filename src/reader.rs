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
//!   * CII and AII encoding-algorithm payloads (built-in IDs 1..=10 plus
//!     app-defined 32..=255 whose URI has been seeded into the vocabulary);
//!   * single and double terminators.
//!
//! Not yet supported (returns `Error::Malformed`):
//!   * EII namespaces (`0x38` / `0x78`) — namespace-declaration loop;
//!   * UTF-16 strings, restricted alphabets;
//!   * processing instructions, comments, DTD, unexpanded entity refs;
//!   * literal qualified names with prefixes (form 3).
//!
//! Encoding-algorithm notes:
//!   * The add-to-table flag on CII-EA / AII-EA is intentionally ignored.
//!     Per X.891 § 7.14.4 / § 10.5 the flag is required to be zero unless
//!     the algorithm itself specifies a canonical character-string form,
//!     and this decoder returns raw octets — not a string — so the encoded
//!     payload has no representation to add to the (string-typed) content
//!     or attribute-value tables.
//!   * App-defined algorithm URIs (IDs 32..=255) live in
//!     `Vocabulary::encoding_algorithms`, populated only from an initial
//!     vocabulary; an unregistered ID surfaces as `Error::UnknownAlgorithm`.

use std::collections::VecDeque;
use std::io::Read;

use crate::algorithm::{AlgorithmData, APPLICATION_DEFINED_START};
use crate::encoding::{
    CHARACTER_CHUNK_ADD_TO_TABLE_FLAG, ELEMENT_ATTRIBUTE_FLAG, INT2_LARGE_MASK,
    INT3_LARGE_LIMIT, INT3_MEDIUM_LIMIT, INT3_SMALL_LIMIT, INT4_LARGE_LIMIT, INT4_MEDIUM_LIMIT,
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
                // X.891 C.27.3: '100' then 11 bits; the 3 low bits of this octet
                // are the top of the value (range starts at 32).
                let idx = ((u32::from(b) & 0x07) << 8) | u32::from(b2);
                let idx = idx + INT3_SMALL_LIMIT;
                let has_attrs = (b & ELEMENT_ATTRIBUTE_FLAG) != 0;
                let name = self.lookup_element_name(idx)?;
                self.emit_element(name, has_attrs)?;
                Ok(Dispatch::Event)
            }
            Eii::IndexLarge => {
                let has_attrs = (b & ELEMENT_ATTRIBUTE_FLAG) != 0;
                let idx = if (b & 0x30) == 0x20 {
                    // X.891 C.27.4: '101' then 19 bits (3 here, 16 in the next
                    // two octets); values from 2080.
                    let b2 = self.read_byte()?;
                    let b3 = self.read_byte()?;
                    (((u32::from(b) & 0x07) << 16) | (u32::from(b2) << 8) | u32::from(b3))
                        + INT3_MEDIUM_LIMIT
                } else {
                    // X.891 C.27.5: '110' + padding, then '0000' and 20 bits in
                    // the next three octets; values from 526368.
                    let b2 = self.read_byte()?;
                    let b3 = self.read_byte()?;
                    let b4 = self.read_byte()?;
                    (((u32::from(b2) & 0x0F) << 16) | (u32::from(b3) << 8) | u32::from(b4))
                        + INT3_LARGE_LIMIT
                };
                let name = self.lookup_element_name(idx)?;
                self.emit_element(name, has_attrs)?;
                Ok(Dispatch::Event)
            }
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
            Eii::CiiIndexLarge | Eii::CiiIndexLargeLarge => {
                if at_document {
                    return Err(Error::Malformed(
                        "character chunk at document level",
                    ));
                }
                let idx = if matches!(eii(b), Eii::CiiIndexLarge) {
                    // X.891 C.28.4: '101' then 18 bits (2 here, 16 in the next
                    // two octets); values from 1040.
                    let b2 = self.read_byte()?;
                    let b3 = self.read_byte()?;
                    (((u32::from(b) & 0x03) << 16) | (u32::from(b2) << 8) | u32::from(b3))
                        + INT4_MEDIUM_LIMIT
                } else {
                    // X.891 C.28.5: '110' + padding, then '0000' and 20 bits in
                    // the next three octets; values from 263184.
                    let b2 = self.read_byte()?;
                    let b3 = self.read_byte()?;
                    let b4 = self.read_byte()?;
                    (((u32::from(b2) & 0x0F) << 16) | (u32::from(b3) << 8) | u32::from(b4))
                        + INT4_LARGE_LIMIT
                };
                let s = self.lookup_character_chunk(idx)?;
                self.pending
                    .push_back(Event::Characters(CharacterChunk::Text(s)));
                Ok(Dispatch::Event)
            }
            Eii::CiiUtf16SmallLength
            | Eii::CiiUtf16MediumLength
            | Eii::CiiUtf16LargeLength => Err(Error::Malformed(
                "UTF-16 character chunks not yet implemented",
            )),
            Eii::CiiEa => {
                if at_document {
                    return Err(Error::Malformed(
                        "character chunk at document level",
                    ));
                }
                let data = self.decode_encoding_algorithm_cii(b)?;
                self.pending
                    .push_back(Event::Characters(CharacterChunk::Encoded(data)));
                Ok(Dispatch::Event)
            }
            Eii::CiiRa => Err(Error::Malformed(
                "restricted alphabet CII not yet implemented",
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
            Nistring::Ea => {
                let data = self.decode_encoding_algorithm_nistring(b)?;
                Ok(CharacterChunk::Encoded(data))
            }
            Nistring::Ra => Err(Error::Malformed(
                "restricted alphabet NISTRING not yet implemented",
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

    // ----- encoding-algorithm CII / AII (§ 10, § C.20 and § C.15) ---------

    /// Decode a CII carrying encoding-algorithm-encoded data. `b1` is the
    /// identification octet already consumed by the caller.
    ///
    /// Wire layout (bit 1 = LSB):
    ///   byte 1: `1 0 0 T 1 1 A A`   T = add-to-table (ignored, see module
    ///                                docs), AA = alg-id bits 7..6
    ///   byte 2: `A A A A A A L L`   AAAAAA = alg-id bits 5..0,
    ///                                LL = length-on-seventh-bit prefix
    fn decode_encoding_algorithm_cii(&mut self, b1: u8) -> Result<AlgorithmData> {
        let b2 = self.read_byte()?;
        let id = (u32::from(b1 & 0x03) << 6) | (u32::from(b2 & 0xFC) >> 2);
        let len = match b2 & 0x03 {
            0 | 1 => u32::from(b2 & 0x01) + 1,
            2 => u32::from(self.read_byte()?) + LEN7_SMALL_LIMIT,
            3 => self.read_u32()? + LEN7_MEDIUM_LIMIT,
            _ => unreachable!("2-bit value"),
        };
        let octets = self.read_octets(len)?;
        self.build_algorithm_data(id, octets)
    }

    /// Decode an AII (NISTRING) carrying encoding-algorithm-encoded data.
    /// `b1` is the identification octet already consumed by the caller.
    ///
    /// Wire layout (bit 1 = LSB):
    ///   byte 1: `0 T 1 1 A A A A`   T = add-to-table (ignored), AAAA = alg-id
    ///                                bits 7..4
    ///   byte 2: `A A A A L L L L`   AAAA = alg-id bits 3..0,
    ///                                LLLL = length-on-fifth-bit prefix
    fn decode_encoding_algorithm_nistring(&mut self, b1: u8) -> Result<AlgorithmData> {
        let b2 = self.read_byte()?;
        let id = (u32::from(b1 & 0x0F) << 4) | (u32::from(b2 & 0xF0) >> 4);
        let len = match b2 & 0x0F {
            0..=7 => u32::from(b2 & 0x07) + 1,
            8 => u32::from(self.read_byte()?) + LEN5_SMALL_LIMIT,
            12 => self.read_u32()? + LEN5_MEDIUM_LIMIT,
            _ => {
                return Err(Error::Malformed(
                    "invalid length prefix in encoding-algorithm NISTRING",
                ))
            }
        };
        let octets = self.read_octets(len)?;
        self.build_algorithm_data(id, octets)
    }

    /// Package `octets` with algorithm `id`, filling in the URI for
    /// app-defined algorithms from the vocabulary. Rejects reserved (11..=31)
    /// and unregistered app-defined IDs with `Error::UnknownAlgorithm`. IDs
    /// outside 1..=255 are ill-formed by construction (the two decode paths
    /// pack 8 bits) but rejected explicitly for defense in depth.
    fn build_algorithm_data(&self, id: u32, octets: Vec<u8>) -> Result<AlgorithmData> {
        match id {
            1..=10 => Ok(AlgorithmData { id, uri: None, octets }),
            id if id >= APPLICATION_DEFINED_START && id <= 255 => {
                let uri = self
                    .voc
                    .encoding_algorithms
                    .get(id)
                    .ok_or(Error::UnknownAlgorithm(id))?
                    .to_owned();
                Ok(AlgorithmData { id, uri: Some(uri), octets })
            }
            _ => Err(Error::UnknownAlgorithm(id)),
        }
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
        let buf = self.read_octets(len)?;
        String::from_utf8(buf).map_err(Error::from)
    }

    fn read_octets(&mut self, len: u32) -> Result<Vec<u8>> {
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
        Ok(buf)
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

#[cfg(test)]
mod tests {
    //! Hand-crafted binary tests for the encoding-algorithm paths.
    //!
    //! The writer doesn't emit encoding algorithms yet, so we can't round-trip
    //! — every stream here is spelled out byte-by-byte, with comments naming
    //! each octet against X.891.

    use super::*;
    use crate::algorithm::{AlgorithmData, BuiltIn};
    use crate::event::{CharacterChunk, Event};

    /// Drive the reader until it either errors or emits `EndDocument`.
    fn drain(bytes: &[u8]) -> Result<Vec<Event>> {
        let mut r = Reader::new(bytes);
        let mut out = Vec::new();
        while let Some(ev) = r.next_event()? {
            let done = matches!(ev, Event::EndDocument);
            out.push(ev);
            if done {
                break;
            }
        }
        Ok(out)
    }

    /// `<a>` opener with no attributes: literal EII form 0, no-attr flag.
    ///   3C           — EII: literal, no attrs, qname state 0
    ///   00 61        — local-name istring: small length 1, 'a'
    const OPEN_A_NO_ATTRS: &[u8] = &[0x3C, 0x00, 0x61];

    #[test]
    fn cii_ea_builtin_hex_binary() {
        // <a>[CII-EA HexBinary(0xDE,0xAD)]</a>
        //   8C 05       — CII-EA: T=0, alg-id top 2 = 00 (id=1, HexBinary),
        //                  b2 = alg-id low 6 (000001) << 2 | length 2 (01)
        //   DE AD       — payload
        //   F0 F0       — element-terminator, document-terminator
        let mut stream = Vec::new();
        stream.extend_from_slice(&crate::MAGIC);
        stream.push(0x00);
        stream.extend_from_slice(OPEN_A_NO_ATTRS);
        stream.extend_from_slice(&[0x8C, 0x05, 0xDE, 0xAD, 0xF0, 0xF0]);

        let events = drain(&stream).expect("decode");
        let chars = events
            .iter()
            .find_map(|e| match e {
                Event::Characters(c) => Some(c),
                _ => None,
            })
            .expect("characters event");
        assert_eq!(
            *chars,
            CharacterChunk::Encoded(AlgorithmData::builtin(
                BuiltIn::HexBinary,
                vec![0xDE, 0xAD]
            ))
        );
    }

    #[test]
    fn aii_ea_builtin_int() {
        // <a x=[NISTRING-EA Int 32-bit 42/>
        //   7C           — EII: literal, has-attrs, qname state 0
        //   00 61        — local-name 'a'
        //   78           — AII: literal, qname state 0
        //   00 78        — attr local-name 'x'
        //   30 43        — NISTRING-EA: T=0, alg-id top 4 = 0000 (id=4, Int);
        //                  b2 = alg-id low 4 (0100) << 4 | length 4 (0011)
        //   00 00 00 2A  — Int(42) payload
        //   FF           — AII double-terminator (element has no children)
        //   F0           — document-terminator
        let mut stream = Vec::new();
        stream.extend_from_slice(&crate::MAGIC);
        stream.push(0x00);
        stream.extend_from_slice(&[
            0x7C, 0x00, 0x61, 0x78, 0x00, 0x78, 0x30, 0x43, 0x00, 0x00, 0x00, 0x2A, 0xFF, 0xF0,
        ]);

        let events = drain(&stream).expect("decode");
        let attr = events
            .iter()
            .find_map(|e| match e {
                Event::StartElement { attributes, .. } => attributes.first(),
                _ => None,
            })
            .expect("attribute");
        assert_eq!(attr.name.local_name, "x");
        assert_eq!(
            attr.value,
            CharacterChunk::Encoded(AlgorithmData::builtin(BuiltIn::Int, vec![0, 0, 0, 42]))
        );
        // Attribute-carried EA data must not leak into a Characters event.
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::Characters(_))),
            "attribute value must not leak into a Characters event"
        );
    }

    #[test]
    fn cii_ea_app_defined_not_registered() {
        // <a>[CII-EA id=100, 1-octet payload]</a>
        //   8D 90 00     — id top 2 = 01, low 6 = 100100 → 0x64 = 100; length 1
        let mut stream = Vec::new();
        stream.extend_from_slice(&crate::MAGIC);
        stream.push(0x00);
        stream.extend_from_slice(OPEN_A_NO_ATTRS);
        stream.extend_from_slice(&[0x8D, 0x90, 0x00, 0xF0, 0xF0]);

        match drain(&stream) {
            Err(Error::UnknownAlgorithm(100)) => (),
            other => panic!("expected UnknownAlgorithm(100), got {other:?}"),
        }
    }

    #[test]
    fn cii_ea_reserved_id_rejected() {
        // Same as above with id = 11 (reserved range 11..=31).
        //   8C 2C 00     — id top 2 = 00, low 6 = 001011 → 11; length 1
        let mut stream = Vec::new();
        stream.extend_from_slice(&crate::MAGIC);
        stream.push(0x00);
        stream.extend_from_slice(OPEN_A_NO_ATTRS);
        stream.extend_from_slice(&[0x8C, 0x2C, 0x00, 0xF0, 0xF0]);

        match drain(&stream) {
            Err(Error::UnknownAlgorithm(11)) => (),
            other => panic!("expected UnknownAlgorithm(11), got {other:?}"),
        }
    }

    #[test]
    fn cii_ea_app_defined_registered_via_vocabulary() {
        // Same 0x8D 0x90 stream as the unregistered test, but this time we
        // prime the reader's vocabulary with a URI for id 100 before feeding
        // the stream. The check that this is possible today doubles as
        // documentation of the seam initial-vocabulary parsing will use.
        let mut stream = Vec::new();
        stream.extend_from_slice(&crate::MAGIC);
        stream.push(0x00);
        stream.extend_from_slice(OPEN_A_NO_ATTRS);
        stream.extend_from_slice(&[0x8D, 0x90, 0xFE, 0xF0, 0xF0]);

        let mut r = Reader::new(stream.as_slice());
        // Fill IDs 32..=100 with placeholder URIs; only id 100 matters here.
        for i in APPLICATION_DEFINED_START..=100 {
            let assigned = r.voc.encoding_algorithms.push(format!("urn:test:{i}"));
            assert_eq!(assigned, i);
        }
        let mut got = None;
        while let Some(ev) = r.next_event().expect("decode") {
            if let Event::Characters(c) = ev {
                got = Some(c);
            }
        }
        assert_eq!(
            got.expect("characters"),
            CharacterChunk::Encoded(AlgorithmData {
                id: 100,
                uri: Some("urn:test:100".to_owned()),
                octets: vec![0xFE],
            })
        );
    }
    // ---- large vocabulary indices (X.891 C.27 / C.28) ----------------------
    //
    // Text-analytics streams carry thousands of distinct character chunks, so
    // a repeated chunk is referenced by an index beyond the two-octet form.

    fn roundtrip(events: &[Event]) -> Vec<Event> {
        let mut w = crate::Writer::new(Vec::new());
        for e in events {
            w.write_event(e).expect("write");
        }
        let bytes = w.finish().expect("finish");
        drain(&bytes).expect("decode")
    }

    fn el(name: &str) -> Event {
        Event::StartElement { name: QualifiedName::local(name), namespaces: Vec::new(), attributes: Vec::new() }
    }

    fn texts(events: &[Event]) -> Vec<String> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Characters(CharacterChunk::Text(s)) => Some(s.clone()),
                _ => None,
            })
            .collect()
    }

    /// `n` distinct chunks, then every one of them again, so the second pass
    /// is written as indices `0..n`.
    fn chunk_document(n: usize) -> Vec<Event> {
        let mut ev = vec![Event::StartDocument, el("r")];
        for pass in 0..2 {
            let _ = pass;
            for i in 0..n {
                ev.push(el("v"));
                ev.push(Event::Characters(CharacterChunk::Text(format!("chunk-{i:07}"))));
                ev.push(Event::EndElement);
            }
        }
        ev.push(Event::EndElement);
        ev.push(Event::EndDocument);
        ev
    }

    #[test]
    fn cii_large_index_roundtrips() {
        // Past 1040 chunks the writer uses the three-octet form.
        let n = 3000;
        let got = texts(&roundtrip(&chunk_document(n)));
        assert_eq!(got.len(), 2 * n);
        for i in 0..n {
            assert_eq!(got[i], format!("chunk-{i:07}"));
            assert_eq!(got[n + i], got[i], "indexed chunk {i}");
        }
    }

    #[test]
    fn cii_large_large_index_roundtrips() {
        // Past 263184 chunks the writer uses the four-octet form.
        let n = 263_184 + 50;
        let got = texts(&roundtrip(&chunk_document(n)));
        assert_eq!(got.len(), 2 * n);
        assert_eq!(got[n + 1039], got[1039]);
        assert_eq!(got[n + 1040], got[1040]);
        assert_eq!(got[n + 263_183], got[263_183]);
        assert_eq!(got[n + 263_184], got[263_184]);
        assert_eq!(got[2 * n - 1], format!("chunk-{:07}", n - 1));
    }

    #[test]
    fn eii_large_index_roundtrips() {
        // Past 2080 element names the writer uses the three-octet form.
        let n = 2500;
        let mut ev = vec![Event::StartDocument, el("r")];
        for _ in 0..2 {
            for i in 0..n {
                ev.push(el(&format!("e{i}")));
                ev.push(Event::EndElement);
            }
        }
        ev.push(Event::EndElement);
        ev.push(Event::EndDocument);
        let names: Vec<String> = roundtrip(&ev)
            .iter()
            .filter_map(|e| match e {
                Event::StartElement { name, .. } => Some(name.local_name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(names.len(), 1 + 2 * n);
        for i in 0..n {
            assert_eq!(names[1 + n + i], format!("e{i}"), "indexed element {i}");
        }
    }

    #[test]
    fn eii_large_large_index_decodes() {
        // Hand-built: the writer would need 526368 names first, so check the
        // arithmetic directly. 0x30 00 00 05 is index 526368 + 5; with an
        // empty table that must be a lookup failure, not "not implemented".
        let mut stream = Vec::new();
        stream.extend_from_slice(&crate::MAGIC);
        stream.push(0x00);
        stream.extend_from_slice(OPEN_A_NO_ATTRS);
        stream.extend_from_slice(&[0x30, 0x00, 0x00, 0x05]);
        let err = drain(&stream).expect_err("index out of range");
        assert!(!format!("{err}").contains("not yet implemented"), "{err}");
    }

}
