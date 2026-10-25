//! VarInt / VarLong — Minecraft wire primitives.
//!
//! Same encoding on both 1.21.1 and 26.3; the safety rules are what matter:
//! max 5 bytes (VarInt) / 10 bytes (VarLong), no trailing continuation bits,
//! no overlong encodings beyond the byte caps. Read functions are
//! allocation-free and bounds-checked — packet hooks call these on the hot
//! path for every intercepted packet.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VarIntError {
    #[error("varint exceeds {max} bytes")]
    TooLong { max: usize },
    #[error("unexpected end of buffer")]
    UnexpectedEnd,
    #[error("varint overflows i32")]
    Overflow,
}

/// Maximum encoded length of a Minecraft VarInt (5) / VarLong (10).
pub const VARINT_MAX: usize = 5;
pub const VARLONG_MAX: usize = 10;

/// Encode `v` as VarInt into `out` (appends). Returns bytes written.
pub fn write_varint(out: &mut Vec<u8>, v: i32) -> usize {
    let mut x = v as u32;
    let mut n = 0;
    loop {
        if x & !0x7F == 0 {
            out.push(x as u8);
            return n + 1;
        }
        out.push(((x & 0x7F) | 0x80) as u8);
        x >>= 7;
        n += 1;
    }
}

/// Encode `v` as VarLong into `out` (appends). Returns bytes written.
pub fn write_varlong(out: &mut Vec<u8>, v: i64) -> usize {
    let mut x = v as u64;
    let mut n = 0;
    loop {
        if x & !0x7F == 0 {
            out.push(x as u8);
            return n + 1;
        }
        out.push(((x & 0x7F) | 0x80) as u8);
        x >>= 7;
        n += 1;
    }
}

/// Read a VarInt from `buf` starting at `pos`. On success returns
/// `(value, bytes_consumed)`; `pos` is advanced.
pub fn read_varint(buf: &[u8], pos: &mut usize) -> Result<i32, VarIntError> {
    let mut value: u32 = 0;
    for i in 0..VARINT_MAX {
        let b = *buf.get(*pos).ok_or(VarIntError::UnexpectedEnd)?;
        *pos += 1;
        value |= ((b & 0x7F) as u32) << (i * 7);
        if b & 0x80 == 0 {
            return Ok(value as i32);
        }
    }
    Err(VarIntError::TooLong { max: VARINT_MAX })
}

/// Read a VarLong from `buf` starting at `pos`.
pub fn read_varlong(buf: &[u8], pos: &mut usize) -> Result<i64, VarIntError> {
    let mut value: u64 = 0;
    for i in 0..VARLONG_MAX {
        let b = *buf.get(*pos).ok_or(VarIntError::UnexpectedEnd)?;
        *pos += 1;
        value |= ((b & 0x7F) as u64) << (i * 7);
        if b & 0x80 == 0 {
            return Ok(value as i64);
        }
    }
    Err(VarIntError::TooLong { max: VARLONG_MAX })
}

/// Length-prefixed UTF-8 string as used by both protocol generations.
pub mod string {
    use super::*;

    pub fn write(out: &mut Vec<u8>, s: &str) {
        write_varint(out, s.len() as i32);
        out.extend_from_slice(s.as_bytes());
    }

    pub fn read(buf: &[u8], pos: &mut usize, max_len: usize) -> Result<String, VarIntError> {
        let len = read_varint(buf, pos)?;
        if len < 0 || len as usize > max_len {
            return Err(VarIntError::Overflow);
        }
        let end = *pos + len as usize;
        let bytes = buf.get(*pos..end).ok_or(VarIntError::UnexpectedEnd)?;
        *pos = end;
        String::from_utf8(bytes.to_vec()).map_err(|_| VarIntError::Overflow)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip_samples() {
        for v in [0, 1, 127, 128, 255, 2097151, 2097152, i32::MAX, i32::MIN, -1] {
            let mut buf = Vec::with_capacity(VARINT_MAX);
            write_varint(&mut buf, v);
            assert!(buf.len() <= VARINT_MAX, "{v} encodes to {} bytes", buf.len());
            let mut pos = 0;
            let back = read_varint(&buf, &mut pos).unwrap();
            assert_eq!(back, v, "roundtrip {v}");
            assert_eq!(pos, buf.len());
        }
    }

    #[test]
    fn varlong_roundtrip_samples() {
        for v in [0, 1, 127, 128, i64::MAX, i64::MIN, -1] {
            let mut buf = Vec::new();
            write_varlong(&mut buf, v);
            assert!(buf.len() <= VARLONG_MAX);
            let mut pos = 0;
            assert_eq!(read_varlong(&buf, &mut pos).unwrap(), v);
        }
    }

    #[test]
    fn known_encodings() {
        // Reference bytes from the protocol docs.
        let mut buf = Vec::new();
        write_varint(&mut buf, 2097152);
        assert_eq!(buf, vec![0x80, 0x80, 0x80, 0x01]);
        buf.clear();
        write_varint(&mut buf, -1);
        assert_eq!(buf, vec![0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
    }

    #[test]
    fn rejects_overlong() {
        let buf = [0xFF; 6];
        let mut pos = 0;
        assert!(matches!(
            read_varint(&buf, &mut pos),
            Err(VarIntError::TooLong { max: 5 })
        ));
    }

    #[test]
    fn rejects_truncated() {
        let buf = [0x80];
        let mut pos = 0;
        assert!(matches!(read_varint(&buf, &mut pos), Err(VarIntError::UnexpectedEnd)));
    }

    #[test]
    fn string_roundtrip() {
        let mut buf = Vec::new();
        string::write(&mut buf, "examplemod:ruby_golem");
        let mut pos = 0;
        assert_eq!(string::read(&buf, &mut pos, 32767).unwrap(), "examplemod:ruby_golem");
        // max_len guard
        let mut pos = 0;
        assert!(string::read(&buf, &mut pos, 4).is_err());
    }
}
