# sweep: Rule Catalog (spec of record)

**26 rules.** This document is the source of truth for behavior; `sweep rules list`
must always match it (CI snapshot enforces). Code changes without a same-commit
update here are bugs.

**Tiers:** `+` `safe` (checked in `safe`+`regular` presets) · `!` `caution`
(visible, unchecked) · `#` `destructive` (`full` preset only, typed `PURGE`) · `~`
report-only (no clean action ever).

**Field meanings:**
- **Targets:** exact paths/globs (Arch; `detect()` substitutes elsewhere).
- **Action:** what execution does (built-in walker delete or external argv).
- **Guard:** condition that blocks/holds the rule with a shown reason.
- **Age/keep:** keep-newer-than-N policy and defaults.
- **Config:** keys under `[rules.<id>]`.
- **Tests:** required sandbox tests for that rule.

---

## Summary tables

| #  | id                 | tier | sudo  | default in `regular`            |
|----|--------------------|------|-------|---------------------------------|
| 1  | `tmp`              | +    | –     | [x] checked                     |
| 2  | `user-cache`       | +    | –     | [x]                             |
| 3  | `thumbnails`       | +    | –     | [x]                             |
| 4  | `trash`            | +    | –     | [x]                             |
| 5  | `journal-user`     | +    | –     | [x]                             |
| 6  | `browser-cache`    | +    | –     | [x] (guard-held if app running) |
| 7  | `editor-cache`     | +    | –     | [x] (guard-held if app running) |
| 8  | `tool-caches`      | +    | –     | [x]                             |
| 9  | `app-caches`       | +    | –     | [x]                             |
| 10 | `log-rotate`       | +    | –     | [x]                             |
| 11 | `xdg-state-junk`   | +    | –     | [x]                             |
| 12 | `pacman-cache`     | !    | sudo  | [ ]                             |
| 13 | `journal-system`   | !    | sudo  | [ ]                             |
| 14 | `aur-builds`       | !    | –     | [ ]                             |
| 15 | `orphans`          | !    | sudo  | [ ]                             |
| 16 | `flatpak-unused`   | !    | –     | [ ]                             |
| 17 | `docker`           | !    | sudo* | [ ]                             |
| 18 | `podman`           | !    | –     | [ ]                             |
| 19 | `dev-artifacts`    | !    | –     | [ ]                             |
| 20 | `steam-shader`     | !    | –     | [ ]                             |
| 21 | `snapshots-report` | ~    | –     | n/a (always listed)             |
| 22 | `downloads`        | #    | –     | locked (needs `full`)           |
| 23 | `cache-deep`       | #    | –     | locked                          |
| 24 | `tmp-full`         | #    | –     | locked                          |
| 25 | `pkgcache-zero`    | #    | sudo  | locked                          |
| 26 | `purge-logs`       | #    | sudo  | locked                          |

\* only when the docker socket requires root; normally group-based, unprivileged.

---

# Tier `+`: safe rules

## 1. `tmp`: Temp files, `+` safe
- **Targets:** `/tmp/*`, `/var/tmp/*`.
- **Protections (shared with `tmp-full`, single source `rules::tmp::PROTECTED`):**
  subdirs matching `systemd-private-*`, `snap.mount*`, `dbus-*` *sockets*;
  entries `X11-unix`, `.XIM-unix`, `.ICE-unix`; any socket/FIFO/device node
  (`S_ISSOCK|S_ISFIFO|S_ISCHR`); `/tmp/.X*-lock`; files with an open fd
  (`/proc/*/fd` readlink scan, execution-time re-check).
- **Action:** bottom-up walker delete of non-protected entries; never follows symlinks.
- **Age:** default **keep ≥ 1 day old** (`keep_days=1`); `--keep-days 0` cleans all non-protected.
- **Guard:** none. **Sudo:** no (falls back to skipping unreadable root-owned entries
  with `skipped{not readable}`; never escalates silently).
- **Config:** `[rules.tmp] enabled`, `[general] keep_days`.
- **Preview copy:** `1.2 GiB · 4,211 files · oldest 12d · newest 2d (37 newer than keep-days kept)`.
- **Tests:** protected entries survive (seed socket + `systemd-private-x` dir); age
  gate counts exact; symlink into `~/` not followed; open-fd file skipped.

## 2. `user-cache`: User caches, `+` safe
- **Targets:** `~/.cache/*` top-level entries, **minus** whitelist `keep` and minus
  entries owned by destructive `cache-deep` handling (same root; `user-cache` skips
  whitelisted, `cache-deep` ignores whitelist).
- **Action:** delete each non-whitelisted entry (whole subdir trees).
- **Age:** none (cache semantics), but skips entries modified < 5 min ago (hot writes).
- **Guard:** none. **Sudo:** no.
- **Config:** `[rules.user-cache] keep = [...]` (default `[]`), `enabled`.
- **Note:** `thumbnails` and `browser-cache` are separate rules, to avoid
  double-count, `user-cache` scan subtracts their bytes and shows `(excl. thumbnails/browser)`.
- **Tests:** whitelist survivor; hot-write skip; byte accounting excludes sibling rules.

## 3. `thumbnails`: Thumbnail cache, `+` safe
- **Targets:** `~/.cache/thumbnails/**` (normal + large + x-large + xx-large).
- **Action:** delete tree. Regenerated by file managers on demand.
- **Age:** none. **Guard:** none. **Sudo:** no. **Config:** `enabled`.
- **Preview:** `612 MiB · 8,940 files`.
- **Tests:** exact byte count; directory recreated as empty on clean (or removed; decided: removed).

## 4. `trash`: XDG trash, `+` safe
- **Targets:** `~/.local/share/Trash/files/**` + `~/.local/share/Trash/info/**`.
- **Action:** delete both trees. Preview counts trashed items and sums sizes.
- **Age:** none. **Guard:** none. **Sudo:** no.
- **Config:** `enabled`. **UI note:** "empties trash; permanent".
- **Tests:** files+info removed; unrelated `~/.local/share` untouched.

## 5. `journal-user`: User systemd journal, `+` safe
- **Targets:** user journal (`journalctl --user -o json` size probe → runtime dir
  under `~/.local/state/log/journal` or `/run/user/<uid>/log/journal`).
- **Action:** argv `journalctl --user --vacuum-size=<max>`; if journalctl absent →
  NotApplicable.
- **Age:** n/a (size-based). **Guard:** systemd user instance reachable.
- **Sudo:** no. **Config:** `[rules.journal-user] max_size = "100M"`.
- **Tests:** argv exactness (mock journalctl in PATH via sandbox); absent-tool detect.

## 6. `browser-cache`: Browser caches, `+` safe
- **Targets (globs):**
  - Firefox: `~/.cache/mozilla/firefox/*/cache2`, `startupCache`, `OfflineCache`
  - Chromium family: `~/.cache/{chromium,google-chrome,BraveSoftware/Brave-Browser,microsoft-edge}/*/Cache`, `Code Cache`, `GPUCache`, `Service Worker/CacheStorage`
  - Snap FF (Arch rarely): `~/snap/firefox/common/.cache/mozilla/firefox/*/cache2`
- **Action:** delete matched dirs only (never `~/.cache/mozilla/firefox/*` profiles:
  cookies/logins live there; `Service Worker` **only** `CacheStorage` subtree).
- **Guard:** browser process running (`firefox|chromium|chrome|brave|edge` comm
  match) → `Detect::Guarded{"<app> running"}`, row pre-unchecked; if user checks it
  anyway → caution-style confirm with warning.
- **Age:** skip entries modified < 5 min. **Sudo:** no. **Config:** `enabled`.
- **Tests:** guarded detect with fake `/proc` seed (proc scanner injectable);
  profile files (cookies.sqlite) survive; per-browser glob coverage.

## 7. `editor-cache`: Editor caches, `+` safe
- **Targets:** `~/.config/Code/Cache`, `Code Cache`, `GPUCache`, `CachedData`,
  `CachedExtensionVSIXs`; same under `~/.config/Cursor`, `VSCodium`, `~/.vscode-oss`;
  `~/.config/{Slack,Discord,Element}/Cache*`.
- **Action:** delete matched cache dirs only, never `User/`, `extensions/`, `globalStorage/`.
- **Guard:** app running. **Age:** 5-min hot skip. **Sudo:** no. **Config:** `enabled`.
- **Tests:** settings survive (`User/settings.json` seeded); guard behavior.

## 8. `tool-caches`: Dev tool caches, `+` safe
- **Targets & actions:**
  | Tool | Path | Action |
  |---|---|---|
  | npm | `~/.npm/_cacache` | delete |
  | pnpm | `~/.local/share/pnpm/store` | argv `pnpm store prune` if pnpm present, else delete `v3/files` only |
  | yarn | `~/.cache/yarn` | delete |
  | pip | `~/.cache/pip` | delete |
  | uv | `~/.cache/uv` | delete (archive + built-wheels; NOT `~/.local/share/uv/python`) |
  | cargo | `~/.cargo/registry/cache`, `~/.cargo/registry/.package-cache` | delete; **never** `registry/src`? → src IS regenerable but re-extract costly: **cache only** (src kept, documented) |
  | rustup | `~/.rustup/downloads`, `~/.rustup/tmp` | delete |
  | go | `$(go env GOMODCACHE)/cache/download` | delete `cache/download` only |
  | gem | `~/.cache/gem` | delete |
  | composer | `~/.cache/composer` | delete |
  | deno | `~/.cache/deno` | delete |
  | pipx | `~/.cache/pipx` | delete |
- **Action semantics:** sub-item rows under one rule (rule scans sum all present).
- **Guard:** none (caches are safe while tools idle; concurrent build worst case =
  re-download). **Age:** 5-min hot skip. **Sudo:** no.
- **Config:** `[rules.tool-caches] disable_tools = ["cargo"]`.
- **Tests:** per-tool fixture presence/absence; cargo `src/` and `bin/` survive;
  go `cache/download` only.

## 9. `app-caches`: Misc app caches, `+` safe
- **Targets:** `~/.cache/electron`, `~/.cache/mesa_shader_cache`,
  `~/.cache/fontconfig`, `~/.cache/libvirt`, `~/.cache/gdk-pixbuf`,
  `~/.cache/ibus`, `~/.cache/dconf`, `~/.cache/glib-2.0`,
  `~/.cache/{obsidian,signal,telegram-desktop}/Cache*`,
  `~/.cache/QtWebEngine` variants.
- **Action:** delete matched. fontconfig/mesa/gdk-pixbuf regenerate on next launch
  (UI note: "regenerated automatically").
- **Age:** 5-min hot skip. **Guard:** matched app running → guarded (obsidian/signal/telegram).
- **Sudo:** no. **Config:** `enabled`, `extra_paths = []` (app-specific additions).
- **Tests:** fixture matching; guarded ones respect process inject.

## 10. `log-rotate`: Rotated logs, `+` safe
- **Targets:** `/var/log/*.gz`, `/var/log/*.[0-9]`, `/var/log/*.1`,
  `/var/log/*-202[0-9]*.gz`, `/var/log/journal/*/*.journal~`;
  `~/.local/state/**/log/*.{gz,[0-9],[0-9][0-9]}`; `~/.cache/thunderbird/*/Mail/*.log` (if present).
- **Action:** delete matching files only (never the live `journal/` files, never
  current `syslog`/`Xorg.0.log`).
- **Age:** rotated suffix implies age; still skip < 1 day.
- **Guard:** none. **Sudo:** no for user paths; `/var/log` entries readable-deleted
  if permitted else **split**: user part runs in user pass, unreadable root-only
  entries are *not* silently escalated; they surface as a linked `journal-system`/
  `purge-logs` system-pass suggestion (`note` field). *(Design decision: keep
  `log-rotate` unprivileged; root rotated logs belong to system pass via a thin
  wrapper rule; see M5.)*
- **Config:** `enabled`. **Tests:** live log survives, `.gz`/`.1` removed.

## 11. `xdg-state-junk`: Crash dumps, `+` safe
- **Targets:** `~/.cache/*/Crashpad/{Reports,crashpad_metrics}*`,
  `~/.config/*/crashes`, `~/.local/share/*/Crashpad`, user coredumps
  `~/.local/state/libuuid/coredump/*` (files named `core.*`), `~/.cache/recently-used.xbel`? → **no** (user data).
- **Action:** delete crash dirs/files. Coredumps: count + size shown separately
  ("12 core dumps · 8.4 GiB"); these are **caution-flavored**: included in rule but
  shown as sub-row requiring individual check (default unchecked).
- **Guard:** none. **Age:** coredumps ≥ 1 day. **Sudo:** no. **Config:** `enabled`.
- **Tests:** crash dirs cleared; coredump sub-row default-off; unrelated config survives.

---

# Tier `!`: caution rules

## 12. `pacman-cache`: Pacman package cache, `!` caution · `sudo`
- **Targets:** `/var/cache/pacman/pkg/*.pkg.tar.*` (+ `.sig` siblings).
- **Action:** `paccache -rk{N}` with N = `keep_versions` (default **2**; `full`
  preset uses **0** unless `[rules.pacman-cache].keep_versions` set). Fallback if
  `paccache` missing (pacman-contrib not installed): NotApplicable with reason
  `"paccache missing; install pacman-contrib"` (never ad-hoc `rm` of the cache dir).
- **Age:** version-based, not time-based. **Guard:** none.
- **Sudo:** yes. System pass (readable scan unprivileged: `du` the dir).
- **Config:** `[rules.pacman-cache] keep_versions = 2`.
- **Preview:** `6.7 GiB · 310 packages · keeping newest 2 versions (≈ 140 kept)`;
  kept count computed from `pacman -Q` pairs where cheap, else estimated + marked ≈.
- **Non-Arch:** absent.
- **Tests:** argv (`-rk2`); keep_versions override; missing paccache detect;
  scan works without root.

## 13. `journal-system`: System journald, `!` caution · `sudo`
- **Targets:** `/var/log/journal` (+ `/run/log/journal` fallback).
- **Action:** `journalctl --vacuum-size=<max>` (default `100M`).
- **Guard:** journalctl present. **Sudo:** yes (system pass; scan = `journalctl --disk-usage` unprivileged when permitted, else estimate via `du` if readable).
- **Config:** `[rules.journal-system] max_size = "100M"`.
- **Preview:** `1.9 GiB → 100 MiB (vacuum)`.
- **Tests:** argv; config parse of human sizes (K/M/G/T, KiB base).

## 14. `aur-builds`: AUR build artifacts, `!` caution
- **Targets:** `~/.cache/paru/clone/*/{src,pkg,pkg.tar.*}`, `~/.cache/yay/*/{src,pkg,pkg.tar.*}`,
  `~/.cache/paru/clone/*/*.log`, plus `~/.cache/.temp` makepkg temp.
- **Kept:** the git checkouts themselves (`<repo>/.git` tree and working files
  *outside* `src/` `pkg/`): rebuild + diff safety. Also kept: `~/.cache/paru/aur.json`.
- **Action:** per-repo deletion of `src/` + `pkg/` + built `*.pkg.tar.*` leftovers.
- **Age:** ≥ 1 day (don't nuke a build in progress). **Guard:** `makepkg`/`git`
  process running in a given repo → that repo skipped.
- **Sudo:** no. **Config:** `enabled`, `keep_days`.
- **Preview:** `12 repos · 8.9 GiB (src 7.1 · pkg 1.6 · logs 0.2) · git checkouts kept`.
- **Tests:** `.git` survives; `src`/`pkg` removed; in-progress build (fresh mtime) kept.

## 15. `orphans`: Orphaned packages, `!` caution · `sudo`
- **Detection:** `pacman -Qtdq` empty → Ready with 0 (shows `0 orphans`, disabled
  selection) or NotApplicable (choose: **Ready-0**, so the row explains "none found").
- **Action:** `pacman -Rns -- <all pkgs>` in ONE invocation (batched; config
  `confirm_batch` paginates the *display* only).
- **Guard:** none. **Sudo:** yes (system pass). **Extra safety:** `--print` mode in
  preview shows the full list + total installed size; confirm copy warns
  "removing orphans also removes their unused deps".
- **Config:** `[rules.orphans] confirm_batch = 200`, `exclude = ["pkg1","pkg2"]`.
- **Non-Arch:** absent (apt: *not* auto-mapped to `autoremove` in v0.1; see
  distro matrix note).
- **Tests:** argv batch; exclude list honored; empty-orphans display state.

## 16. `flatpak-unused`: Unused Flatpak runtimes, `!` caution
- **Detection:** `flatpak` binary + any installation (`flatpak remotes`).
- **Preview:** `flatpak uninstall --unused --dry-run` output parsed → list runtimes
  + size (`flatpak list --runtime --columns=application,size` when available).
- **Action:** `flatpak uninstall --unused -y`.
- **Guard:** flatpak absent / no installations → NotApplicable.
- **Sudo:** no (user installation); system-wide installation present → needs_sudo=true for that half (split pass: user half user-pass, system half system-pass).
- **Tests:** argv; no-flatpak detect; split-pass selection logic.

## 17. `docker`: Docker reclaim, `!` caution · `sudo`*
- **Detection:** `docker` binary AND daemon reachable (`docker info` ≤ 2 s timeout).
- **Preview (list-first, mandatory):** `docker system df --format json` →
  "dangling 42 images · 18.2 GiB reclaimable; build cache 6.1 GiB". Sub-rows:
  `[x] dangling images` (checkable; checked by default when the rule is selected),
  `[ ] build cache` (unchecked by default), `[ ] stopped containers` (unchecked by
  default, lists names), `[ ] unused volumes` (unchecked by default **and** requires
  typing `volumes` at confirm; volumes = data).
- **Action:** `docker image prune -f` / `docker builder prune -f` /
  `docker container prune -f` / `docker volume prune -f` (gated); one argv per checked sub-row.
- **Guard:** daemon unreachable → Guarded {"daemon not running"}.
- **Sudo:** needs_sudo only if socket permission requires (detect via `docker info`
  failure + group membership probe; typically no).
- **Tests:** sub-row defaults; volume gate requires typed word; unreachable daemon guard;
  JSON parse of `system df`.

## 18. `podman`: Podman reclaim, `!` caution
- **Detection:** `podman` present. **Preview:** `podman system df --format json`.
- **Action:** `podman image prune -f` (+ build cache sub-row, unchecked by default);
  **never** `podman system prune --all` (would kill stopped containers by default;
  stopped-containers sub-row unchecked by default, lists names).
- **Guard:** none. **Sudo:** no (rootless); rootful storage detected → system pass.
- **Tests:** argv choices; rootless/rootful pass split.

## 19. `dev-artifacts`: Stale build artifacts, `!` caution *(the big one)*
- **Targets:** directories named in `paths`
  (`node_modules`, `target`, `.venv`, `__pycache__`, `.pytest_cache`, `.gradle`,
  `.cache/pypoetry`, `dist`? → **excluded** (may hold release builds), `build`? →
  **excluded** (too ambiguous), `elm-stuff`, `.n2`, `.bundle`) discovered in a
  **single mount-bounded walk of `$HOME`** honoring `scan.exclude_paths`.
- **Age:** directory's **recursive max mtime older than `min_age_days` (30)**,
  computed during the same walk (per-dir newest-descendant tracking, no second pass).
- **Protections:** never enters `.git/` subtrees; never descends into a second
  mount; **self-excludes `~/Projects/sweep`** (via `Ctx::exe_dir` + config);
  skips dotdirs at depth 1 except `.cache`-adjacent? → no: applies at any depth but
  skips anything under `~/`, depth-1 dirs unless named match (e.g. `~/code/target`? →
  depth irrelevant, age gate is the protection).
- **Action:** delete matched directories (whole subtree). Preview lists **top-20 by
  size** with project path + age ("`~/Projects/foo/target` · 12.4 GiB · 47d").
- **Guard:** none (age is the guard). **Sudo:** no.
- **Config:** `[rules.dev-artifacts] min_age_days = 30`, `paths = [...]`,
  `[scan] exclude_paths = [...]`.
- **Performance:** shares walker with scan; progress bar mandatory (budget ≤ 30 s SSD).
- **Tests:** fresh vs stale dirs; `.git` boundary; exclude_paths; self-exclusion;
  age = newest descendant (seed nested file mtime).

## 20. `steam-shader`: Steam shader cache, `!` caution
- **Targets:** `~/.steam/steam/appshadercache`, `~/.local/share/Steam/appshadercache`,
  `~/.local/share/Steam/shadercache/**`, `~/.steam/steam/steamapps/shadercache`.
- **Action:** delete. **Warning note:** "shaders recompile → stutter on next launch".
- **Guard:** `steam` process running → guarded. **Age:** none. **Sudo:** no.
- **Tests:** glob coverage; guard.

## 21. `snapshots-report`: btrfs/snapper report, `~` report-only
- **Detection:** root fstype == btrfs **and** (`snapper` present or `/etc/snapper/configs/*`).
- **Output (always shown in scan when applicable, never selectable):**
  "btrfs · 3 configs · 48 snapshots · ~120 GiB (size via `snapper list -s`; if
  unavailable: count-only + hint)". In `sweep clean` it renders as an info row with
  suggested manual commands. `clean()` → unreachable (`report_only=true`).
- **Sudo:** no (snapper list may run unprivileged; size hint mentions sudo).
- **Tests:** btrfs detect paths; report-only never enters Plan (assert).

---

# Tier `#`: destructive rules (`full` preset only)

All five: unlocked only under `--preset full` (or `p` → full in checklist), red
rows with `lock`, each independently gated by typed `PURGE` after a summary screen.
`--yes` does NOT bypass (D7); only `--i-know-what-im-doing` does.

## 22. `downloads`: Entire `~/Downloads`, `#` destructive
- **Targets:** everything under `~/Downloads` (recursive), top-level entries moved
  through the same protected checks (sockets impossible but symlinks: unlinked, not followed).
- **Preview (mandatory in gate screen):** total bytes, **file count**, **newest
  file age** ("newest: 2 hours ago"), oldest, top-5 sample names, and with
  `--keep-days N`: "37 files newer than N days will be preserved".
- **Action:** delete tree (or move top-level entries older than keep_days). After
  run: report survivors count when age rescue active.
- **Guard:** none (the PURGE gate IS the guard). **Sudo:** no.
- **Config:** `[rules.downloads] keep_days = 0`, `enabled` (can disable entirely).
- **Edge cases:** `~/Downloads` absent → NotApplicable; is a symlink → refuse
  (Guarded {"symlink; refusing"}); equals XDG `DOWNLOAD` dir other than default →
  still handled (resolve via `xdg-user-dirs`), path shown in preview.
- **Tests:** keep-days survivors exact; symlinked Downloads refused; gate required
  even with `--yes`; `--i-know-what-im-doing` documented bypass works.

## 23. `cache-deep`: Full `~/.cache` wipe, `#` destructive
- **Targets:** all of `~/.cache` (whitelist ignored).
- **Preview:** per-subdir count table (top 10) + grand total.
- **Action:** delete entries inside `~/.cache` (dir itself recreated).
- **Guard:** running-app guards still apply to browser/editor cache subdirs (a
  running Firefox's `cache2` is skipped even here; documented).
- **Sudo:** no. **Config:** `enabled`.
- **Tests:** whitelist ignored here but honored in `user-cache`; running-guard holds;
  fresh (<5 min) entries still skipped? → **decision: NOT skipped in cache-deep**
  (destructive = destructive, gate covers it); test asserts.

## 24. `tmp-full`: Full `/tmp` + `/var/tmp`, `#` destructive
- **Targets:** all entries incl. recent files.
- **Keeps (non-negotiable):** the shared PROTECTED list + open-fd files.
- **Action:** walker delete. **Age:** none. **Sudo:** no (root-owned leftovers
  reported as skipped with count; visible honesty).
- **Tests:** protected/open-fd survivors under full mode; root-owned skip reporting.

## 25. `pkgcache-zero`: Pacman cache → zero, `#` destructive · `sudo`
- **Action:** `paccache -rk0` (fallback `pacman -Scc --noconfirm` when paccache absent).
- **Gate extras:** warning line "updates after this re-download packages".
- **Sudo:** yes. **Config:** none (its semantics are zero).
- **Tests:** argv variants; warning text snapshot.

## 26. `purge-logs`: All logs, `#` destructive · `sudo`
- **Targets:** `/var/log/*` contents (truncate live logs, delete archives), journal
  vacuum to `0`.
- **Action:** truncate files in place (inode kept: services keep writing) rather
  than unlink where the file is a live log; delete rotated archives.
- **Sudo:** yes. **Gate extras:** shows current total + biggest 3 files.
- **Tests:** truncate-not-unlink semantics (inode stability), archive deletion.

---

# Cross-cutting behavior

## Distro matrix
| Family | Active rules | Missing (with reason surfaced by `doctor`) |
|---|---|---|
| **Arch** | all 26 | – |
| Debian | 1–11, 13 (journal), 16 if docker, 17, 19, 20, 21(btrfs), + `apt-clean` substitute* | 12,14,15,25 (pacman) |
| Fedora | same as Debian + `dnf clean` substitute* | same |
| openSUSE | same + `zypper clean` substitute* | same |
| Other | 1–11 (+16/17/18/19/20 when tools present), 21 | pkg-manager rules |

\* `apt/dnf/zypper` clean rules are **generic cache-clean entries mapped onto rule id
`pacman-cache`'s slot only in UI grouping**; actually they get their own ids
(`apt-cache-clean` etc.) implemented in `rules/pkg.rs` as family-specific siblings
with `!` (caution) keep semantics (apt: `apt clean` + `apt autoclean`; dnf: `dnf clean all`;
zypper: `zypper clean --all`). They appear ONLY on their family. This keeps ids
honest (no fake pacman on Debian).

## Rule lifecycle states (as shown by `rules list`)
`hidden` (detect NotApplicable) · `ready` · `guarded(reason)` · `checked-by-preset`
· `sudo` · `report-only` · `disabled-by-config`.

## Scan/preview honesty rules
1. Sizes shown are *reclaimable now* after age gates are applied.
2. When an external command decides the real number (paccache, docker df), the row
   shows `≈` until execution.
3. Every skipped item at execution appears in the summary with a reason; no silent skips.
4. Double-count prevention: overlapping roots (`~/.cache` vs thumbnails/browser/
   electron) are subtracted at aggregation; the JSON has a `net_of` field listing
   overlaps excluded for each rule.

## Test coverage requirement (per rule)
detect (present/absent/edge) · scan exactness (bytes+files on fixture) · age gate
boundaries (N-1, N, N+1 days) · clean survivors list · guard behavior where
declared · argv snapshot where external command · config key parsing where
declared. **No rule ships without its tests (M3/M4 acceptance gates).**
