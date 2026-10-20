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
  `docs/SAFETY.md` · `docs/CONFIG.md` · `docs/JSON-SCHEMAS.md` ·
  `docs/CI-CD.md` (CI/CD, security scanning, release, AUR packaging).

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

**D11 — Distribution: AUR primary; Flatpak deferred.**
*Context:* Arch-first audience; Flathub would add reach but imposes a sandbox.
*Decision:* ship `sweep-bin` (static musl release asset) first, then `sweep`
(source PKGBUILD). GitHub releases carry gnu + musl binaries, checksums, SBOM,
and SLSA provenance. **Flatpak is not packaged before 1.0.**
*Rejected:* Flatpak for 0.1 (private `/tmp`, no sudo, no pacman/systemd, broad
home permission, and the `org.freedesktop.Flatpak` host-command escape hatch is
against the safety posture); crates.io as a distribution channel.
*Consequences:* distribution stays native; Flatpak stays in the parking lot and
is revisited post-1.0 only with a home-only scope if demand appears.

**D12 — CI/CD: security-gated, tag-driven releases.**
*Context:* the repo started with one fmt/clippy/test job.
*Decision:* five workflows (`ci`, `security`, `release`, `packaging`,
`scheduled`); `cargo-deny` is the vulnerability gate (advisories + licenses +
bans + sources); PRs require fmt/clippy/test/doctest/docs/release-build/deny;
releases build only on `v*` tags via a dry-runnable `release.yml` with SBOM +
provenance; no secrets in build jobs. Detail: `docs/CI-CD.md`.
*Rejected:* `cargo-audit` alone (advisories only), release-on-every-push,
publishing from `main` without tags.
*Consequences:* required checks mean a PR-based flow; the PR gate stays under
~5 minutes and slow checks move to `scheduled.yml`.

**D13 — No emoji in UI output.**
*Context:* explicit user requirement; emoji break alignment and terminal
portability.
*Decision:* tier glyphs are ASCII `+ ! #` in every mode; no emoji anywhere in
TUI output. Unicode box drawing stays, with `--no-unicode` for full ASCII.
*Consequences:* width math + the five golden snapshots change; the UI specs'
examples keep the M1 glyphs until M2 updates them in the same commit as the
code; lands as the first M2 task (ROADMAP).

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
- Workflow/packaging changes require `docs/CI-CD.md` update in the same commit.
- UI output must contain no emoji (D13); M1's emoji tier glyphs are replaced by
  ASCII `+ ! #` in M2.

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

- Host: Linux. Repo: `~/Projects/sweep` (git, public GitHub); the workspace root
  `~/` is not a git repo.
- Session plan docs live in `/home/pinefeast/.opencode/plan`; project plans and
  specs live in the repo (`docs/`, `ROADMAP.md`, `CONTEXT.md`).
- Target test OS for acceptance: Arch-based; secondary manual matrix Ubuntu LTS + Fedora.
- No network dependency at runtime. CI today = one fmt/clippy/test job; the full
  plan (security scans, tag-gated release, AUR validation) is `docs/CI-CD.md`,
  tracked as ROADMAP M1.5 (foundation) and M7 (ship).

## 5. Repo docs list (M0 baseline + M1.5 additions — per-file content spec)

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
| `docs/CI-CD.md` | CI workflows, security scans + `deny.toml` policy, release matrix/SBOM/provenance, AUR packaging, Flatpak deferral, release runbook |

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

**Playbook D — plan questions:** decisions are in §2 (D1–D13). If a question isn't
covered: add a new record here (D14+) with context/decision/rejection — never
silently diverge from a documented decision.

**Playbook E — release:** follow `docs/CI-CD.md` §6.5. Tag only after the M7
acceptance gate; verify checksums + provenance before publishing; update AUR
`sweep-bin` first, then `sweep`.

## 7. Revision log (plan & decisions over time)

| Date | Rev | Change |
|---|---|---|
| 2026-10-19 | R1 | Initial plan: Rust, gated full mode, all soft hogs, name `sweep`, dev-artifacts kept caution, bonuses (top/history/timer/snapper) |
| 2026-10-19 | R2 | Extensification pass: all 7 docs expanded to full spec depth (threat model, 26-rule catalog, prompt copy catalog, ADRs, M0–M7 tasks+gates) |
| 2026-10-19 | R3 | M0 built: repo scaffolded at `~/Projects/sweep` (edition 2024, clap surface complete, `SWEEP_TEST_GUARD` sandbox harness, 14 tests green); docs copied/adapted; history-record schema restored into `docs/ARCHITECTURE.md` §11 |
| 2026-10-19 | R4 | M1 scan engine: jwalk 0.8 walker pruning subtrees via process_read_dir children.retain (verified from source); scan is lock-free — flock scope = mutating commands only (ARCHITECTURE §4, SAFETY §10); hand-rolled ANSI/emoji-aware table renderer (+unicode-width 0.2) instead of comfy-table presets; "newest" = newest counted entry (age-gated files excluded from every count) |
| 2026-10-19 | R5 | Repo hardening + distribution planned (`docs/CI-CD.md`): five-workflow CI/CD design; `cargo-deny` as the vulnerability gate; tag-gated release with SBOM + SLSA provenance; AUR `sweep-bin` then source `sweep`; Flatpak deferred (D11); no-emoji UI rule (D13). ROADMAP gains M1.5 (infra, before M2) and expanded M7 release tasks |
| 2026-10-20 | R7 | Dependabot's first cargo group PR (7 bumps) diagnosed: jwalk 0.9 is unmaintained and its API break (removed `read_children_error`) broke `src/scan/walker.rs`; pin held at 0.8.x and `jwalk >=0.9.0` added to Dependabot's ignore list. The same bump retired both advisory ignores: indicatif 0.18 dropped `number_prefix`, inquire 0.9 dropped `fxhash` — `deny.toml` `[advisories] ignore` is now empty. Other six bumps (comfy-table 8, dirs 7, indicatif 0.18, inquire 0.9, toml 1.1, clap_mangen 0.3) verified green across all checks including MSRV 1.88 |
| 2026-10-20 | R6 | M1.5 infra landed: five workflows (ci/security/scheduled/release/packaging), `deny.toml` (two dated `unmaintained` ignores: fxhash via inquire, number_prefix via indicatif), `SECURITY.md`, `CHANGELOG.md`, Dependabot; Cargo metadata `rust-version = "1.88"` (discovered by bisection — comfy-table let-chains floor, above the 1.85 edition-2024 floor); release profile `strip = "debuginfo"` for `cargo auditable` survival; machete ignores in `[package.metadata.cargo-machete]`; actionlint placed in `security.yml` (resolves the ROADMAP parenthetical). No `sweep` behavior change (only a `src/config.rs` doc-comment link fix for the `docs` gate) |

## 8. Open questions

- None blocking. All design calls resolved (D1–D10).
- Parking lot + risk register live in `ROADMAP.md` (bottom).
- Deferred product questions for post-0.1 dogfooding: checklist `/` filter,
  persistent size cache, apt-orphan parity.
- Flatpak packaging is **decided** (D11: not before 1.0), not an open question.
