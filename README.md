# c-imagine

**A CRUSSTY module that boots a mini NeoForge 1.21.1 runtime *inside* Paper 26.3.**

Drop NeoForge 1.21.1 mods into `mods/`, start Paper — the mods load. Paper
stays the kernel and the only server process; there is no second JVM, no
sidecar, no proxy. The runtime is a native Rust module injected by
[CRUSSTY](https://github.com/PLANETA9091/CRUSSTY) (JVMTI), so it adds no
plugin-API overhead and is profiled against hard memory/CPU budgets in CI.

```
Paper 26.3 (kernel, owns world/net/protocol)
  └── CRUSSTY JVMTI runtime
        └── imagine (this module, Rust)
              ├── imagine-loader     mods/ + neoforge.mods.toml + jar-in-jar + dep graph
              ├── imagine-registry   1.21.1 resource locations -> 26.3 runtime ids
              ├── imagine-protocol   VarInt codecs + content translation (F2V bridge)
              ├── imagine-jcf        class-file inspection (@Mod discovery, Java 21 gate)
              └── imagine            cplugin entry: boot, watch kernel, apply plan
```

## Status: M0 — foundations

What works today:

- full `mods/` scan pipeline in Rust: manifests, Maven version ranges,
  JarJar nested jars, duplicate/cycle/missing-dep detection, deterministic
  load order — unit-tested
- CRUSSTY module entry that watches kernel boot and applies the plan on the
  main thread
- registry/protocol primitives the F2V visual bridge builds on
- CI: build + clippy + tests + loader profile with budgets, plus an
  end-to-end job that boots a real Paper 26.3 server with the module and
  test mods

Roadmap (each milestone is gated by the e2e/visual pipelines in CI):

| Milestone | Deliverable |
|---|---|
| M0 | discovery + plan + CI harness (this release) |
| M1 | JVM-side enforcement: child-first classloader for `mods/*.jar`, `@Mod` entrypoint invocation, Java-21 gate enforcement |
| M2 | registry injection: modded blocks/items/entities registered into Paper's runtime registries via the CRUSSTY transform pipeline |
| M3 | event translation: Paper events -> NeoForge bus semantics (`IModBusEvent`, `SubscribeEvent` dispatch) |
| M4 | F2V bridge M1: modded entities/blocks visible to vanilla 26.3 clients (entity proxies, block-entity data) |
| M5 | full modpack soak: curated NeoForge 1.21.1 modpack runs, visual comparison vanilla vs modded in CI |

## Build

```bash
cargo build --workspace --release
# artifact: target/release/libimagine.so
```

Install into a CRUSSTY-managed server:

```bash
crussty init --platform paper --version 26.3 --dir my-server
mkdir -p my-server/modules/imagine
cp module.json target/release/libimagine.so my-server/modules/imagine/
mkdir -p my-server/mods && cp your-neoforge-mods-1.21.1.jar my-server/mods/
cd my-server && crussty run
```

Expected log markers:

```
[imagine] cplugin_init: mini-NeoForge runtime for Paper 26.3 (mods target: 1.21.1)
imagine: mod '<modid>' <version> (<license>) <- <jar path>
[imagine]: plan ready (N mod(s), 0 error(s))
```

## Testing

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

CI (`.github/workflows/`):

- **ci** — build, clippy, unit tests, loader profile with budgets on every push/PR
- **e2e** — boots a real Paper 26.3 + CRUSSTY + imagine + generated test mods,
  asserts log markers, uploads server logs
- **visual** (manual) — xvfb-run vanilla client screenshot pipeline for the
  F2V comparison harness

See `docs/TESTING.md` for the methodology and `ARCHITECTURE.md` for the
design contract (memory model, budgets, compatibility matrix).

## License

MIT — inherited from the CRUSSTY platform.
