//! A tiny little-endian encoding with bounds on everything it reads. A
//! message from a host process is untrusted: a plugin may have corrupted it,
//! so a count is checked against a limit and against the bytes left before
//! anything is allocated for it.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// The message ended before a value was complete.
    Truncated,
    /// A count or length above its limit.
    TooLarge {
        what: &'static str,
        max: usize,
        got: usize,
    },
    /// A value that is not allowed.
    Invalid(&'static str),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireError::Truncated => write!(f, "message ended early"),
            WireError::TooLarge { what, max, got } => {
                write!(f, "{what} is {got}, above the limit of {max}")
            }
            WireError::Invalid(what) => write!(f, "invalid {what}"),
        }
    }
}

impl std::error::Error for WireError {}

#[derive(Default)]
pub struct Writer(Vec<u8>);

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }

    pub fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    pub fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    pub fn raw(&mut self, v: &[u8]) {
        self.0.extend_from_slice(v);
    }

    /// A length, then the bytes.
    pub fn bytes(&mut self, v: &[u8]) {
        self.u32(v.len() as u32);
        self.raw(v);
    }

    pub fn str(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }
}

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    pub fn left(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// Fails if bytes remain: a message with trailing data is malformed.
    pub fn finish(self) -> Result<(), WireError> {
        if self.left() == 0 {
            Ok(())
        } else {
            Err(WireError::Invalid("trailing bytes"))
        }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        if self.left() < n {
            return Err(WireError::Truncated);
        }
        let out = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        let mut out = [0; N];
        out.copy_from_slice(self.take(N)?);
        Ok(out)
    }

    pub fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.array::<1>()?[0])
    }

    pub fn u16(&mut self) -> Result<u16, WireError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32, WireError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn i32(&mut self) -> Result<i32, WireError> {
        Ok(i32::from_le_bytes(self.array()?))
    }

    pub fn u64(&mut self) -> Result<u64, WireError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    pub fn f32(&mut self) -> Result<f32, WireError> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    pub fn f64(&mut self) -> Result<f64, WireError> {
        Ok(f64::from_le_bytes(self.array()?))
    }

    pub fn bool(&mut self) -> Result<bool, WireError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(WireError::Invalid("flag")),
        }
    }

    /// A count of items that each take at least `min_item` bytes. It must
    /// not exceed `max`, nor be more than the bytes left could hold.
    pub fn count(
        &mut self,
        what: &'static str,
        max: usize,
        min_item: usize,
    ) -> Result<usize, WireError> {
        let n = self.u32()? as usize;
        if n > max {
            return Err(WireError::TooLarge { what, max, got: n });
        }
        if n.saturating_mul(min_item.max(1)) > self.left() {
            return Err(WireError::Truncated);
        }
        Ok(n)
    }

    pub fn bytes(&mut self, what: &'static str, max: usize) -> Result<&'a [u8], WireError> {
        let n = self.count(what, max, 1)?;
        self.take(n)
    }

    pub fn str(&mut self, what: &'static str, max: usize) -> Result<String, WireError> {
        let bytes = self.bytes(what, max)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| WireError::Invalid("text"))
    }

    pub fn raw<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        self.array()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip() {
        let mut w = Writer::new();
        w.u8(7);
        w.u16(513);
        w.u32(70_000);
        w.i32(-5);
        w.u64(1 << 40);
        w.f32(1.5);
        w.f64(-2.25);
        w.bool(true);
        w.str("héllo");
        let bytes = w.into_bytes();
        let mut r = Reader::new(&bytes);
        assert_eq!(r.u8(), Ok(7));
        assert_eq!(r.u16(), Ok(513));
        assert_eq!(r.u32(), Ok(70_000));
        assert_eq!(r.i32(), Ok(-5));
        assert_eq!(r.u64(), Ok(1 << 40));
        assert_eq!(r.f32(), Ok(1.5));
        assert_eq!(r.f64(), Ok(-2.25));
        assert_eq!(r.bool(), Ok(true));
        assert_eq!(r.str("text", 16).as_deref(), Ok("héllo"));
        assert_eq!(r.finish(), Ok(()));
    }

    #[test]
    fn a_count_above_its_limit_is_refused_before_anything_is_reserved() {
        let mut w = Writer::new();
        w.u32(u32::MAX);
        let bytes = w.into_bytes();
        let err = Reader::new(&bytes).count("events", 100, 4).unwrap_err();
        assert!(matches!(err, WireError::TooLarge { max: 100, .. }));
    }

    #[test]
    fn a_count_the_bytes_cannot_hold_is_truncation() {
        let mut w = Writer::new();
        w.u32(50);
        w.u32(1);
        let bytes = w.into_bytes();
        let err = Reader::new(&bytes).count("events", 100, 4).unwrap_err();
        assert_eq!(err, WireError::Truncated);
    }

    #[test]
    fn short_input_and_bad_flags_are_errors() {
        assert_eq!(Reader::new(&[1, 2]).u32(), Err(WireError::Truncated));
        assert_eq!(Reader::new(&[2]).bool(), Err(WireError::Invalid("flag")));
        let mut w = Writer::new();
        w.bytes(&[0xff, 0xfe]);
        let bytes = w.into_bytes();
        assert_eq!(
            Reader::new(&bytes).str("name", 8),
            Err(WireError::Invalid("text"))
        );
    }

    #[test]
    fn trailing_bytes_are_refused() {
        assert!(Reader::new(&[1]).finish().is_err());
    }
}
