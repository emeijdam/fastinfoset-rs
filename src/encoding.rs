//! On-wire constants from X.891.
//!
//! Names mirror `com.sun.xml.fastinfoset.EncodingConstants` so cross-checking
//! against the reference implementation is straightforward.

#![allow(dead_code)]

// --- Document information item (DII) header flags --------------------------
// Layout of the single byte that follows the 4-octet magic. MSB is reserved
// and always 0.
pub const DII_ADDITIONAL_DATA_FLAG: u8 = 0x40;
pub const DII_INITIAL_VOCABULARY_FLAG: u8 = 0x20;
pub const DII_NOTATIONS_FLAG: u8 = 0x10;
pub const DII_UNPARSED_ENTITIES_FLAG: u8 = 0x08;
pub const DII_CHARACTER_ENCODING_SCHEME_FLAG: u8 = 0x04;
pub const DII_STANDALONE_FLAG: u8 = 0x02;
pub const DII_VERSION_FLAG: u8 = 0x01;

// --- Element / attribute flag bits within their identification octet -------
pub const ELEMENT_ATTRIBUTE_FLAG: u8 = 0x40;
pub const LITERAL_QNAME_PREFIX_NAMESPACE_NAME_MASK: u8 = 0x03;
pub const LITERAL_QNAME_PREFIX_FLAG: u8 = 0x02;
pub const LITERAL_QNAME_NAMESPACE_NAME_FLAG: u8 = 0x01;

pub const CHARACTER_CHUNK_ADD_TO_TABLE_FLAG: u8 = 0x10;
pub const NISTRING_ADD_TO_TABLE_FLAG: u8 = 0x40;

// --- 5th-bit octet-string length encoding (NISTRING attribute values) ------
pub const LEN5_SMALL_MASK: u8 = 0x07;
pub const LEN5_SMALL_LIMIT: u32 = 9; // first medium-encoded length
pub const LEN5_MEDIUM_LIMIT: u32 = 265; // first large-encoded length

// --- 2nd-bit octet-string length encoding (literal qualified-name parts) ---
pub const LEN2_SMALL_LIMIT: u32 = 65;
pub const LEN2_MEDIUM_LIMIT: u32 = 321;

// --- 7th-bit octet-string length encoding (character chunks) ---------------
pub const LEN7_SMALL_LIMIT: u32 = 3;
pub const LEN7_MEDIUM_LIMIT: u32 = 259;

// --- Integer-on-second-bit (used for indices) ------------------------------
pub const INT2_SMALL_MASK: u8 = 0x3F;
pub const INT2_MEDIUM_MASK: u8 = 0x1F;
pub const INT2_LARGE_MASK: u8 = 0x0F;
pub const INT2_SMALL_LIMIT: u32 = 64;
pub const INT2_MEDIUM_LIMIT: u32 = 8256;

// --- Terminators -----------------------------------------------------------
pub const TERMINATOR: u8 = 0xF0;
pub const DOUBLE_TERMINATOR: u8 = 0xFF;

// --- Encoding algorithm / restricted alphabet ranges -----------------------
pub const ENCODING_ALGORITHM_BUILTIN_END: u32 = 9;
pub const ENCODING_ALGORITHM_APPLICATION_START: u32 = 32;

// --- Identification octets for non-element children of the document -------
pub const PROCESSING_INSTRUCTION: u8 = 0xE1;
pub const COMMENT: u8 = 0xE2;
pub const DOCUMENT_TYPE_DECLARATION: u8 = 0xC4;
pub const UNEXPANDED_ENTITY_REFERENCE: u8 = 0xC8;

// --- Encoder-side identification prefixes ----------------------------------
pub const ELEMENT_LITERAL_QNAME_FLAG: u8 = 0x3C;
pub const ATTRIBUTE_LITERAL_QNAME_FLAG: u8 = 0x78;
pub const CHARACTER_CHUNK: u8 = 0x80;
pub const CHARACTER_CHUNK_INDEX_PREFIX: u8 = 0xA0; // 1010xxxx — index follows on 4th-bit integer

// --- Length-encoding small limits (encoder side, decoder uses these too) ---
pub const LEN2_SMALL_LIMIT_ENC: u32 = 65; // first multi-octet length
pub const LEN5_SMALL_LIMIT_ENC: u32 = 9;
pub const LEN7_SMALL_LIMIT_ENC: u32 = 3;
pub const LEN2_MEDIUM_LIMIT_ENC: u32 = 321;
pub const LEN5_MEDIUM_LIMIT_ENC: u32 = 265;
pub const LEN7_MEDIUM_LIMIT_ENC: u32 = 259;

// 2nd-bit length flag octets (top bits only — `_b | flag` set on first octet)
pub const LEN2_MEDIUM_FLAG: u8 = 0x40;
pub const LEN2_LARGE_FLAG: u8 = 0x60;
// 5th-bit length flags (NISTRING)
pub const LEN5_MEDIUM_FLAG: u8 = 0x08;
pub const LEN5_LARGE_FLAG: u8 = 0x0C;
// 7th-bit length flags (CII)
pub const LEN7_MEDIUM_FLAG: u8 = 0x02;
pub const LEN7_LARGE_FLAG: u8 = 0x03;

// Integer-encoding bit-3 (element-name index) flags
pub const INT3_SMALL_LIMIT: u32 = 32;
pub const INT3_MEDIUM_LIMIT: u32 = 2080;
pub const INT3_LARGE_LIMIT: u32 = 526368;
pub const INT3_MEDIUM_FLAG: u8 = 0x20;
pub const INT3_LARGE_FLAG: u8 = 0x28;
pub const INT3_LARGE_LARGE_FLAG: u8 = 0x30;

// Integer-encoding bit-4 (character-chunk index) flags
pub const INT4_SMALL_LIMIT: u32 = 16;
pub const INT4_MEDIUM_LIMIT: u32 = 1040;
pub const INT4_LARGE_LIMIT: u32 = 263184;
pub const INT4_MEDIUM_FLAG: u8 = 0x10;
pub const INT4_LARGE_FLAG: u8 = 0x14;
pub const INT4_LARGE_LARGE_FLAG: u8 = 0x18;

// Integer-encoding bit-2 large limit (other-bit-2 stuff)
pub const INT2_LARGE_LIMIT: u32 = 1_048_576;
