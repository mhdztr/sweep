# sweep: Session Context (continuity layer)

Purpose: **any future session continues without re-asking anything.**
Seeded from the plan doc `sweep-CONTEXT.md`; this is the live repo copy.

---

## 1. What & where (one screen)

- **Project:** `sweep`, an advanced, pretty, safety-first storage-cleaner CLI for Linux;
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

**D1: Language: Rust.**
*Context:* CLI needs TUI-quality prompts, progress, fast scans, single binary, AUR.
*Decision:* Rust with clap/inquire/indicatif/comfy-table/owo-colors/jwalk.
*Rejected:* Go (weaker prompt/TUI ecosystem polish), Python (runtime+deps on target
systems), Bash+fzf (fragile, untestable at this scope).
*Consequences:* slower iteration than Python; mitigated by milestone-scoped scope.

**D2: Full-removal UX: two-step gate.**
*Context:* user wants an "everything including ~/Downloads" mode without regret.
*Decision:* scan summary screen → typed `PURGE` per destructive category;
`--yes` does NOT bypass; only `--i-know-what-im-doing` does.
*Rejected:* single y/N; trash-first (defeats freeing space).
*Consequences:* scripted full cleans need the explicit escape-hatch flag (tested).

**D3: Scope: files + soft hogs.**
*Decision:* includes pacman/paru/AUR, journals, orphans, docker/podman, flatpak,
dev artifacts, each an independent checklist rule.
*Rejected:* files-only; files+pkg-caches-only.
*Consequences:* more external-command surface → argv-only execution policy + list-first previews.

**D4: `dev-artifacts`: kept, caution-tier, 30-day gate, unchecked by default.**
*Consequences:* the biggest win (tens of GiB) ships with the strongest age guard
and top-20 preview; self-exclusion of the sweep repo mandatory.

**D5: Name: `sweep`.** Verb-friendly, short completions, AUR-safe.

**D6: Bonuses all in v0.1:** `sweep top`, `sweep history`, systemd timer
(report-only default, shipped disabled), Snapper/btrfs report-only.

**D7: Destructive bypass = `--i-know-what-im-doing` only.** `--yes` never implies it.

**D8: Sudo = single batched system pass** via
`sudo sweep --internal-system-pass <plan.json>` with schema+digest+uid validation;
pkexec fallback; decline reported, not fatal.

**D9: Snapshot deletion: never.** Snapper support measures/reports only.

**D10: Timer default = report-only scan.** Auto-clean requires explicit unit
override, documented, never packaged as default.

**D11: Distribution: AUR primary; Flatpak deferred.**
*Context:* Arch-first audience; Flathub would add reach but imposes a sandbox.
*Decision:* ship `sweep-bin` (static musl release asset) first, then `sweep`
(source PKGBUILD). GitHub releases carry gnu + musl binaries, checksums, SBOM,
and SLSA provenance. **Flatpak is not packaged before 1.0.**
*Rejected:* Flatpak for 0.1 (private `/tmp`, no sudo, no pacman/systemd, broad
home permission, and the `org.freedesktop.Flatpak` host-command escape hatch is
against the safety posture); crates.io as a distribution channel.
*Consequences:* distribution stays native; Flatpak stays in the parking lot and
is revisited post-1.0 only with a home-only scope if demand appears.

**D12: CI/CD: security-gated, tag-driven releases.**
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

**D13: No emoji in UI output.**
*Context:* explicit user requirement; emoji break alignment and terminal
portability.
*Decision:* tier glyphs are ASCII `+ ! #` in every mode; no emoji anywhere in
TUI output. Unicode box drawing stays, with `--no-unicode` for full ASCII
**within the checklist renderer** (D16); `scan` output is not yet ASCII-clean
under the flag (deferred, ROADMAP M3) (the dynamic-text half of that guarantee
is enforced in `ui::render::sanitize`; see D16).
*Consequences:* width math + the golden snapshots changed; the UI specs'
examples were updated to ASCII in the same commit as the code (M2, first task).

**D16: Dynamic text is sanitized at the render boundary: ASCII under `--no-unicode`, controls always stripped.**
*Context:* the reviewer (B1) found that the slice-2 renderer ASCII-ized only its
chrome: a row `title` (`render.rs` `row.title`) and a note text (incl.
`guard {reason}`) were appended verbatim, so `Selection::new(&[…], Regular, false,
false, &{"a" => "na\u{ef}ve running"})` rendered a non-ASCII byte with `unicode: false`
and `frame.is_ascii() == false`; the tests passed only because their fixtures
were ASCII. The same sink is a terminal-escape injection vector: `display_width`/
`truncate` treat `ESC[…` as zero-width and copy it whole, so a crafted title
(future `system/proc.rs` process names) would be emitted and executed by the
terminal. `CONTEXT.md`/`docs/CLI-UX.md` already asserted the opposite invariant.
*Decision:* one pure helper, `ui::render::sanitize(text, unicode)`, applied to
every piece of *dynamic* text (row title, guard/disabled reason, any note) in
`render_row`, and to the footer's content before painting. It strips (a) every
control char (C0 `0x00-0x1f` incl. `ESC` `0x1b`, C1 `0x80-0x9f` incl. the
8-bit CSI `0x9b`, and DEL `0x7f`) **always**, regardless of `theme.unicode`
(escape injection is not a Unicode-vs-ASCII question), and (b) every non-ASCII
char when `!theme.unicode`. Chars are **dropped**, not replaced with `?`: the
sanitizer must never lengthen the string it hands the width/truncate math.
`theme.unicode == true` keeps legitimate Unicode (accents, CJK, box drawing,
`·`, `…`) unchanged; only controls go. Chrome-only lines are unchanged, since
their content is module literals that are already ASCII-clean.
*Rejected:* `?`-per-char substitution (lengthens the string; can overflow a full
row and collapse it to a bare ellipsis, regression-tested); scoping the
guarantee to chrome and "fixing the docs instead" (rejected by the reviewer)
(this rejection applies to `ui::render.rs`; the CLI-wide claim is a separate,
deferred item).
*Consequences:* `docs/CLI-UX.md` §3/§8/§11 and this file now state exactly what
`--no-unicode` guarantees; tests that claimed the invariant but used ASCII
fixtures (`src/ui/render.rs`, `tests/checklist_render.rs`) now carry a non-ASCII
title **and** a non-ASCII guard reason and assert `frame.is_ascii()`; a new test
asserts no `ESC`/C1/DEL byte survives under **either** unicode setting. A second
bug surfaced while testing the first: running `sanitize` over an already-painted
line (the first footer draft) stripped the SGR introducer and left the CSI *body*
(`[2m`) as visible text, so the sanitizer runs on content *before*
`Theme` paints it. Scope: this guarantee is the **checklist renderer's**
(`ui::render.rs`); `scan`'s pre-existing `ui/mod.rs` header/footer and
`ui/table.rs` `—` placeholders are non-ASCII under the flag and are **not**
covered; tracked as a ROADMAP M3 task (see D13).

**D14: The interactive checklist is hand-rolled; `inquire` is not its renderer.**
*Context:* ROADMAP M2 and ARCHITECTURE §7 planned `inquire::MultiSelect` with a
custom renderer. inquire 0.9.4 was rewritten: `MultiSelectConfig` exposes only
`vim_mode`/`page_size`/`keep_filter`/`reset_cursor`: no custom-keybinding hook and
no per-row-state renderer, so the required keys (`space/a/n/i/p/enter/esc·q/Ctrl-C/?`)
and row states (ready/guarded/locked/disabled/report-only) are not expressible.
*Decision:* the checklist is a hand-rolled prompt loop (slice 2) over a pure,
renderer-agnostic `ui::checklist::Selection` state model (slice 1). `inquire` is
retained only for typed-`PURGE` input in `ui/confirm.rs` (M2/M4).
*Rejected:* forcing inquire (can't express the keys/states); dropping inquire now
(PURGE input not yet written).
*Consequences:* the `Cargo.toml` machete ignore comment for `inquire` is corrected;
ARCHITECTURE §7 updated; the core is unit-testable without a tty.

**D15: Raw-mode terminal I/O is hand-rolled on `libc`/termios; no TUI dependency.**
*Context:* D14 requires a hand-rolled checklist; `inquire` 0.9.4 cannot express the
required keys/row states. The ROADMAP RISK table named "simple own prompt loop on
raw tty (termios)" as the fallback. The project already depends on `libc` for
`terminal_width()` (ARCHITECTURE §7) and is Linux-only (§17).
*Decision:* raw mode, key decoding, cursor hide/show, and SIGINT/SIGWINCH handling
live in `src/ui/tty.rs`, implemented with `libc` (`tcgetattr`/`tcsetattr`/`ioctl`/
`signal`, ISIG kept on so Ctrl-C keeps `SIGINT` semantics). No `crossterm`/`ratatui`/
`console` dependency is added. The renderer is a pure `src/ui/render.rs`
(`render_frame`, `render_totals_bar`, `render_help`); the event loop is a thin
impure shell in `ui/checklist.rs` (`run_interactive`), with the key→intent mapping
(`apply_key`) factored out as a pure, tty-free function.
*Rejected:* `crossterm` (new dep tree + `deny.toml`/machete churn for a Linux-only
CLI), `inquire` (D14).
*Consequences:* confined, SAFETY-commented `unsafe` in `src/ui/tty.rs` (the
`libc` call-sites: `isatty`, `tcgetattr`, `tcsetattr`, `fcntl`/`O_NONBLOCK`, the
cursor `write`, `read`, `poll`, `sigaction`/`sigemptyset`,
`mem::zeroed`/`assume_init`, and `Drop`'s `tcsetattr`; 21 `unsafe {` blocks in
`src/ui/tty.rs`, 13 production (each SAFETY-commented) and 8 test-only) plus one
`isatty` probe in `src/ui/checklist.rs` (does the checklist own stdout? the
module is otherwise pure) and the pre-existing `ioctl` in `src/ui/mod.rs`;
`--no-unicode`/color stay on the existing `Theme`; the renderer is unit-testable
without a tty.

## 3. Conventions

### Code
- Rust: `rustfmt` defaults; `clippy -D warnings`; no `unwrap()/expect()` outside tests.
- Errors: `thiserror` inside layers, `anyhow` at top; every user-facing error has
  an id-free plain message + remediation hint where possible.
- All home-relative paths through `Ctx::home` (never bare `$HOME` literal);
  this enables `SWEEP_HOME` sandboxes; harness sets `SWEEP_TEST_GUARD`.
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

## 5. Repo docs list (M0 baseline + M1.5 additions: per-file content spec)

| File | Content |
|---|---|
| `AGENTS.md` | 1) what sweep is (3 lines) 2) layout map 3) build/test/fmt commands 4) safety invariants (never-touch, gates, $HOME rule) 5) docs-sync rule 6) current milestone pointer → ROADMAP |
| `CONTEXT.md` | this file's §2–§3 (decisions + conventions) + revision log seeded |
| `ROADMAP.md` | M0–M7 with live status (copy from plan; ticks happen here post-M0) |
| `README.md` | pitch, features (5 bullets), install (AUR/build), quickstart, presets table, safety blurb + link, FAQ, status badge |
| `docs/MASTER-PLAN.md` | from plan (positioning, stories, inventory, metrics, glossary) |
| `docs/ARCHITECTURE.md` | from plan (modules, types, flows, config, schemas, deps, budgets, testing) |
| `docs/RULES-CATALOG.md` | from plan; **spec of record**, snapshot-tested vs `rules list` |
| `docs/CLI-UX.md` | from plan; command reference + prompt catalog + errors + exit codes |
| `docs/SAFETY.md` | from plan; threat model, gates, never-touch, sudo protocol, checklist |
| `docs/CONFIG.md` | ARCHITECTURE §9 expanded with examples per rule + custom-rule recipe |
| `docs/JSON-SCHEMAS.md` | v1 schemas for scan/clean/top/history + versioning changelog |
| `docs/CI-CD.md` | CI workflows, security scans + `deny.toml` policy, release matrix/SBOM/provenance, AUR packaging, Flatpak deferral, release runbook |

## 6. Resume playbooks

**Playbook A: fresh session, continue building:**
1. Read `ROADMAP.md` → first unchecked milestone = current work.
2. Read `AGENTS.md` → commands (`cargo test`, etc.) + invariants.
3. Read the spec doc(s) relevant to that milestone's next task.
4. Implement → DoD (fmt/clippy/test/docs) → tick tasks → commit.

**Playbook B: mid-milestone interruption:** status lives in ROADMAP checkboxes;
unfinished subtask = first unchecked box under current `[~]` milestone. No other
state needed (no scratch files; PR/commit messages carry context).

**Playbook C: bug report / review:** reproduce → check `docs/CLI-UX.md` (is behavior
doc-conforming?) → `docs/SAFETY.md` (if deletion-related: treat as P0, apply PR
safety checklist) → fix with tests + doc updates.

**Playbook D: plan questions:** decisions are in §2 (D1–D14). If a question isn't
covered: add a new record here (D14+) with context/decision/rejection. Never
silently diverge from a documented decision.

**Playbook E: release:** follow `docs/CI-CD.md` §6.5. Tag only after the M7
acceptance gate; verify checksums + provenance before publishing; update AUR
`sweep-bin` first, then `sweep`.

## 7. Revision log (plan & decisions over time)

| Date | Rev | Change |
|---|---|---|
| 2026-10-19 | R1 | Initial plan: Rust, gated full mode, all soft hogs, name `sweep`, dev-artifacts kept caution, bonuses (top/history/timer/snapper) |
| 2026-10-19 | R2 | Extensification pass: all 7 docs expanded to full spec depth (threat model, 26-rule catalog, prompt copy catalog, ADRs, M0–M7 tasks+gates) |
| 2026-10-19 | R3 | M0 built: repo scaffolded at `~/Projects/sweep` (edition 2024, clap surface complete, `SWEEP_TEST_GUARD` sandbox harness, 14 tests green); docs copied/adapted; history-record schema restored into `docs/ARCHITECTURE.md` §11 |
| 2026-10-19 | R4 | M1 scan engine: jwalk 0.8 walker pruning subtrees via process_read_dir children.retain (verified from source); scan is lock-free; flock scope = mutating commands only (ARCHITECTURE §4, SAFETY §10); hand-rolled ANSI/emoji-aware table renderer (+unicode-width 0.2) instead of comfy-table presets; "newest" = newest counted entry (age-gated files excluded from every count) |
| 2026-10-19 | R5 | Repo hardening + distribution planned (`docs/CI-CD.md`): five-workflow CI/CD design; `cargo-deny` as the vulnerability gate; tag-gated release with SBOM + SLSA provenance; AUR `sweep-bin` then source `sweep`; Flatpak deferred (D11); no-emoji UI rule (D13). ROADMAP gains M1.5 (infra, before M2) and expanded M7 release tasks |
| 2026-10-20 | R8 | M2 task 1 (D13 executed): M1 emoji tier glyphs replaced with ASCII `+ ! #`: `Tier::glyph()` lost its `unicode` param, 4 call sites in `src/ui/table.rs` updated, 4 non-ASCII goldens re-recorded, and CLI-UX/RULES-CATALOG/SAFETY examples converted with their D13 banners removed. `unicode-width` retained (ANSI strip + wide user data); no behavior change. |
| 2026-10-20 | R7 | Dependabot's first cargo group PR (7 bumps) diagnosed: jwalk 0.9 is unmaintained and its API break (removed `read_children_error`) broke `src/scan/walker.rs`; pin held at 0.8.x and `jwalk >=0.9.0` added to Dependabot's ignore list. The same bump retired both advisory ignores: indicatif 0.18 dropped `number_prefix`, inquire 0.9 dropped `fxhash`; `deny.toml` `[advisories] ignore` is now empty. Other six bumps (comfy-table 8, dirs 7, indicatif 0.18, inquire 0.9, toml 1.1, clap_mangen 0.3) verified green across all checks including MSRV 1.88 |
| 2026-10-20 | R6 | M1.5 infra landed: five workflows (ci/security/scheduled/release/packaging), `deny.toml` (two dated `unmaintained` ignores: fxhash via inquire, number_prefix via indicatif), `SECURITY.md`, `CHANGELOG.md`, Dependabot; Cargo metadata `rust-version = "1.88"` (discovered by bisection: comfy-table let-chains floor, above the 1.85 edition-2024 floor); release profile `strip = "debuginfo"` for `cargo auditable` survival; machete ignores in `[package.metadata.cargo-machete]`; actionlint placed in `security.yml` (resolves the ROADMAP parenthetical). No `sweep` behavior change (only a `src/config.rs` doc-comment link fix for the `docs` gate) |
| 2026-10-20 | R9 | M2 task 2 slice 1: renderer-agnostic checklist core (`src/ui/checklist.rs`): `Selection`/`RowState`/`Row` over `&[RuleResult]`, tier-grouped ordering (report-only rows last, D9), intent methods (toggle/select-all-safe/none/invert/preset-cycle), live `Totals`, `selected()`; SAFETY §2 defaults transcribed, destructive locked outside `full` (unlocked-but-unchecked under `full`, and under `--i-know-what-im-doing` in every preset; the flag is sticky across `p` cycles), `--yes` inert, guarded/report-only never selectable, `!applied` wins over guards (`Disabled`), guarded destructive stays locked outside `full`. No CLI wiring (`Commands::Clean` still stubbed). D14: checklist is hand-rolled, not inquire. Tests: `tests/checklist_core.rs` (no tty). |
| 2026-10-20 | R10 | M2 task 2 slice 2: checklist renderer + event loop. New `src/ui/tty.rs` (hand-rolled raw mode on `libc`/termios: `tcgetattr`/`tcsetattr`, cursor hide/show, SIGINT/SIGWINCH via async-signal-safe `AtomicBool`, RAII `RawMode` guard restoring termios + cursor on Drop; ISIG kept on so Ctrl-C aborts with signal semantics) and `src/ui/render.rs` (pure frame renderer: tier groups from the slice-1 ordering, `>` cursor marker, state notes, live totals bar `  4 selected · 13.3 GiB  ·  2 guarded · 1 locked`, `?` help overlay; ASCII-only under `--no-unicode`, color via the existing `Theme`; every line clamped to width). `src/ui/checklist.rs` gains `run_interactive` + the pure `apply_key` key→intent seam + `Outcome`/`LoopStep`, with the loop erasing the previous frame by moving up `frame_height` rows (the `p` notice is one frame; abort = `Outcome::Aborted`). D15 records the no-new-dependency raw-mode decision. No CLI wiring (`Commands::Clean` still stubbed); slice 2 is library-only. Tests: `tests/checklist_render.rs` (12), `tests/checklist_keys.rs` (10), `tests/checklist_tty.rs` (6, scripted key seam, no live tty), `tests/checklist_pty.rs` (7, real pty driving `run_interactive`: multi-byte reads, lone-ESC abort, CRLF frames, a >`READ_BUF` single write fully drained). Every line is clamped to `render::MIN_WIDTH` or wider (the `truncate` clamp is ANSI-safe so a colored narrow frame cannot leak a partial SGR). Follow-up: the split-arrow pty test wrote its `ESC`/`[`/`B` bytes 180 ms apart, past `tty::READ_TIMEOUT_MS` (100 ms), so the lone `ESC` correctly resolved to abort; the harness now takes a `gap_ms` and the split test uses 15 ms (inside one poll window), keeping the lone-ESC-abort guarantee explicit. The unused `read_key` wrapper was deleted (`read_key_event` carries the idle/EOF distinction); the pacing-is-the-source's-job assumption is now documented at the loop's idle arm. Hardening: `read_key_event` reads fd 0 with `read(2)` (not `std::io::stdin()`, whose `BufReader` stranded bytes larger than the read buffer where `poll` could not see them, deadlocking a paste); fd 0 is `O_NONBLOCK` for the session and the read loop drains all queued bytes. A partially buffered CSI prefix is resolved to `Abort` only after a *full* idle poll window with no further byte (so a split `ESC`/`[`/`B` reassembles while a genuinely lone `ESC` still aborts). stdin EOF maps to `Input::Closed` and `read_key_event` *returns* on it (draining any queued keys first) rather than setting a flag and looping: a hung-up fd reads 0 forever and never `EAGAIN`, so the earlier loop spun hot. The EOF contract is tested deterministically at both levels (`read_available` maps a closed pipe to `Eof` and an empty `O_NONBLOCK` pipe to `WouldBlock`; the `run_interactive_with_source` seam yields `Source::Eof` and asserts the loop stops with `Outcome::Aborted`), rather than through a pty hangup, which delivers `SIGHUP` and kills the child before the EOF can be observed. |
| 2026-10-21 | R11 | M2 task 2 slice 2 revision round (reviewer B1 + C1, D16): dynamic text is now sanitized at the render boundary. New pure `ui::render::sanitize(text, unicode)` strips every control char (C0 `0x00-0x1f` incl. `ESC`, C1 `0x80-0x9f` incl. the 8-bit CSI `0x9b`, DEL `0x7f`) **always**, and every non-ASCII char under `--no-unicode`; chars are dropped, not replaced, so the width/truncate math can never be lengthened. `render_row` runs it over the title and every note before `Theme` paints; `render_footer` runs it over its content before painting. Before this, `--no-unicode` ASCII-ized chrome only, so a non-ASCII title or `guard` reason leaked (probe: `frame.is_ascii() == false`) and an `ESC[2J` in a title reached the terminal verbatim (`display_width`/`truncate` treat `CSI` as zero-width and copy it whole). Chrome behavior is unchanged. Tests: 4 new unit tests + 1 new integration test (non-ASCII title **and** non-ASCII guard reason under `--no-unicode` assert `frame.is_ascii()`; a crafted `ESC[2J`/`0x9b`/`DEL` title and reason assert no such byte survives under unicode, `--no-unicode` and color, and that every remaining `ESC` is the renderer's own SGR); the two tests that claimed the invariant while using ASCII fixtures now use the failing fixtures (`tests/checklist_render.rs` 13, `src/ui/render.rs` 17). A second bug surfaced while testing: sanitizing an already-painted line stripped the SGR introducer and left the CSI body `[2m` as visible text, so the sanitizer runs on content before paint. Docs synced: CLI-UX §3/§8/§11 + the flag table state the dynamic-text guarantee exactly, and this R row + D16 record it. Still library-only (`Commands::Clean` remains the stub); no new dependencies. |
| 2026-10-21 | R12 | B2 (slice-2 cleanup): the `--no-unicode` "pure ASCII" guarantee was documented as CLI-wide but is only true for the checklist renderer (`ui::render.rs`, D16). Docs re-scoped (CLI-UX §flag-table/§3/§8/§11, D13/D16) to the checklist and the `scan` gap named explicitly; global ASCII-clean `scan` output deferred to a new ROADMAP M3 task. No code change (pre-existing M1 `scan` chrome: `ui/mod.rs` `·`, `ui/table.rs` `—`). |

## 8. Open questions

- None blocking. All design calls resolved (D1–D10).
- Parking lot + risk register live in `ROADMAP.md` (bottom).
- Deferred product questions for post-0.1 dogfooding: checklist `/` filter,
  persistent size cache, apt-orphan parity.
- Flatpak packaging is **decided** (D11: not before 1.0), not an open question.
