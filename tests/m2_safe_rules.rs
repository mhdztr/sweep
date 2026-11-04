use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use std::collections::BTreeMap;
use sweep::cli::Preset;
use sweep::ctx::Ctx;
use sweep::rules::{
    Rule, app_caches::AppCachesRule, browser_cache::BrowserCacheRule,
    editor_cache::EditorCacheRule, journal_user::JournalUserRule, log_rotate::LogRotateRule,
    tool_caches::ToolCachesRule, xdg_state::XdgStateJunkRule,
};
use sweep::scan::{self, ScanService};
use sweep::ui::checklist::Selection;

fn service() -> ScanService {
    ScanService {
        cancel: Arc::new(AtomicBool::new(false)),
        events: None,
        pool: Arc::new(
            rayon::ThreadPoolBuilder::new()
                .num_threads(1)
                .build()
                .expect("pool"),
        ),
        now: SystemTime::now(),
        capture_candidates: true,
    }
}

fn seed(home: &Path, relative: &str, bytes: usize) -> PathBuf {
    let path = home.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("parent dirs");
    std::fs::write(&path, vec![b'x'; bytes]).expect("seed");
    filetime::set_file_mtime(
        &path,
        filetime::FileTime::from_system_time(SystemTime::now() - Duration::from_secs(3600)),
    )
    .expect("old mtime");
    path
}

fn set_age(path: &Path, age: Duration) {
    filetime::set_file_mtime(
        path,
        filetime::FileTime::from_system_time(SystemTime::now() - age),
    )
    .expect("set mtime");
}

#[test]
fn browser_cache_scans_only_cache_subtrees_not_profile_data() {
    let temp = tempfile::tempdir().expect("home");
    let home = temp.path();
    let cache = seed(
        home,
        ".cache/mozilla/firefox/abc.default/cache2/entries/cache.bin",
        1024,
    );
    seed(
        home,
        ".cache/mozilla/firefox/abc.default/cookies.sqlite",
        2048,
    );
    let ctx = Ctx::for_test(home.to_path_buf());
    let result = BrowserCacheRule.scan(&ctx, &service()).expect("scan");
    assert_eq!(result.info.bytes, 1024);
    assert_eq!(result.info.candidates, vec![cache]);
}

#[test]
fn editor_cache_preserves_settings_and_extensions() {
    let temp = tempfile::tempdir().expect("home");
    let home = temp.path();
    let cache = seed(home, ".config/Code/Cache/data.bin", 512);
    seed(home, ".config/Code/User/settings.json", 2048);
    seed(home, ".config/Code/extensions/example/package.json", 1024);
    let ctx = Ctx::for_test(home.to_path_buf());
    let result = EditorCacheRule.scan(&ctx, &service()).expect("scan");
    assert_eq!(result.info.bytes, 512);
    assert_eq!(result.info.candidates, vec![cache]);
}

#[test]
fn tool_cache_targets_leave_cargo_sources_untouched() {
    let temp = tempfile::tempdir().expect("home");
    let home = temp.path();
    let cache = seed(home, ".cargo/registry/cache/example.crate", 300);
    seed(home, ".cargo/registry/src/example/lib.rs", 900);
    let ctx = Ctx::for_test(home.to_path_buf());
    let result = ToolCachesRule.scan(&ctx, &service()).expect("scan");
    assert_eq!(result.info.bytes, 300);
    assert_eq!(result.info.candidates, vec![cache]);
}

#[test]
fn app_cache_supports_only_home_cache_additions() {
    let temp = tempfile::tempdir().expect("home");
    let home = temp.path();
    let cache = seed(home, ".cache/custom/app-cache/data.bin", 400);
    let denied = seed(home, "Documents/keep.txt", 800);
    let mut ctx = Ctx::for_test(home.to_path_buf());
    ctx.config.rules.app_caches.extra_paths = vec![
        "~/.cache/custom/app-cache".into(),
        "~/.cache".into(),
        denied.display().to_string(),
    ];
    let result = AppCachesRule.scan(&ctx, &service()).expect("scan");
    assert_eq!(result.info.bytes, 400);
    assert_eq!(result.info.candidates, vec![cache]);
}

#[test]
fn log_rotate_keeps_live_logs_and_requires_one_day_age() {
    let temp = tempfile::tempdir().expect("home");
    let home = temp.path();
    let rotated = seed(home, ".fake-var-log/syslog.1", 300);
    seed(home, ".fake-var-log/syslog", 500);
    let journal = seed(home, ".fake-var-log/journal/machine/system.journal~", 200);
    let user = seed(home, ".local/state/app/log/app.2", 100);
    for path in [&rotated, &journal, &user] {
        set_age(path, Duration::from_secs(86_400 * 2));
    }
    let mut ctx = Ctx::for_test(home.to_path_buf());
    ctx.var_log_root = home.join(".fake-var-log");
    let result = LogRotateRule.scan(&ctx, &service()).expect("scan");
    assert_eq!(result.info.bytes, 600);
    let mut expected = vec![rotated, journal, user];
    expected.sort();
    assert_eq!(result.info.candidates, expected);
}

#[test]
fn coredump_subrow_is_scannable_but_unselected_by_default() {
    let temp = tempfile::tempdir().expect("home");
    let home = temp.path();
    let dump = seed(home, ".local/state/libuuid/coredump/core.123", 900);
    set_age(&dump, Duration::from_secs(86_400 * 2));
    let ctx = Ctx::for_test(home.to_path_buf());
    let out = scan::run_with_candidates(&ctx, Arc::new(AtomicBool::new(false)), None, true);
    let row = out
        .results
        .iter()
        .find(|result| result.id == "xdg-state-coredumps")
        .expect("coredump row");
    assert!(row.applied);
    assert_eq!(row.info.candidates, vec![dump]);

    let selection = Selection::new(
        &out.results,
        Preset::Regular,
        false,
        false,
        &BTreeMap::new(),
    );
    assert!(
        selection
            .selected()
            .iter()
            .all(|selected| selected.id != "xdg-state-coredumps")
    );
}

#[test]
fn non_cache_rotated_logs_do_not_reduce_user_cache_totals() {
    let temp = tempfile::tempdir().expect("home");
    let home = temp.path();
    let cache = seed(home, ".cache/ordinary/cache.bin", 100);
    set_age(
        cache.parent().expect("cache directory"),
        Duration::from_secs(3600),
    );
    seed(home, ".cargo/registry/cache/example.crate", 4_000);
    let rotated = seed(home, ".fake-var-log/large-archive.1", 3_000);
    set_age(&rotated, Duration::from_secs(86_400 * 2));
    let mut ctx = Ctx::for_test(home.to_path_buf());
    ctx.var_log_root = home.join(".fake-var-log");

    let out = scan::run(&ctx, Arc::new(AtomicBool::new(false)), None);
    let user_cache = out
        .results
        .iter()
        .find(|result| result.id == "user-cache")
        .expect("user cache result");

    assert_eq!(user_cache.info.bytes, 100);
    assert!(!user_cache.net_of.contains(&"log-rotate"));
    assert!(!user_cache.net_of.contains(&"tool-caches"));
}

#[test]
fn xdg_crash_scan_excludes_user_coredumps() {
    let temp = tempfile::tempdir().expect("home");
    let home = temp.path();
    let report = seed(home, ".cache/example/Crashpad/Reports/report.dmp", 700);
    seed(home, ".local/state/libuuid/coredump/core.123", 900);
    let ctx = Ctx::for_test(home.to_path_buf());
    let result = XdgStateJunkRule.scan(&ctx, &service()).expect("scan");
    assert_eq!(result.info.bytes, 700);
    assert_eq!(result.info.candidates, vec![report]);
}

#[test]
fn journal_user_scan_supports_both_runtime_and_persistent_roots() {
    let temp = tempfile::tempdir().expect("home");
    let home = temp.path();
    seed(home, ".local/state/log/journal/user.journal", 100);
    seed(home, ".fake-run/log/journal/runtime.journal", 200);
    let ctx = Ctx::for_test(home.to_path_buf());
    let result = JournalUserRule.scan(&ctx, &service()).expect("scan");
    assert_eq!(result.info.bytes, 300);
    assert_eq!(result.info.files, 2);
}
