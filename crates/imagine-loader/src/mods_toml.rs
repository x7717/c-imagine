//! `META-INF/neoforge.mods.toml` parsing (NeoForge 1.21.1 schema).
//!
//! Reference schema (NeoForge 21.1):
//! ```toml
//! modLoader="javafml"
//! loaderVersion="[4,)"
//! license="MIT"
//! issueTrackerURL="..."
//! [[mods]]
//! modId="examplemod"
//! version="1.0.0"
//! displayName="Example"
//! authors="you"
//! description='''text'''
//! logoFile="logo.png"
//! [[dependencies.examplemod]]
//! modId="neoforge"
//! type="required"          # REQUIRED | OPTIONAL | DISABLED (case-insensitive)
//! versionRange="[21.1.0,)"
//! ordering="AFTER"         # NONE | BEFORE | AFTER
//! side="BOTH"              # BOTH | CLIENT | SERVER
//! ```
//!
//! We model only what the runtime needs to make decisions; unknown keys are
//! ignored on purpose (forward compatible with newer mod manifests).

use crate::versions::VersionRange;
use serde::Deserialize;

pub const MANIFEST_PATH_NEOFORGE: &str = "META-INF/neoforge.mods.toml";
pub const MANIFEST_PATH_LEGACY: &str = "META-INF/mods.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepType {
    Required,
    Optional,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepOrdering {
    None,
    Before,
    After,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Both,
    Client,
    Server,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub mod_id: String,
    pub dep_type: DepType,
    pub range: VersionRange,
    pub ordering: DepOrdering,
    pub side: Side,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModManifest {
    pub mod_id: String,
    pub version: String,
    pub display_name: Option<String>,
    pub dependencies: Vec<Dependency>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Manifest {
    pub loader: String,
    pub loader_version: String,
    pub license: String,
    pub mods: Vec<ModManifest>,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum ParseError {
    #[error("invalid toml: {0}")]
    Toml(String),
    #[error("missing [[mods]] section")]
    NoMods,
    #[error("mod #{0} has empty/missing modId")]
    EmptyModId(usize),
}

// --- serde views (lenient: every scalar optional, validated by hand) -------

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DepRaw {
    #[serde(default, rename = "modId")]
    mod_id: Option<String>,
    #[serde(default, rename = "type")]
    dep_type: Option<String>,
    #[serde(default, rename = "versionRange")]
    version_range: Option<String>,
    #[serde(default)]
    ordering: Option<String>,
    #[serde(default)]
    side: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModRaw {
    #[serde(default, rename = "modId")]
    mod_id: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default, rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileRaw {
    #[serde(default, rename = "modLoader")]
    mod_loader: Option<String>,
    #[serde(default, rename = "loaderVersion")]
    loader_version: Option<String>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    mods: Vec<ModRaw>,
    /// TOML semantics: `[[dependencies.<modId>]]` is a ROOT-level array of
    /// tables keyed by the owning mod id — not nested under `[[mods]]`.
    /// (Same layout NeoForge's own loader consumes.)
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, Vec<DepRaw>>,
}

fn dep_type(s: &str) -> DepType {
    match s.to_ascii_uppercase().as_str() {
        "OPTIONAL" => DepType::Optional,
        "DISABLED" => DepType::Disabled,
        _ => DepType::Required,
    }
}

fn ordering(s: &str) -> DepOrdering {
    match s.to_ascii_uppercase().as_str() {
        "BEFORE" => DepOrdering::Before,
        "AFTER" => DepOrdering::After,
        _ => DepOrdering::None,
    }
}

fn side(s: &str) -> Side {
    match s.to_ascii_uppercase().as_str() {
        "CLIENT" => Side::Client,
        "SERVER" => Side::Server,
        _ => Side::Both,
    }
}

/// Parse manifest text into a [`Manifest`].
pub fn parse(text: &str) -> Result<Manifest, ParseError> {
    let raw: FileRaw =
        toml::from_str(text).map_err(|e| ParseError::Toml(e.to_string()))?;

    let mut mods = Vec::with_capacity(raw.mods.len());
    for (i, m) in raw.mods.iter().enumerate() {
        let mod_id = m.mod_id.clone().unwrap_or_default();
        if mod_id.trim().is_empty() {
            return Err(ParseError::EmptyModId(i));
        }
        let owned: Vec<DepRaw> = raw
            .dependencies
            .get(&mod_id)
            .cloned()
            .unwrap_or_default();
        let dep_count = owned.len();
        let mut deps = Vec::with_capacity(dep_count);
        for d in &owned {
            let dep_mod_id = d.mod_id.clone().unwrap_or_default();
            if dep_mod_id.trim().is_empty() {
                continue; // ignore malformed dependency entries, NeoForge skips them too
            }
            let range = match d.version_range.as_deref() {
                Some(r) => VersionRange::parse(r).unwrap_or_else(|_| VersionRange::any()),
                None => VersionRange::any(),
            };
            deps.push(Dependency {
                mod_id: dep_mod_id,
                dep_type: dep_type(d.dep_type.as_deref().unwrap_or("required")),
                range,
                ordering: ordering(d.ordering.as_deref().unwrap_or("none")),
                side: side(d.side.as_deref().unwrap_or("both")),
            });
        }
        mods.push(ModManifest {
            mod_id,
            version: m.version.clone().unwrap_or_else(|| "0.0.0".into()),
            display_name: m.display_name.clone(),
            dependencies: deps,
        });
    }
    if mods.is_empty() {
        return Err(ParseError::NoMods);
    }

    Ok(Manifest {
        loader: raw.mod_loader.unwrap_or_else(|| "javafml".into()),
        loader_version: raw.loader_version.unwrap_or_else(|| "[0,)".into()),
        license: raw.license.unwrap_or_else(|| "Unknown".into()),
        mods,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    

    const FULL: &str = r#"
modLoader="javafml"
loaderVersion="[4,)"
license="MIT"
issueTrackerURL="https://example.invalid/tracker"

[[mods]]
modId="examplemod"
version="1.2.3"
displayName="Example Mod"
authors="someone"
description='''does things'''

[[dependencies.examplemod]]
modId="neoforge"
type="required"
versionRange="[21.1.0,)"
ordering="NONE"
side="BOTH"

[[dependencies.examplemod]]
modId="jei"
type="optional"
versionRange="[19.21.0,)"
ordering="AFTER"
side="BOTH"
"#;

    #[test]
    fn parses_full_manifest() {
        let m = parse(FULL).unwrap();
        assert_eq!(m.loader, "javafml");
        assert_eq!(m.license, "MIT");
        assert_eq!(m.mods.len(), 1);
        let mod0 = &m.mods[0];
        assert_eq!(mod0.mod_id, "examplemod");
        assert_eq!(mod0.version, "1.2.3");
        assert_eq!(mod0.dependencies.len(), 2);
        let d0 = &mod0.dependencies[0];
        assert_eq!(d0.mod_id, "neoforge");
        assert_eq!(d0.dep_type, DepType::Required);
        assert!(d0.range.contains("21.1.77"));
        let d1 = &mod0.dependencies[1];
        assert_eq!(d1.dep_type, DepType::Optional);
        assert_eq!(d1.ordering, DepOrdering::After);
    }

    #[test]
    fn rejects_missing_mods_section() {
        let err = parse("modLoader=\"javafml\"\nlicense=\"MIT\"\n").unwrap_err();
        assert!(matches!(err, ParseError::NoMods));
    }

    #[test]
    fn legacy_mods_toml_path_constant() {
        assert_eq!(MANIFEST_PATH_LEGACY, "META-INF/mods.toml");
        assert_eq!(MANIFEST_PATH_NEOFORGE, "META-INF/neoforge.mods.toml");
    }
}
