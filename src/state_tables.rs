//! Per-context byte dispatch tables.
//!
//! Each of EII, AII, NISTRING, ISTRING and CII reads a single octet and
//! decides what production follows. Rather than building 256-entry static
//! lookup tables (as the Java reference does), we use a `match` over the
//! ranges defined in `DecoderStateTables` — same behaviour, slightly less
//! cache-friendly, but the ranges are tiny so it doesn't matter.

#![allow(dead_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eii {
    NoAiisIndexSmall, // bit 6 = 0 (no attrs), low 5 bits = index (range 0x00..=0x1F)
    AiisIndexSmall,   // bit 6 = 1, low 5 bits = index            (0x40..=0x5F)
    IndexMedium,
    IndexLarge,
    Namespaces,
    Literal,
    CiiUtf8SmallLength,
    CiiUtf8MediumLength,
    CiiUtf8LargeLength,
    CiiUtf16SmallLength,
    CiiUtf16MediumLength,
    CiiUtf16LargeLength,
    CiiRa,
    CiiEa,
    CiiIndexSmall,
    CiiIndexMedium,
    CiiIndexLarge,
    CiiIndexLargeLarge,
    UnexpandedEntityRef,
    ProcessingInstruction,
    Comment,
    TerminatorSingle,
    TerminatorDouble,
    Illegal,
}

pub fn eii(b: u8) -> Eii {
    match b {
        0x00..=0x1F => Eii::NoAiisIndexSmall,
        0x20..=0x27 => Eii::IndexMedium,
        0x28..=0x30 => Eii::IndexLarge,
        0x38 => Eii::Namespaces,
        0x3C | 0x3D | 0x3F => Eii::Literal,
        0x40..=0x5F => Eii::AiisIndexSmall,
        0x60..=0x67 => Eii::IndexMedium,
        0x68..=0x70 => Eii::IndexLarge,
        0x78 => Eii::Namespaces,
        0x7C | 0x7D | 0x7F => Eii::Literal,
        0x80..=0x81 => Eii::CiiUtf8SmallLength,
        0x82 => Eii::CiiUtf8MediumLength,
        0x83 => Eii::CiiUtf8LargeLength,
        0x84..=0x85 => Eii::CiiUtf16SmallLength,
        0x86 => Eii::CiiUtf16MediumLength,
        0x87 => Eii::CiiUtf16LargeLength,
        0x88..=0x8B => Eii::CiiRa,
        0x8C..=0x8F => Eii::CiiEa,
        0x90..=0x91 => Eii::CiiUtf8SmallLength,
        0x92 => Eii::CiiUtf8MediumLength,
        0x93 => Eii::CiiUtf8LargeLength,
        0x94..=0x95 => Eii::CiiUtf16SmallLength,
        0x96 => Eii::CiiUtf16MediumLength,
        0x97 => Eii::CiiUtf16LargeLength,
        0x98..=0x9B => Eii::CiiRa,
        0x9C..=0x9F => Eii::CiiEa,
        0xA0..=0xAF => Eii::CiiIndexSmall,
        0xB0..=0xB3 => Eii::CiiIndexMedium,
        0xB4..=0xB7 => Eii::CiiIndexLarge,
        0xB8 => Eii::CiiIndexLargeLarge,
        0xC8..=0xCB => Eii::UnexpandedEntityRef,
        0xE1 => Eii::ProcessingInstruction,
        0xE2 => Eii::Comment,
        0xF0 => Eii::TerminatorSingle,
        0xFF => Eii::TerminatorDouble,
        _ => Eii::Illegal,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aii {
    IndexSmall,
    IndexMedium,
    IndexLarge,
    Literal,
    TerminatorSingle,
    TerminatorDouble,
    Illegal,
}

pub fn aii(b: u8) -> Aii {
    match b {
        0x00..=0x3F => Aii::IndexSmall,
        0x40..=0x5F => Aii::IndexMedium,
        0x60..=0x6F => Aii::IndexLarge,
        0x78 | 0x79 | 0x7B => Aii::Literal,
        0xF0 => Aii::TerminatorSingle,
        0xFF => Aii::TerminatorDouble,
        _ => Aii::Illegal,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nistring {
    Utf8SmallLength,
    Utf8MediumLength,
    Utf8LargeLength,
    Utf16SmallLength,
    Utf16MediumLength,
    Utf16LargeLength,
    Ra,
    Ea,
    IndexSmall,
    IndexMedium,
    IndexLarge,
    Empty,
    Illegal,
}

pub fn nistring(b: u8) -> Nistring {
    match b {
        0x00..=0x07 => Nistring::Utf8SmallLength,
        0x08 => Nistring::Utf8MediumLength,
        0x0C => Nistring::Utf8LargeLength,
        0x10..=0x17 => Nistring::Utf16SmallLength,
        0x18 => Nistring::Utf16MediumLength,
        0x1C => Nistring::Utf16LargeLength,
        0x20..=0x2F => Nistring::Ra,
        0x30..=0x3F => Nistring::Ea,
        0x40..=0x47 => Nistring::Utf8SmallLength,
        0x48 => Nistring::Utf8MediumLength,
        0x4C => Nistring::Utf8LargeLength,
        0x50..=0x57 => Nistring::Utf16SmallLength,
        0x58 => Nistring::Utf16MediumLength,
        0x5C => Nistring::Utf16LargeLength,
        0x60..=0x6F => Nistring::Ra,
        0x70..=0x7F => Nistring::Ea,
        0x80..=0xBF => Nistring::IndexSmall,
        0xC0..=0xDF => Nistring::IndexMedium,
        0xE0..=0xEF => Nistring::IndexLarge,
        0xFF => Nistring::Empty,
        _ => Nistring::Illegal,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Istring {
    SmallLength,  // literal, 1-octet length-1 (range 1..=64)
    MediumLength, // literal, extra-octet length, base 65
    LargeLength,  // literal, 4-octet length, base 321
    IndexSmall,
    IndexMedium,
    IndexLarge,
}

pub fn istring(b: u8) -> Istring {
    match b {
        0x00..=0x3F => Istring::SmallLength,
        0x40..=0x5F => Istring::MediumLength,
        0x60..=0x7F => Istring::LargeLength,
        0x80..=0xBF => Istring::IndexSmall,
        0xC0..=0xDF => Istring::IndexMedium,
        _ => Istring::IndexLarge,
    }
}
