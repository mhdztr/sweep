# sweep — Safety Model (the *guarantees*)

Mantra: **no deletion happens without the user having seen the size, the count,
and the tier of what's about to go — and every destructive path is abortable.**

This document is normative: if code contradicts it, code is wrong.

---

## 1. Threat model — what we defend against

| # | Scenario | Defense |
|---|---|---|
| T1 | **User error** — fat-finger selects Downloads | tier defaults (destructive off), summary P1 with default=cancel, typed PURGE per category, newest-age display |
| T2 | **Script misuse** — `--yes` in a cron job wiping everything | `--yes` never bypasses destructive (D7); only explicit `--i-know-what-im-doing`; timer unit ships report-only |
| T3 | **Symlink race** in shared `/tmp` (entry swapped between scan & delete) | never follow symlinks anywhere; fd-relative unlink where feasible; protected/`S_ISSOCK` re-check at execution time |
| T4 | **Wrong scope** — walker descends into a mount (external HDD, bind mount, NFS) with different content | mount-bounded walks (device id check at every subtree) |
| T5 | **Stale/tampered sudo plan** — a crafted plan file escalates deletions | plan JSON carries schema version + HMAC-ish checksum (sha256 of canonical plan + uid + run_id written by parent; child re-verifies), child re-runs `detect()` per rule, child re-validates gate acks; plan file in `state/tmp` with 0600, deleted after |
| T6 | **Silent partial failure** — half-cleaning reported as success | continue-on-error + exit 3 + summary always lists errors/skips |
| T7 | **Config tricks** — user custom rule points at `/etc` or `~` root | custom rule validation: expanded path must be ≠ home/root, not under `/etc,/usr,/boot,/var/lib`; destructive custom still needs PURGE gate |
| T8 | **Two sweeps at once** corrupting state or racing deletes | `flock` single-instance lock (`state/sweep.lock`); second instance exits 1 with pid info |
| T9 | **Low disk / read-only state** mid-run | log/history write failures degrade to warnings (never abort clean); trash mode pre-checks free space |
| T10 | **Running-app corruption** (browser/editor/docker in use) | process guards at detect + execution; docker list-first; volume sub-gate typed word |
| T11 | **Destructive-with-`--yes` confusion in CI** | tested behavior: exits 2 with actionable message; `--json` requires non-interactive flow |

Non-goals of the threat model: multi-user adversarial hosts (sweep assumes the
invoking user owns their data), compromised root (sudo already implies trust),
kernel/filesystem bugs.

---

## 2. Gating matrix (normative)

| Tier | Visible when | Default selection | Confirmations | Bypass |
|---|---|---|---|---|
| 🟩 safe | always (if detect=ready) | **checked** in `safe`+`regular` | **P1** shared summary (default N) | `--yes` |
| 🟨 caution | always | **unchecked** | **P1** + **P2** per rule (list-first where data-bearing) | `--yes` |
| 🟥 destructive | only under `full` preset | **locked → unlocked-but-unchecked** under full | **P1** + **P3 typed PURGE per category** | only `--i-know-what-im-doing` |
| 📋 report-only | when applicable | n/a | never selectable | n/a |
| guarded rows | always | unchecked | P6 if user overrides | n/a |

**Phase invariants:**
1. `scan` phase performs zero mutations (CI-verified: snapshot test asserts no
   filesystem deltas after `sweep scan` in sandbox).
2. `--dry-run` stops before `Execute`; prints samples (≤20/rule) + argv list.
3. Enter/`y`/`Y` semantics: P1 and P2 accept only literal `y`/`Y` (Enter = no).
   P3 accepts only exact `PURGE`.
4. Any prompt: `Esc`/`Ctrl-C` → abort message `— nothing was deleted` → exit 2.
5. Execution re-validates each target against the allowed-prefix list
   (independent second check, not trusting the plan alone).

## 3. Never-touch list (hardcoded; not configurable away)

- **Home & user data:** `$HOME` itself; `~/Documents`, `~/Pictures`, `~/Videos`,
  `~/Music`, `~/Projects`, `~/Desktop` — **no rule may root here** (only
  `~/Downloads` destructively, `~/.cache*` and `~/.config/<app>/Cache*` scoped
  otherwise). Custom rules targeting these roots are rejected at config validation.
- **System:** `/etc`, `/usr`, `/boot`, `/var/lib/pacman`, `/var/lib/container*`
  (docker *volumes* data), service units, package databases.
- **Inside `/tmp`:** `systemd-private-*`, `snap.mount*`, `X11-unix`, `.XIM-unix`,
  `.ICE-unix`, `.X*-lock`, sockets/FIFOs/device nodes, any path with an open fd.
- **Symlinks:** never followed (scan counts size 0 & doesn't descend; delete
  unlinks the link itself only when the link's parent rule applies).
- **The sweep repo itself** (`Ctx::exe_dir` walk-up to `.git`) — excluded from
  `dev-artifacts` and custom `$HOME`-scoped rules.
- **`.git` directories** — `dev-artifacts` never enters them (project history).
- **Volumes (docker/podman)** — never in default selection; typed-word gate (P5).
- **btrfs snapshots** — report-only, D9, never deleted.
- **Live journald/log inodes** under `purge-logs` — truncated, not unlinked.

## 4. Age gates (normative defaults)

| Rule | Gate | Default | Override |
|---|---|---|---|
| `tmp` | keep newer than | 1 day | `--keep-days`, `[general].keep_days` |
| `tmp-full` | none (destructive) | – | – |
| `downloads` | rescue newer than | 0 (off) | `--keep-days`, `[rules.downloads].keep_days` |
| `dev-artifacts` | untouched older than | 30 days | `[rules.dev-artifacts].min_age_days` |
| `user-cache`/`browser-cache`/`editor-cache`/`tool-caches`/`app-caches` | hot-write skip | 5 min | – (not configurable v0.1) |
| `aur-builds` | keep newer than | 1 day | config |
| `xdg-state-junk` coredumps | keep newer than | 1 day | config |
| `log-rotate` | rotated + ≥1 day | 1 day | config |
| custom rules | `older_than_days` | **required** | per-rule |

Age basis = **mtime** (newest descendant for directories). Documented limitation:
`noatime` mounts make atime useless; "stale" means "not written to".

## 5. Running-process guards

| Guard scope | Match method | Held rules |
|---|---|---|
| Browsers | `/proc/[pid]/{comm,cmdline}` basename ∈ {firefox, chromium, chrome, brave, edge} | `browser-cache` |
| Editors/apps | ∈ {code, cursor, codium, slack, discord, element, obsidian, signal, telegram-desktop} | `editor-cache`, `app-caches` (matched paths) |
| Steam | `steam`, `steamwebhelper` | `steam-shader` |
| Build in progress | `makepkg` running **and** cwd under a given AUR repo → per-repo skip | `aur-builds` |
| Open files in `/tmp` | readlink `/proc/*/fd/*` prefix match | `tmp`, `tmp-full` |
| Docker daemon | `docker info` ≤2 s timeout | `docker` (Guarded) |

Guarded row = `Detect::Guarded{reason}`: pre-unchecked + shown. User override →
P6 warning confirm. Execution re-checks (TOCTOU minimized: check right before delete).

## 6. Failure & interruption matrix

| Situation | Behavior | Exit |
|---|---|---|
| one rule's delete fails (EACCES/EBUSY) | collect error, continue other rules, summary + log detail | 3 |
| scan of one rule unreadable | rule shows `? error (reason)`, excluded from totals with note | 0 |
| Esc / Ctrl-C at any prompt | `aborted — nothing was deleted` | 2 |
| Ctrl-C during execution | finish current rule, mark rest `skipped (aborted)`, print partial summary | 2 |
| sudo declined | system items `skipped (sudo declined)`; user pass result stands | 0 (or 3 if a selected rule errored) |
| sudo pass command fails | errors reported per item | 3 |
| config invalid | refuse before any scan | 1 |
| lock held | refuse with owner pid | 1 |
| history/log write fails | warning, run result still printed | as applicable |
| wrong PURGE text | abort that category; per P3: **entire run aborts** (all-or-nothing at gate stage) | 2 |

## 7. Sudo system-pass protocol (D8, step by step)

```
parent (user pass complete):
  1. plan_selected_sudo = [items…] with per-item {id, gate_ack?, config_hash}
  2. write plan JSON to state/tmp/system-pass-<run_id>.json (mode 0600):
       { schema:1, run_id, uid, created_at, cfg_hash,
         items:[{id, params…, purge_ack: bool}], digest: sha256(canonical ∪ secret-less fields ∪ uid) }
  3. prompt P4 (once) → if declined: mark skipped, done
  4. exec `sudo --preserve-env=SWEEP_CONFIG? no — env NOT preserved;
     argv: sweep --internal-system-pass <path>`   (argv only, no shell)
child (internal pass):
  5. re-parse, rebuild Ctx (fresh config read), verify uid == plan.uid,
     verify digest, verify schema, ensure real $HOME (or SWEEP_TEST_GUARD harness)
  6. for each item: re-run detect() → must be Ready; re-verify gate acks exist
     for destructive ids; re-validate allowed prefixes (same check as user pass)
  7. execute sequentially; emit single JSON result on stdout:
       { ok:[{id,freed,removed}], skipped:[…], errors:[{id,message}] }
  8. parent merges child JSON into RunReport (never trusts free text/other streams)
```
Fail-safe: any step 5/6 failure → child refuses, parent reports
`system pass refused: <reason>` (exit 3), user pass results unaffected.

## 8. Audit log & history (what gets recorded)

**Action log** `state/logs/<run_id>.log` (text, human-first):
```
[2026-10-19T14:03:11Z] run start id=01J… preset=regular dry_run=false trash=false keep_days=1
[2026-10-19T14:03:15Z] gate ok: P1=confirm P2=[pacman-cache] P3=[downloads:purge]
[2026-10-19T14:03:15Z] rule tmp: deleted 4100 entries, 1288490188 B (kept 37 newer-than-gate, 2 open-fd, 4 protected)
[2026-10-19T14:03:16Z] cmd  argv=["paccache","-rk2"] → rc=0
[2026-10-19T14:03:19Z] rule docker: skipped (daemon not running)
[2026-10-19T14:03:19Z] run end: freed=14280000000 B errors=0 exit=0
```
Rules: never log file *contents*; sample paths ≤10/rule in `-v` only;
log pruned to last 30 files.

**History record** (`history.jsonl`, see ARCHITECTURE §7): append-only; one JSON
object per run: ts, preset, mode, items[{id,freed,removed,skipped[]}], freed_total,
duration_ms, dry_run. Pruned to `history_keep_runs` (100).

## 9. Undo / trash semantics

- `--trash` implements XDG: move to `Trash/files/<timestamped-name>/` + write
  `Trash/info/<name>.trashinfo` (deletionDate, path). Refuses if free space <
  5% + 512 MiB (would double-use space). UI states "space is NOT freed until you
  empty trash".
- Hard deletes have **no undo** — honesty copy in P1: "deletions are permanent".
- Mitigations: dry-run first, gates, logs with samples, age gates, `--keep-days`.

## 10. Concurrency & environment hardening

- Single-instance `flock` for entire run (scan through history write).
- Plans/logs written via temp + rename; state dir 0700.
- External commands: argv arrays only (never `sh -c`), `Command::env_clear`-ish
  minimal env for sudo pass, PATH resolved once at startup and recorded.
- No network syscalls in any path (policy; enforced by code review checklist).
- Scan after mount changes: device ids re-checked at execution (T4 defense in depth).

## 11. PR safety checklist (must pass review)

- [ ] New/changed rule has: tier justification, protected/age/guard behavior, tests
      for detect/scan/clean/gate (per RULES-CATALOG §test coverage requirement)
- [ ] No new deletion path outside `exec/del.rs` primitives
- [ ] No `$HOME` string literal — uses `Ctx::home`
- [ ] No `sh -c`, no new network, no `unwrap()` outside tests
- [ ] Destructive changes ship WITH their gate in the same change (D-safety rule)
- [ ] Docs updated: `docs/RULES-CATALOG.md` + `docs/CLI-UX.md` copy catalog
- [ ] Golden snapshots re-recorded deliberately (not blindly accepted)
- [ ] Exit-code behavior for new failure paths defined in CLI-UX §6

## 12. Safety verification strategy (tests)

| Layer | What | Where |
|---|---|---|
| Sandbox guard | harness refuses real `$HOME` (`SWEEP_TEST_GUARD`) | tests/harness |
| Mutation assertion | `scan` + `dry-run` runs leave sandbox byte-identical (mtime+content manifest) | gate_tests |
| Gate flow | wrong PURGE → 2; `--yes` blocked on destructive with message; `--i-know-what-im-doing` proceeds | gate_tests |
| Protected entries | sockets/systemd-private/X11/open-fd survive `tmp` AND `tmp-full` | rules/tmp |
| Mount bound | fake device boundary stops walk (bind-mount simulated via chroot-ish fixture or dev-id stub) | walker_tests |
| Prefix re-check | plan targeting outside allowed prefixes refused at execute | exec_tests |
| Sudo plan | tampered digest → child refuses; schema mismatch → refuses (run child without sudo in tests) | sudo_tests |
| Guard processes | fake /proc seed (injectable procfs) → guarded detect + P6 | rules/browsers |
| Lock | second concurrent instance exits 1 | command_tests |
| Snapshot fidelity | docs table == `rules list` JSON == implementation | golden, CI |
