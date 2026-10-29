# Implementation plan: harden exec/ per SAFETY

This plan is read-only as requested. All references are to files/lines in /home/pinefeast/Projects/sweep.

## 1) Findings (what exists today, constraints found)

### Current exec layout
- `src/exec/mod.rs:1-156` — defines `ExecError`, `ExitCode`, `Plan`/`PlanItem`/`RunReport`, `execute()` that takes lock for mutating runs (if not dry_run) via `state::acquire_lock`, iterates non-sudo items, calls `del::remove_items` for non-dry-run, collects errors/`has_error` → sets `ExitCode::ExecErr` if any error, runs no system pass yet (always empty), no SIGINT handling during execution.
- `src/exec/del.rs:1-127` — `is_protected_path()` (string contains on `/systemd-private-`, `/X11-unix`, `/.X11-unix`, `/.XIM-unix`, `/.ICE-unix`, endswith `.X0-lock`, contains `/X`+endswith `-lock`), `check_prefix_allowed()` allows home/tmp/var_tmp or starts with `/var/cache` or `/var/log` (note: `p.starts_with(var_tmp)` twice in the code; also var_tmp covered), `remove_items()` checks empty/root/prefix/protected, uses `fs::symlink_metadata`, deletes symlinks with `remove_file`, calls `remove_dir_bottom_up()` for dirs (does bottom-up, uses `symlink_metadata`, removes symlinks with `remove_file`, removes files with `remove_file`, calls `remove_dir` at end), frees size as `meta.size()` for dirs (from initial stat) — **does not re-stat after traversal, does not use fd-relative operations, does not do rename-then-unlink, does not check open fds / hot-skip / guards, does not re-check on execution, mount/dev checks missing, paths come from plan as-is with no propagation tracking**.
- `src/exec/sudo.rs:1-5` — stub returning `Ok(())`.
- `src/exec/trash.rs` exists (referenced). 

### Current plan/path propagation
- `src/cli/clean.rs:160-196` builds `PlanItem`s from selected checklist items but sets `paths: vec![]` (comment says “Get paths from rule scan results would be better; for now use simple paths”), builds a `Plan` with `schema: crate::SCHEMA_VERSION` (1), calls `exec::execute(&ctx, &plan)`. **So paths are not propagated from scan → plan → exec currently; execution would do nothing useful.**
- `src/ui/checklist.rs:96-106` `Selected { id, bytes, needs_sudo, guard_overridden }` — no paths stored.
- `src/scan/mod.rs:84-103` `RuleResult` has `info: ScanInfo { samples: Vec<PathBuf>, ... }` (≤20). Rules produce samples in walker; `scan/mod.rs` assembles but doesn't attach full candidate lists to results. Also `tmp.rs` passes samples via walker stats. 

### Safety model (from docs)
- SAFETY.md §3 never-touch: home/user data roots protected, system dirs protected, `/tmp` special entries (systemd-private-*, snap.mount*, X11-unix, .XIM-unix, .ICE-unix, .X*-lock), sockets/FIFOs/device nodes, open fds, symlinks never followed (unlink link itself only if parent rule applies), sweep repo/self excluded, `.git` dirs protected, volumes gated, btrfs snapshots report-only.
- SAFETY.md §6 failure/interruption: Ctrl-C during execution → finish current rule, mark rest skipped (aborted), print partial summary, exit 2. Continue-on-error (rule-level failures collected). 
- SAFETY.md §7 sudo system pass: strict plan validation (schema, uid, digest, re-run detect(), re-verify gate acks for destructive, re-validate allowed prefixes). 
- SAFETY.md §10: flock for mutating runs only; temp+rename for plans/logs; state dir 0700; argv arrays only (no sh -c); no network; scan re-checks device ids at execution (T4). 
- ARCHITECTURE.md §3 execution order, §6 deletion primitives (bottom-up, no-follow symlinks, refusing /, refusing outside allowed prefix — double-check independent of plan, on same-fs dirs uses rename to temp name first then unlinks), §4 concurrency (execution sequential).

### Current gaps relevant to the requested hardening
1. **Shared protected/prefix/mount**: protections are split — walker has `rules/tmp.rs::is_protected` (checks name/kind including sockets/FIFOs/char dev, prefixes/globs) and uses prune; `exec/del.rs` has its own string-based `is_protected_path()` and weaker prefix check. No shared module, no mount/device checks at execution, no shared protection logic between walker and deleter. 
2. **No-follow hardening**: walker sets `follow_links(false)`, counts symlinks as 0-byte files never descended (ARCH/walker). `del.rs` does `symlink_metadata` and deletes symlink files with `remove_file` (ok), but `remove_dir_bottom_up` uses `symlink_metadata` then recurses into dirs / deletes files — also ok for not following, but needs to be more robust (fd-relative operations, ensure no TOCTOU). Also need to ensure we never follow symlinks anywhere.
3. **Rename-then-unlink**: ARCH §6 says “on same-fs dirs uses `rename` to a temp name first then unlinks (best-effort atomicity)”. Current `del.rs` does direct `remove_dir` after emptying; no rename-then-unlink.
4. **Open-fd/hot-skip/guard re-check hooks**: No open-fd detection, no hot-skip (mtime-based “hot window”), no guard re-check at execution time (guards exist in concept for browsers/editors per SAFETY §5 but not wired in proc/guards). `system/proc.rs` exists? Let us look.
5. **Plan validation**: `Plan` has schema/version but no validation logic; `execute()` doesn't validate plan contents/prefixes beyond what del does. Sudo pass has full validation requirements per SAFETY §7 but not implemented.
6. **Proper paths propagation from scan to plan**: Missing entirely (clean.rs builds empty paths). Need to carry candidate paths/samples from scan results through checklist/selection to plan.
7. **Exit codes/continue-on-error/ctrl-c**: `execute()` continues on errors (collects) and returns `ExitCode::ExecErr` if any error — matches continue-on-error. But no SIGINT handling during execution (SAFETY says finish current rule, mark rest skipped aborted, exit 2). Also need proper exit code propagation.
