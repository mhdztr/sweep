# sweep: CLI & Terminal UX (the *interface*)

Complete command reference, prompt copy catalog, mock outputs, exit codes, env vars,
tokens. Every string here is what ships (golden snapshots enforce it).

---

## 1. Global conventions

- **Synopsis:** `sweep [GLOBAL FLAGS] <COMMAND> [ARGS]`
- **No args** → behaves as `sweep clean` (interactive; if stdin is not a TTY →
  prints `error: interactive mode requires a TTY; use --preset/--only with --yes`
  and exits 1).
- **Precedence:** CLI flags > env vars > config file > defaults.
- **Everything destructive is gated; every gate is abortable (Esc/Ctrl-C → exit 2).**
- **`--json`**: single JSON document on stdout, human chatter goes to stderr.
- **`--quiet`**: only errors + final summary line; **`-v`**: adds argv/errno context.
- **Never color in pipe** unless `--color always` equivalent (`--no-color` and
  `NO_COLOR` always win).

### Global flags (apply to every command)

| Flag | Env | Effect |
|---|---|---|
| `--dry-run` | – | plan preview: sample paths + would-run commands; zero changes |
| `--json` | – | JSON stdout (schema `version:1`) |
| `--no-color` | `NO_COLOR=1` | disable ANSI colors |
| `--no-unicode` | `SWEEP_NO_UNICODE=1`, `LC_ALL=C`-ish detection | ASCII rendering for box drawing (`+ - |`) and `...` ellipsis instead of `│─┌…`; in the interactive checklist (§3) this also covers all dynamic text (row titles, guard/disabled reasons). Tier glyphs/labels (`+ ! # ~`), checkboxes (`[x]`/`[ ]`) and `lock`/`sudo`/`warn` markers are always ASCII. Note: `scan`'s header/summary separators (`·`) and table placeholders (`—`) remain non-ASCII under this flag today (deferred; see ROADMAP M3 task "ASCII-clean `scan` output"). See §3 and §8. |
| `--keep-days <N>` | `SWEEP_KEEP_DAYS` | override global age gate (N ≥ 0) |
| `--trash` | `SWEEP_TRASH=1` | soft-delete to XDG trash instead of hard delete |
| `--yes` / `-y` | `SWEEP_YES=1` | auto-confirm **safe & caution** prompts; **not** destructive |
| `--i-know-what-im-doing` | – | the ONLY bypass of typed `PURGE` (implies yes; prints loud banner) |
| `--preset <safe\|regular\|full>` | `SWEEP_PRESET` | selection preset (command-specific too) |
| `--no-unicode` / `--color <auto\|always\|never>` | `NO_COLOR`, `FORCE_COLOR` | output mode |
| `-v` / `--verbose`, `-q` / `--quiet` | `SWEEP_VERBOSE` | verbosity |
| `-h` / `--help`, `-V` / `--version` | – | clap-standard |

> **M1 note: `--color <auto|always|never>` is documented but not implemented
> yet.** The flag (`ColorChoice`, plan §2.6) is deferred to M-next: M1 ships
> no such clap argument, so the row above is the v0.1 contract, not current
> behavior. Color in M1 is controlled by `--no-color` / `NO_COLOR` /
> `FORCE_COLOR` / `TERM=dumb` (resolved by `should_color()`, see
> `docs/ARCHITECTURE.md` §7 and the env table in CLI-UX §7).

Hidden internal: `--internal-system-pass <PLAN_PATH>` (documented in SAFETY §7).

---

## 2. Command reference

### 2.1 `sweep scan`
**Purpose:** read-only measurement → pretty table. Never changes anything.
**Args/flags:** `--json` · `--preset <p>` (affects "regular: X" total line only) ·
`--path <DIR>` *(reserved v0.2; v0.1 scans the fixed rule roots)*.

```
sweep scan
sweep scan --json | jq '.totals.reclaimable_bytes'
```
Output mock (wide):
```
╭──────────────────────────────────────────────────────────────╮
│  sweep 0.1.0 · storage cleaner            arch linux · pwnt │
│  root: 1.7 TiB total · 412 GiB used · 40.2 GiB reclaimable │
╰──────────────────────────────────────────────────────────────╯

  Scanning… ████████████████████ 26/26 rules           4.1s

 ┌──────────┬───────────────────────────────┬────────────┬──────────┬────────┐
 │ tier     │ category                      │ location   │ size     │ files  │
 ├──────────┼───────────────────────────────┼────────────┼──────────┼────────┤
 │ + safe   │ tmp & var-tmp                 │ /tmp       │  1.2 GiB │  4211  │
 │ + safe   │ user caches                   │ ~/.cache   │  8.4 GiB │ 61203  │
 │ + safe   │ thumbnails                    │ ~/.cache   │ 612 MiB  │  8940  │
 │ + safe   │ trash                         │ ~/.local   │  1.9 GiB │   612  │
 │ + safe   │ warn firefox running          │ ~/.cache   │  3.1 GiB │ 22014  │
 │ ! caut.  │ pacman pkg cache [keep 2] sudo│ /var/cache │  6.7 GiB │   310  │
 │ ! caut.  │ stale dev artifacts (30d+)    │ ~          │ 41.3 GiB │  3140  │
 │ # dang.  │ full downloads lock           │ ~/Download │ 22.9 GiB │  1204  │
 │ ~ info   │ btrfs snapshots (48, report)  │ snapper    │    —     │    —   │
 └──────────┴───────────────────────────────┴────────────┴──────────┴────────┘
  reclaimable: 40.2 GiB · regular preset selects: 13.3 GiB · scanned in 4.1s
```
Exit: 0 (always, unless error → 1).

Notes:
- Table rows may append a dim `(…)` note inside the category cell: age-gate / overlap
  counts (`tmp & var-tmp  (1 newer than keep-days kept)`, `user caches  (excl. thumbnails)`).
  M1's goldens show the 4-rule output (tmp & var-tmp, user caches, thumbnails, trash).
- `-v` adds per-rule detail lines after the table.

### 2.2 `sweep clean`
**Purpose:** the main flow: scan → select → gate → execute → report.
**Flags:** all globals + `--preset <p>` · `--only <id,id>` · `--exclude <id,id>` ·
`--no-prompt` (alias of `--yes`, requires non-destructive selection or
`--i-know-what-im-doing`).

```
sweep clean                              # interactive
sweep clean --preset full                # interactive, destructive unlocked
sweep clean --preset safe --yes          # scripted
sweep clean --only tmp,trash --dry-run
```

**Flow steps:** header → scan progress → checklist (§3) → gate(s) (§4) → execution
progress → summary card (§2.2b) → history append.

#### 2.2b Summary card (every clean run)
```
  ╭──────────────────────────────────────────────────────╮
  │  [x] freed 13.3 GiB in 8.2s                         │
  │     cleaned: 7 · skipped: 1 (firefox running)       │
  │     errors:  0                                      │
  │     log: ~/.local/state/sweep/logs/20261019T140311  │
  ╰──────────────────────────────────────────────────────╯
```
Variants: partial failure adds `warn 2 errors (see log)` (exit 3); aborted gate adds
`aborted, nothing was deleted` (exit 2); dry-run adds `dry run, nothing changed`;
trash mode adds `moved to trash; empty trash to actually free the space`.

### 2.3 `sweep top`
**Purpose:** biggest dirs/files report (decision support).
**Flags:** `--path <DIR>` (default `~`) · `--dir` | `--file` (default `--dir`) ·
`--depth <N>` (dirs, default 3) · `--min-size <HUMAN>` (default `100M`) ·
`--limit <N>` (default 20) · `--json`.
```
  biggest directories under ~  (depth ≤ 3, ≥ 100 MiB)

  1. 148.2 GiB  ~/Projects/legacy-game/target + node_modules
  2.  62.4 GiB  ~/.cache/paru/clone/linux-zen-git/src
  3.  41.3 GiB  ~/Downloads
  4.  22.1 GiB  ~/.local/share/Steam
  …
  scan: 6.2s · mounts bounded · symlinks not followed
```
Exit 0; empty result is exit 0 with `nothing ≥ 100 MiB found`.

### 2.4 `sweep history`
**Purpose:** past runs.
**Flags:** `--last <N>` (default 20) · `--json` · `--logs` (list log files too).
```
  when                preset    items  freed      duration  mode
  2026-10-19 14:03    regular   7      13.3 GiB   8.2s      clean
  2026-10-12 09:41    safe      5       2.1 GiB   3.9s      clean
  2026-10-01 21:15    full      12     58.7 GiB  41.0s      clean
  2026-09-28 08:12    regular   0           —     0.1s      dry-run
```
No history yet → `no runs recorded yet` + hint (exit 0).

### 2.5 `sweep doctor`
**Purpose:** explain the environment & what's active/skipped.
```
  sweep doctor

  host        arch linux (rolling) · kernel 6.12.9-arch1-1
  filesystem  / : btrfs (1.7 TiB total, 1.2 TiB free) · /tmp : tmpfs
  package     pacman 7.1.0 · paru present (AUR rules active)
  sudo        sudo present · user in wheel
  desktop     xdg + systemd user session
  snapper     3 configs, 48 snapshots (report-only rule active)
  containers  docker: running · podman: 5.x present
  flatpak     1 user installation · 4 unused runtimes detected
  sweep       config: /home/u/.config/sweep/config.toml (valid, 0 warnings)
  rules       26 total: 11 safe · 9 caution · 5 destructive · 1 report-only
  skipped     orphans: pacman present, nothing to do · apt rules: debian family not detected
```
Exit 0; each line degrades to `unavailable (reason)` instead of vanishing.

### 2.6 `sweep rules list`
**Purpose:** introspection; MUST match docs/RULES-CATALOG.md (CI).
**Flags:** `--tier <t>` · `--json`.
```
  id                  tier         sudo  default   state
  tmp                 safe         -     regular   ready
  browser-cache       safe         -     regular   guarded (firefox running)
  pacman-cache        caution      yes   off       ready (keep 2)
  downloads           destructive  -     full      locked (purge gate)
  snapshots-report    report       -     -         ready (btrfs, 48 snaps)
```
Unknown id given elsewhere (`--only bogus`) → exit 1 with `unknown rule 'bogus'; did you mean 'trash'?` (edit-distance suggestion).

### 2.7 `sweep config`
```
sweep config init        # writes default config (refuses overwrite unless --force)
sweep config path        # prints resolved config path
sweep config edit        # $EDITOR on the file (falls back: prints path)
```
`init` prints the annotated template (subset):
```toml
[general]
preset = "regular"        # safe | regular | full
keep_days = 1             # age gate for temp-style rules
# … (full schema documented in docs/CONFIG.md)
```

### 2.8 `sweep --version` / `sweep help`
Standard clap: `sweep 0.1.0`, per-command help with examples section.

---

## 3. Interactive checklist spec

> Implementation (R10): the hand-rolled loop (D14) and the pure renderer
> (`ui/render.rs`) + raw-mode/keys (`ui/tty.rs`, D15) landed in M2. This section
> is the target spec; the shipping slice-2 renderer draws a **compact two-line
> header** (`  sweep <ver> · preset: <name>` + `  <n> rows · space toggle · ?
> help · esc abort`) instead of the header box, and the row line does **not**
> print `location` yet (the size is still right-aligned). Every line is clamped
> to the terminal width, which is floored at `render::MIN_WIDTH`; under
> `--no-unicode` the middot `·` separators degrade to `|` and box drawing to `-`
> (D13).
>
> **D13/B1, precisely:** within the interactive checklist (`ui::render`),
> `--no-unicode` guarantees pure ASCII for all checklist text, dynamic text
> included: a row `title`, a `guard`/`disabled` reason, or any other note is
> stripped of every non-ASCII char, not just the chrome (separators, box
> drawing, ellipsis, `?` chrome) the flag originally covered. Independently of
> the flag, **control chars are always stripped from
> dynamic text**: C0 (`0x00-0x1f`, including `ESC` `0x1b`), C1 (`0x80-0x9f`,
> including the 8-bit CSI `0x9b`) and DEL (`0x7f`) never reach the terminal from
> a rule title or guard reason, so a crafted string cannot inject a terminal
> escape sequence. The stripping is one pure helper (`render::sanitize`), applied
> to the content **before** `Theme` paints it. With `--no-unicode` off, ordinary
> Unicode in dynamic text (accents, CJK) is preserved exactly as before.
>
> This guarantee is scoped to the checklist renderer (`ui::render.rs`);
> `scan`'s pre-existing header/footer/table chrome (`ui/mod.rs`, `ui/table.rs`)
> is not yet ASCII-clean under the flag.

### 3.1 Layout
Header (identity + preset) → scan progress → checklist card → live total bar.
Rows: `[x] + safe  title   warn guard sudo` + right-aligned size (the `location`
column is specced but the slice-2 renderer does not print it yet).
The cursor row is marked with `>` in its own left column (`  > [x] ···`,
unmarked rows keep the column blank: `    [x] ···`).
Row order: safe → caution → destructive → report-only (stable, docs-listed).

### 3.2 Keybindings
| Key | Action |
|---|---|
| `space` | toggle row (destructive rows: only when preset=full, else no-op) |
| `↑`/`↓`, `Ctrl-P`/`Ctrl-N` | move cursor |
| `a` | select all **safe** rows |
| `n` | deselect all |
| `i` | invert selection |
| `p` | cycle preset safe → regular → full → safe (re-applies defaults; warns "replaces your selection") |
| `enter` | proceed to gates (allowed even with 0 selected → "nothing selected, exiting") |
| `esc` / `q` / `Ctrl-C` | abort → exit 2, nothing done |
| `?` | toggles help footer |

### 3.3 Row states
`ready` (toggleable) · `guarded (reason)` (unchecked; toggling warns but allows →
becomes caution-confirm) · `locked` (destructive outside full: toggle refused with
hint "switch preset to full (p)") · `disabled by config` (gray, unselectable,
shows `disabled in config`) · `report-only` (info row, no checkbox).

### 3.4 Live total bar
`  4 selected · 13.3 GiB  ·  2 guarded · 1 locked`; recompute <16 ms (pre-aggregated).
All four groups always render (a zero group shows `0`), so the bar never jumps.

### 3.5 Footer hints + `?` help (exact strings)
Footer (leading two spaces for alignment with the rows):
`  space toggle · a all-safe · n none · i invert · p preset · enter proceed · ? help · esc abort`.
`?` toggles a keys overlay (toggle row / move cursor / select-all-safe / deselect /
invert / preset cycle / proceed / hide help / abort), and the `p` cycle flashes
`preset: <name> (replaces your selection)` for one frame.

---

## 4. Gate / prompt copy catalog (exact strings)

### P1: end-of-selection summary (all tiers; **default = cancel**)
```
  ┌─────────────────────────────────────────────┐
  │  4 categories · 13.3 GiB will be deleted    │
  │  1 destructive category included            │
  │  deletions are permanent (use --trash to    │
  │  stage instead)                             │
  └─────────────────────────────────────────────┘
  Proceed? [y/N] (cancel is default)  ▸
```
Typed `y`/`Y` only; **Enter = cancel** (documented, tested).

### P2: per-caution-rule confirm (shown when a caution rule selected)
```
  ! pacman package cache: paccache -rk2 (sudo)
     6.7 GiB · 310 packages · keeping newest 2 versions
     [y/N]
```
List-first variants: orphans → paginated package list; flatpak → runtime list;
docker → `docker system df` table; stopped containers/volumes → names.

### P3: typed PURGE gate (per destructive category; after P1)
```
  warn DESTRUCTIVE: full downloads
    1,204 files · 22.9 GiB · newest: 2 hours ago · oldest: 3 years ago
    samples: archlinux-2026.10.01.iso · recordings/2026-08.flac · …

    This is permanent. Type PURGE to confirm (Enter/cancel aborts):
  ▸ _
```
Wrong text → `aborted (text mismatch): nothing was deleted` → exit 2.
`--yes` at this point prints: `--yes does not confirm destructive rules; re-run
with --preset full --i-know-what-im-doing for scripting` (exit 2).

### P4: sudo pass
```
  System rules selected: pacman-cache, journal-system
  A single sudo elevation will run after the user-level cleanup. [y/N]
```
Decline → those items reported `skipped (sudo declined)`; user pass result stands.

### P5: volume-type danger (docker volumes sub-row)
```
  docker volumes hold DATA (databases, bind data). Type 'volumes' to confirm: ▸
```

### P6: running-app override
```
  warn firefox is running; cleaning its cache now may corrupt open sessions.
  Clean anyway? [y/N] ▸
```

### P7: empty / nothing results
`nothing to clean; you're already tidy (scan found 0 reclaimable bytes in safe rules)`

---

## 5. Error catalog (stderr, exit 1 unless noted)

| Situation | Message |
|---|---|
| not a TTY in interactive mode | `error: interactive mode requires a TTY; use --preset/--only with --yes` |
| unknown rule id | `error: unknown rule 'bogus'; did you mean 'trash'?` (1) |
| unknown preset | `error: invalid preset 'mega' (expected: safe, regular, full)` (1) |
| invalid SWEEP_KEEP_DAYS | `error: invalid SWEEP_KEEP_DAYS 'abc' (expected a non-negative integer)` (1) |
| config parse error | `error: config ~/.config/sweep/config.toml:12: invalid tier 'hot' (expected safe/caution/destructive)` (1) |
| config unknown rule key | `error: config: unknown rule section [rules.pacman]; did you mean [rules.pacman-cache]?` (1) |
| lock held | `error: another sweep is running (pid 1234, since 14:02)` (1) |
| sudo unavailable mid-pass | `warning: sudo unavailable; 2 system rules skipped` + summary note (0/3 per §exit) |
| scan path unreadable | `warning: /var/log/journal: permission denied (rule degraded)`; run continues (0) |
| disk full during log write | `warning: cannot write history: no space left`; cleaning still reported on stdout (0) |
| malformed `--only` csv | `error: --only expects comma-separated rule ids` (1) |
| `--json` + interactive | `error: --json requires --yes/--only/--preset non-interactive flow` (1) |

## 6. Exit codes (recap: normative)

| code | when |
|---|---|
| 0 | success · nothing to do · dry-run completed · report-only command ok |
| 1 | usage/config/environment errors (nothing executed) |
| 2 | user aborted (Esc, Ctrl-C at prompt, wrong PURGE, `--yes` hitting destructive) |
| 3 | completed with ≥1 rule error or system-pass failure (summary details) |

## 7. Environment variables

| Var | Effect |
|---|---|
| `NO_COLOR` | disable color (wins over everything except… nothing) |
| `FORCE_COLOR` | force color in pipes (unless NO_COLOR) |
| `SWEEP_NO_UNICODE` | ASCII output |
| `SWEEP_PRESET`, `SWEEP_KEEP_DAYS`, `SWEEP_TRASH`, `SWEEP_YES`, `SWEEP_VERBOSE` | flag mirrors |
| `SWEEP_CONFIG` | alternate config path |
| `SWEEP_HOME` | **test-only** home override (refuses real-home mutation with `SWEEP_TEST_GUARD`) |
| `SWEEP_STATE_DIR` | alternate state dir (tests, containers) |
| `XDG_*` | standard resolution via `dirs` |

## 8. Visual tokens

| Token | Value | Use |
|---|---|---|
| safe color | `#4caf50` green | tier label, `[x]` |
| caution color | `#ffb300` amber | tier label, `warn` |
| destructive color | `#ef5350` red | tier label, `lock`, PURGE screen |
| info | `#4dd0e1` cyan | header, report-only |
| dim | `#8a8a8a` | meta/locations/sizes secondary |
| header box | cyan border, bold title | every command's first line |
| size format | KiB base, 4 sig figs (`1.2 GiB`, `612 MiB`, `942 B`) | everywhere |
| age format | `2h`, `3d`, `47d`, `never` | previews |

Width rules: ≥100 cols full · 60–99 drop `files`+`location`→tail · <60 two-line
compact rows · `--no-unicode`: box drawing and the ellipsis (`│─┌…` → `|-+...`)
**and** (in the checklist renderer) every non-ASCII char in dynamic text (row
titles, guard/disabled reasons) → stripped; tier glyphs `+ ! # ~`, checkboxes
`[x] [ ]` and `lock`/`sudo`/`warn` are always ASCII. `scan`'s header/summary
separators (`·`) and table placeholders (`—`) remain non-ASCII under the flag
today (deferred; see ROADMAP M3 task "ASCII-clean `scan` output"). Control chars
(C0 incl. `ESC`, C1 incl. `0x9b`, DEL) are always stripped from dynamic text
regardless of the flag, so no rule title or guard reason can inject a terminal
escape sequence.

## 9. JSON examples (v1, abridged)

```jsonc
// sweep scan --json
{"version":1,"kind":"scan",
 "host":{"distro":"arch","family":"arch","pkgman":"pacman","fstype":"btrfs"},
 "rules":[{"id":"tmp","title":"Temp files","tier":"safe","applied":true,
   "reason":null,"needs_sudo":false,"bytes":1288490188,"files":4211,
   "dirs":312,"newest_age_secs":10800,"net_of":[]}],
 "totals":{"reclaimable_bytes":43138000000,"regular_bytes":14280000000,
   "scan_ms":4100},
 "config":{"preset":"regular","keep_days":1}}
```

```jsonc
// sweep clean --json (final)
{"version":1,"kind":"clean",
 "run":{"freed_bytes":14280000000,"removed":61211,"duration_ms":8200,
   "dry_run":false,"trash":false,
   "skipped":[{"id":"browser-cache","reason":"firefox running"}],
   "errors":[],
   "sudo_pass":{"declined":false,"failed":false,"items":[]}}}
```

## 10. Shell integration

- **Completions:** `sweep completion bash|zsh|fish > …` (clap_complete) + packaged
  in AUR at standard dirs.
- **Man page:** `man sweep` (clap_mangen, shipped in package).
- **Suggested aliases (README):** `alias sweep-full='sweep clean --preset full'`.
- **Timer:** `systemctl enable --now sweep.timer` → weekly report-only scan logged
  to journal (`journalctl -u sweep`); auto-clean mode is an explicit unit override
  documented in README (never default).
- **Prompt integration:** exit code 2 (aborted) never prints errors (safe in `&&` chains).

## 11. Accessibility & degenerate terminals

- Never color-only information: tier = glyph + word + color.
- `NO_COLOR`/`--no-color`/`TERM=dumb` → full grayscale usability.
- `--no-unicode`/bad locale → ASCII box drawing and ellipsis, and (in the interactive checklist) all dynamic text (titles, reasons) stripped to ASCII; `scan`'s table placeholders (`—`) and separators (`·`) are not yet ASCII-clean (deferred, ROADMAP M3).
- Narrow (<60) terminals: two-line rows, no columns assumed.
- Screen readers: prompt text is plain sentences; progress bars degrade to
  "scanning (n rules)…" when not a TTY? → progress only when TTY (clean rules
  still print per-rule completion lines with `-v`).
