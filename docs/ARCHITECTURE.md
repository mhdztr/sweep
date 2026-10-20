# sweep — Architecture (the *how*, structure)

Rust binary crate; single static-friendly binary; no workspace in v1 (split only if
a `sweep-lib` ever becomes useful). Edition: latest stable Rust. MSRV: stable − 2.

---

## 0. Design principles

1. **Pure-local:** no network, ever, in any code path (scan, clean, doctor, config).
2. **One context, built once:** `Ctx` is constructed at startup and threaded
   immutably; no global mutable state, no ambient `$HOME` reads deep in rules
   (every `$HOME`-relative path goes through `Ctx::home` → enables `SWEEP_HOME` tests).
3. **Detect → Scan → Plan → Gate → Execute → Report**, strictly ordered phases with
   typed values between them. A phase cannot be skipped by a flag except `--dry-run`
   (which stops before Execute).
4. **Continue-on-error:** the unit of failure is the rule, not the run.
5. **Docs are the spec:** rule behavior in code must match `docs/RULES-CATALOG.md`;
   CLI behavior must match `docs/CLI-UX.md` (CI snapshot tests enforce).

---

## 1. Module layout

```
~/Projects/sweep/
├── Cargo.toml
├── src/
│   ├── main.rs              # clap parse → Ctx build → dispatch; panic hook → friendly msg
│   ├── cli/
│   │   ├── mod.rs           # clap derive structs: Root, Commands, GlobalFlags
│   │   ├── scan.rs          # read-only scan → table/JSON
│   │   ├── clean.rs         # scan → checklist/scripted → gate → execute → report
│   │   ├── top.rs           # biggest files/dirs report
│   │   ├── history.rs       # run-history viewer (--last N, --json)
│   │   ├── doctor.rs        # environment diagnostics
│   │   ├── rules_cmd.rs     # `rules list` introspection
│   │   └── config_cmd.rs    # `config init` / `config edit`
│   ├── rules/
│   │   ├── mod.rs           # Rule trait, Tier, Detect, registry::all_rules(&Ctx)
│   │   ├── tmp.rs           # tmp, tmp-full, var-tmp (protected-list lives here)
│   │   ├── cache.rs         # user-cache, cache-deep, thumbnails, app-caches
│   │   ├── trash.rs         # trash (XDG trash: ~/.local/share/Trash/{files,info})
│   │   ├── browsers.rs      # browser-cache, editor-cache (+ running guards)
│   │   ├── tools.rs         # tool-caches (npm/pnpm/pip/uv/cargo/go/gem/composer)
│   │   ├── pkg.rs           # pacman-cache, pkgcache-zero, orphans, aur-builds,
│   │   │                    #   journal-system, journal-user, flatpak-unused
│   │   ├── containers.rs    # docker, podman (list-first)
│   │   ├── dev.rs           # dev-artifacts (age-gated home scan)
│   │   ├── logs.rs          # log-rotate, purge-logs, xdg-state-junk
│   │   ├── downloads.rs     # downloads (destructive, age rescue)
│   │   ├── snapshots.rs     # snapshots-report (report-only, snapper)
│   │   ├── steam.rs         # steam-shader
│   │   └── custom.rs        # CustomRule from TOML [[custom]]
│   ├── scan/
│   │   ├── mod.rs           # run_registry(): detect → scan all → Vec<RuleResult>
│   │   ├── walker.rs        # jwalk walker: symlink-safe, mount-bounded, exclusions,
│   │   │                    #   mtime tracking, progress callback, cancellation token
│   │   └── fsinfo.rs        # statvfs totals/free, mount table, fstype detection
│   ├── exec/
│   │   ├── mod.rs           # execute(plan): phase order, continue-on-error, dry-run,
│   │   │                    #   per-rule Report aggregation, SIGINT handling
│   │   ├── sudo.rs          # system-pass: plan serialization, re-exec, plan binding,
│   │   │                    #   pkexec fallback, decline handling
│   │   ├── trash.rs         # XDG trash implementation (info/ + files/, mtime rename)
│   │   └── del.rs           # safe delete primitives: fd-relative unlink, dir-walk
│   │                        #   delete, protected-entry filters (shared with walker)
│   ├── ui/
│   │   ├── mod.rs           # Theme (tier→color), NO_COLOR/unicode gates, header box
│   │   ├── checklist.rs     # inquire multi-select + custom renderer + keybindings
│   │   ├── table.rs         # comfy-table presets (wide/narrow/ascii), size formatting
│   │   ├── progress.rs      # indicatif: scan spinner-phase, clean per-rule bars
│   │   ├── confirm.rs       # summary card, per-rule y/N, PURGE typed gate, abort copy
│   │   ├── summary.rs       # final card: freed/skipped/errors/log path
│   │   └── format.rs        # human bytes (KiB→GiB), durations, counts, age strings
│   ├── system/
│   │   ├── mod.rs           # SystemCtx { distro, family, pkgman, init, fstype,
│   │   │                    #   snapper, sudo_kind, docker/podman/flatpak availability }
│   │   ├── distro.rs        # /etc/os-release parse (ID, ID_LIKE), family mapping
│   │   ├── pkgman.rs        # detection order + command argv tables per family
│   │   ├── proc.rs          # /proc scan: comm guards, open-fd detection for /tmp
│   │   └── snapper.rs       # snapper list-configs / list parsing → report struct
│   ├── config.rs            # toml load, defaults, merge, validation, ConfigError
│   ├── state.rs             # XDG state paths, action log writer, history.jsonl
│   └── report.rs            # JSON models (scan/clean/top/history), schema version
├── tests/
│   ├── harness/mod.rs       # sandbox!() fixture: temp home, SWEEP_HOME, guard asserts
│   ├── walker_tests.rs      # symlinks, mount-bounded, protected /tmp entries
│   ├── rules/*.rs           # one file per rule family
│   ├── gate_tests.rs        # PURGE flow, --yes boundaries, dry-run zero-touch
│   └── golden/*.snap        # insta snapshots: scan table, doctor, rules list
├── docs/                    # spec-of-record copies (see CONTEXT §5)
├── completions/             # planned (M7): bash/zsh/fish (clap_complete)
├── dist/sweep.service       # planned (M7): Type=oneshot, report-only default
├── dist/sweep.timer         # planned (M7): OnCalendar=weekly, disabled in pkg
├── packaging/
│   ├── aur/sweep-bin/       # planned: PKGBUILD + .SRCINFO (prebuilt release asset)
│   └── aur/sweep/           # planned: PKGBUILD + .SRCINFO (source build)
├── .github/workflows/       # ci.yml today; security/release/packaging/scheduled planned
├── AGENTS.md · CONTEXT.md · ROADMAP.md · README.md · LICENSE (MIT)
```

### Module responsibilities (one line each)

| Module | Owns | Must NOT do |
|---|---|---|
| `cli/` | argument parsing, choosing flow, printing | filesystem logic, deletion |
| `rules/` | what/where/how of each cleanup target | prompting, color output |
| `scan/` | measuring sizes safely | deleting, following symlinks |
| `exec/` | performing deletions per plan | deciding selection, gating UI |
| `ui/` | all rendering & prompts | business rules, side effects (except inquire IO) |
| `system/` | environment facts | mutating the system |
| `config` | load/merge/validate | defaults duplication (comes from one table) |
| `state` | logs/history/pruning | deciding what to log (caller decides) |

---

## 2. Core types

```rust
enum Tier { Safe, Caution, Destructive }           // report-only = Rule::report_only() flag

enum Detect {
    NotApplicable { reason: String },              // hidden from UI entirely
    Ready,                                         // applicable, no blocker
    Guarded { reason: String },                    // shown, refused at execution until
}                                                  //   explicitly re-checked (then caution confirm)

struct Ctx {
    home: PathBuf,               // $SWEEP_HOME ?? $HOME (never read $HOME elsewhere)
    config: Config,              // validated merge of defaults + user toml
    system: SystemCtx,           // facts from system/
    flags: GlobalFlags,          // dry_run, json, color, unicode, keep_days, trash, yes, verbose
    run_id: String,              // ulid-ish timestamp for logs/history correlation
}

struct ScanInfo {
    bytes: u64,
    files: u64,
    dirs: u64,
    newest_age: Option<Duration>,     // None if empty
    samples: Vec<PathBuf>,            // first N (for dry-run/preview)
    note: Option<String>,             // e.g. "kept 2 versions", "310 pkgs"
}

struct Plan {
    selected: Vec<PlanItem>,          // rule ids in execution order (user pass, then system pass)
    gate_ack: BTreeMap<String, GateAck>, // destructive id → {purge_typed: true, at: ts}
    keep_days: Option<u32>,
    dry_run: bool,
    trash: bool,
    schema: u32,                      // plan schema version (sudo re-exec validates)
}

struct RunReport {
    items: Vec<ItemReport>,           // per rule: freed, removed, skipped[], errors[]
    freed_total: u64, duration: Duration,
    exit_hint: ExitCode,              // 0 | 2 | 3
}
```

**Error taxonomy** (`thiserror`):
`ConfigError` (parse/validation with line+key context) ·
`ScanError` (io on a path — degraded: rule shows `?` with error, run continues) ·
`PlanError` (unknown rule id, invalid preset combo) ·
`GateError` (aborted, wrong PURGE text → maps to exit 2) ·
`ExecError` (per-rule io/command failures → collected, exit 3 at end) ·
`SystemPassError` (sudo declined/unavailable → reported, not fatal).
Top level: `anyhow` context + mapping table to exit codes (§13).

---

## 3. Data flow (with pseudocode)

```
main:
  args = Root::parse()
  ctx = Ctx::build(args)?            # config → system detect → flags merge → run_id
  match args.cmd:
    Scan  → results = scan::run(&ctx);  emit table|json; exit 0
    Clean → results = scan::run(&ctx)   # cached for this process
            selection = ui::checklist(&ctx, &results)   # or scripted from preset/only/exclude
            plan = build_plan(selection, ctx.flags)?    # applies age/gate metadata
            if plan.empty: "nothing selected" exit 0
            ui::gates(&ctx, &plan)?                     # summary + per-caution + PURGE
            report = exec::run(&ctx, &plan)             # user pass, then system pass
            state::append_history(report); state::log_run(report)
            ui::summary(report); exit(report.exit_hint)
```

**Clean flow detail:**

```
exec::run:
  for item in plan.selected.filter(sudo=false):        # user pass
      if dry_run: print preview(item); continue
      match rule.clean(ctx, plan): ok(report) | err(e) → collect, continue
      if SIGINT flag: break, mark aborted
  sys = plan.selected.filter(sudo=true)
  if sys non-empty and not dry_run:
      if sudo available:  write plan.json (state/tmp) →
                          spawn `sudo sweep --internal-system-pass plan.json`
                          (validated: schema, gate_acks, rule ids, checksum) → collect
      else: mark all sys items skipped{sudo declined/unavailable}
  aggregate → RunReport
```

**Internal system pass:** re-parses hidden command, rebuilds `Ctx` (same config),
re-runs `detect()` on each planned system rule (environment must still qualify),
verifies plan checksum + `gate_ack` presence for destructive ids, executes, prints a
single JSON result on stdout for the parent to parse (parent never trusts free text).

---

## 4. Concurrency model

- **Scanning:** one dedicated `rayon` pool sized `available_parallelism()` (the
  N thread-hit budget); rules run on scoped threads **outside** that pool, while
  `jwalk` walks with `Parallelism::RayonExistingPool` + a 5 s busy timeout as a
  hang guard. Results reduce into the rule's `ScanInfo` via `AtomicU64` +
  mutexed counters (no shared `RefCell` across threads). Progress events →
  `std::sync::mpsc` → indicatif ticker (UI thread).
- **Order:** rules scan in parallel but with a **global budget**: max N thread-hits
  (default = available_parallelism) so a huge home scan can't starve the UI.
- **Cancellation:** `AtomicBool` token checked per directory entry; SIGINT sets it;
  walker returns partial `ScanInfo` marked `partial: true` (scan phase: drop rule
  from selection with note; execution phase: finish current rule then abort — §SAFETY).
- **Execution is sequential by design** (predictable output, ordered progress, no
  parallel `rm` storms); only internal walker of each rule may parallelize.
- **Single-instance lock:** `flock` on `~/.local/state/sweep/sweep.lock` is
  acquired by **mutating** commands (a clean run start, the sudo system pass,
  state writes) for the whole mutating run. Read-only commands (`scan`, `top`,
  `history`, `doctor`) never create or acquire the lock — `scan` must mutate
  nothing (§0.2). A second mutating instance exits 1 with
  `another sweep is running (pid N, since HH:MM)`.

---

## 5. Scan engine (walker)

Requirements it satisfies: symlink safety, mount bounds, protected /tmp entries,
progress, partial results, mtime for age gates.

```
walk(root):
  assert root exists & is dir
  root_dev = stat(root).dev
  for entry in jwalk(root, follow_links=false, skip_hidden=false):
      if cancelled: break
      if entry.file_type is symlink: count as file (size 0), never descend
      if stat(entry.path).dev != root_dev: skip subtree          # mount bound
      if protected(entry): skip subtree (systemd-private, X11-unix, sockets…)  # /tmp only
      accumulate(bytes, files, dirs)
      newest_mtime = max(newest_mtime, entry.mtime)
      samples.push (until N)
      progress.maybe_tick()
```

- **Implementation notes (M1, jwalk 0.8):** subtree pruning happens in
  `process_read_dir` via `children.retain` (a pruned entry is **neither yielded
  nor descended**); children are sorted inside the same callback (deterministic
  walk order); `skip_hidden(false)` is set explicitly — jwalk's default is
  `true`, which would silently drop dotfiles from caches/tmp; the age gate
  filters **non-dir** entries at count time (directories are always counted,
  at 0 bytes); one `lstat` per entry, with mtime/dev stashed in jwalk's
  `client_state`; `WalkOptions.root_dev` is a documented test seam (it pins
  the mount-bound check for fixtures); symlinks count as 0-byte files and are
  never followed; directories contribute 0 bytes, so scan totals are file-size
  sums only (du tolerance: compare against `du --apparent-size`, which also
  counts dir inodes — expect the scan ≤ du).
- **Protected list** shared between walker (scan) and `del.rs` (execute) — one
  source of truth (`rules::tmp::PROTECTED`, glob patterns + `S_ISSOCK/S_ISFIFO`).
- **Age computation:** `age = now − newest_mtime` (atime unreliable on noatime fs —
  documented; `min_age_days` for dev-artifacts uses **mtime** too, i.e. "untouched
  since last write", which is what "stale build dir" means).
- **Cache within run:** `scan::run` results memoized in `Ctx`-adjacent `ScanCache`
  so checklist totals, dry-run previews, and execution sizes never re-walk.
- **Budgets:** hard display budget — scan phase shows elapsed ticker >1 s; if a rule
  exceeds `scan.slow_after_ms` (default 2000), UI adds "(large — still scanning)".

## 6. Execution engine

1. **Order:** user pass order = rule registration order (stable, docs-listed);
   system pass = same, appended after (so output groups "user" then "system").
2. **Deletion primitives** (`exec/del.rs`): `remove_tree(path, protected)` —
   bottom-up, never following symlinks, refusing `/`, refusing anything not under
   an allowed prefix (double-check at execute time, independent of plan), on same-fs
   dirs uses `rename` to a temp name first then unlinks (best-effort atomicity).
3. **Dry-run printing:** per rule → up to 20 sample paths (`+ 4,210 more`), total
   bytes/files, and external command argv that would run.
4. **Trash mode:** `--trash` moves deletable top-level entries into
   `~/.local/share/Trash/files/<timestamped>/` + writes `.trashinfo` entries;
   pre-computes that trash target has space; refuses destructive `downloads` under
   trash when it would just relocate (still honors user choice; UI notes
   "space is NOT freed until you empty trash").
5. **Free-space sanity:** before destructive passes, `statvfs` check warns if
   trash/log writes would fail.
6. **Per-rule report** always includes `freed` measured by (bytes_before −
   bytes_after where rescannable cheaply, else accounted bytes from deletion list).

## 7. UI layer

- **Theme tokens:** `tier.safe=#4caf50`, `tier.caution=#ffb300`,
  `tier.destructive=#ef5350`, `info=#4dd0e1`, `dim=#8a8a8a`, `header_bg`… — all
  resolved through `should_color()` = `NO_COLOR` unset ∧ `--no-color` absent ∧
  `TERM != dumb` ∧ (tty or `--color always`).
- **Width handling:** `terminal_size()`; ≥100 cols → full table; 60–99 → drop
  `files` column + shorten paths with `~/…/tail`; <60 → compact list rows
  (two-line per rule). `--no-unicode` swaps box drawing to ASCII; tier and state
  glyphs are ASCII in every mode (D13).
- **Checklist:** hand-rolled prompt loop (D14) over a pure, renderer-agnostic
  `ui::checklist::Selection` state model plus a slice-2 renderer/event loop;
  prompt text = header
  box; render loop re-renders live totals on every toggle. Keys: `space` toggle,
  `↑/↓` (also `Ctrl-P/N`), `a` all safe, `n` none, `i` invert, `p` preset cycle,
  `/` filter (v0.2 candidate), `enter` confirm, `esc`/`q`/`Ctrl-C` abort.
- **Row states:** `[x]/[ ]`, tier glyph (`+`/`!`/`#`), title, guard note,
  `sudo`/`lock` labels, right-aligned human size. Guarded rows show
  `[!] reason` and are *refused* at confirm if checked (msg: "browser running —
  re-run after closing, or use --force-guards" *(flag intentionally deferred to
  Could)*).
- **Progress:** scan = single spinner + "n/m rules" + elapsed; clean = per-rule
  `indicatif::MultiProgress` bar (bytes where measurable, else file-count ticks).

## 8. System integration

**Distro detect:** `/etc/os-release` → `ID`, `ID_LIKE` →
`Family::{Arch, Debian, Fedora, Suse, Other{}}`. `pkgman` probe order per family:
Arch: `pacman` → (`paru`|`yay` for AUR rule presence), Debian: `apt`, Fedora: `dnf`,
Suse: `zypper`.

**Command argv table (never through a shell — `std::process::Command` argv only):**

| Rule | argv ( Arch ) |
|---|---|
| pacman-cache | `paccache -rk{N}` (fallback if pkgutils missing: `pacman -Sc` with keep prompt → not used; rule reports "paccache unavailable") |
| pkgcache-zero | `pacman -Scc` (`-y` accept via argv) |
| orphans | `pacman -Qtdq` → `pacman -Rns -- <pkgs…>` (batched, argv list) |
| journal-system | `journalctl --vacuum-size={max}` |
| flatpak-unused | `flatpak uninstall --unused -y` |
| docker | `docker system df --format json` (preview) → `docker image prune -f` (+ `--filter` when config) |
| podman | `podman system df --format json` → `podman image prune -f` |

**Proc guards:** scan `/proc/[0-9]+/comm` + `cmdline` basename matching table
(`firefox`, `chromium`, `chrome`, `brave`, `code`, `cursor`, `electron` apps by
argv0). Open-fd check for `/tmp`: readlink `/proc/[pid]/fd/*` → prefix match.

**Snapper:** `snapper list-configs --csv` (fallback: `/etc/snapper/configs/*`);
per config `snapper list --csv` → count + `total size` if column present else
`du -s --apparent-size` **not** run (too slow) → report "N snapshots (size n/a:
run `sudo snapper list -s`)" when the size column is absent.

## 9. Config (`~/.config/sweep/config.toml`) — full key reference

Precedence: **CLI flags > env > config file > built-in defaults.**

```toml
[general]
preset = "regular"                # default preset: safe|regular|full
keep_days = 1                     # global age gate (tmp-style rules); 0 = off
trash_instead_of_delete = false   # == --trash
color = "auto"                    # auto|always|never
unicode = "auto"                  # auto|always|never
history_keep_runs = 100           # prune history.jsonl beyond this
scan_slow_after_ms = 2000         # UI hint threshold

[scan]
exclude_paths = ["~/code", "~/vm"]   # never descended by home-scoped scans
follow_mounts = false                # (reserved) always false in v0.1

[presets.regular]
include = []                      # extra rule ids pre-checked
exclude = ["docker"]              # rule ids never pre-checked by regular

# per-rule sections — keys validated against the rule's schema; unknown key = error
[rules.tmp]
enabled = true

[rules.pacman-cache]
keep_versions = 2                 # 0..=N; full preset uses 0 unless overridden

[rules.journal-user]  / [rules.journal-system]
max_size = "100M"                 # human size parser: K/M/G/T (KiB base)

[rules.user-cache]
keep = ["wl-clipboard"]           # whitelist: entries never removed by user-cache

[rules.dev-artifacts]
min_age_days = 30
paths = ["node_modules", "target", ".venv", "__pycache__", ".pytest_cache", ".gradle"]

[rules.downloads]
keep_days = 0                     # rescue: skip files newer than N days (0 = none)

[rules.orphans]
confirm_batch = 200               # if more than N orphans, paginate list at confirm

[[custom]]                        # user-defined rules — see ARCHITECTURE §10
id = "iso-archive"
title = "Old ISO images"
paths = ["~/Archives/isos"]
glob = "*.iso"
older_than_days = 90
tier = "caution"                  # safe|caution|destructive
sudo = false
```

**Validation:** unknown top-level keys warn (not fail) — forward compatible;
unknown `[rules.<id>]` for a *known* rule = error with suggestion; `[custom]`
validated strictly (id regex `^[a-z0-9-]{3,40}$`, tier enum, ≥1 path, glob compiles,
`older_than_days` required ≥ 0, reserved prefixes `tmp-`, `pacman-` rejected).

## 10. Custom rules semantics

`CustomRule` implements `Rule`: `detect` = all paths exist (else NotApplicable with
path name) → `scan` = walk each path applying glob at each level + age filter →
`clean` = delete matched files (age-gated, symlinks unlinked only). Tier honored
(including destructive → PURGE gate applies). `sudo=true` custom rules allowed but
root path validation requires absolute expanded path outside `/etc,/usr,/boot`.

## 11. State & data files

| Path (XDG) | File | Format | Retention |
|---|---|---|---|
| state | `~/.local/state/sweep/history.jsonl` | JSONL per run | `history_keep_runs` |
| state | `~/.local/state/sweep/logs/<run_id>.log` | plain text audit log | last 30 logs pruned |
| state | `~/.local/state/sweep/sweep.lock` | flock | n/a |
| state/tmp | `system-pass-<run_id>.json` | plan for sudo re-exec | deleted after pass |
| config | `~/.config/sweep/config.toml` | toml | user-owned |
| cache | *(none in v0.1 — no persistent size cache; sizes always fresh)* | — | — |
| trash | `~/.local/share/Trash/{files,info}` | XDG trash | user-managed |

**History record** — one JSON object per line in `history.jsonl`, appended after
every clean run (schema v1):

```jsonc
{"v":1,"ts":"2026-10-19T14:03:11Z","preset":"regular","mode":"interactive",
 "items":[{"id":"tmp","freed":1288490188,"removed":4100,
           "skipped":["browser-cache:firefox running"]}],
 "freed_total":60123456789,"duration_ms":9211,"dry_run":false,"trash":false}
```

**Audit log line schema** (see SAFETY §9) — every executed command and top-level
path-tree removal, with run_id correlation.

## 12. JSON outputs & versioning

`report.rs` owns all serializable structs. Top-level always contains
`"version": <u32>` (starts at 1) + `"kind": "scan"|"clean"|"top"|"history"`.

- **scan:** `host{distro,family,pkgman,fstype,hostname}`, `rules[{id,title,tier,
  applied,reason,needs_sudo,bytes,files,dirs,newest_age_secs,samples?}]`,
  `totals{reclaimable_bytes,regular_bytes,scan_ms}`, `config{preset,keep_days}`.
- **clean:** same `rules` (only selected), plus `run{freed_bytes,removed,
  skipped[{id,reason}],errors[{id,message}],duration_ms,dry_run,trash,
  sudo_pass{declined,failed,items}}`.
- **top:** `roots[], mode: dirs|files, entries[{path,bytes,files?,depth}]`.
- **history:** array of history records (see state table).
- `samples` and `host.hostname` omitted unless `-v`.
- **Versioning policy:** additive fields don't bump; removals/renames do; documented
  in `docs/JSON-SCHEMAS.md` with a changelog table per version.

## 13. Exit codes & top-level error mapping

| Code | Mapping |
|---|---|
| 0 | success; also "nothing to clean"/"nothing selected"; also pure dry-run |
| 1 | `ConfigError`, `PlanError`(unknown rule/preset), io failure building Ctx, internal panic caught by hook, lock held |
| 2 | `GateError` (user abort: esc, wrong PURGE, Ctrl-C at prompt) |
| 3 | run completed with ≥1 `ExecError` or system-pass failure — summary prints details |

`--quiet` suppresses non-essential output but never suppresses errors/summary of
exit ≠ 0. `-v` adds per-rule argv + errno context.

## 14. Dependencies (with role + notable features)

| Crate | Features / role |
|---|---|
| `clap` 4 (derive) | arg parsing; `clap_complete` (bash/zsh/fish), `clap_mangen` (roff) |
| `inquire` | MultiSelect/Confirm/Text prompts (PURGE), custom renderers |
| `indicatif` | ProgressBar, MultiProgress, steady ticks |
| `comfy-table` | presets: UTF8_FULL vs ASCII for narrow/`--no-unicode` |
| `owo-colors` + custom NO_COLOR gate | tier colors |
| `jwalk` (serde not needed) + `rayon` | parallel walk |
| `serde`/`serde_json` (preserve_order off) / `toml` (parse only) | all data files |
| `dirs` | XDG resolution (home/config/state/data) |
| `nix` (features: fs, process) / `libc` | statvfs, flock, kill, signal setup |
| `glob` (or `globset`) | custom rules + protected globs |
| `filetime` | (optional) explicit mtime handling for age gates if needed |
| `thiserror` + `anyhow` | error layers |
| `ulid` or hand-rolled ts+rand | `run_id` |
| dev: `assert_cmd`, `predicates`, `tempfile`, `insta` (json + text), `serial_test` | tests (lock file makes tests serial where needed) |

## 15. Performance budgets

| Operation | Budget | Strategy |
|---|---|---|
| `scan` (cache rules, SSD) | < 5 s | parallel walk, per-rule roots, no double walk |
| `scan` (incl. pacman cache) | < 10 s | same; pkg cache may use `paccache`/du on `/var/cache/pacman/pkg` parallel |
| `dev-artifacts` full home (SSD) | < 30 s with progress | single walk, matching dir names at any depth, mount-bounded, exclude_paths |
| checklist toggle feedback | < 16 ms | totals precomputed per rule |
| startup (arg parse → header) | < 100 ms | lazy: system detect only after header; doctor does full detect |
| memory during scan | O(depth × threads) | no per-file retention beyond samples (N=20/rule) |

## 16. Testing architecture

- **`sandbox!()` harness:** builds `tempfile::TempDir`, seeds fake home tree
  (cache/tmp/downloads/config fixtures), sets `SWEEP_HOME`, and **asserts
  `Ctx::home != real $HOME`** (env var `SWEEP_TEST_GUARD=1` set by harness; main
  aborts if guard present and target is real home).
- **Rule tests:** for each rule — detect on/off scenarios, scan counts/bytes exact,
  age gates (seeded mtimes via `filetime`), clean result + survivors.
- **Gate tests:** PURGE wrong text → GateError(2); `--yes` does not bypass
  destructive; `--i-know-what-im-doing` does (documented behavior locked by test).
- **Golden tests (`insta`):** `scan` table (wide/narrow/no-unicode/no-color),
  `doctor`, `rules list`, dry-run previews — docs/copy drift caught in CI.
- **Command tests (`assert_cmd`):** exit codes 0/1/2/3 paths, JSON validity +
  schema-required keys, lock contention.
- **Never in tests/CI:** real sudo, real `$HOME` mutation, network (enforced by
  the no-network code policy + no test needs it). Release and packaging jobs may
  fetch crates/tooling; the binary itself never performs network I/O.

## 17. Portability notes (Linux-only, per requirement)

- Everything targets Linux `/proc`, XDG, `os-release`. Non-Linux builds are not
  supported (`compile_error!` on non-unix + runtime check of `/proc`).
- Root fs differences (btrfs ext4 xfs zfs) handled via `statvfs` + mount table only.
- Containers: `doctor` warns when running without `/proc` visibility or read-only
  `/tmp` (rules degrade to NotApplicable with reasons).

## 18. CI/CD, release, and distribution

Plan of record: `docs/CI-CD.md`. Summary: five workflows (`ci`, `security`,
`release`, `packaging`, `scheduled`); `cargo-deny` gates advisories, licenses,
bans, and sources; PR checks also run doctests, `cargo doc`, and a release build;
releases are tag-driven (`v*`) and ship `.sha256`, a CycloneDX SBOM, and SLSA
provenance. AUR is the primary channel (`sweep-bin` first, then a source
package); Flatpak is deferred (D11) because the sandbox has a private `/tmp` and
no sudo. `sweep` itself performs no network I/O.
