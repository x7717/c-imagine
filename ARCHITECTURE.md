# ARCHITECTURE

This document is the design contract for c-imagine. Every milestone PR must
either comply with it or update it first ("analysis → implementation →
verification → re-analysis" loop).

## 1. Mission

Run NeoForge 1.21.1 mods on Paper 26.3 as if Paper were a NeoForge 1.21.1
server — with Paper remaining the kernel (world sim, networking, protocol,
permissions) and the mod runtime contributing **zero second-server
overhead**: one JVM, one process, one native module.

Non-goals:

- running Forge/Fabric/NeoForge *server software* inside Paper (this is not
  a hybrid kernel);
- supporting mods that replace core class hierarchies at the JAR level
  (mixins targeting 1.21.1 internals are translated, not executed as-is);
- client-side mod execution (clients stay vanilla; visuals come from the
  F2V bridge).

## 2. Process & memory model

```
┌────────────────────────────────────────────────────────────┐
│ JVM (one)                                                  │
│  ┌──────────────────────────────┐  ┌────────────────────┐  │
│  │ Paper 26.3 kernel            │  │ mod classes        │  │
│  │  - world/tick/net/protocol   │  │ (child-first       │  │
│  │  - registries (runtime ids)  │◄─┼─  URLClassLoader   │  │
│  └──────────────▲───────────────┘  │   over mods/*.jar) │  │
│                 │ JNI/JVMTI        └─────────▲──────────┘  │
│  ┌──────────────┴───────────────────────────┴──────────┐  │
│  │ CRUSSTY JVMTI agent                                 │  │
│  │  ┌───────────────────────────────────────────────┐  │  │
│  │  │ imagine .so (Rust)                            │  │  │
│  │  │  loader | registry | protocol | jcf | events  │  │  │
│  │  └───────────────────────────────────────────────┘  │  │
│  └─────────────────────────────────────────────────────┘  │
└────────────────────────────────────────────────────────────┘
```

Rules:

1. **One JVM.** Mod bytecode runs in a child-first classloader inside the
   Paper JVM. The Rust core is one dlopened module (RTLD_LOCAL). No helper
   processes, no extra Java heaps.
2. **Kernel-first.** Paper owns protocol and world state. imagine never
   spawns sockets or tick loops of its own; all work rides CRUSSTY's
   scheduler (kernel main thread) and event bus.
3. **Allocation discipline.** Steady-state (per-tick) allocations from the
   mod runtime are budgeted (see §8). Startup allocations are bounded by
   jar sizes; manifests are parsed streaming, jars are never fully
   unpacked to disk.

## 3. Boot sequence

```
JVMTI OnLoad ──► cplugin_init            (imagine, before kernel)
   │  wire SDK, register class watchers
   ▼
Paper classes load ──► transform pipeline sees MinecraftServer
   ▼
kernel ready ──► on_kernel_ready ──► main thread task:
   │  1. scan mods/ (imagine-loader)
   │  2. LoadPlan {mods, warnings, errors}   (pure data)
   │  3. JVM enforcement (M1): child-first loader over accepted jars
   │  4. entrypoint invocation: @Mod ctor + FMLCommonSetupEvent analog
   ▼
imagine: plan ready (N mods, 0 errors)      ← e2e gate marker
```

Fatal plan errors (duplicate ids, missing required deps, cycles, bad
manifests) disable the offending mods but never the kernel — mirroring
NeoForge's refuse-to-load behavior for broken mods.

## 4. Component contracts

| Crate | Owns | Never does |
|---|---|---|
| imagine-loader | scan/parse/jarjar/plan (pure Rust, pure data) | touch the JVM |
| imagine-registry | resource locations, runtime ids, mapping tables | protocol bytes |
| imagine-protocol | VarInt/strings, content bindings, translation decisions | filesystem |
| imagine-jcf | class-file inspection, @Mod discovery, Java-21 gate | loading classes |
| imagine | cplugin entry, wiring, JNI enforcement (M1+) | business logic |

## 5. Registry model

- Paper 26.3 registries own **runtime numeric ids** (what goes on the wire).
- Modded content registers under its 1.21.1 `ResourceLocation`; imagine
  allocates a stable runtime id in its emulated registry, then (M2)
  injects a kernel-visible representation (vanilla proxy block/entity +
  block-entity/entity metadata) so Paper's protocol can carry it.
- `MappingTable` carries 1.21.1 → 26.3 vanilla renames; protocol id
  overrides are data-driven, never hardcoded in logic.
- Ids are allocated once per session, never reused, never reordered
  (append-only registries — F2V encoders cache them).

## 6. Event translation (M3)

NeoForge bus semantics emulated on Paper events:

| NeoForge 1.21.1 | Paper 26.3 source | Notes |
|---|---|---|
| BlockEvent.BreakEvent | BlockBreakEvent | cancellable both ways |
| LivingDeathEvent | EntityDeathEvent | damage source mapping |
| PlayerInteractEvent.* | PlayerInteractEvent | left/right split |
| TickEvent.ServerTickEvent | ServerTickEvent | start/end phases |
| FMLCommonSetupEvent | boot task | invoked once after registries |
| RegisterEvent | registry hook (M2) | DeferredRegister drain |

Dispatch is priority-ordered and cancellable; mod handlers run on the
kernel main thread (CRUSSTY scheduler) — mods never get their own threads
by default.

## 7. F2V bridge (M4+, see docs/F2V-RS.md)

Vanilla 26.3 clients must see modded content. Strategy: protocol-level
translation at the CRUSSTY network brick — modded entity spawn → vanilla
entity type + metadata rewrite; modded block → vanilla block + block-entity
payload; modded item → vanilla item + component patch. Translation tables
come from imagine-registry; encoders live in imagine-protocol.

## 8. Performance budgets (CI-enforced)

| Metric | Budget | Where enforced |
|---|---|---|
| loader plan for 50 synthetic mods | < 500 ms | ci profile job |
| loader RSS delta over baseline | < 64 MB | ci profile job |
| module .so resident code | < 5 MB | release build check |
| steady-state tick alloc (0 mods) | < 128 KB/tick | e2e counters (M1+) |
| first-mod latency (plan → entrypoints) | < 200 ms/mod | e2e (M1+) |

A PR that breaks a budget is a failing PR: profile output is attached to
the run artifacts, the offending allocation site is named in the PR.

## 9. Compatibility matrix (living doc — docs/COMPATIBILITY.md)

| Feature | Status |
|---|---|
| neoforge.mods.toml (javafml) | ✅ parsed |
| Maven version ranges | ✅ enforced |
| JarJar nested mods | ✅ discovered |
| @Mod discovery via class file | ✅ (jcf) |
| mod bytecode execution | 🔜 M1 |
| registry injection | 🔜 M2 |
| event translation | 🔜 M3 |
| F2V visuals for vanilla clients | 🔜 M4 |
| modpack soak (real mods) | 🔜 M5 |

## 10. Risks

1. **Paper 26.3 internal churn** — kernel internals move between builds;
   the transform-based approach must target stable seams (registry APIs,
   packet codec entrypoints) and version-gate the rest.
2. **Mod bytecode assumptions** — mods may assume NeoForge classloading
   (JarJar classpath, resource packing). The child-first loader must
   replicate lookup order, including `META-INF/services`.
3. **Protocol drift 1.21.1 ↔ 26.3** — content encodings changed between
   generations; the translation layer is data-driven so fixes are table
   edits, not logic edits.
4. **Perf regressions from hooks** — every hot-path hook must be
   O(1) with precomputed tables; the CI profile job exists to catch this.
