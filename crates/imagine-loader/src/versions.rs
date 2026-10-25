//! Maven-style version ranges & version comparison (subset used by
//! `neoforge.mods.toml` `versionRange` values).
//!
//! Supported grammar (matches the common cases found in real mods):
//!
//! ```text
//! version_range := set ("," set)*          # union of sets, any match = ok
//! set           := exact | interval
//! exact         := VERSION                 # e.g. 1.21.1
//! interval      := ("[" | "(") (VERSION)? "," (VERSION)? ("]" | ")")
//! VERSION       := dotted/alphanumeric token, e.g. 21.1.77, 1.21.x
//! ```
//!
//! Version comparison follows Maven's ComparableVersion ordering for the
//! common cases: split on `.` and `-`; numeric tokens compare numerically
//! and rank above non-numeric ones at the same position; known qualifiers
//! (`alpha` < `beta` < `milestone`/`m` < `rc`/`cr` < (release) < `sp`) are
//! ordered; unknown qualifiers compare lexically after `rc`.

use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bound {
    Inclusive(String),
    Exclusive(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interval {
    pub low: Option<Bound>,
    pub high: Option<Bound>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VersionRange {
    pub sets: Vec<Interval>,
}

impl fmt::Display for VersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.sets.is_empty() {
            return write!(f, "any");
        }
        for (i, s) in self.sets.iter().enumerate() {
            if i > 0 {
                write!(f, ",")?;
            }
            match (&s.low, &s.high) {
                (None, None) => write!(f, "[,)")?,
                (Some(Bound::Inclusive(l)), None) => write!(f, "[{l},)")?,
                (Some(Bound::Exclusive(l)), None) => write!(f, "({l},)")?,
                (None, Some(Bound::Inclusive(h))) => write!(f, "[,{h}]")?,
                (None, Some(Bound::Exclusive(h))) => write!(f, ",{h})")?,
                (Some(l), Some(h)) => {
                    let open = match l {
                        Bound::Inclusive(_) => '[',
                        Bound::Exclusive(_) => '(',
                    };
                    let close = match h {
                        Bound::Inclusive(_) => ']',
                        Bound::Exclusive(_) => ')',
                    };
                    let l = match l {
                        Bound::Inclusive(v) | Bound::Exclusive(v) => v,
                    };
                    let h = match h {
                        Bound::Inclusive(v) | Bound::Exclusive(v) => v,
                    };
                    write!(f, "{open}{l},{h}{close}")?;
                }
            }
        }
        Ok(())
    }
}

impl VersionRange {
    pub fn any() -> Self {
        VersionRange {
            sets: vec![Interval {
                low: None,
                high: None,
            }],
        }
    }

    pub fn parse(input: &str) -> Result<Self, String> {
        let s = input.trim();
        if s.is_empty() {
            return Ok(Self::any());
        }
        let mut sets = Vec::new();
        for part in split_top_level(s) {
            let p = part.trim();
            if p.is_empty() {
                continue;
            }
            // interval?
            let first = p.chars().next().unwrap();
            if first == '[' || first == '(' {
                sets.push(parse_interval(p)?);
            } else {
                // exact version set
                sets.push(Interval {
                    low: Some(Bound::Inclusive(p.to_string())),
                    high: Some(Bound::Inclusive(p.to_string())),
                });
            }
        }
        if sets.is_empty() {
            return Err(format!("empty version range '{input}'"));
        }
        Ok(VersionRange { sets })
    }

    /// True when `v` satisfies any set in the union.
    pub fn contains(&self, v: &str) -> bool {
        self.sets.is_empty() || self.sets.iter().any(|i| interval_contains(i, v))
    }
}

fn split_top_level(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut cur = String::new();
    for c in s.chars() {
        match c {
            '[' | '(' => {
                depth += 1;
                cur.push(c);
            }
            ']' | ')' => {
                depth -= 1;
                cur.push(c);
            }
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur);
    }
    out
}

fn parse_interval(p: &str) -> Result<Interval, String> {
    let chars: Vec<char> = p.chars().collect();
    let open = chars[0];
    let close = *chars.last().ok_or("empty interval")?;
    if (open != '[' && open != '(') || (close != ']' && close != ')') {
        return Err(format!("malformed interval '{p}'"));
    }
    let inner: String = chars[1..chars.len() - 1].iter().collect();
    let (lo_raw, hi_raw) = match inner.split_once(',') {
        Some((a, b)) => (a.trim(), b.trim()),
        None => (inner.trim(), ""),
    };
    let low = if lo_raw.is_empty() {
        None
    } else {
        Some(if open == '[' {
            Bound::Inclusive(lo_raw.to_string())
        } else {
            Bound::Exclusive(lo_raw.to_string())
        })
    };
    let high = if hi_raw.is_empty() {
        None
    } else {
        Some(if close == ']' {
            Bound::Inclusive(hi_raw.to_string())
        } else {
            Bound::Exclusive(hi_raw.to_string())
        })
    };
    Ok(Interval { low, high })
}

fn interval_contains(i: &Interval, v: &str) -> bool {
    let lo_ok = match &i.low {
        None => true,
        Some(Bound::Inclusive(l)) => compare(v, l) != Ordering::Less,
        Some(Bound::Exclusive(l)) => compare(v, l) == Ordering::Greater,
    };
    if !lo_ok {
        return false;
    }
    match &i.high {
        None => true,
        Some(Bound::Inclusive(h)) => compare(v, h) != Ordering::Greater,
        Some(Bound::Exclusive(h)) => compare(v, h) == Ordering::Less,
    }
}

fn qualifier_rank(tok: &str) -> Option<i32> {
    let t = tok.to_ascii_lowercase();
    // numeric-suffixed forms: alpha4 -> alpha
    let stem = t.trim_end_matches(|c: char| c.is_ascii_digit());
    let stem = stem.strip_suffix('.').unwrap_or(stem);
    Some(match stem {
        "dev" => 0,
        "a" | "alpha" => 1,
        "b" | "beta" => 2,
        "m" | "milestone" | "pre" => 3,
        "rc" | "cr" => 4,
        "snapshot" => 5,
        "" => 7, // release
        "sp" => 8,
        other => return Some((6 + other.len().min(1000)) as i32), // unknown, lexical-ish after rc
    })
}

/// Maven-flavored comparison of two version strings.
pub fn compare(a: &str, b: &str) -> Ordering {
    let ta = tokens(a);
    let tb = tokens(b);
    let n = ta.len().max(tb.len());
    for i in 0..n {
        let xa = ta.get(i);
        let xb = tb.get(i);
        match (xa, xb) {
            (None, None) => return Ordering::Equal,
            (Some(x), None) => return extra_token_order(x),
            (None, Some(x)) => return extra_token_order(x).reverse(),
            (Some(x), Some(y)) => {
                let na = x.parse::<u64>();
                let nb = y.parse::<u64>();
                let ord = match (na, nb) {
                    (Ok(va), Ok(vb)) => va.cmp(&vb),
                    (Ok(_), Err(_)) => Ordering::Greater,
                    (Err(_), Ok(_)) => Ordering::Less,
                    (Err(_), Err(_)) => {
                        let (ra, rb) = (
                            qualifier_rank(x.as_str()).unwrap_or(6),
                            qualifier_rank(y.as_str()).unwrap_or(6),
                        );
                        if ra != rb {
                            ra.cmp(&rb)
                        } else {
                            x.cmp(y)
                        }
                    }
                };
                if ord != Ordering::Equal {
                    return ord;
                }
            }
        }
    }
    Ordering::Equal
}

/// Order contribution of a token present in one version but missing in the
/// other (the shorter one is treated as padded with `0` / release).
/// Returns how the LONGER version compares to the shorter one.
fn extra_token_order(tok: &str) -> Ordering {
    // numeric padding: 1.0 vs 1.0.1 -> longer is greater; 1.0 vs 1.0.0 -> equal
    if !tok.is_empty() && tok.chars().all(|c| c.is_ascii_digit()) {
        return if tok.parse::<u64>().unwrap_or(0) == 0 {
            Ordering::Equal
        } else {
            Ordering::Greater
        };
    }
    match qualifier_rank(tok) {
        // pre-release qualifiers (dev..snapshot) rank BELOW release
        Some(r) if r < 7 => Ordering::Less,
        // release padding / unknown / sp rank above
        _ => Ordering::Greater,
    }
}

fn tokens(v: &str) -> Vec<String> {
    v.split(['.', '-', '+', '_'])
        .filter(|t| !t.is_empty())
        .map(|t| t.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_compare() {
        assert_eq!(compare("1.21.1", "1.21.1"), Ordering::Equal);
        assert_eq!(compare("1.21.2", "1.21.10"), Ordering::Less);
        assert_eq!(compare("21.1.77", "21.1.9"), Ordering::Greater);
        assert_eq!(compare("1.0", "1.0.0"), Ordering::Equal);
    }

    #[test]
    fn qualifier_compare() {
        assert_eq!(compare("1.0.0-alpha", "1.0.0"), Ordering::Less);
        assert_eq!(compare("1.0.0-beta", "1.0.0-alpha"), Ordering::Greater);
        assert_eq!(compare("1.0.0-rc1", "1.0.0-beta9"), Ordering::Greater);
        assert_eq!(compare("1.0.0", "1.0.0-rc1"), Ordering::Greater);
        assert_eq!(compare("26.3-rc-3", "26.3"), Ordering::Less);
    }

    #[test]
    fn range_single_interval() {
        let r = VersionRange::parse("[1.21,1.22)").unwrap();
        assert!(r.contains("1.21.1"));
        assert!(!r.contains("1.22"));
        assert!(!r.contains("1.20.4"));
    }

    #[test]
    fn range_open_left() {
        let r = VersionRange::parse("[21.1.0,)").unwrap();
        assert!(r.contains("21.1.77"));
        assert!(r.contains("99.0.0"));
        assert!(!r.contains("21.0.9"));
    }

    #[test]
    fn range_union() {
        let r = VersionRange::parse("[1.0,2.0),[3.0,)").unwrap();
        assert!(r.contains("1.5"));
        assert!(!r.contains("2.5"));
        assert!(r.contains("3.1"));
    }

    #[test]
    fn range_exact() {
        let r = VersionRange::parse("1.21.1").unwrap();
        assert!(r.contains("1.21.1"));
        assert!(!r.contains("1.21.2"));
    }

    #[test]
    fn round_trip_display() {
        let r = VersionRange::parse("[1.0,2.0)").unwrap();
        assert_eq!(r.to_string(), "[1.0,2.0)");
    }
}
