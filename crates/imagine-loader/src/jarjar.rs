//! JarJar (nested jar) expansion — NeoForge ships dependency jars inside
//! `META-INF/jarjar/` together with a `metadata.json` index.
//!
//! We honor the index when present, but also *scan* `META-INF/jarjar/` for
//! any `*.jar` entry directly, because some packs ship jars without a
//! complete index (observed with vendor-bundled libraries). Discovery is
//! recursive: a nested jar may itself contain JarJar entries (bounded by
//! `MAX_DEPTH`).

use crate::mods_toml::{self, Manifest};
use crate::{DiscoveredMod, ModSource};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const JARJAR_DIR: &str = "META-INF/jarjar/";
pub const JARJAR_METADATA: &str = "META-INF/jarjar/metadata.json";
pub const MAX_DEPTH: usize = 8;

/// A jar opened for inspection: parsed manifests + nested jar handles.
pub struct JarEntry {
    pub mods: Vec<DiscoveredMod>,
    pub nested: Vec<(String, Vec<u8>)>,
}

/// Inspect a jar byte buffer: read its manifest(s), expand JarJar nested
/// jars recursively, return every logical mod found. `origin` is the jar
/// path the bytes belong to (for provenance in the load plan).
pub fn discover(bytes: &[u8], origin: &Path) -> Result<JarEntry, String> {
    discover_at(bytes, Some(origin), 0)
}

fn discover_at(bytes: &[u8], parent: Option<&Path>, depth: usize) -> Result<JarEntry, String> {
    if depth > MAX_DEPTH {
        return Err("jar-in-jar nesting too deep (possible zip bomb)".into());
    }
    
    discover_inner(bytes, parent, depth)
}

fn discover_inner(bytes: &[u8], parent: Option<&Path>, depth: usize) -> Result<JarEntry, String> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(reader).map_err(|e| format!("bad zip: {e}"))?;

    let mut mods = Vec::new();
    let mut nested = Vec::new();

    // 1. manifest at this level
    let manifest = read_first_manifest(&mut archive);
    if let Some(m) = &manifest {
        append_manifest_mods(m, parent, &mut mods);
    }

    // 2. JarJar metadata.json (index of nested jars)
    let index_paths: Vec<String> = if let Ok(mut f) = archive.by_name(JARJAR_METADATA) {
        let mut s = String::new();
        let _ = f.read_to_string(&mut s);
        parse_jarjar_index(&s)
            .unwrap_or_default()
            .into_iter()
            .collect()
    } else {
        Vec::new()
    };

    // 3. scan every jar in META-INF/jarjar/ (index union actual contents)
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().to_string();
        if !name.starts_with(JARJAR_DIR) || !name.ends_with(".jar") {
            continue;
        }
        let mut buf = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut buf)
            .map_err(|e| format!("read nested {name}: {e}"))?;
        nested.push((name.clone(), buf));
    }
    drop(archive);

    for (name, nested_bytes) in &nested {
        if !index_paths.is_empty() && !index_paths.contains(name) {
            continue; // not listed in metadata.json — treat as data
        }
        let child_parent = parent
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("<root>"));
        let child = discover_at(
            nested_bytes,
            Some(&child_parent),
            depth + 1,
        )?;
        // Nested mods keep provenance: parent jar + inner path.
        for mut cm in child.mods {
            cm.source = ModSource::Nested {
                parent: child_parent.clone(),
                path: name.clone(),
            };
            mods.push(cm);
        }
    }

    Ok(JarEntry { mods, nested })
}

fn append_manifest_mods(m: &Manifest, parent: Option<&Path>, out: &mut Vec<DiscoveredMod>) {
    for mm in &m.mods {
        out.push(DiscoveredMod {
            mod_id: mm.mod_id.clone(),
            version: mm.version.clone(),
            license: m.license.clone(),
            source: match parent {
                Some(p) => ModSource::Root(p.to_path_buf()),
                None => ModSource::Root(PathBuf::from("<memory>")),
            },
            display_name: mm.display_name.clone(),
            dependencies: mm.dependencies.clone(),
        });
    }
}

fn read_first_manifest<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
) -> Option<Manifest> {
    for path in [mods_toml::MANIFEST_PATH_NEOFORGE, mods_toml::MANIFEST_PATH_LEGACY] {
        if let Ok(mut f) = archive.by_name(path) {
            let mut s = String::new();
            if f.read_to_string(&mut s).is_ok() {
                if let Ok(m) = mods_toml::parse(&s) {
                    return Some(m);
                }
            }
        }
    }
    None
}

fn parse_jarjar_index(json: &str) -> Option<Vec<String>> {
    #[derive(serde::Deserialize)]
    struct JarMeta {
        #[serde(default)]
        path: String,
    }
    #[derive(serde::Deserialize)]
    struct Index {
        #[serde(default)]
        jars: Vec<JarMeta>,
    }
    let idx: Index = serde_json::from_str(json).ok()?;
    Some(idx.jars.into_iter().map(|j| j.path).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::zip_writer::build_jar;

    const MOD_TOML: &str = r#"
modLoader="javafml"
loaderVersion="[4,)"
license="MIT"
[[mods]]
modId="inner"
version="0.2.0"
"#;

    #[test]
    fn discovers_root_manifest() {
        let jar = build_jar(&[(mods_toml::MANIFEST_PATH_NEOFORGE, MOD_TOML.as_bytes())], &[]);
        let entry = discover(&jar, Path::new("test.jar")).unwrap();
        assert_eq!(entry.mods.len(), 1);
        assert_eq!(entry.mods[0].mod_id, "inner");
        assert_eq!(entry.mods[0].version, "0.2.0");
    }

    #[test]
    fn expands_nested_jarjar() {
        let nested = build_jar(&[(mods_toml::MANIFEST_PATH_NEOFORGE, MOD_TOML.as_bytes())], &[]);
        let index = r#"{"jars":[{"identifier":{"group":"g","artifact":"a"},"version":{"range":"[0,)","exact":"0.2.0"},"path":"META-INF/jarjar/inner.jar","isObfuscated":false}]}"#;
        let parent_jar = build_jar(
            &[("META-INF/jarjar/metadata.json", index.as_bytes())],
            &[("META-INF/jarjar/inner.jar", &nested)],
        );
        let entry = discover(&parent_jar, Path::new("parent.jar")).unwrap();
        let ids: Vec<&str> = entry.mods.iter().map(|m| m.mod_id.as_str()).collect();
        assert!(ids.contains(&"inner"), "nested mod must be discovered: {ids:?}");
    }

    #[test]
    fn bad_zip_is_error_not_panic() {
        assert!(discover(b"not a zip", Path::new("bad.jar")).is_err());
    }
}
