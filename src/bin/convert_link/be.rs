//! Hand-rolled big-endian reader for GameCube data. Every failed read
//! carries the byte offset; parse code returns errors instead of panicking.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BeError {
    OutOfBounds {
        offset: usize,
        wanted: usize,
        len: usize,
    },
    NotUtf8 {
        offset: usize,
    },
}

impl fmt::Display for BeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BeError::OutOfBounds {
                offset,
                wanted,
                len,
            } => write!(
                f,
                "read of {wanted} bytes at offset {offset:#x} past end of {len}-byte buffer"
            ),
            BeError::NotUtf8 { offset } => write!(f, "non-UTF-8 string at offset {offset:#x}"),
        }
    }
}

impl std::error::Error for BeError {}

pub type BeResult<T> = Result<T, BeError>;

#[derive(Clone)]
pub struct BeReader<'a> {
    data: &'a [u8],
    pos: usize,
}

// Several methods are unused until the P2/P3 chunk parsers land; all are
// exercised by the tests below.
#[allow(dead_code)]
impl<'a> BeReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Sub-reader at an absolute offset; the parent's position is unaffected.
    pub fn at(&self, pos: usize) -> BeReader<'a> {
        BeReader {
            data: self.data,
            pos,
        }
    }

    pub fn seek(&mut self, pos: usize) -> BeResult<()> {
        if pos > self.data.len() {
            return Err(BeError::OutOfBounds {
                offset: pos,
                wanted: 0,
                len: self.data.len(),
            });
        }
        self.pos = pos;
        Ok(())
    }

    pub fn skip(&mut self, n: usize) -> BeResult<()> {
        let pos = self.pos.checked_add(n).ok_or(BeError::OutOfBounds {
            offset: self.pos,
            wanted: n,
            len: self.data.len(),
        })?;
        self.seek(pos)
    }

    pub fn bytes(&mut self, n: usize) -> BeResult<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .filter(|&end| end <= self.data.len())
            .ok_or(BeError::OutOfBounds {
                offset: self.pos,
                wanted: n,
                len: self.data.len(),
            })?;
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    pub fn u8(&mut self) -> BeResult<u8> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> BeResult<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub fn i16(&mut self) -> BeResult<i16> {
        Ok(self.u16()? as i16)
    }

    pub fn u32(&mut self) -> BeResult<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn f32(&mut self) -> BeResult<f32> {
        Ok(f32::from_bits(self.u32()?))
    }

    pub fn str_fixed(&mut self, n: usize) -> BeResult<&'a str> {
        let offset = self.pos;
        let bytes = self.bytes(n)?;
        std::str::from_utf8(bytes).map_err(|_| BeError::NotUtf8 { offset })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u16_is_big_endian() {
        assert_eq!(BeReader::new(&[0x12, 0x34]).u16(), Ok(0x1234));
    }

    #[test]
    fn i16_sign() {
        assert_eq!(BeReader::new(&[0xFF, 0xFE]).i16(), Ok(-2));
        // the JNT1 rotation edge: 0x8000 = -32768 = -π in s16 angle units
        assert_eq!(BeReader::new(&[0x80, 0x00]).i16(), Ok(-32768));
    }

    #[test]
    fn u32_and_f32() {
        assert_eq!(
            BeReader::new(&[0xDE, 0xAD, 0xBE, 0xEF]).u32(),
            Ok(0xDEAD_BEEF)
        );
        assert_eq!(BeReader::new(&[0x3F, 0x80, 0x00, 0x00]).f32(), Ok(1.0));
        assert_eq!(BeReader::new(&[0xC0, 0x40, 0x00, 0x00]).f32(), Ok(-3.0));
    }

    #[test]
    fn str_fixed_reads_fourcc() {
        assert_eq!(BeReader::new(b"JNT1xx").str_fixed(4), Ok("JNT1"));
        assert_eq!(
            BeReader::new(&[0xFF, 0xFE, 0xFD, 0xFC]).str_fixed(4),
            Err(BeError::NotUtf8 { offset: 0 })
        );
    }

    #[test]
    fn seek_skip_pos_roundtrip() {
        let mut r = BeReader::new(&[0, 1, 2, 3, 4, 5]);
        r.seek(4).unwrap();
        assert_eq!(r.pos(), 4);
        assert_eq!(r.u8(), Ok(4));
        r.seek(1).unwrap();
        r.skip(2).unwrap();
        assert_eq!(r.pos(), 3);
        assert_eq!(r.u8(), Ok(3));
    }

    #[test]
    fn out_of_bounds_is_error_with_offset() {
        let mut r = BeReader::new(&[0, 1, 2]);
        r.seek(2).unwrap();
        assert_eq!(
            r.u16(),
            Err(BeError::OutOfBounds {
                offset: 2,
                wanted: 2,
                len: 3
            })
        );

        assert_eq!(
            BeReader::new(&[0, 1, 2]).seek(4),
            Err(BeError::OutOfBounds {
                offset: 4,
                wanted: 0,
                len: 3
            })
        );
        assert_eq!(
            BeReader::new(&[0, 1, 2]).bytes(8),
            Err(BeError::OutOfBounds {
                offset: 0,
                wanted: 8,
                len: 3
            })
        );
        assert_eq!(
            BeReader::new(&[]).u8(),
            Err(BeError::OutOfBounds {
                offset: 0,
                wanted: 1,
                len: 0
            })
        );
    }

    #[test]
    fn at_subreader_does_not_move_parent() {
        let r = BeReader::new(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let mut sub = r.at(2);
        assert_eq!(sub.u16(), Ok(0xCCDD));
        assert_eq!(r.pos(), 0);
    }
}
