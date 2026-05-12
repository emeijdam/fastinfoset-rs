//! Bit-level I/O. Fast Infoset packs identifiers bit-by-bit, MSB-first,
//! with octet alignment at specific points in the grammar (§ 8.2 of X.891).

use std::io::{Read, Write};

use crate::error::{Error, Result};

pub struct BitReader<R: Read> {
    inner: R,
    /// The current byte being consumed; valid bits live in the high `bits_left` positions.
    current: u8,
    /// Number of unread bits in `current` (0..=8).
    bits_left: u8,
    /// Total octets consumed from `inner`. Useful for diagnostics.
    octets_read: u64,
}

impl<R: Read> BitReader<R> {
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            current: 0,
            bits_left: 0,
            octets_read: 0,
        }
    }

    pub fn read_bit(&mut self) -> Result<bool> {
        if self.bits_left == 0 {
            self.refill()?;
        }
        let bit = (self.current & 0x80) != 0;
        self.current <<= 1;
        self.bits_left -= 1;
        Ok(bit)
    }

    /// Read up to 32 bits, MSB-first; the result is right-aligned.
    pub fn read_bits(&mut self, count: u8) -> Result<u32> {
        assert!(count <= 32, "read_bits supports at most 32 bits");
        let mut v: u32 = 0;
        for _ in 0..count {
            v = (v << 1) | u32::from(self.read_bit()?);
        }
        Ok(v)
    }

    /// Discard any leftover bits to align on the next octet boundary.
    pub fn align(&mut self) {
        self.bits_left = 0;
        self.current = 0;
    }

    pub fn is_aligned(&self) -> bool {
        self.bits_left == 0
    }

    pub fn read_octet(&mut self) -> Result<u8> {
        if self.bits_left != 0 {
            // Read 8 bits straddling the current byte boundary.
            return Ok(self.read_bits(8)? as u8);
        }
        self.refill()?;
        let b = self.current;
        self.current = 0;
        self.bits_left = 0;
        Ok(b)
    }

    pub fn read_octets(&mut self, n: usize) -> Result<Vec<u8>> {
        let mut buf = Vec::with_capacity(n);
        if self.bits_left == 0 {
            buf.resize(n, 0);
            let mut filled = 0;
            while filled < n {
                let got = self.inner.read(&mut buf[filled..])?;
                if got == 0 {
                    return Err(Error::UnexpectedEof);
                }
                filled += got;
            }
            self.octets_read += n as u64;
        } else {
            for _ in 0..n {
                buf.push(self.read_bits(8)? as u8);
            }
        }
        Ok(buf)
    }

    /// Total octets read from the underlying reader. Mid-byte progress is not counted.
    pub fn octet_position(&self) -> u64 {
        self.octets_read
    }

    pub fn into_inner(self) -> R {
        self.inner
    }

    fn refill(&mut self) -> Result<()> {
        let mut buf = [0u8; 1];
        match self.inner.read(&mut buf)? {
            0 => Err(Error::UnexpectedEof),
            _ => {
                self.current = buf[0];
                self.bits_left = 8;
                self.octets_read += 1;
                Ok(())
            }
        }
    }
}

pub struct BitWriter<W: Write> {
    inner: W,
    /// Accumulator: filled bits sit in the high positions, low (`8 - bits_filled`) bits are zero.
    current: u8,
    bits_filled: u8,
    octets_written: u64,
}

impl<W: Write> BitWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            current: 0,
            bits_filled: 0,
            octets_written: 0,
        }
    }

    pub fn write_bit(&mut self, bit: bool) -> Result<()> {
        if bit {
            self.current |= 1 << (7 - self.bits_filled);
        }
        self.bits_filled += 1;
        if self.bits_filled == 8 {
            self.flush_current()?;
        }
        Ok(())
    }

    /// Write the low `count` bits of `value`, MSB-first.
    pub fn write_bits(&mut self, value: u32, count: u8) -> Result<()> {
        assert!(count <= 32, "write_bits supports at most 32 bits");
        for i in (0..count).rev() {
            self.write_bit(((value >> i) & 1) == 1)?;
        }
        Ok(())
    }

    /// Pad the current byte with zeros to align on the next octet boundary.
    pub fn align(&mut self) -> Result<()> {
        if self.bits_filled > 0 {
            self.flush_current()?;
        }
        Ok(())
    }

    pub fn is_aligned(&self) -> bool {
        self.bits_filled == 0
    }

    pub fn write_octet(&mut self, byte: u8) -> Result<()> {
        if self.bits_filled == 0 {
            self.inner.write_all(&[byte])?;
            self.octets_written += 1;
            Ok(())
        } else {
            self.write_bits(u32::from(byte), 8)
        }
    }

    pub fn write_octets(&mut self, bytes: &[u8]) -> Result<()> {
        if self.bits_filled == 0 {
            self.inner.write_all(bytes)?;
            self.octets_written += bytes.len() as u64;
            Ok(())
        } else {
            for b in bytes {
                self.write_bits(u32::from(*b), 8)?;
            }
            Ok(())
        }
    }

    pub fn octet_position(&self) -> u64 {
        self.octets_written
    }

    pub fn finish(mut self) -> Result<W> {
        self.align()?;
        self.inner.flush()?;
        Ok(self.inner)
    }

    fn flush_current(&mut self) -> Result<()> {
        self.inner.write_all(&[self.current])?;
        self.current = 0;
        self.bits_filled = 0;
        self.octets_written += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_bits() {
        let mut buf = Vec::new();
        let mut w = BitWriter::new(&mut buf);
        // Pattern: 0, 11, 0, 1111, 00, then 8 bits 0xA5
        w.write_bit(false).unwrap();
        w.write_bits(0b11, 2).unwrap();
        w.write_bit(false).unwrap();
        w.write_bits(0b1111, 4).unwrap();
        w.write_bits(0b00, 2).unwrap();
        w.write_octet(0xA5).unwrap();
        w.align().unwrap();
        drop(w);

        let mut r = BitReader::new(&buf[..]);
        assert_eq!(r.read_bit().unwrap(), false);
        assert_eq!(r.read_bits(2).unwrap(), 0b11);
        assert_eq!(r.read_bit().unwrap(), false);
        assert_eq!(r.read_bits(4).unwrap(), 0b1111);
        assert_eq!(r.read_bits(2).unwrap(), 0b00);
        assert_eq!(r.read_octet().unwrap(), 0xA5);
    }

    #[test]
    fn straddling_octet_read() {
        // Two bytes: 0xE0 0x00. Reading 4 bits (1110), then 12 bits (0000 0000 0000)
        let data = [0xE0u8, 0x00];
        let mut r = BitReader::new(&data[..]);
        assert_eq!(r.read_bits(4).unwrap(), 0b1110);
        assert_eq!(r.read_bits(12).unwrap(), 0);
    }

    #[test]
    fn magic_decodes() {
        let data = crate::MAGIC;
        let mut r = BitReader::new(&data[..]);
        assert_eq!(r.read_octets(4).unwrap(), data);
    }

    #[test]
    fn aligned_octet_after_bits() {
        let data = [0b1010_0000u8, 0x42];
        let mut r = BitReader::new(&data[..]);
        assert_eq!(r.read_bits(3).unwrap(), 0b101);
        // Align then read full octet.
        r.align();
        assert_eq!(r.read_octet().unwrap(), 0x42);
    }
}
