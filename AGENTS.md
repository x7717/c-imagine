# c-imagine — agent conventions

This repo follows the CRUSSTY ecosystem conventions (see
`PLANETA9091/CRUSSTY` AGENTS.md) plus the project's own working loop.

## Working loop (mandatory)

Every feature/fix follows: **analysis → implementation → verification →
re-analysis**. In practice:

1. update `ARCHITECTURE.md` / `docs/COMPATIBILITY.md` if the design moves;
2. implement with unit tests;
3. CI must pass (build + clippy `-D warnings` + tests + profile budgets);
4. after each milestone, re-run the e2e pipeline and diff the behavior
   against real NeoForge 1.21.1 (`docs/COMPATIBILITY.md` must reflect it).

A regression found in one component blocks everything until fixed — no
skipping ahead with a known-broken base.

## Commits

- Conventional commits: `feat:`, `fix:`, `docs:`, `ci:`, `test:`, `chore:`.
- Author: the repo owner, noreply email
  (`311858687+x7717@users.noreply.github.com`).

## Code rules

- Rust: no panics on mod-driven paths (`catch_unwind` boundaries at every
  mod entrypoint, M1+); errors as data (`LoadPlan.errors`), never `unwrap`
  on external input.
- Allocation discipline on hot paths (see ARCHITECTURE §8 budgets).
- Language: repository, code, docs, commit messages — English.

## CI

- `ci.yml` — build/clippy/test/profile on every push & PR.
- `e2e.yml` — real Paper 26.3 boot with the module; markers:
  `cplugin_init`, `plan ready`.
- `visual.yml` — manual client screenshot harness (F2V comparisons).

Test mods and the modpack live in `testdata/`; CI downloads pack jars from
Modrinth by manifest (no binary jars committed).
