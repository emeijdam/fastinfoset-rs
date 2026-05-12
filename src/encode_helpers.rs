//! Encoder-side bit helpers — length and integer encodings.
//!
//! All routines write bytes to `W: Write`; they don't manage state. The
//! `_b` flag-prefix byte from the Java encoder is passed in explicitly as
//! `prefix` where applicable.

use std::io::Write;

use crate::encoding::*;
use crate::error::{Error, Result};

#[inline]
fn put(w: &mut impl Write, byte: u8) -> Result<()> {
    w.write_all(&[byte]).map_err(Error::Io)
}

// ---- octet-string length, starts on the 2nd bit (literal qname components) -

pub fn write_length_2nd_bit<W: Write>(w: &mut W, length: u32) -> Result<()> {
    if length < LEN2_SMALL_LIMIT_ENC {
        put(w, (length - 1) as u8)
    } else if length < LEN2_MEDIUM_LIMIT_ENC {
        put(w, LEN2_MEDIUM_FLAG)?;
        put(w, (length - LEN2_SMALL_LIMIT_ENC) as u8)
    } else {
        put(w, LEN2_LARGE_FLAG)?;
        let v = length - LEN2_MEDIUM_LIMIT_ENC;
        w.write_all(&v.to_be_bytes()).map_err(Error::Io)
    }
}

// ---- octet-string length, starts on the 5th bit (NISTRING values) ---------

pub fn write_length_5th_bit<W: Write>(w: &mut W, prefix: u8, length: u32) -> Result<()> {
    if length < LEN5_SMALL_LIMIT_ENC {
        put(w, prefix | ((length - 1) as u8))
    } else if length < LEN5_MEDIUM_LIMIT_ENC {
        put(w, prefix | LEN5_MEDIUM_FLAG)?;
        put(w, (length - LEN5_SMALL_LIMIT_ENC) as u8)
    } else {
        put(w, prefix | LEN5_LARGE_FLAG)?;
        let v = length - LEN5_MEDIUM_LIMIT_ENC;
        w.write_all(&v.to_be_bytes()).map_err(Error::Io)
    }
}

// ---- octet-string length, starts on the 7th bit (CII character chunks) ----

pub fn write_length_7th_bit<W: Write>(w: &mut W, prefix: u8, length: u32) -> Result<()> {
    if length < LEN7_SMALL_LIMIT_ENC {
        put(w, prefix | ((length - 1) as u8))
    } else if length < LEN7_MEDIUM_LIMIT_ENC {
        put(w, prefix | LEN7_MEDIUM_FLAG)?;
        put(w, (length - LEN7_SMALL_LIMIT_ENC) as u8)
    } else {
        put(w, prefix | LEN7_LARGE_FLAG)?;
        let v = length - LEN7_MEDIUM_LIMIT_ENC;
        w.write_all(&v.to_be_bytes()).map_err(Error::Io)
    }
}

// ---- integer on 2nd bit with first bit forced to 1 (string index ref) -----

pub fn write_int_2nd_bit_b1<W: Write>(w: &mut W, i: u32) -> Result<()> {
    if i < u32::from(INT2_SMALL_MASK) + 1 {
        // [0, 63] — 6 bits + leading "10"
        put(w, 0x80 | (i as u8))
    } else if i < 8256 {
        let v = i - 64;
        put(w, 0x80 | LEN2_MEDIUM_FLAG | ((v >> 8) as u8))?;
        put(w, (v & 0xFF) as u8)
    } else if i < INT2_LARGE_LIMIT {
        let v = i - 8256;
        put(w, 0x80 | LEN2_LARGE_FLAG | ((v >> 16) as u8))?;
        put(w, ((v >> 8) & 0xFF) as u8)?;
        put(w, (v & 0xFF) as u8)
    } else {
        Err(Error::Other(format!("integer index {i} exceeds maximum")))
    }
}

// ---- integer on 2nd bit with first bit forced to 0 (attr-name index) ------

pub fn write_int_2nd_bit_b0<W: Write>(w: &mut W, i: u32) -> Result<()> {
    if i < 64 {
        put(w, i as u8)
    } else if i < 8256 {
        let v = i - 64;
        put(w, LEN2_MEDIUM_FLAG | ((v >> 8) as u8))?;
        put(w, (v & 0xFF) as u8)
    } else if i < INT2_LARGE_LIMIT {
        let v = i - 8256;
        put(w, LEN2_LARGE_FLAG | ((v >> 16) as u8))?;
        put(w, ((v >> 8) & 0xFF) as u8)?;
        put(w, (v & 0xFF) as u8)
    } else {
        Err(Error::Other(format!("integer index {i} exceeds maximum")))
    }
}

// ---- integer on 3rd bit (element-name index) ------------------------------

pub fn write_int_3rd_bit<W: Write>(w: &mut W, prefix: u8, i: u32) -> Result<()> {
    if i < INT3_SMALL_LIMIT {
        put(w, prefix | (i as u8))
    } else if i < INT3_MEDIUM_LIMIT {
        let v = i - INT3_SMALL_LIMIT;
        put(w, prefix | INT3_MEDIUM_FLAG | ((v >> 8) as u8))?;
        put(w, (v & 0xFF) as u8)
    } else if i < INT3_LARGE_LIMIT {
        let v = i - INT3_MEDIUM_LIMIT;
        put(w, prefix | INT3_LARGE_FLAG | ((v >> 16) as u8))?;
        put(w, ((v >> 8) & 0xFF) as u8)?;
        put(w, (v & 0xFF) as u8)
    } else if i < INT2_LARGE_LIMIT {
        let v = i - INT3_LARGE_LIMIT;
        put(w, prefix | INT3_LARGE_LARGE_FLAG)?;
        put(w, (v >> 16) as u8)?;
        put(w, ((v >> 8) & 0xFF) as u8)?;
        put(w, (v & 0xFF) as u8)
    } else {
        Err(Error::Other(format!("element-name index {i} exceeds maximum")))
    }
}

// ---- integer on 4th bit (character-chunk index, restricted alphabet, etc.) -

pub fn write_int_4th_bit<W: Write>(w: &mut W, prefix: u8, i: u32) -> Result<()> {
    if i < INT4_SMALL_LIMIT {
        put(w, prefix | (i as u8))
    } else if i < INT4_MEDIUM_LIMIT {
        let v = i - INT4_SMALL_LIMIT;
        put(w, prefix | INT4_MEDIUM_FLAG | ((v >> 8) as u8))?;
        put(w, (v & 0xFF) as u8)
    } else if i < INT4_LARGE_LIMIT {
        let v = i - INT4_MEDIUM_LIMIT;
        put(w, prefix | INT4_LARGE_FLAG | ((v >> 16) as u8))?;
        put(w, ((v >> 8) & 0xFF) as u8)?;
        put(w, (v & 0xFF) as u8)
    } else if i < INT2_LARGE_LIMIT {
        let v = i - INT4_LARGE_LIMIT;
        put(w, prefix | INT4_LARGE_LARGE_FLAG)?;
        put(w, (v >> 16) as u8)?;
        put(w, ((v >> 8) & 0xFF) as u8)?;
        put(w, (v & 0xFF) as u8)
    } else {
        Err(Error::Other(format!("character-chunk index {i} exceeds maximum")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_2nd_bit_small() {
        // length 16 → byte 0x0F (length-1 in low 7 bits, MSB clear).
        let mut out = Vec::new();
        write_length_2nd_bit(&mut out, 16).unwrap();
        assert_eq!(out, vec![0x0F]);
    }

    #[test]
    fn length_5th_bit_with_add_to_table() {
        // NISTRING add-to-table UTF-8 small length 4 → 0x43
        let mut out = Vec::new();
        write_length_5th_bit(&mut out, NISTRING_ADD_TO_TABLE_FLAG, 4).unwrap();
        assert_eq!(out, vec![0x43]);
    }

    #[test]
    fn length_5th_bit_medium() {
        // NISTRING add-to-table UTF-8 medium length 19 → 0x48 0x0A
        let mut out = Vec::new();
        write_length_5th_bit(&mut out, NISTRING_ADD_TO_TABLE_FLAG, 19).unwrap();
        assert_eq!(out, vec![0x48, 0x0A]);
    }

    #[test]
    fn length_7th_bit_small() {
        // CII add-to-table UTF-8 small length 1 → 0x90 (when prefix = 0x80|0x10)
        let mut out = Vec::new();
        write_length_7th_bit(
            &mut out,
            CHARACTER_CHUNK | CHARACTER_CHUNK_ADD_TO_TABLE_FLAG,
            1,
        )
        .unwrap();
        assert_eq!(out, vec![0x90]);
    }

    #[test]
    fn int_2nd_bit_first_bit_one_small() {
        // index 0 → 0x80
        let mut out = Vec::new();
        write_int_2nd_bit_b1(&mut out, 0).unwrap();
        assert_eq!(out, vec![0x80]);
    }
}
