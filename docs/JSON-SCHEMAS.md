# sweep: JSON Schemas (v1)

> Spec of record: `docs/ARCHITECTURE.md` §11 (state/history) + §12 (schemas).

## 11. State & data files

| Path (XDG) | File | Format | Retention |
|---|---|---|---|
| state | `~/.local/state/sweep/history.jsonl` | JSONL per run | `history_keep_runs` |
| state | `~/.local/state/sweep/logs/<run_id>.log` | plain text audit log | last 30 logs pruned |
| state | `~/.local/state/sweep/sweep.lock` | flock | n/a |
| state/tmp | `system-pass-<run_id>.json` | plan for sudo re-exec | deleted after pass |
| config | `~/.config/sweep/config.toml` | toml | user-owned |
| cache | *(none in v0.1: no persistent size cache; sizes always fresh)* | — | — |
| trash | `~/.local/share/Trash/{files,info}` | XDG trash | user-managed |

**History record**: one JSON object per line in `history.jsonl`, appended after
every clean run (schema v1):

```jsonc
{"v":1,"ts":"2026-10-19T14:03:11Z","preset":"regular","mode":"interactive",
 "items":[{"id":"tmp","freed":1288490188,"removed":4100,
           "skipped":["browser-cache:firefox running"]}],
 "freed_total":60123456789,"duration_ms":9211,"dry_run":false,"trash":false}
```

**Audit log line schema** (see SAFETY §9), every executed command and top-level
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

