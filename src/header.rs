//! Document header — the bytes between the magic and the first child of
//! the document information item (§ 12.6).

use std::io::{Read, Write};

use crate::encoding::*;
use crate::error::{Error, Result};
use crate::MAGIC;

/// Optional components advertised in the document header.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HeaderFlags {
    pub additional_data: bool,
    pub initial_vocabulary: bool,
    pub notations: bool,
    pub unparsed_entities: bool,
    pub character_encoding_scheme: bool,
    pub standalone: bool,
    pub version: bool,
}

impl HeaderFlags {
    pub fn any(self) -> bool {
        self.additional_data
            || self.initial_vocabulary
            || self.notations
            || self.unparsed_entities
            || self.character_encoding_scheme
            || self.standalone
            || self.version
    }

    fn from_byte(b: u8) -> Self {
        Self {
            additional_data: (b & DII_ADDITIONAL_DATA_FLAG) != 0,
            initial_vocabulary: (b & DII_INITIAL_VOCABULARY_FLAG) != 0,
            notations: (b & DII_NOTATIONS_FLAG) != 0,
            unparsed_entities: (b & DII_UNPARSED_ENTITIES_FLAG) != 0,
            character_encoding_scheme: (b & DII_CHARACTER_ENCODING_SCHEME_FLAG) != 0,
            standalone: (b & DII_STANDALONE_FLAG) != 0,
            version: (b & DII_VERSION_FLAG) != 0,
        }
    }

    fn to_byte(self) -> u8 {
        let mut b = 0u8;
        if self.additional_data {
            b |= DII_ADDITIONAL_DATA_FLAG;
        }
        if self.initial_vocabulary {
            b |= DII_INITIAL_VOCABULARY_FLAG;
        }
        if self.notations {
            b |= DII_NOTATIONS_FLAG;
        }
        if self.unparsed_entities {
            b |= DII_UNPARSED_ENTITIES_FLAG;
        }
        if self.character_encoding_scheme {
            b |= DII_CHARACTER_ENCODING_SCHEME_FLAG;
        }
        if self.standalone {
            b |= DII_STANDALONE_FLAG;
        }
        if self.version {
            b |= DII_VERSION_FLAG;
        }
        b
    }
}

pub fn read_header<R: Read>(reader: &mut R) -> Result<HeaderFlags> {
    let mut magic = [0u8; 4];
    reader.read_exact(&mut magic).map_err(map_eof)?;
    if magic != MAGIC {
        return Err(Error::BadMagic);
    }
    let mut b = [0u8; 1];
    reader.read_exact(&mut b).map_err(map_eof)?;
    Ok(HeaderFlags::from_byte(b[0]))
}

pub fn write_header<W: Write>(writer: &mut W, flags: HeaderFlags) -> Result<()> {
    writer.write_all(&MAGIC)?;
    writer.write_all(&[flags.to_byte()])?;
    Ok(())
}

fn map_eof(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::UnexpectedEof {
        Error::UnexpectedEof
    } else {
        Error::Io(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_empty_header() {
        let mut buf = Vec::new();
        write_header(&mut buf, HeaderFlags::default()).unwrap();
        assert_eq!(buf, [0xE0, 0x00, 0x00, 0x01, 0x00]);

        let flags = read_header(&mut buf.as_slice()).unwrap();
        assert!(!flags.any());
    }

    #[test]
    fn round_trip_all_flags() {
        let f = HeaderFlags {
            additional_data: true,
            initial_vocabulary: true,
            notations: true,
            unparsed_entities: true,
            character_encoding_scheme: true,
            standalone: true,
            version: true,
        };
        let mut buf = Vec::new();
        write_header(&mut buf, f).unwrap();
        assert_eq!(buf[4], 0x7F);
        let g = read_header(&mut buf.as_slice()).unwrap();
        assert_eq!(g, f);
    }

    #[test]
    fn rejects_bad_magic() {
        let data = [0u8; 5];
        let err = read_header(&mut data.as_slice()).unwrap_err();
        matches!(err, Error::BadMagic);
    }
}
