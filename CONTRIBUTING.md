# Contributing to sweep

Thanks for helping make `sweep` safer. This project is pre-alpha: the safety model matters more than feature velocity.

## Read first

1. [`AGENTS.md`](AGENTS.md) — session entry point, repo layout, safety invariants.
2. [`ROADMAP.md`](ROADMAP.md) — current milestone and definition of done.
3. [`docs/SAFETY.md`](docs/SAFETY.md) — threat model, never-touch list, gate protocol.
4. [`docs/CI-CD.md`](docs/CI-CD.md) — required when changing workflows or packaging.

## Non-negotiables

- `scan` and `--dry-run` must never mutate anything. CI enforces this.
- `--yes` must never bypass destructive gates. Only `--i-know-what-im-doing` can.
- No rule may root in `Documents`, `Pictures`, `Videos`, `Music`, `Projects`, `/etc`, `/usr`, or `/boot`. `~/Downloads` is destructive and typed-`PURGE` gated.
- All `$HOME`-relative paths go through `Ctx::home` / `env::resolve_home()`.
- No `sh -c`, no network, ever. External commands are argv arrays.
- A destructive capability ships with its gate in the same change.

## Docs-sync rule

A rule or CLI behavior change must update `docs/RULES-CATALOG.md` and `docs/CLI-UX.md` in the same commit. A workflow or packaging change must update `docs/CI-CD.md`. `sweep rules list` output is snapshot-tested against the catalog.

## Development workflow

```sh
cargo build
cargo test
cargo run -- --version
cargo run -- scan          # read-only smoke check
```

Before proposing a change, run every applicable check from the repository root and report the actual result:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --locked
cargo test --doc --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items --locked
cargo build --release --locked
cargo run -- --version
cargo test --test cli_smoke --locked
```

Run `cargo deny check`, `cargo machete`, `actionlint`, and `typos` when installed. A skipped command is not a pass. Review the complete diff against the `docs/SAFETY.md` checklist.

## Style and tests

- Keep `src/main.rs` thin: parse, dispatch, exit code.
- UI text contains no emoji. Tier glyphs are ASCII `+ ! #` (decision D13).
- Sandbox tests use `tests/common/mod.rs` helpers with a temp home, `SWEEP_HOME`, and `SWEEP_TEST_GUARD=1`.
- Env-var tests must serialize with the `ENV_LOCK` pattern from `src/env.rs`.
- Tests never require sudo or network.
- Golden snapshots (`insta`) are re-recorded deliberately with `cargo insta review`, never blindly accepted.

## Commits

Use one logical change per commit with the prefixes `docs:`, `fix:`, `feat:`, `chore:`, or `view:`. Conventional commit messages are welcome. Do not add Copilot `Co-authored-by` trailers or other generated attribution. Do not change collaborators or repository permissions.
