# sweep — Roadmap (the *sequence*)

Milestones are strictly ordered; each has an **acceptance gate** that must pass
before the next begins. Status: `[ ] not started · [~] in progress · [x] done`.

**Global Definition of Done (applies to every milestone):**
- `cargo fmt --check` + `clippy -D warnings` + `cargo test` green in CI.
- Docs in sync (RULES-CATALOG / CLI-UX / SAFETY match code; golden snapshots updated deliberately).
- No new safety invariant weakened without a `CONTEXT.md` §7 revision entry.
- README/ROADMAP status updated in the same change.

---

## M0 — Scaffold + context docs
**Goal:** repo exists at `~/Projects/sweep`, builds, CI green, all context MDs in place.
**Depends on:** plan approval (done).

Tasks:
- [x] `cargo init --name sweep` at `~/Projects/sweep`; edition/MSRV set; MIT LICENSE
- [x] Adapt plan docs → repo: `AGENTS.md`, `CONTEXT.md` (from `sweep-CONTEXT.md`),
      `ROADMAP.md` (from this file), `README.md` (elevator pitch, pre-alpha status)
- [x] `docs/`: MASTER-PLAN, ARCHITECTURE, RULES-CATALOG, CLI-UX, SAFETY,
      CONFIG (from ARCHITECTURE §9), JSON-SCHEMAS (from ARCHITECTURE §12)
- [x] Tooling: `rustfmt.toml`, `clippy` config (Cargo `[lints]`), `.editorconfig`,
      `.gitignore`, GitHub Actions workflow (fmt/clippy/test on stable)
- [x] `main.rs` skeleton: clap root + `--version` + dispatch stubs printing "not yet"
- [x] Test harness scaffold: `sandbox()` + `SWEEP_TEST_GUARD` refusal logic
      (`tests/common/mod.rs`, `tests/env_guard.rs`, `tests/sandbox.rs`, `src/env.rs`)
- [x] Seed `CONTEXT.md` decisions D1–D10 + conventions (verbatim from plan)

**Acceptance gate:**
- [x] `cargo build && cargo test && cargo clippy -- -D warnings` pass locally & CI.
- [x] All 8+ docs exist at planned paths; `AGENTS.md` lists real build/test commands.
- [x] `sweep --version` → `sweep 0.1.0`.

## M1 — Scan engine + `sweep scan`
**Goal:** read-only, trustworthy, pretty measurement.
**Depends on:** M0.

Tasks:
- [x] `Ctx` build: config load (defaults-only path), `GlobalFlags`, `run_id`, lock acquire
- [x] `system/`: `SystemCtx` minimal (distro id + fstype via statvfs/mounts) — full detect in M5
- [x] `scan/walker.rs`: jwalk+rayon, symlink-safe, mount-bounded, protected-list
      hook, mtime/newest tracking, samples(20), progress mpsc, cancellation token
- [x] `rules/mod.rs`: `Rule` trait, `Tier`, `Detect`, registry; first 4 rules:
      `tmp` (full protected list + age gate), `user-cache` (whitelist + hot skip),
      `thumbnails`, `trash`
- [x] `scan/mod.rs`: run registry → `Vec<RuleResult>`, overlap subtraction (`net_of`)
      for user-cache vs thumbnails
- [x] `ui/`: header box, `format.rs` (bytes/age), `table.rs` (wide/narrow/ascii),
      scan progress spinner
- [x] `sweep scan` command + `--json` (schema v1, `kind:"scan"`)
- [x] `--no-color`/`NO_COLOR`/`--no-unicode`/width handling
- [x] Tests: walker fixtures (symlink, protected, mount stub, hot-skip), scan
      mutation-free assertion, golden table snapshots (wide/narrow/ascii/no-color),
      JSON required-keys test

**Acceptance gate:**
- [x] Real Arch machine: `sweep scan` <5 s for these roots; totals match `du` within tolerance.
      Verified on this host 2026-10-19 (EndeavourOS / Arch family): `time cargo run -- scan` =
      0.300 s wall (well under 5 s); `du -s --apparent-size` over `/tmp`, `/var/tmp`,
      `~/.cache`, `~/.local/share/Trash` = 20,970,427,228 B vs `scan --json --keep-days 0`
      `totals.reclaimable_bytes` 21,176,387,183 B → **+0.98%**, inside ±5%. Approximate by
      nature: `du` hits EACCES on root-owned `systemd-private-*` dirs (undercounts
      `/var/tmp`), and scan counts directories as 0 bytes.
- [x] Golden snapshots pass; sandbox manifest proves zero mutations.
- [x] All 4 rules have exact byte/count tests.

## M2 — Interactive clean + full safe tier
**Goal:** the daily-driver loop works end-to-end for all 11 safe rules.
**Depends on:** M1.

Tasks:
- [ ] `ui/checklist.rs`: inquire MultiSelect + custom renderer, keybindings
      (space/a/n/i/p/enter/esc/?), live totals, row states (ready/guarded/locked/disabled)
- [ ] Presets: safe/regular/full selection logic (full still gates destructive at M4 —
      for now full == safe+caution selection preview with destructive rows shown "arrives M4")
- [ ] `ui/confirm.rs`: P1 summary (default cancel), abort copy, exit 2 paths
- [ ] `exec/`: `del.rs` primitives (bottom-up, no-follow, prefix re-check, hot-skip),
      plan execution sequential, continue-on-error, per-rule Report
- [ ] Dry-run printing (samples + argv), `--yes` (safe/caution only)
- [ ] `ui/progress.rs` per-rule bars + `ui/summary.rs` card
- [ ] Remaining safe rules: `journal-user`, `browser-cache`(+guard), `editor-cache`(+guard),
      `tool-caches` (all 12 tool paths), `app-caches`, `log-rotate`, `xdg-state-junk`
      (coredump sub-row default-off)
- [ ] `system/proc.rs`: comm guards + injectable /proc for tests; open-fd scan for tmp
- [ ] State: action log writer + `history.jsonl` append + pruning
- [ ] Exit codes 0/1/2/3 wired everywhere; `--trash` (exec/trash.rs) implemented
- [ ] Tests per safe rule (detect/scan/clean/age/guard) + gate tests (P1 abort,
      --yes boundaries) + checklist keybinding logic tests (renderer-agnostic core)

**Acceptance gate:**
- Full interactive run on real machine frees space; skipped-reasons all visible.
- Guard tests prove firefox-running → refused at execute (with P6 override path).
- `--dry-run` byte-identical sandbox assertion holds for a full clean.

## M3 — Caution rule library
**Goal:** all 9 selectable caution rules implemented with list-first confirms.
**Depends on:** M2.

Tasks:
- [ ] `rules/pkg.rs`: `pacman-cache` (paccache argv, keep_versions, unprivileged scan),
      `journal-system`, `aur-builds` (src/pkg/logs, git kept, in-progress guard),
      `orphans` (list + batched argv + exclude), `flatpak-unused` (dry-run parse + split pass)
- [ ] `rules/containers.rs`: `docker` (df JSON parse, 4 sub-rows, volume typed gate P5),
      `podman` (image/buildcache/stopped sub-rows, rootless/rootful pass split)
- [ ] `rules/dev.rs`: `dev-artifacts` — single home walk, recursive-newest tracking,
      progress, top-20 preview, self-exclusion, exclude_paths
- [ ] `rules/steam.rs`: `steam-shader`
- [ ] `rules/snapshots.rs`: `snapshots-report` (report-only; never enters Plan)
- [ ] `ui/confirm.rs`: P2 per-rule confirms + list pagination (`confirm_batch`)
- [ ] `rules list` command (states: ready/guarded/sudo/disabled/locked)
- [ ] Config: `[rules.*]` sections validated (known keys), `[presets]` honored
- [ ] Tests per rule per catalog requirement + sub-row defaults + argv snapshots

**Acceptance gate:**
- `sweep rules list` == `docs/RULES-CATALOG.md` snapshot.
- dev-artifacts: fixture proves 30d boundary, `.git` boundary, self-exclusion, excludes.
- docker volume gate proven to require typed word (unit test on gate logic).

## M4 — Destructive tier + gating + history
**Goal:** full purge exists, is dramatic, and is safe.
**Depends on:** M3.

Tasks:
- [ ] Rules: `downloads` (age rescue, xdg-user-dirs resolution, symlink refusal),
      `cache-deep` (whitelist ignored, running-guards hold), `tmp-full`,
      `pkgcache-zero`, `purge-logs` (truncate-vs-unlink)
- [ ] Unlock logic: destructive rows selectable only under `full`; P3 typed PURGE per
      category; `--yes` refusal message (exit 2); `--i-know-what-im-doing` bypass + banner
- [ ] Gate ordering: P1 → P3s → execution; abort = all-or-nothing at gate stage
- [ ] `sweep history` viewer (+`--last`, `--json`, `--logs`)
- [ ] Log pruning (30), history pruning (`history_keep_runs`)
- [ ] Gate tests: wrong text → 2, survivors under keep-days exact, `--json` refuses
      interactive, all-or-nothing abort, bypass flag behavior locked by test

**Acceptance gate:**
- Manual script: full-preset dry-run shows everything; typed-PURGE flow works;
  mistype aborts with zero mutations (sandbox manifest).
- History rows match runs; pruning boundaries tested.

## M5 — System pass + environment + doctor
**Goal:** sudo is single, honest, tamper-checked; distros handled; diagnostics exist.
**Depends on:** M4.

Tasks:
- [ ] `exec/sudo.rs`: plan write (0600, digest), P4 prompt, re-exec argv, child
      validation (schema/digest/uid/detect/gates/prefixes), JSON result merge,
      pkexec fallback, decline path
- [ ] Wire split-pass items: pacman-cache, journal-system, orphans, pkgcache-zero,
      purge-logs, flatpak system-half, podman rootful, docker if root
- [ ] `/proc` hardening final: open-fd skip + TOCTOU re-check at execution
- [ ] `system/distro.rs` full: family matrices; family-specific sibling rules
      (`apt-cache-clean`, `dnf-cache-clean`, `zypper-cache-clean`) + missing-rule reasons
- [ ] `sweep doctor` (all lines from CLI-UX §2.5, degrades not vanishes)
- [ ] `system/snapper.rs` sizing + report integration
- [ ] Tests: tamper/decline/schema-failure child refusals (run child without sudo),
      distro fixtures (3 os-release samples), doctor golden snapshot

**Acceptance gate:**
- Mixed selection = exactly one elevation (verified by counting `sudo` invocations in test double).
- Ubuntu + Fedora VM/manual pass: correct rule subset + doctor explanations.

## M6 — Extensibility + scriptability + `top`
**Goal:** power users extend; scripts integrate.
**Depends on:** M5.

Tasks:
- [ ] `rules/custom.rs`: `[[custom]]` TOML rules (validation per ARCHITECTURE §10/§T7)
- [ ] `sweep config init|path|edit` (+`--force`), annotated template, CONFIG.md finalized
- [ ] Full config surface: `[scan].exclude_paths`, `[presets]`, per-rule keys all honored;
      precedence tests (flag > env > config > default)
- [ ] `sweep top` (dirs/files/depth/min-size/limit/path, mount-bounded, `--json`)
- [ ] `--json` completeness: clean/top/history + schema docs (`docs/JSON-SCHEMAS.md`)
- [ ] Unknown-rule suggestions (edit distance), error catalog messages from CLI-UX §5
- [ ] Perf pass: walker benchmarks (`criterion` optional), scan memoization within run
- [ ] Tests: custom rule happy/sad paths, precedence matrix, top output golden

**Acceptance gate:**
- A TOML-only rule appears in checklist and cleans correctly (integration test).
- Every error string in CLI-UX §5 exists verbatim (snapshot).

## M7 — Ship (v0.1.0)
**Goal:** installable on a fresh Arch box, zero-config, documented.
**Depends on:** M6.

Tasks:
- [ ] Full test matrix pass: unit/integration/golden/gate; coverage review of 26 rules
- [ ] `sweep completion bash|zsh|fish` generation + packaged; man page (clap_mangen)
- [ ] `dist/sweep.service` + `sweep.timer` (report-only default: `sweep scan --json`
      → journal; auto-clean documented as explicit override), timer **disabled** in pkg
- [ ] `PKGBUILD`: `sweep-bin` (prebuilt, sha256) + `sweep` (source); completions+man+units
      in package() (split docs package optional)
- [ ] Release CI: tag → build (x86_64 gnu + musl static) + checksums + notes
- [ ] README final: install (AUR/manual), quickstart, presets, safety section
      (link SAFETY.md), rule table (generated), FAQ (Downloads? timer? sudo? trash?)
- [ ] Manual test matrix: Arch (btrfs+snapper), Arch (ext4), Ubuntu LTS, Fedora —
      checklist recorded in release notes
- [ ] Tag `v0.1.0`

**Acceptance gate (release):**
- Fresh Arch VM: AUR install → `sweep doctor` clean → `sweep clean` end-to-end
  without config → frees space → history entry exists → `man sweep` renders →
  completions tab-work → timer enable → journal report appears.
- All golden snapshots match docs; exit codes exercised in CI.

---

## Dependency graph
```
M0 → M1 → M2 → M3 → M4 → M5 → M6 → M7
        (safe rules)  (caution) (destructive) (sudo/distro) (ext/top) (ship)
```
No parallelization planned (single dev, sequential keeps safety invariants coherent).

## Risk register

| Risk | Impact | Mitigation |
|---|---|---|
| inquire custom renderer limitations | checklist UX缩水 | spike in M2 first task; fallback = simple own prompt loop on raw tty (termios) |
| sudo re-exec complexity | M5 slip | child runs fine unprivileged in tests; pkexec fallback; decline path always works |
| dev-artifacts walk too slow on huge homes | UX regression | progress + exclude_paths + budget 30 s; can ship behind caution with scan note |
| docker/podman JSON drift | wrong previews | list-first always shows raw `system df` table as fallback |
| docs drift | trust loss | CI snapshot `rules list` vs docs table |
| jwalk/mount-boundary edge cases | wrong deletions | walker tests + execution-time prefix re-check (defense in depth) |
| AUR package maintenance | adoption | `-bin` first, source pkg optional |

## Parking lot (explicitly deferred past 0.1)

Fullscreen ncdu-style browser · `/` filter in checklist · plugin binaries/WASM ·
i18n · Nix flake / Homebrew · `sweep watch` · apt `autoremove`-equivalent orphan
rule · persistent size cache between runs · `--json` streaming events · shell
prompt segment helper.
