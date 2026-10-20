# sweep — Master Plan (the *what* and *why*)

A pretty, advanced, safety-first **storage cleaner CLI for Linux** — built in Rust,
deeply integrated with Arch-based distros, functional on every mainstream distro.

**One-liner:** scan your system for reclaimable space → toggle categories in an
interactive checklist → clean them. Regular cleanup by default; a gated *full purge*
destroys everything, including the entirety of `~/Downloads`.

---

## 1. Positioning

| Tool | What it does | How `sweep` differs |
|---|---|---|
| bleachbit | GUI cleaner, GTK, flat rules | `sweep` is CLI-first, tiered/gated, rule-extensible, Arch-native (pacman/AUR/orphans) |
| stacer | GUI system optimizer | no GUI at all; scriptable, JSON output, exit codes, systemd-timer friendly |
| ncdu / dust | *inspect* disk usage | inspection only — `sweep top` borrows their idea but the rest is action |
| `paccache`, `docker system df`… | single-purpose primitives | `sweep` orchestrates all of them behind one checklist with consistent safety |
| `rm -rf` on caches | works until it doesn't | guards, age gates, running-process checks, dry-run, logs, history |

**Design north star:** *a cleaner your paranoid friend would approve of.*

## 2. Personas & primary use cases

1. **Arch tinkerer (primary):** after a month of `pacman -S`/AUR builds, 60+ GiB gone
   to pkg cache, AUR src dirs, `cargo`/`npm` caches, journals, orphans. Wants one
   command with a checklist, not six wiki pages.
2. **Low-disk alert (laptop):** disk 95% full, needs space *now* but refuses to lose
   Downloads or project build dirs blindly. Runs `sweep clean`, reads sizes, selects,
   confirms.
3. **Scripter/automation user:** cron/systemd timer, `--json`, stable exit codes,
   `--dry-run` reports to journal; never interactive.
4. **Multi-distro household:** same tool on Arch + Ubuntu box; rules self-select.

## 3. Product goals (with success criteria)

| # | Goal | Success criterion |
|---|---|---|
| G1 | Reclaim storage safely | zero data-loss incidents attributable to defaults; every destructive act double-gated |
| G2 | Pretty terminal UX | scan → select → clean flow reads in <10 s; tiers legible by color+icon+text (never color alone) |
| G3 | Arch-first, Linux-wide | full catalog on Arch; correct subsets elsewhere; `doctor` explains any absence |
| G4 | Scriptable | every interactive flow has a non-interactive equivalent; JSON schema versioned; exit codes 0/1/2/3 documented |
| G5 | Extensible | user adds a working cleanup rule via TOML only, no recompile |
| G6 | Fast | typical cache scan <5 s (SSD); progress feedback always visible; no silent stalls >1 s |

## 4. Core concepts

### 4.1 Rule
Everything cleanable is a `Rule` — an id, title, tier, glyph, and detect/scan/clean
functions plus sudo/guard metadata. The registry is filtered by `detect()` against
the environment, which is how "works everywhere, deepest on Arch" falls out of the
design instead of being special-cased. Full specs: `RULES-CATALOG.md`.

### 4.2 Tiers of danger
- `+` **safe** — default-checked in `regular` preset. One shared end-of-selection
  confirmation (default = cancel).
- `!` **caution** — visible, default-*unchecked*. Per-rule confirmation showing
  count/samples first (orphans/docker/flatpak show their actual item lists).
- `#` **destructive** — only selectable under the `full` preset; each requires a
  **typed `PURGE`** after a summary screen. `--yes` does not bypass (D7).
- `~` **report-only** — snapper snapshots: measured and reported, never touched (D9).

All TUI glyphs are ASCII (D13); no emoji anywhere in rendered
output.

### 4.3 Presets
| Preset | Checked by default | Purpose |
|---|---|---|
| `safe` | all `+` | nervous first run / automated timer runs |
| `regular` (default) | all `+` + user's `[presets.regular].include` | the weekly clean |
| `full` | all `+` + `!` + `#` unlocked | deep clean before reinstall / sale / disk surgery |

### 4.4 Two-pass execution (D8)
User-level rules run first, unprivileged, to completion; all selected system rules
serialize into one plan file and execute in a **single** sudo re-exec. Declining sudo
never corrupts the user-level result.

## 5. Feature inventory (MoSCoW)

### Must (v0.1)
| ID | Feature | Milestone |
|---|---|---|
| F01 | Parallel size scan + pretty `sweep scan` table | M1 |
| F02 | Interactive checklist (tiers, live total, keybindings) | M2 |
| F03 | 11 safe rules incl. running-app guards + /tmp protections | M2 |
| F04 | 10 caution rules incl. pacman/AUR/orphans/docker/flatpak/dev-artifacts | M3 |
| F05 | 5 destructive rules + typed `PURGE` two-step gate | M4 |
| F06 | `--dry-run`, `--yes`, `--only/--exclude`, `--keep-days`, `--trash` | M2–M4 |
| F07 | Batched sudo system pass | M5 |
| F08 | Distro detection + non-Arch rule matrix | M5 |
| F09 | Action log + `history.jsonl` + `sweep history` | M4 |
| F10 | Custom TOML rules | M6 |
| F11 | `--json` on scan/clean/top/history (versioned schema) | M6 |
| F12 | `sweep doctor`, `sweep rules list`, `sweep config init` | M5–M6 |

### Should (v0.1 if cheap, else 0.2)
| ID | Feature | Milestone |
|---|---|---|
| F13 | `sweep top` (dirs/files, depth, min-size, path-scoped) | M6 |
| F14 | systemd service+timer units (report-only default, disabled) | M7 |
| F15 | Snapper/btrfs space report | M5 |
| F16 | Shell completions + man page | M7 |
| F17 | AUR packages (`sweep-bin`, `sweep`) + verifiable release artifacts (gnu + static musl, SBOM, provenance) | M7 |

### Could (explicitly parked — see ROADMAP bottom)
Fullscreen ncdu-style browser, plugin binaries/WASM, i18n, Nix/Homebrew, `watch` mode.

## 6. User stories & acceptance scenarios

**US1 — weekly Arch clean.** *As an Arch user I run `sweep clean`;* I see 13.3 GiB
selected in safe rules, pacman cache unchecked; I check it (keep 2), leave orphans
alone, press enter, confirm → summary shows freed + skipped ("firefox running")
and a history entry appears.

**US2 — pre-reinstall deep purge.** *I run `sweep clean --preset full`;* destructive
rows unlock red; I toggle Downloads, see "1,204 files · newest 2 hours ago"; after
selection a summary screen demands I type `PURGE`; I mistype → everything aborts,
exit 2, nothing deleted.

**US3 — fresh download rescue.** *Same run with `--keep-days 7`:* the Downloads
preview says 37 files newer than 7 days will be preserved; after the run those 37
remain on disk.

**US4 — scripted safe clean.** *`sweep clean --preset safe --yes --json > report.json`*
in a systemd timer: exit 0, `run.freed_bytes` > 0, no prompts ever (verified in CI
by a pty-less test).

**US5 — cautious dev.** *I never want my projects touched.* I check `rules list`,
see `dev-artifacts` is caution/off by default; even when enabled it lists
untouched->30d candidates with ages and self-excludes the sweep repo; I keep it off
via `[presets.regular].exclude`.

**US6 — no sudo.** *I decline the password prompt:* user rules stay cleaned, system
rules report `skipped (sudo declined)`, output tells me exactly which.

**US7 — Ubuntu laptop.** *I install it on Ubuntu:* `doctor` shows family=debian;
pacman/orphans rules absent, `apt clean` rule present; scan works; I never see
dead rules.

**US8 — automation report.** *Timer runs report-only scan weekly:* journal gets the
JSON summary; disk untouched; I review `sweep history` monthly and trigger manual
cleans.

## 7. Non-goals (with rationale)

| Non-goal | Why |
|---|---|
| GUI | scope; CLI is the product (G4) |
| Deleting Documents/Pictures/Videos/Music/Projects | user data, never rules (SAFETY never-touch) |
| Network operations during scan/clean | determinism, speed, offline-first; no telemetry ever |
| Deleting btrfs snapshots | D9 — snapshots are user recovery data; report only |
| Managing swap/zram/TRIM | different tool category; docs mention only |
| Auto-cleaning without explicit selection (except opt-in timer safe-preset) | would violate the core promise |
| Package *upgrading*, kernel trimming, service management | out of cleaner scope |
| Undo of hard deletes | impossible honestly; mitigated by gates + dry-run + logs |

## 8. Quality attributes

1. **Safety** > completeness > UX polish > speed. When in conflict, safety wins.
2. **Legibility:** every dangerous row carries a text label (`destructive`, `locked`, `sudo`) plus color, never color alone; `NO_COLOR`/`--no-unicode` keep it usable.
3. **Predictability:** flags > env > config > defaults precedence; identical inputs → identical plan.
4. **Honesty:** sizes are "reclaimable now", skipped items always explained, freed counts come from actual deletion results, not estimates.
5. **Composability:** JSON everywhere, exit codes stable, no interactive prompt when `--yes` or non-tty (fail with guidance instead).

## 9. Success metrics (v0.1)

- Correctness: 0 known paths to silent deletion outside gates; all 26 rules have a sandbox test.
- UX: cold start `sweep scan` <5 s typical SSD; total output of `clean` flow fits 2 screens + prompts.
- Coverage: catalog matches `rules list` output 100% (CI-verified via snapshot).
- Supply chain: `cargo deny` green with no unexplained ignores; every release
  ships checksums, an SBOM, and a provenance attestation that verifies.
- Portability: passes on Arch, Ubuntu LTS, Fedora current (manual matrix in M7).

## 10. Release strategy

| Version | Content | Gate |
|---|---|---|
| `0.1.0` | M0–M7 complete | acceptance per milestone + fresh-Arch walkthrough (ROADMAP M7) |
| `0.2.0` | parked Could-items + polish from dogfooding | test matrix green |
| `1.0.0` | schema stability承诺: JSON schema v1 frozen, rule ids frozen, config keys frozen | deprecation policy published |

**Compatibility promise from 1.0:** rule ids, config keys, JSON fields only *add*;
breaking changes bump schema `version` and are listed in docs/JSON-SCHEMAS.md.

**Distribution (D11):** AUR is the primary channel — `sweep-bin` (static musl
release asset) first, then a source `sweep` package. GitHub Releases carry gnu
and musl binaries with `.sha256`, a CycloneDX SBOM, and SLSA build provenance.
Flatpak is **not** packaged before 1.0: the sandbox has a private `/tmp`, no
sudo, and no pacman/systemd, so a home-only build would be a lesser product
under the same name. crates.io is deferred. Full design, security checks, and
the release runbook live in `docs/CI-CD.md`.

## 11. Phases overview

| Phase | Name | Outcome |
|---|---|---|
| M0 | Scaffold + context docs | repo at `~/Projects/sweep`, all MDs, CI green |
| M1 | Scan engine | `sweep scan` parallel walk + pretty table + JSON |
| **M1.5** | Repo hardening | five-workflow CI/CD, security scans, release scaffold, packaging skeleton |
| M2 | Interactive clean | checklist, presets, 11 safe rules, progress, summary |
| M3 | Caution library | 10 caution rules, guards, list-first confirms, dry-run |
| M4 | Destructive tier | PURGE gate, Downloads, deep wipe, trash, logs, history |
| M5 | System pass | batched sudo, doctor, distro matrix, proc hardening, snapper |
| M6 | Extensibility | custom TOML rules, config init, `top`, `--json` everywhere |
| M7 | Ship | tests, completions, man, systemd units, PKGBUILD, releases |

Detailed tasks + acceptance gates: `ROADMAP.md`.

## 12. Glossary

| Term | Meaning |
|---|---|
| **rule** | one cleanable target (code/config/JSON name; UI prose says "category") |
| **tier** | danger class: safe / caution / destructive / report-only |
| **preset** | named selection set: safe / regular / full |
| **plan** | the final set of selected rules + gate acknowledgements, serialized for execution (and for sudo re-exec) |
| **system pass** | the single batched sudo execution of all `needs_sudo` rules |
| **guard** | condition that hides or blocks a rule with a shown reason (running app, daemon down) |
| **age gate** | keep-files-newer-than-N-days filter |
| **scan** | read-only measurement pass; never modifies anything |
| **action log** | per-run file under `~/.local/state/sweep/logs/` |
| **history** | append-only `history.jsonl` of past runs |
