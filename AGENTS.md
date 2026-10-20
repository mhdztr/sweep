# AGENTS.md — session entry point for `sweep`

Read this first in any new session (human or agent). Then `ROADMAP.md`, then
`CONTEXT.md` if you need decisions/conventions. Spec details live in `docs/`.

## What this is

`sweep` — an advanced, safety-first **storage cleaner CLI for Linux** (Rust, single
binary, Arch-first, distro-aware). Flow: `scan` (read-only) → `clean` (checklist
with safe/caution/destructive tiers) → gates (summary / per-rule / typed `PURGE`)
→ two-pass execution (user pass, then **one** batched sudo pass) → summary + history.

## Build / test / lint commands

```sh
cargo build                    # debug build
cargo test                     # unit + integration + sandbox tests
cargo clippy --all-targets -- -D warnings
cargo fmt --check
cargo run -- --version         # smoke
cargo run -- scan              # read-only scan
cargo test --test cli_smoke    # CLI surface tests only
```

Release: `cargo build --release` (lto=thin, strip="debuginfo" in profile —
the latter keeps `cargo auditable` metadata intact). The tag-driven release +
AUR pipelines are specified in `docs/CI-CD.md` (scaffolded in M1.5, first tag
at M7).

## Repo layout

- `src/main.rs` — parse → dispatch → exit code (keep thin)
- `src/lib.rs` — library root (tests share this code)
- `src/cli/` — clap surface (spec: `docs/CLI-UX.md`)
- `src/env.rs` — home resolution + `SWEEP_TEST_GUARD` refusal
- future modules per `docs/ARCHITECTURE.md` §1: `rules/`, `scan/`, `exec/`,
  `ui/`, `system/`, `config.rs`, `state.rs`, `report.rs`
- `docs/` — **spec of record** (see sync rule below); CI/CD + release + AUR
  plan: `docs/CI-CD.md`
- `ROADMAP.md` — milestone status (live), `CONTEXT.md` — decisions + conventions

## Safety invariants (violating these = P0)

1. **Never-touch list** (`docs/SAFETY.md` §3): no rule may root in
   Documents/Pictures/Videos/Music/Projects, `/etc`, `/usr`, `/boot`;
   `~/Downloads` only as destructive + typed-PURGE gated.
2. **`scan`/`--dry-run` mutate nothing.** Execution re-validates allowed prefixes
   independently of the plan.
3. **`--yes` never bypasses destructive gates** — only `--i-know-what-im-doing`.
4. **All `$HOME`-relative paths go through `Ctx::home`** (resolved by
   `env::resolve_home()`); never a bare `$HOME` literal — sandboxes depend on it.
5. **No `sh -c`** — external commands are argv arrays only. **No network, ever.**
6. Destructive capability ships **with** its gate in the same change.

## Docs-sync rule

A change to a rule or CLI behavior **must** update `docs/RULES-CATALOG.md` /
`docs/CLI-UX.md` in the same commit; a workflow or packaging change **must**
update `docs/CI-CD.md`. The maintainer commits manually; agents end their
session with a suggested conventional commit message (`docs:`/`chore:`/`ci:`)
instead of committing. `sweep rules list` output is snapshot-tested against the
catalog. Golden snapshots (`insta`) are re-recorded deliberately
(`cargo insta review`), never blindly accepted. UI output must contain no emoji
(decision D13); the existing M1 tier glyphs are replaced by ASCII `+ ! #` in M2.

## Current milestone

See the first unchecked milestone in `ROADMAP.md`; work ticks happen there.
M1.5 is infrastructure-only and must not change `sweep` behavior. Definition of
Done for every milestone is at the top of that file.

## Test conventions

- Sandbox via `tests/common/mod.rs` helpers: temp home + `SWEEP_HOME` +
  `SWEEP_TEST_GUARD=1` (the guard makes real-home mutation refuse to run).
- Env-var tests must serialize (process-global) — use the `ENV_LOCK` pattern from
  `src/env.rs`.
- Never require sudo or network in tests.
