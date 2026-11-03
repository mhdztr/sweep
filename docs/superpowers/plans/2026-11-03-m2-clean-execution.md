# M2 Clean Execution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Finish the existing M2 slice for the four implemented safe rules so `sweep clean` can safely preview or execute real scanned candidates, record results, and run in scripted safe mode.

**Architecture:** Keep the existing phase order and hand-rolled TUI. Add an opt-in candidate capture path used by `clean` but not ordinary `scan`, carry eligible non-directory paths through `ScanInfo` into `PlanItem`, and let `exec::del` independently re-check every path before unlinking it. Keep destructive rules, sudo, and the remaining safe-rule catalog out of this slice.

**Tech Stack:** Rust 2024, clap, jwalk, libc/termios, indicatif, serde JSONL, assert_cmd, tempfile, PTY tests, insta snapshots.

---

## File map

- Modify `AGENTS.md`: mandatory verification, review, and commit policy.
- Modify `src/scan/walker.rs`: optional eligible-file candidate capture.
- Modify `src/scan/mod.rs`: candidate propagation and overlap subtraction.
- Modify `src/rules/tmp.rs`, `src/rules/cache.rs`, `src/rules/trash.rs`: enable candidate capture for clean scans.
- Modify `src/cli/clean.rs`: scripted safe flow, candidate-backed plan creation, and documented refusal boundaries.
- Modify `src/exec/mod.rs`, `src/exec/del.rs`: plan execution, mutation safety, and report handling.
- Modify `src/ui/progress.rs`, `src/ui/summary.rs`: stable safe output without emoji or unbounded padding.
- Add `tests/clean_execution.rs`: direct plan execution, dry-run mutation proof, history/log proof, and error continuation.
- Modify existing scan/walker/clean tests as candidate fields and flow contracts require.
- Modify `README.md`, `ROADMAP.md`, `docs/ARCHITECTURE.md`, `docs/CLI-UX.md`, `docs/SAFETY.md`, and `CHANGELOG.md`: keep implementation status and safety contract in sync.

## Working-tree rule

The current worktree has pre-existing uncommitted M2 edits. Preserve them, inspect
them before each overlapping edit, and never use reset/checkout/clean to discard
them. Commits must stage only the intended slice.

### Task 1: Add the mandatory delivery gate to project instructions

**Files:**
- Modify: `AGENTS.md`

- [ ] **Step 1: Add the project policy after the existing test conventions**

Add this section:

```markdown
## Mandatory delivery gate

Before reporting a change as complete, run every applicable check below from the
repository root and report the actual result. A skipped or unavailable command is
not a pass:

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

Run applicable repository tooling when installed: `cargo deny check`,
`cargo machete`, `actionlint`, and `typos`. Before delivery, review the complete
diff for correctness, regressions, dependency bloat, and the `docs/SAFETY.md`
checklist. Never claim a check passed unless it was run in the current session.

Commits use the required prefixes `docs:`, `fix:`, `feat:`, `chore:`, or
`view:`. Pushes are always left to the maintainer.
```

- [ ] **Step 2: Verify the policy is present**

Run:

```sh
grep -n "Mandatory delivery gate\|cargo test --all-targets\|review the complete diff" AGENTS.md
git diff --check
```

Expected: all policy phrases are present and `git diff --check` exits 0.

### Task 2: Capture real clean candidates without making ordinary scans heavy

**Files:**
- Modify: `src/scan/walker.rs`
- Modify: `src/scan/mod.rs`
- Modify: `src/rules/tmp.rs`
- Modify: `src/rules/cache.rs`
- Modify: `src/rules/trash.rs`
- Test: `tests/scan_walker.rs`
- Test: `tests/scan_rules.rs`

- [ ] **Step 1: Write the failing candidate-capture test**

Add a test proving capture is opt-in and only counted non-directory paths are
captured:

```rust
#[test]
fn candidate_capture_is_opt_in_and_excludes_directories() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(root.path().join("nested")).expect("mkdir");
    std::fs::write(root.path().join("nested/file"), b"data").expect("write");

    let pool = Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .build()
            .expect("pool"),
    );
    let mut opts = WalkOptions::new(pool);
    opts.capture_candidates = true;
    let stats = walk(root.path(), &opts).expect("walk");

    assert_eq!(stats.candidates, vec![root.path().join("nested/file")]);
    assert!(stats.samples.iter().any(|p| p.ends_with("nested")));
}
```

Add a pipeline assertion that user-cache candidates do not retain thumbnail
files after overlap subtraction, while tmp age-gated files remain absent.

- [ ] **Step 2: Run the focused test and confirm the field is missing**

Run:

```sh
cargo test --test scan_walker candidate_capture_is_opt_in_and_excludes_directories --locked
```

Expected: compile failure because candidate capture is not yet defined.

- [ ] **Step 3: Add opt-in candidates to walker and scan models**

Add `capture_candidates: bool` to `WalkOptions`, defaulting to `false`, and add
`candidates: Vec<PathBuf>` to `WalkStats` and `ScanInfo`. In the walker consumer,
after the gate and before counters update, append only counted `File` or
`Symlink` entries when capture is enabled:

```rust
if opts.capture_candidates
    && matches!(info.kind, EntryKind::File | EntryKind::Symlink)
{
    stats.candidates.push(entry.path());
}
```

Merge candidate vectors in `WalkStats::merge`, and pass them through each
`RuleScan` to `RuleResult`.

- [ ] **Step 4: Add a candidate-enabled scan entry point**

Keep the current API unchanged for ordinary scan callers:

```rust
pub fn run(ctx: &Ctx, cancel: Arc<AtomicBool>, events: Option<mpsc::Sender<ScanEvent>>) -> ScanOutput {
    run_with_candidates(ctx, cancel, events, false)
}

pub fn run_with_candidates(
    ctx: &Ctx,
    cancel: Arc<AtomicBool>,
    events: Option<mpsc::Sender<ScanEvent>>,
    capture_candidates: bool,
) -> ScanOutput {
}
```

Rename the current `run` implementation to `run_with_candidates`, add the
`capture_candidates` parameter to `ScanService`, and leave its registry ordering,
detect/scan assembly, overlap byte subtraction, and totals code unchanged.
Use the service flag when constructing `WalkOptions` in the four implemented
rules. Call `run_with_candidates(..., true)` only from `cli::clean`.

- [ ] **Step 5: Filter overlap candidates**

After the existing thumbnails byte/file/dir subtraction, remove every candidate
from `user-cache.info.candidates` that is also present in
`thumbnails.info.candidates`, preserving deterministic walker order.

- [ ] **Step 6: Run the scan suite**

Run:

```sh
cargo test --test scan_walker --locked
cargo test --test scan_rules --locked
cargo test --test scan_mutation_free --locked
cargo test --test scan_json --locked
```

Expected: all scan tests pass and ordinary scan output/schema remains unchanged.

### Task 3: Make execution consume candidates safely

**Files:**
- Modify: `src/cli/clean.rs`
- Modify: `src/exec/mod.rs`
- Modify: `src/exec/del.rs`
- Add: `tests/clean_execution.rs`

- [ ] **Step 1: Write failing direct-execution tests**

Add sandbox tests for: eligible files are removed while unrelated files survive;
root/outside-prefix/protected/symlink inputs are refused safely; dry-run leaves
the manifest and state directory unchanged; and one rule error does not prevent a
later rule from running. Use `Ctx::for_test`, `PlanItem`, and `exec::execute`;
never invoke sudo or a real home.

Example plan fixture:

```rust
let plan = Plan {
    selected: vec![PlanItem {
        id: "tmp",
        needs_sudo: false,
        tier: Tier::Safe,
        paths: vec![old_file.clone()],
        purge_ack: false,
    }],
    gate_ack: BTreeMap::new(),
    keep_days: Some(1),
    dry_run: false,
    trash: false,
    schema: sweep::SCHEMA_VERSION,
};
```

- [ ] **Step 2: Run the new tests and confirm current unsafe behavior**

Run:

```sh
cargo test --test clean_execution --locked
```

Expected: tests fail until candidate-backed execution and independent checks are
implemented.

- [ ] **Step 3: Switch plan creation from samples to candidates**

In `cli::clean`, build `path_map` from `r.info.candidates`, never
`r.info.samples`. Extract a pure helper for unit testing:

```rust
fn build_plan_items(
    selected: &[Selected],
    results: &[RuleResult],
) -> Vec<PlanItem> {
    let by_id: BTreeMap<&str, &RuleResult> = results
        .iter()
        .map(|result| (result.id, result))
        .collect();
    selected
        .iter()
        .filter_map(|item| {
            by_id.get(item.id).map(|result| PlanItem {
                id: item.id,
                needs_sudo: result.needs_sudo,
                tier: result.tier,
                paths: result.info.candidates.clone(),
                purge_ack: false,
            })
        })
        .collect()
}
```

- [ ] **Step 4: Strengthen the deletion boundary**

In `exec/del.rs`, reject empty paths and `/`, require an allowed prefix using
path components, use `symlink_metadata`, unlink symlinks without following them,
share tmp protected-name checks with `rules::tmp::is_protected`, skip sockets/
FIFOs/character devices, count bytes only after successful unlink, and never
recursively delete a directory from the candidate list in this M2 slice.

Add `Plan::validate` that rejects an unsupported schema, destructive item, or
sudo item in this M2 user pass, returning a stable `ExecError` remediation.

- [ ] **Step 5: Make dry-run and state behavior explicit**

Ensure dry-run takes no lock and writes no log/history. Real runs hold the lock
through deletion and state writes. Print the summary once, not from both the CLI
and execution layer.

- [ ] **Step 6: Run focused execution tests**

```sh
cargo test --test clean_execution --locked
cargo test --test clean_mutation_free --locked
cargo test --test clean_wiring --locked
```

### Task 4: Add scripted safe clean mode and preserve gate boundaries

**Files:**
- Modify: `src/cli/clean.rs`
- Modify: `tests/cli_smoke.rs`
- Modify: `tests/clean_execution.rs`
- Modify: `docs/CLI-UX.md`, `docs/SAFETY.md`

- [ ] **Step 1: Write failing no-TTY scripted tests**

Run the binary with `SWEEP_HOME` and `SWEEP_TEST_GUARD=1` and assert
`clean --preset safe --yes` succeeds, removes only fixture candidates, and emits
the summary. Also pin that `--json` clean remains refused and `--yes` with
`--preset full` does not select destructive rows.

- [ ] **Step 2: Implement scripted selection after context build**

Move the TTY preflight below the scripted-mode decision. For `--no-prompt` or
`--yes` with a selection flag, scan with candidates, filter `--only`/`--exclude`,
construct `Selection`, take preset-selected safe/caution rows, and skip
interactive P1 only for those non-destructive rows. Destructive rows remain
unchecked/locked until the future typed-PURGE flow.

Interactive mode continues to require a TTY and keeps P1 default-cancel behavior.
`--trash`, `--json`, and destructive scripted execution remain explicit refusals.

- [ ] **Step 3: Add exit and mutation boundary tests**

Pin scripted safe success, scripted dry-run mutation freedom, interactive abort
exit 2, `--trash` exit 1, clean `--json` exit 1, and full-preset `--yes` never
removing destructive targets.

- [ ] **Step 4: Update CLI and safety docs**

Replace “clean stops at preview” with the precise four-rule execution status,
scripted-safe support, and the remaining intentional refusals.

### Task 5: Stabilize progress, summary, and state records

**Files:**
- Modify: `src/ui/progress.rs`
- Modify: `src/ui/summary.rs`
- Modify: `src/state.rs`, `src/exec/mod.rs`
- Test: `tests/clean_execution.rs`

- [ ] **Step 1: Add pure summary tests before changing copy**

Test exact no-color/no-unicode summary strings, zero-error and error cases,
skipped counts, dry-run labeling, and that every physical line fits the
documented minimum width. Do not use a Unicode checkmark; use ASCII markers.

- [ ] **Step 2: Implement bounded summary rendering**

Render the documented card with ASCII-safe copy, explicit `freed`, `cleaned`,
`skipped`, `errors`, and dry-run state. Compute padding from the actual card width
instead of appending a fixed number of spaces after variable content.

- [ ] **Step 3: Keep progress stderr-only and JSON-safe**

Progress is shown only for a TTY, never contaminates JSON stdout, uses ASCII
ticks under `--no-unicode`, and clears itself on normal and error exits.

- [ ] **Step 4: Test state output**

Assert that a mutating run creates one mode-0600 log and one schema-v1 JSONL
history record, pruning honors configured retention, and state write failures
warn without changing a successful cleanup result.

### Task 6: Synchronize status and documentation

**Files:**
- Modify: `README.md`
- Modify: `ROADMAP.md`
- Modify: `CHANGELOG.md`
- Modify: `docs/ARCHITECTURE.md`
- Modify: `docs/CLI-UX.md`
- Modify: `docs/SAFETY.md`

- [ ] **Step 1: Update implementation status**

Document the four implemented safe rules as executable in M2, scripted safe
mode, candidate capture only for clean scans, and the remaining M2 exclusions:
remaining safe rules, full dry-run/PURGE, trash, sudo, and history viewer.

- [ ] **Step 2: Update the roadmap without falsely closing M2**

Tick only tasks actually implemented: execution primitives for the current four
rules, dry-run printing, per-rule progress/summary, and state logging if their
acceptance tests pass. Leave remaining safe rules, full gate wiring, and the M2
acceptance gate unchecked.

- [ ] **Step 3: Run documentation drift checks**

```sh
grep -RIn "stops at\|preview only\|not implemented" README.md ROADMAP.md docs src
git diff --check
```

Expected: every remaining refusal is intentional and matches the current code.

### Task 7: Full verification, review, and local commits

**Files:**
- All changed files from Tasks 1–6

- [ ] **Step 1: Run the complete mandatory delivery gate**

Run each command separately from the repository root:

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

Also run installed checks:

```sh
cargo deny check
cargo machete
actionlint
typos
```

Record unavailable tools as unavailable, never as passed.

- [ ] **Step 2: Perform code and safety review**

Inspect:

```sh
git diff --check
git diff --stat
git diff -- AGENTS.md src/scan src/rules src/cli/clean.rs src/exec src/ui tests docs README.md ROADMAP.md CHANGELOG.md
git status --short
```

Review specifically for path-prefix bypasses, symlink following, protected tmp
entries, `$HOME` literals, `sh -c`, network calls, destructive gate bypasses,
state leaks, stdout JSON contamination, unchecked errors, test gaps, and
unrelated changes. Fix findings before committing.

- [ ] **Step 3: Commit only coherent slices**

Stage explicit paths and use the required prefixes:

```sh
git add AGENTS.md
git commit -m "docs: require full verification and code review"

git add src/scan src/rules tests/scan_walker.rs tests/scan_rules.rs
git commit -m "feat: carry clean candidates from scan"

git add src/cli/clean.rs src/exec src/ui tests/clean_execution.rs tests/clean_wiring.rs tests/clean_mutation_free.rs tests/cli_smoke.rs
git commit -m "feat: execute safe clean plans"

git add README.md ROADMAP.md CHANGELOG.md docs/ARCHITECTURE.md docs/CLI-UX.md docs/SAFETY.md
git commit -m "docs: sync M2 clean execution status"
```

If a coherent slice has no changes, do not create an empty commit. Never push.

- [ ] **Step 4: Re-run status and report evidence**

```sh
git status --short --branch
git log --oneline -8
```

Report commit IDs, every verification command and result, unavailable tools, and
the exact remaining roadmap items.
