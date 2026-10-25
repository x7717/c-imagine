# Testing methodology

Three layers, all enforced in CI. Rule: **nothing merges with a failing
layer; a bug found in any layer blocks further milestones until fixed.**

## 1. Unit (Rust, every push)

`cargo test --workspace` — loader pipeline (manifests, ranges, JarJar,
duplicates, cycles), registry invariants, VarInt codecs, class-file
inspector. Pure, fast, deterministic (sorted/ordered everywhere).

## 2. Profile (every push)

`ci.yml` profile job: generates a synthetic 50-mod pack (in-memory jars),
measures plan time and RSS delta of the loader, prints a table, and fails
if ARCHITECTURE §8 budgets are violated. Profile output is uploaded as an
artifact so regressions can be attributed to a commit.

## 3. E2E (every push to master + nightly)

`e2e.yml`:

1. download Paper 26.3 build (fill.papermc.io), install CRUSSTY runtime;
2. build `libimagine.so` (release), install as module;
3. generate `mods/` content:
   - `crussty-testmod` style generated jars (manifest + class files,
     produced by the test generator — no network needed), and
   - optional Modrinth modpack jars (manifest-driven download);
4. boot the server (headless, EULA accepted, timeout-bounded);
5. assert log markers in order:
   - `[imagine] cplugin_init`
   - `imagine: mod '<id>'` for every test mod
   - `[imagine]: plan ready (N mod(s), 0 error(s))`
6. negative case: a corrupt mod jar must produce `error:` markers and a
   still-running kernel;
7. upload `logs/` and the module log on success *and* failure.

## 4. Visual (manual/nightly, M4+)

`visual.yml` — see `docs/F2V-RS.md`: vanilla client vs modded client
screenshot comparison in a gallery world, SSIM-gated.

## Local quick loop

```bash
cargo test --workspace
./scripts/gen_test_jars.sh   # rebuilds synthetic test mods into testdata/out
cargo run -p imagine-loader --example plan -- testdata/out   # (M1: example CLI)
```
