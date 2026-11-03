# M2 Clean Execution Design

## Goal

Complete the M2 safe-tier clean flow as a small, safety-first vertical slice:
scan, select, gate, preview or execute safe cleanup, report the result, and
persist an auditable history entry without weakening any destructive-tier or
sudo safeguards.

## Scope

This slice covers:

- real candidate-path propagation from scan results into the execution plan;
- the existing interactive checklist and P1 summary gate, plus the M2-safe
  execution path;
- dry-run samples and argv previews with zero filesystem mutation;
- sequential deletion with execution-time safety checks and per-rule errors;
- progress and summary output that remain readable in narrow, no-color, and
  no-unicode terminals;
- action logs and bounded history records for mutating runs;
- focused unit, integration, PTY, mutation, and CLI tests;
- project guidance requiring the complete verification matrix and code/safety
  review before delivery.

This slice explicitly does not implement destructive PURGE gates, the sudo
system pass, all remaining safe rules, or the M3 caution-rule library.

## Design

The flow remains strictly ordered:

```text
scan -> checklist -> P1 summary -> plan -> dry-run or user execution -> report
                                                        -> log/history
```

`scan::RuleResult` remains the source of measured candidates. The plan builder
will carry the complete candidate set available to the rule rather than treating
the display sample list as a deletion plan. The execution layer will still
perform its own path-prefix, protected-entry, symlink, and root checks; no plan
is trusted merely because it came from the scanner.

Execution is sequential and continue-on-error at rule boundaries. A failed
target is recorded on its rule, later rules still run, and the final exit hint
is `3` when any execution error occurs. Dry-run prints bounded samples and the
argv-shaped operation to stderr, takes no lock, and writes no history or log.

The current hand-rolled checklist, raw terminal layer, and pure renderer remain
in place. UI changes will be limited to correcting behavior/copy/width issues
needed by this flow; no new TUI dependency is introduced. Dynamic content is
sanitized before rendering, and ASCII/no-color behavior remains test-locked.

## Safety and error handling

- `--yes` may only confirm safe/caution paths; it never bypasses destructive
  gates.
- `--trash` stays refused until the XDG-trash implementation is complete rather
  than silently behaving as hard delete.
- No `sh -c`, network, or bare `$HOME` path is introduced.
- Mutating runs take the existing single-instance lock and release it after
  state writes.
- Log/history write failures warn without changing the cleanup result.
- No-TTY, gate abort, invalid selection, and execution failures preserve the
  documented exit-code mapping.

## Verification policy

Before delivery, the project must run and record the applicable results from:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --locked
cargo test --doc --locked
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --document-private-items --locked
cargo build --release --locked
cargo run -- --version
cargo test --test cli_smoke --locked
```

Applicable repository security/tooling checks will also run when installed
(`cargo deny`, `cargo machete`, `actionlint`, and `typos`). The final diff must
receive a code review and a separate safety review against `docs/SAFETY.md`.

## Acceptance criteria

1. A selected safe rule carries real candidates into the plan and executes only
   targets that survive independent execution checks.
2. Dry-run leaves a byte-for-byte sandbox manifest unchanged and does not create
   state/history files.
3. Successful execution prints a stable summary and appends a schema-v1
   history record plus an audit log.
4. Per-rule failures continue the run and return exit code `3`.
5. Existing checklist, scan, safety, and renderer tests remain green.
6. `AGENTS.md` records the mandatory verification and review gate for future
   sessions.
