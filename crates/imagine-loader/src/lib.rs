//! imagine-loader — discovers and models NeoForge 1.21.1 mods inside `mods/`.
//!
//! The loader is deliberately pure Rust with no JVM dependency: everything it
//! produces is plain data (`LoadPlan`) that the `imagine` module then enforces
//! on the Java side. This keeps the hot path allocation-light and unit-testable
//! without booting a server.
//!
//! Pipeline (mirrors NeoForge's own discovery semantics where observable):
//!
//! 1. `scan_dir` — enumerate `*.jar` / `*.zip` candidates in `mods/`
//! 2. `read_manifest` — parse `META-INF/neoforge.mods.toml` (fallback
//!    `META-INF/mods.toml` for legacy jars)
//! 3. `jarjar` — expand `META-INF/jarjar/` nested jars recursively
//! 4. `planner` — dependency graph + BEFORE/AFTER ordering + cycle check
//!
//! Anything malformed is reported in `LoadPlan.errors`, never panicked on:
//! a bad mod must not take down the kernel.

pub mod jarjar;
pub mod mods_toml;
pub mod planner;
pub mod versions;

pub use versions::VersionRange;

#[cfg(test)]
pub(crate) mod tests_support;

use std::fmt;
use std::path::{Path, PathBuf};

/// Where a mod jar came from — affects logging and duplicate resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModSource {
    /// Top-level jar in `mods/`.
    Root(PathBuf),
    /// Jar nested inside another jar (NeoForge JarJar).
    Nested { parent: PathBuf, path: String },
}

impl fmt::Display for ModSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModSource::Root(p) => write!(f, "{}", p.display()),
            ModSource::Nested { parent, path } => write!(f, "{}!{}", parent.display(), path),
        }
    }
}

/// One discovered mod (metadata only — no bytecode is executed here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredMod {
    /// `[[mods]] modId` — the NeoForge mod id.
    pub mod_id: String,
    /// Version string as written in the toml (not resolved).
    pub version: String,
    /// SPDX-ish license string from the manifest.
    pub license: String,
    /// Every `[[mods]]` section is one logical mod; a jar may declare several.
    pub source: ModSource,
    /// Display name, if present.
    pub display_name: Option<String>,
    /// Declared dependencies (typed + ranged + ordered) from the manifest.
    pub dependencies: Vec<mods_toml::Dependency>,
}

/// A fully-resolved load plan: the single source of truth the module entry
/// point (`crates/imagine`) enforces on the Java side.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadPlan {
    /// Mods in load order (dependencies first, BEFORE/AFTER ordering applied).
    pub mods: Vec<DiscoveredMod>,
    /// Non-fatal problems (e.g. optional dependency missing).
    pub warnings: Vec<String>,
    /// Fatal problems (duplicate modid, missing required dep, cycle, …).
    /// When non-empty the runtime must refuse to enable the offending mods.
    pub errors: Vec<String>,
    /// Jars skipped because no manifest was found (still logged).
    pub skipped_jars: Vec<PathBuf>,
}

impl LoadPlan {
    pub fn is_ok(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn mod_ids(&self) -> Vec<&str> {
        self.mods.iter().map(|m| m.mod_id.as_str()).collect()
    }
}

/// Candidate jar located by `scan_dir`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JarCandidate {
    pub path: PathBuf,
    pub len: u64,
}

/// Step 1: enumerate jar candidates in a `mods/` directory (non-recursive,
/// matching NeoForge; directories are ignored in this milestone).
pub fn scan_dir(dir: &Path) -> std::io::Result<Vec<JarCandidate>> {
    let mut out = Vec::new();
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };
    for entry in rd {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext_ok = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("jar") | Some("zip")
        );
        if !ext_ok {
            continue;
        }
        let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
        out.push(JarCandidate { path, len });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// Full pipeline: scan a `mods/` directory and produce a load plan.
pub fn build_plan(mods_dir: &Path) -> std::io::Result<LoadPlan> {
    let mut plan = LoadPlan::default();
    let mut all_mods: Vec<DiscoveredMod> = Vec::new();

    for cand in scan_dir(mods_dir)? {
        let bytes = match std::fs::read(&cand.path) {
            Ok(b) => b,
            Err(e) => {
                plan.errors
                    .push(format!("unreadable jar {}: {e}", cand.path.display()));
                continue;
            }
        };
        match jarjar::discover(&bytes, &cand.path) {
            Ok(jar) => {
                if jar.mods.is_empty() {
                    plan.skipped_jars.push(cand.path.clone());
                    plan.warnings.push(format!(
                        "no neoforge.mods.toml in {} — skipped (library jar?)",
                        cand.path.display()
                    ));
                }
                for m in jar.mods {
                    all_mods.push(m);
                }
            }
            Err(e) => plan.errors.push(format!(
                "failed to inspect {}: {e}",
                cand.path.display()
            )),
        }
    }

    // Duplicate modids are fatal in NeoForge; keep first, error the rest.
    let mut kept: Vec<DiscoveredMod> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for m in all_mods {
        if seen.contains(&m.mod_id) {
            plan.errors
                .push(format!("duplicate modId '{}' (kept first, dropped later)", m.mod_id));
        } else {
            seen.push(m.mod_id.clone());
            kept.push(m);
        }
    }

    planner::order(kept, &mut plan);
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::zip_writer::build_jar;

    pub(crate) fn fake_jar_with_toml(toml: &str) -> Vec<u8> {
        build_jar(&[("META-INF/neoforge.mods.toml", toml.as_bytes())], &[])
    }

    pub(crate) const TOML_B: &str = r#"
modLoader="javafml"
loaderVersion="[4,)"
license="MIT"
[[mods]]
modId="beta"
version="2.0.0"
"#;

    #[test]
    fn scan_dir_finds_jars_sorted() {
        let td = std::env::temp_dir().join(format!("iml-scan-{}", std::process::id()));
        std::fs::create_dir_all(&td).unwrap();
        std::fs::write(td.join("b.jar"), b"").unwrap();
        std::fs::write(td.join("a.jar"), b"").unwrap();
        std::fs::write(td.join("notes.txt"), b"").unwrap();
        let found = scan_dir(&td).unwrap();
        let names: Vec<String> = found
            .iter()
            .map(|c| c.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["a.jar", "b.jar"]);
        let _ = std::fs::remove_dir_all(&td);
    }

    #[test]
    fn build_plan_orders_and_dedups() {
        let td = std::env::temp_dir().join(format!("iml-plan-{}", std::process::id()));
        std::fs::create_dir_all(&td).unwrap();
        // alpha depends on beta -> beta must come first.
        let toml_a = r#"
modLoader="javafml"
loaderVersion="[4,)"
license="MIT"
[[mods]]
modId="alpha"
version="1.0.0"
[[dependencies.alpha]]
modId="beta"
type="required"
versionRange="[1,2)"
ordering="AFTER"
"#;
        std::fs::write(td.join("a.jar"), fake_jar_with_toml(toml_a)).unwrap();
        std::fs::write(td.join("b.jar"), fake_jar_with_toml(TOML_B)).unwrap();
        let plan = build_plan(&td).unwrap();
        assert!(plan.is_ok(), "errors: {:?}", plan.errors);
        let ids = plan.mod_ids();
        assert_eq!(ids, vec!["beta", "alpha"], "beta (no deps) first");
        assert_eq!(plan.errors.len(), 0);
        let _ = std::fs::remove_dir_all(&td);

        // Duplicate modid is fatal (NeoForge behavior): error reported.
        let td2 = std::env::temp_dir().join(format!("iml-plan-dup-{}", std::process::id()));
        std::fs::create_dir_all(&td2).unwrap();
        std::fs::write(td2.join("b.jar"), fake_jar_with_toml(TOML_B)).unwrap();
        std::fs::write(td2.join("dup.jar"), fake_jar_with_toml(TOML_B)).unwrap();
        let plan2 = build_plan(&td2).unwrap();
        assert!(!plan2.is_ok(), "duplicate modId must be fatal");
        assert!(plan2.errors.iter().any(|e| e.contains("duplicate modId 'beta'")));
        let _ = std::fs::remove_dir_all(&td2);
    }

    #[test]
    fn missing_mods_dir_is_empty_plan() {
        let plan = build_plan(Path::new("/nonexistent/mods/imagine-test")).unwrap();
        assert!(plan.is_ok());
        assert!(plan.mods.is_empty());
    }
}
