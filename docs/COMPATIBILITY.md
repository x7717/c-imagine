# Compatibility matrix (living document)

Reflects verified behavior vs real NeoForge 1.21.1. Updated by every
milestone; a checkmark means *e2e-verified*, not "implemented somewhere".

## Manifest & discovery

| Feature | Status | Notes |
|---|---|---|
| `META-INF/neoforge.mods.toml` parse | ✅ | full 1.21.1 schema subset used by mods |
| legacy `META-INF/mods.toml` fallback | ✅ | |
| Maven version ranges `[a,b)`, unions | ✅ | unit-tested incl. qualifiers (rc/beta) |
| JarJar nested jars + metadata.json | ✅ | recursive, depth-bounded |
| duplicate modId detection | ✅ | first kept, later rejected (NeoForge behavior) |
| required-dep missing → refuse mod | ✅ | optional deps → warning |
| dependency cycle detection | ✅ | DFS with path in error message |
| BEFORE/AFTER ordering | ✅ | deterministic tie-break (alphabetical) |
| `@Mod` discovery without running Java | ✅ | imagine-jcf, RuntimeVisibleAnnotations |

## Runtime

| Feature | Status | Notes |
|---|---|---|
| mod bytecode execution | 🔜 M1 | child-first URLClassLoader over mods/*.jar |
| Java 21 class-file gate | ✅ jcf-level | runtime enforcement in M1 |
| `FMLCommonSetupEvent` analog | 🔜 M1 | |
| registry injection (blocks/items/entities) | 🔜 M2 | Paper runtime registries |
| event bus translation | 🔜 M3 | see ARCHITECTURE §6 |
| custom network payloads (mod → mod) | 🔜 M3 | routed through imagine-protocol |
| F2V visuals for vanilla clients | 🔜 M4 | docs/F2V-RS.md |
| real modpack soak | 🔜 M5 | Modrinth manifest-driven |

## Known deltas vs real NeoForge 1.21.1

- JavaFX/kotlin-for-forge language providers: not yet (M5 decision).
- Mixin-heavy mods: not supported at JAR level; per-mod translation notes
  will be tracked here as the soak progresses.
- Client-only mods: loaded but inert server-side (same as NeoForge).
