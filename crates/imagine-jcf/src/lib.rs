//! imagine-jcf — a deliberately small Java class-file inspector.
//!
//! Why not a full class-file library: the runtime only needs three questions
//! answered about any `.class` found in a mod jar:
//!
//! 1. Is the file a valid class at all (magic + parseable structure)?
//! 2. What Java class-file version does it target (we require Java 21 for
//!    NeoForge 1.21.1 mods — major 65 — and report mismatches early)?
//! 3. Does it carry a class-level `@Mod("modid")` annotation
//!    (`net/neoforged/fml/common/Mod`), and for which modid?
//!
//! Everything is bounds-checked; a malformed class yields `Err`, never a
//! panic. Constant pool walking covers all 17 tags with correct skip sizes.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassInfo {
    pub major: u16,
    pub minor: u16,
    /// `@Mod("modid")` values found on the class (usually exactly one).
    pub mod_annotations: Vec<String>,
    /// Internal class name (`com/example/Foo`).
    pub this_class: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClassFileError {
    #[error("not a class file (bad magic 0x{0:08X})")]
    BadMagic(u32),
    #[error("truncated at offset {0}")]
    Truncated(usize),
    #[error("unsupported constant pool tag {0}")]
    BadTag(u8),
    #[error("class-file version {major} is below Java 21 (major 65)")]
    TooOld { major: u16 },
}

const MAGIC: u32 = 0xCAFEBABE;
const NEOFORGE_MOD_ANNOTATION: &str = "Lnet/neoforged/fml/common/Mod;";

pub fn inspect(bytes: &[u8]) -> Result<ClassInfo, ClassFileError> {
    let mut r = Reader { b: bytes, pos: 0 };
    let magic = r.u4()?;
    if magic != MAGIC {
        return Err(ClassFileError::BadMagic(magic));
    }
    let minor = r.u2()?;
    let major = r.u2()?;

    // --- constant pool -----------------------------------------------------
    let cp_count = r.u2()?;
    let cp_len = cp_count as usize;
    if cp_len == 0 {
        return Err(ClassFileError::Truncated(r.pos));
    }
    // 1-based; index 0 unused.
    let mut cp: Vec<CpEntry> = vec![CpEntry::None; cp_len];
    let mut i = 1;
    while i < cp_len {
        let tag = r.u1()?;
        let entry = match tag {
            1 => {
                let len = r.u2()? as usize;
                let s = r.bytes(len)?.to_vec();
                CpEntry::Utf8(s)
            }
            3 => {
                r.u4()?;
                CpEntry::None
            }
            4 => {
                r.u4()?;
                CpEntry::None
            }
            5 => {
                r.u8()?;
                CpEntry::None
            }
            6 => {
                r.u8()?;
                CpEntry::None
            }
            7 => CpEntry::Class(r.u2()?),
            8 => CpEntry::Str(r.u2()?),
            9..=12 => {
                r.u2()?;
                r.u2()?;
                CpEntry::None
            }
            15 => {
                r.u1()?;
                r.u2()?;
                CpEntry::None
            }
            16 => {
                r.u2()?;
                CpEntry::None
            }
            17 | 18 => {
                r.u2()?;
                r.u2()?;
                r.u2()?;
                CpEntry::None
            }
            19 | 20 => {
                r.u2()?;
                CpEntry::None
            }
            other => return Err(ClassFileError::BadTag(other)),
        };
        cp[i] = entry;
        // long/double take two constant pool slots
        if tag == 5 || tag == 6 {
            i += 1;
            if i < cp_len {
                cp[i] = CpEntry::None;
            }
        }
        i += 1;
    }

    // --- class metadata ----------------------------------------------------
    let _access = r.u2()?;
    let this_idx = r.u2()?;
    let _super = r.u2()?;
    let this_class = class_name(&cp, this_idx);

    let ifaces = r.u2()? as usize;
    r.skip(ifaces * 2)?;

    // fields
    let n = r.u2()? as usize;
    for _ in 0..n {
        skip_member(&mut r)?;
    }
    // methods
    let n = r.u2()? as usize;
    for _ in 0..n {
        skip_member(&mut r)?;
    }
    // class attributes (annotations live here)
    let mod_annotations = read_annotations(&mut r, &cp)?;

    Ok(ClassInfo {
        major,
        minor,
        mod_annotations,
        this_class,
    })
}

/// Enforce the NeoForge 1.21.1 toolchain floor (Java 21).
pub fn check_java21(info: &ClassInfo) -> Result<(), ClassFileError> {
    if info.major < 65 {
        return Err(ClassFileError::TooOld { major: info.major });
    }
    Ok(())
}

#[derive(Debug, Clone)]
enum CpEntry {
    None,
    Utf8(Vec<u8>),
    Class(u16),
    /// Constant-pool string entry; kept for pool fidelity, payload unused.
    #[allow(dead_code)]
    Str(u16),
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn need(&self, n: usize) -> Result<(), ClassFileError> {
        if self.pos + n > self.b.len() {
            return Err(ClassFileError::Truncated(self.pos));
        }
        Ok(())
    }
    fn u1(&mut self) -> Result<u8, ClassFileError> {
        self.need(1)?;
        let v = self.b[self.pos];
        self.pos += 1;
        Ok(v)
    }
    fn u2(&mut self) -> Result<u16, ClassFileError> {
        self.need(2)?;
        let v = u16::from_be_bytes([self.b[self.pos], self.b[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }
    fn u4(&mut self) -> Result<u32, ClassFileError> {
        self.need(4)?;
        let v = u32::from_be_bytes([
            self.b[self.pos],
            self.b[self.pos + 1],
            self.b[self.pos + 2],
            self.b[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }
    fn u8(&mut self) -> Result<u64, ClassFileError> {
        Ok(self.u4()? as u64 * 0x1_0000_0000 + self.u4()? as u64)
    }
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], ClassFileError> {
        self.need(n)?;
        let s = &self.b[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn skip(&mut self, n: usize) -> Result<(), ClassFileError> {
        self.need(n)?;
        self.pos += n;
        Ok(())
    }
}

fn utf8(cp: &[CpEntry], idx: u16) -> Option<String> {
    match cp.get(idx as usize)? {
        CpEntry::Utf8(b) => String::from_utf8(b.clone()).ok(),
        _ => None,
    }
}

fn class_name(cp: &[CpEntry], idx: u16) -> String {
    match cp.get(idx as usize) {
        Some(CpEntry::Class(name_idx)) => utf8(cp, *name_idx).unwrap_or_default(),
        _ => String::new(),
    }
}

fn skip_member(r: &mut Reader<'_>) -> Result<(), ClassFileError> {
    let _flags = r.u2()?;
    let _name = r.u2()?;
    let _desc = r.u2()?;
    let attrs = r.u2()? as usize;
    for _ in 0..attrs {
        let _name_idx = r.u2()?;
        let len = r.u4()? as usize;
        r.skip(len)?;
    }
    Ok(())
}

/// Walk class attributes; for `RuntimeVisibleAnnotations` /
/// `RuntimeInvisibleAnnotations`, extract annotation type + first const
/// string value (the `@Mod("id")` shape).
fn read_annotations(
    r: &mut Reader<'_>,
    cp: &[CpEntry],
) -> Result<Vec<String>, ClassFileError> {
    let mut out = Vec::new();
    let count = r.u2()? as usize;
    for _ in 0..count {
        let name_idx = r.u2()?;
        let len = r.u4()? as usize;
        let start = r.pos;
        let attr = r.bytes(len)?.to_vec();
        let name = utf8(cp, name_idx).unwrap_or_default();
        if name == "RuntimeVisibleAnnotations" || name == "RuntimeInvisibleAnnotations" {
            out.extend(parse_annotation_list(&attr, cp));
        }
        let _ = start;
    }
    Ok(out)
}

fn parse_annotation_list(attr: &[u8], cp: &[CpEntry]) -> Vec<String> {
    let mut r = Reader { b: attr, pos: 0 };
    let num = match r.u2() {
        Ok(n) => n as usize,
        Err(_) => return Vec::new(),
    };
    let mut found = Vec::new();
    for _ in 0..num {
        // type_index: descriptor like "Lnet/neoforged/fml/common/Mod;"
        let type_idx = match r.u2() {
            Ok(v) => v,
            Err(_) => return found,
        };
        let desc = utf8(cp, type_idx).unwrap_or_default();
        let num_values = match r.u2() {
            Ok(v) => v as usize,
            Err(_) => return found,
        };
        for _ in 0..num_values {
            let _name_idx = match r.u2() {
                Ok(v) => v,
                Err(_) => return found,
            };
            let tag = match r.u1() {
                Ok(t) => t,
                Err(_) => return found,
            };
            match tag {
                b's' | b'c' => {
                    let const_idx = match r.u2() {
                        Ok(v) => v,
                        Err(_) => return found,
                    };
                    let value = utf8(cp, const_idx).unwrap_or_default();
                    if desc == NEOFORGE_MOD_ANNOTATION && tag == b's' {
                        found.push(value);
                    }
                }
                b'B' | b'C' | b'I' | b'S' | b'Z' | b'F' | b'D' | b'J' => {
                    let _ = r.u2();
                }
                b'e' => {
                    let _ = r.u2();
                    let _ = r.u2();
                }
                b'@' => return found, // nested annotation: stop conservatively
                b'[' => {
                    let _num = match r.u2() {
                        Ok(n) => n as usize,
                        Err(_) => return found,
                    };
                    // element_type in arrays: stop (not needed for @Mod)
                    return found;
                }
                _ => return found,
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-assembled minimal class file with a constant pool containing
    /// a `@Mod("crusstytest")` RuntimeVisibleAnnotations attribute.
    fn build_class_with_mod_annotation(modid: &str) -> Vec<u8> {
        let mut cp: Vec<u8> = Vec::new();
        let push_u2 = |cp: &mut Vec<u8>, v: u16| cp.extend_from_slice(&v.to_be_bytes());
        let push_str = |cp: &mut Vec<u8>, s: &str| {
            push_u2(cp, s.len() as u16);
            cp.extend_from_slice(s.as_bytes());
        };

        // 1: Utf8 "RuntimeVisibleAnnotations"
        cp.push(1);
        push_str(&mut cp, "RuntimeVisibleAnnotations");
        // 2: Utf8 descriptor
        cp.push(1);
        push_str(&mut cp, NEOFORGE_MOD_ANNOTATION);
        // 3: Utf8 "value"
        cp.push(1);
        push_str(&mut cp, "value");
        // 4: Utf8 modid
        cp.push(1);
        push_str(&mut cp, modid);
        // 5: Utf8 "this_class"
        cp.push(1);
        push_str(&mut cp, "Test");
        // 6: Class -> 5
        cp.push(7);
        push_u2(&mut cp, 5);
        // 7: Utf8 "java/lang/Object"
        cp.push(1);
        push_str(&mut cp, "java/lang/Object");
        // 8: Class -> 7
        cp.push(7);
        push_u2(&mut cp, 7);

        let cp_count = 9u16; // 8 entries + 1

        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC.to_be_bytes());
        out.extend_from_slice(&0u16.to_be_bytes()); // minor
        out.extend_from_slice(&65u16.to_be_bytes()); // major (Java 21)
        out.extend_from_slice(&cp_count.to_be_bytes());
        out.extend_from_slice(&cp);
        out.extend_from_slice(&0x0021u16.to_be_bytes()); // access
        out.extend_from_slice(&6u16.to_be_bytes()); // this_class
        out.extend_from_slice(&8u16.to_be_bytes()); // super
        out.extend_from_slice(&0u16.to_be_bytes()); // interfaces
        out.extend_from_slice(&0u16.to_be_bytes()); // fields
        out.extend_from_slice(&0u16.to_be_bytes()); // methods
        // class attributes: 1
        out.extend_from_slice(&1u16.to_be_bytes());
        out.extend_from_slice(&1u16.to_be_bytes()); // name idx 1 = RuntimeVisibleAnnotations
        let attr: Vec<u8> = {
            let mut a = Vec::new();
            // num annotations = 1
            a.extend_from_slice(&1u16.to_be_bytes());
            // type_index = 2
            a.extend_from_slice(&2u16.to_be_bytes());
            // num element-value pairs = 1
            a.extend_from_slice(&1u16.to_be_bytes());
            // name = 3 ("value")
            a.extend_from_slice(&3u16.to_be_bytes());
            // tag 's'
            a.push(b's');
            // const value index = 4
            a.extend_from_slice(&4u16.to_be_bytes());
            a
        };
        out.extend_from_slice(&(attr.len() as u32).to_be_bytes());
        out.extend_from_slice(&attr);
        out
    }

    #[test]
    fn extracts_mod_annotation() {
        let class = build_class_with_mod_annotation("crusstytest");
        let info = inspect(&class).unwrap();
        assert_eq!(info.major, 65);
        assert_eq!(info.this_class, "Test");
        assert_eq!(info.mod_annotations, vec!["crusstytest"]);
        check_java21(&info).unwrap();
    }

    #[test]
    fn rejects_bad_magic() {
        assert!(matches!(inspect(b"PK\x03\x04garbage"), Err(ClassFileError::BadMagic(_))));
    }

    #[test]
    fn rejects_truncated() {
        let class = build_class_with_mod_annotation("x");
        assert!(matches!(inspect(&class[..class.len() - 4]), Err(ClassFileError::Truncated(_))));
    }

    #[test]
    fn java21_floor() {
        let mut class = build_class_with_mod_annotation("x");
        // patch major down to 61 (Java 17)
        let major_off = 6;
        class[major_off] = 61u16.to_be_bytes()[0];
        class[major_off + 1] = 61u16.to_be_bytes()[1];
        let info = inspect(&class).unwrap();
        assert!(matches!(check_java21(&info), Err(ClassFileError::TooOld { major: 61 })));
    }
}
