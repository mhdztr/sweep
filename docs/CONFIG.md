# sweep — Configuration Reference (`~/.config/sweep/config.toml`)

> Generated companion to `docs/ARCHITECTURE.md` §9–§10 (spec of record).
> Precedence: **CLI flags > env vars > config file > built-in defaults.**

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

