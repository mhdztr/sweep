//! Rule-level scan tests (plan §4 `tests/scan_rules.rs`), exact numbers from
//! the canonical fixture table. `Ctx::for_test` homes only; no process env.

mod common;

use common::{scan_fixture, seed};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};
use sweep::ctx::Ctx;
use sweep::rules::cache::{ThumbnailsRule, UserCacheRule};
use sweep::rules::tmp::TmpRule;
use sweep::rules::{Detect, Rule};
use sweep::scan::ScanService;
use sweep::scan::walker::SkipKind;

/// Age-relative file time: `secs` before now.
fn ft_age(secs: u64) -> filetime::FileTime {
    filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(secs))
}

fn ctx_for(home: &Path) -> Ctx {
    Ctx::for_test(home.to_path_buf())
}

fn svc() -> ScanService {
    ScanService {
        cancel: Arc::new(AtomicBool::new(false)),
        events: None,
        pool: Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(2)
                .build()
                .expect("pool"),
        ),
        now: SystemTime::now(),
    }
}

/// Create a FIFO at `path`; panics on failure.
fn mkfifo(path: &Path) {
    use std::os::unix::ffi::OsStrExt as _;
    let c = std::ffi::CString::new(path.as_os_str().as_bytes()).expect("nul-free path");
    // SAFETY: `c` is a valid NUL-terminated path string; mode 0o644 is within
    // the process's umask. mkfifo creates exactly one new filesystem node.
    let rc = unsafe { libc::mkfifo(c.as_ptr(), 0o644) };
    assert_eq!(rc, 0, "mkfifo {:?}", std::io::Error::last_os_error());
}

// --------------------------------------------------------------------------
// tmp rule
// --------------------------------------------------------------------------

#[test]
fn tmp_protected_survive_dirs_sockets_fifos_and_lockfiles() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    let root = home.join(".fake-tmp");
    // 5 top-level protected entries: prefixed dir, exact-name dir, socket,
    // fifo (special kind), glob lockfile.
    std::fs::create_dir_all(root.join("systemd-private-x/run")).expect("mkdir");
    std::fs::write(root.join("systemd-private-x/run/file"), vec![0u8; 999]).expect("write");
    std::fs::create_dir_all(root.join("X11-unix")).expect("mkdir");
    let _sock = std::os::unix::net::UnixListener::bind(root.join("dbus-123")).expect("bind socket");
    mkfifo(&root.join("plain.fifo"));
    std::fs::write(root.join(".X123-lock"), b"lock").expect("write");

    let scan = TmpRule.scan(&ctx_for(home), &svc()).expect("scan");
    assert_eq!(scan.info.bytes, 0, "protected entries contribute 0 bytes");
    assert_eq!(scan.info.files, 0, "protected entries contribute 0 files");
    assert_eq!(
        scan.skipped.get(&SkipKind::Protected),
        Some(&5),
        "exactly the 5 top-level protected entries; subtree not descended"
    );
    assert_eq!(scan.info.dirs, 1, "only the .fake-tmp root itself");
}

#[test]
fn tmp_age_gate_excludes_n_minus_1_day() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    // keep_days = 1 ⇒ cutoff = now − 86400 s; 10 s younger ⇒ excluded (D-9).
    seed(home, ".fake-tmp/gate.bin", 100, ft_age(86_400 - 10));

    let scan = TmpRule.scan(&ctx_for(home), &svc()).expect("scan");
    assert_eq!(scan.info.files, 0);
    assert_eq!(scan.info.bytes, 0);
    assert_eq!(scan.skipped.get(&SkipKind::KeptNewer), Some(&1));
}

#[test]
fn tmp_age_gate_counts_n_days() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    // Exactly N·86400 ± 10 s ⇒ counted (mtime <= cutoff).
    seed(home, ".fake-tmp/gate.bin", 100, ft_age(86_400 + 10));

    let scan = TmpRule.scan(&ctx_for(home), &svc()).expect("scan");
    assert_eq!(scan.info.files, 1);
    assert_eq!(scan.info.bytes, 100);
    assert!(
        !scan.skipped.contains_key(&SkipKind::KeptNewer),
        "counted entries are not skipped"
    );
}

#[test]
fn tmp_age_gate_counts_n_plus_1_day() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    seed(home, ".fake-tmp/gate.bin", 100, ft_age(172_800 + 10));

    let scan = TmpRule.scan(&ctx_for(home), &svc()).expect("scan");
    assert_eq!(scan.info.files, 1);
    assert_eq!(scan.info.bytes, 100);
}

#[test]
fn tmp_keep_days_zero_counts_everything() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    seed(home, ".fake-tmp/fresh.bin", 100, ft_age(0));
    seed(home, ".fake-tmp/any.bin", 50, ft_age(10 * 86_400));
    let mut ctx = ctx_for(home);
    ctx.flags.keep_days = Some(0); // gate off

    let scan = TmpRule.scan(&ctx, &svc()).expect("scan");
    assert_eq!(scan.info.files, 2, "keep_days=0 counts even fresh files");
    assert_eq!(scan.info.bytes, 150);
    assert!(!scan.skipped.contains_key(&SkipKind::KeptNewer));
}

#[test]
fn tmp_special_files_contribute_zero_bytes_and_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    let root = home.join(".fake-tmp");
    std::fs::create_dir_all(&root).expect("mkdir");
    // ORDINARY names, only the special-kind check protects them.
    let _sock = std::os::unix::net::UnixListener::bind(root.join("plain.sock")).expect("socket");
    mkfifo(&root.join("plain.fifo"));

    let scan = TmpRule.scan(&ctx_for(home), &svc()).expect("scan");
    assert_eq!(scan.info.bytes, 0, "sockets/fifos are 0 bytes");
    assert_eq!(scan.info.files, 0, "sockets/fifos are pruned, not counted");
    assert_eq!(scan.skipped.get(&SkipKind::Protected), Some(&2));
}

// --------------------------------------------------------------------------
// user-cache / thumbnails / trash
// --------------------------------------------------------------------------

#[test]
fn user_cache_whitelist_excluded() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    scan_fixture(home);
    let mut ctx = ctx_for(home);
    ctx.config.rules.user_cache.keep = vec!["app-cache".to_string()];

    let scan = UserCacheRule.scan(&ctx, &svc()).expect("scan");
    // app-cache (10 024 B) pruned; thumbnails (5 120 B) + hot-fresh-entry
    // (500 B) pruned ⇒ remaining: thumbnails only.
    assert_eq!(scan.info.bytes, 5_120);
    assert_eq!(scan.info.files, 2);
    assert_eq!(scan.info.dirs, 2, ".cache + thumbnails");
    assert_eq!(scan.skipped.get(&SkipKind::Whitelist), Some(&1));
}

#[test]
fn user_cache_hot_entry_excluded() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    scan_fixture(home);

    let scan = UserCacheRule.scan(&ctx_for(home), &svc()).expect("scan");
    assert_eq!(
        scan.info.bytes, 15_144,
        "10 000 + 24 + 4 096 + 1 024; fresh-entry (500 B) hot-pruned"
    );
    assert_eq!(scan.info.files, 4);
    assert_eq!(scan.info.dirs, 3, ".cache + app-cache + thumbnails");
    assert_eq!(scan.skipped.get(&SkipKind::Hot), Some(&1));
    assert!(
        !scan
            .info
            .samples
            .iter()
            .any(|p| p.ends_with("fresh-entry") || p.ends_with("hot.txt")),
        "hot entry never appears in samples"
    );
}

#[test]
fn thumbnails_exact_bytes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    scan_fixture(home);

    let scan = ThumbnailsRule.scan(&ctx_for(home), &svc()).expect("scan");
    assert_eq!(scan.info.bytes, 5_120, "4 096 + 1 024");
    assert_eq!(scan.info.files, 2);
    assert_eq!(scan.info.dirs, 1, "the thumbnails root itself");
    assert!(scan.skipped.is_empty(), "plain walk, no pruning");
}

#[test]
fn trash_counts_files_and_info_dirs_only() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    scan_fixture(home);
    // Sibling of Trash/, outside both roots, contributes nothing.
    seed(
        home,
        ".local/share/not-trash/sibling.bin",
        4_096,
        ft_age(10 * 86_400),
    );

    let scan = sweep::rules::trash::TrashRule
        .scan(&ctx_for(home), &svc())
        .expect("scan");
    assert_eq!(scan.info.bytes, 2_176, "2 048 + 128; sibling adds 0");
    assert_eq!(scan.info.files, 2);
    assert_eq!(scan.info.dirs, 2, "files/ root + info/ root");
}

// --------------------------------------------------------------------------
// full pipeline (detect + subtraction + totals) and detect reasons
// --------------------------------------------------------------------------

#[test]
fn user_cache_subtracts_thumbnails_no_double_count() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    scan_fixture(home);
    let ctx = ctx_for(home);

    let out = sweep::scan::run(&ctx, Arc::new(AtomicBool::new(false)), None);
    let get = |id: &str| {
        out.results
            .iter()
            .find(|r| r.id == id)
            .unwrap_or_else(|| panic!("rule {id} present"))
    };

    let t = get("tmp");
    assert!(t.applied);
    assert_eq!(t.info.bytes, 3_282, "1 234 + 2 048; newer.bin excluded");
    assert_eq!(t.info.files, 2);
    assert_eq!(t.info.dirs, 2, "both fake tmp roots counted");
    assert_eq!(t.info.note.as_deref(), Some("1 newer than keep-days kept"));

    let th = get("thumbnails");
    assert_eq!(th.info.bytes, 5_120);
    assert_eq!(th.info.files, 2);
    assert_eq!(th.info.dirs, 1);

    let uc = get("user-cache");
    assert_eq!(
        uc.info.bytes, 10_024,
        "15 144 gross minus thumbnails 5 120: counted exactly once"
    );
    assert_eq!(uc.info.files, 2, "4 − 2");
    assert_eq!(uc.info.dirs, 2, "3 − 1");
    assert_eq!(uc.net_of, vec!["thumbnails"]);
    assert_eq!(uc.info.note.as_deref(), Some("(excl. thumbnails)"));

    let tr = get("trash");
    assert_eq!(tr.info.bytes, 2_176);
    assert_eq!(tr.info.files, 2);
    assert_eq!(tr.info.dirs, 2);

    // 3 282 + 5 120 + 10 024 + 2 176 = 20 602
    assert_eq!(out.totals.reclaimable_bytes, 20_602);
    assert_eq!(
        out.totals.regular_bytes, 20_602,
        "all four M1 rules are Safe tier"
    );
}

#[test]
fn detect_not_applicable_missing_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path(); // empty: no .cache
    let ctx = ctx_for(home);

    let out = sweep::scan::run(&ctx, Arc::new(AtomicBool::new(false)), None);
    let uc = out
        .results
        .iter()
        .find(|r| r.id == "user-cache")
        .expect("user-cache present");
    assert!(!uc.applied);
    assert_eq!(uc.info.bytes, 0, "not-applied rules report zeros");
    assert_eq!(uc.reason.as_deref(), Some("~/.cache not found"));
}

#[test]
fn detect_disabled_in_config() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    scan_fixture(home); // .cache exists, disabled wins over found
    let mut ctx = ctx_for(home);
    ctx.config.rules.user_cache.enabled = false;

    let out = sweep::scan::run(&ctx, Arc::new(AtomicBool::new(false)), None);
    let uc = out
        .results
        .iter()
        .find(|r| r.id == "user-cache")
        .expect("user-cache present");
    assert!(!uc.applied);
    assert_eq!(uc.reason.as_deref(), Some("disabled in config"));

    // And detect() itself agrees (reasons are copy-locked).
    assert!(matches!(
        UserCacheRule.detect(&ctx),
        Detect::NotApplicable { ref reason } if reason == "disabled in config"
    ));
}

#[test]
fn thumbnails_detect_reason_names_the_thumbnails_dir() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    // `~/.cache` EXISTS but the rule's ROOT `~/.cache/thumbnails` does not;
    // the reason must name the missing root, not its present parent (critic
    // finding: "~/.cache not found" was factually wrong here).
    std::fs::create_dir_all(home.join(".cache")).expect("mkdir .cache");
    let ctx = ctx_for(home);

    assert!(matches!(
        ThumbnailsRule.detect(&ctx),
        Detect::NotApplicable { ref reason } if reason == "~/.cache/thumbnails not found"
    ));

    // The copy the table/JSON shows comes from the same detect() reason.
    let out = sweep::scan::run(&ctx, Arc::new(AtomicBool::new(false)), None);
    let th = out
        .results
        .iter()
        .find(|r| r.id == "thumbnails")
        .expect("thumbnails row present");
    assert!(!th.applied);
    assert_eq!(th.reason.as_deref(), Some("~/.cache/thumbnails not found"));
}

#[test]
fn tmp_location_is_the_primary_root_through_display_path() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_for(tmp.path());

    // Plan §2.10 "location `/tmp`" + Rule trait "display_path": ONE primary
    // root, rendered relative to home in a sandbox, never the joined
    // non-spec "~/.fake-tmp, ~/.fake-var-tmp" string the goldens used to
    // pin (critic blocker 1). On a real host the same call yields "/tmp".
    assert_eq!(TmpRule.location(&ctx), "~/.fake-tmp");
    // Same convention as the other rules.
    assert_eq!(UserCacheRule.location(&ctx), "~/.cache");
    assert_eq!(ThumbnailsRule.location(&ctx), "~/.cache");
}
