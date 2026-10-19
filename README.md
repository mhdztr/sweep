# sweep

**Advanced, safety-first storage cleaner for Linux — Arch-first, distro-aware everywhere.**

`sweep` scans your system for reclaimable space, lets you toggle cleanup categories
in a color-coded checklist, and cleans them behind explicit safety gates. Regular
cleanup is a two-key affair; the *full purge* (everything, including your entire
`~/Downloads`) demands you type `PURGE` per category — on purpose.

> **Status: pre-alpha (v0.1 in development).** See [ROADMAP.md](ROADMAP.md) for
> what works today. Core commands land milestone by milestone.

## Why

Six wiki pages (`paccache`, `journalctl --vacuum`, `docker system df`, AUR cache
locations, orphan removal, browser caches) collapse into one checklist with:

- **Tiers of danger** — 🟩 safe defaults · 🟨 opt-in cautions · 🟥 gated destruction
- **Nothing blind** — scan first, sizes + counts + newest-file age before any delete
- **Honest skips** — running Firefox, live sockets, open files: reported, never silently touched
- **Scriptable** — `--json`, stable exit codes, `--dry-run`, presets for automation

## Quickstart

```sh
sweep scan                 # read-only: what's reclaimable?
sweep clean                # interactive checklist (default flow)
sweep clean --preset safe --yes          # scripted, safe tier only
sweep clean --preset full --dry-run      # preview the deep purge, touch nothing
```

## Presets

| Preset | What's selected |
|---|---|
| `safe` | only always-safe categories (tmp, caches, trash, thumbnails…) |
| `regular` *(default)* | safe + your config includes; cautions opt-in |
| `full` | everything; destructive categories unlock behind typed `PURGE` |

## Safety invariants (non-negotiable)

1. `scan` and `--dry-run` never modify anything (CI-enforced).
2. Destructive rules only exist under `full` and require typed `PURGE` per category —
   `--yes` does **not** bypass them.
3. Hardcoded never-touch list: your Documents/Pictures/Videos/Projects, `/etc`,
   `/usr`, live sockets, `systemd-private-*`, open files, `.git` histories.
4. Sudo happens once, batched, after user-level cleanup — or not at all (decline = skip + report).

Full model: [docs/SAFETY.md](docs/SAFETY.md).

## Commands

`scan` · `clean` · `top` · `history` · `doctor` · `rules list` ·
`config init|path|edit` · `completion <shell>` — details in [docs/CLI-UX.md](docs/CLI-UX.md).

## Install (once v0.1 ships)

```sh
# Arch (AUR)
yay -S sweep-bin
# or from source
cargo install --path .
```

## Documentation

| Doc | What |
|---|---|
| [ROADMAP.md](ROADMAP.md) | build milestones + status |
| [CONTEXT.md](CONTEXT.md) | decisions, conventions, resume playbook |
| [docs/MASTER-PLAN.md](docs/MASTER-PLAN.md) | product scope, stories, metrics |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | modules, types, config, schemas |
| [docs/RULES-CATALOG.md](docs/RULES-CATALOG.md) | all 26 rules, spec of record |
| [docs/CLI-UX.md](docs/CLI-UX.md) | commands, prompts, exit codes |
| [docs/SAFETY.md](docs/SAFETY.md) | threat model, gates, sudo protocol |

## License

MIT — see [LICENSE](LICENSE).
