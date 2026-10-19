# sweep — Session Context (continuity layer)

Purpose: **any future session continues without re-asking anything.**
Seeded from the plan doc `sweep-CONTEXT.md`; this is the live repo copy.

---

## 1. What & where (one screen)

- **Project:** `sweep` — advanced, pretty, safety-first storage-cleaner CLI for Linux;
  Arch-first, distro-aware; Rust; single binary; no network, ever.
- **Location:** `~/Projects/sweep` (created at M0).
- **Main flow:** `sweep scan` (read-only) → `sweep clean` (checklist with
  safe/caution/destructive tiers) → gates (P1/P2/P3-PURGE) → two-pass execution
  (user pass, then one batched sudo pass) → summary card + history append.
- **Docs in this repo:** `README.md` (pitch) · `AGENTS.md` (agent entry) ·
  `ROADMAP.md` (live status) · this file · `docs/MASTER-PLAN.md` ·
  `docs/ARCHITECTURE.md` · `docs/RULES-CATALOG.md` · `docs/CLI-UX.md` ·
  `docs/SAFETY.md` · `docs/CONFIG.md` · `docs/JSON-SCHEMAS.md`.

## 2. Decision records (ADR-lite, normative)

**D1 — Language: Rust.**
*Context:* CLI needs TUI-quality prompts, progress, fast scans, single binary, AUR.
*Decision:* Rust with clap/inquire/indicatif/comfy-table/owo-colors/jwalk.
*Rejected:* Go (weaker prompt/TUI ecosystem polish), Python (runtime+deps on target
systems), Bash+fzf (fragile, untestable at this scope).
*Consequences:* slower iteration than Python; mitigated by milestone-scoped scope.

**D2 — Full-removal UX: two-step gate.**
*Context:* user wants an "everything including ~/Downloads" mode without regret.
*Decision:* scan summary screen → typed `PURGE` per destructive category;
`--yes` does NOT bypass; only `--i-know-what-im-doing` does.
*Rejected:* single y/N; trash-first (defeats freeing space).
*Consequences:* scripted full cleans need the explicit escape-hatch flag (tested).

**D3 — Scope: files + soft hogs.**
*Decision:* includes pacman/paru/AUR, journals, orphans, docker/podman, flatpak,
dev artifacts — each an independent checklist rule.
*Rejected:* files-only; files+pkg-caches-only.
*Consequences:* more external-command surface → argv-only execution policy + list-first previews.

**D4 — `dev-artifacts`: kept, caution-tier, 30-day gate, unchecked by default.**
*Consequences:* the biggest win (tens of GiB) ships with the strongest age guard
and top-20 preview; self-exclusion of the sweep repo mandatory.

**D5 — Name: `sweep`.** Verb-friendly, short completions, AUR-safe.

**D6 — Bonuses all in v0.1:** `sweep top`, `sweep history`, systemd timer
(report-only default, shipped disabled), Snapper/btrfs report-only.

**D7 — Destructive bypass = `--i-know-what-im-doing` only.** `--yes` never implies it.

**D8 — Sudo = single batched system pass** via
`sudo sweep --internal-system-pass <plan.json>` with schema+digest+uid validation;
pkexec fallback; decline reported, not fatal.

**D9 — Snapshot deletion: never.** Snapper support measures/reports only.

**D10 — Timer default = report-only scan.** Auto-clean requires explicit unit
override, documented, never packaged as default.

## 3. Conventions

### Code
- Rust: `rustfmt` defaults; `clippy -D warnings`; no `unwrap()/expect()` outside tests.
- Errors: `thiserror` inside layers, `anyhow` at top; every user-facing error has
  an id-free plain message + remediation hint where possible.
- All home-relative paths through `Ctx::home` (never bare `$HOME` literal) —
  enables `SWEEP_HOME` sandboxes; harness sets `SWEEP_TEST_GUARD`.
- External commands: argv arrays only (never `sh -c`), resolved PATH recorded.
- Rule ids/config keys/JSON fields are **stable API**: rename = breaking = revision log entry.
- Rule module ↔ doc pairing: `rules/foo.rs` changes require `docs/RULES-CATALOG.md`
  update in the same commit.

### Naming
- Code/config/JSON: kebab-case ids (`pacman-cache`). UI prose: "category"; docs/code: "rule".
- Tiers in code: `Tier::{Safe,Caution,Destructive}`; report-only = `report_only()` flag.

### UI copy style (matches CLI-UX §4)
- Sentence case, no exclamation marks, no blame ("skipped: firefox running", not
  "ERROR: you have firefox open").
- Every skip/error has a reason; every destructive screen states permanence.
- Defaults are safe: Enter cancels at P1; destructive needs typed PURGE.

### Process
- Conventional commits (`feat(rules): …`, `fix(exec): …`, `docs: …`).
- Milestone status updated in `ROADMAP.md` same-session; DoD checklist before any
  milestone is marked done.
- Safety-invariant changes require updating `docs/SAFETY.md` + this log together.

## 4. Environment facts

- Host: Linux, workspace `~/`; not a git repo at workspace root (repo lives at
  `~/Projects/sweep` once created).
- Plan mode currently restricts writes to `/home/pinefeast/.opencode/plan`
  (note: `write` tool guard was buggy on 2026-10-19 — shell heredoc worked; retry
  `write` at M0, fall back to shell if still broken).
- Target test OS for acceptance: Arch-based; secondary manual matrix Ubuntu LTS + Fedora.
- No network dependency anywhere; CI = fmt/clippy/test only.

## 5. Repo docs list (create at M0 — per-file content spec)

| File | Content |
|---|---|
| `AGENTS.md` | 1) what sweep is (3 lines) 2) layout map 3) build/test/fmt commands 4) safety invariants (never-touch, gates, $HOME rule) 5) docs-sync rule 6) current milestone pointer → ROADMAP |
| `CONTEXT.md` | this file's §2–§3 (decisions + conventions) + revision log seeded |
| `ROADMAP.md` | M0–M7 with live status (copy from plan; ticks happen here post-M0) |
| `README.md` | pitch, features (5 bullets), install (AUR/build), quickstart, presets table, safety blurb + link, FAQ, status badge |
| `docs/MASTER-PLAN.md` | from plan (positioning, stories, inventory, metrics, glossary) |
| `docs/ARCHITECTURE.md` | from plan (modules, types, flows, config, schemas, deps, budgets, testing) |
| `docs/RULES-CATALOG.md` | from plan — **spec of record**, snapshot-tested vs `rules list` |
| `docs/CLI-UX.md` | from plan — command reference + prompt catalog + errors + exit codes |
| `docs/SAFETY.md` | from plan — threat model, gates, never-touch, sudo protocol, checklist |
| `docs/CONFIG.md` | ARCHITECTURE §9 expanded with examples per rule + custom-rule recipe |
| `docs/JSON-SCHEMAS.md` | v1 schemas for scan/clean/top/history + versioning changelog |

## 6. Resume playbooks

**Playbook A — fresh session, continue building:**
1. Read `ROADMAP.md` → first unchecked milestone = current work.
2. Read `AGENTS.md` → commands (`cargo test`, etc.) + invariants.
3. Read the spec doc(s) relevant to that milestone's next task.
4. Implement → DoD (fmt/clippy/test/docs) → tick tasks → commit.

**Playbook B — mid-milestone interruption:** status lives in ROADMAP checkboxes;
unfinished subtask = first unchecked box under current `[~]` milestone. No other
state needed (no scratch files; PR/commit messages carry context).

**Playbook C — bug report / review:** reproduce → check `docs/CLI-UX.md` (is behavior
doc-conforming?) → `docs/SAFETY.md` (if deletion-related: treat as P0, apply PR
safety checklist) → fix with tests + doc updates.

**Playbook D — plan questions:** decisions are in §2 (D1–D10). If a question isn't
covered: add a new record here (D11+) with context/decision/rejection — never
silently diverge from a documented decision.

## 7. Revision log (plan & decisions over time)

| Date | Rev | Change |
|---|---|---|
| 2026-10-19 | R1 | Initial plan: Rust, gated full mode, all soft hogs, name `sweep`, dev-artifacts kept caution, bonuses (top/history/timer/snapper) |
| 2026-10-19 | R2 | Extensification pass: all 7 docs expanded to full spec depth (threat model, 26-rule catalog, prompt copy catalog, ADRs, M0–M7 tasks+gates) |
| 2026-10-19 | R3 | M0 built: repo scaffolded at `~/Projects/sweep` (edition 2024, clap surface complete, `SWEEP_TEST_GUARD` sandbox harness, 14 tests green); docs copied/adapted; history-record schema restored into `docs/ARCHITECTURE.md` §11 |

## 8. Open questions

- None blocking. All design calls resolved (D1–D10).
- Parking lot + risk register live in `ROADMAP.md` (bottom).
- Deferred product questions for post-0.1 dogfooding: checklist `/` filter,
  persistent size cache, apt-orphan parity.
