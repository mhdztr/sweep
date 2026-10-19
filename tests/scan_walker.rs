//! Walker integration tests (plan §4). Real fixtures on tmpfs; no env reads.

use std::os::unix::fs::MetadataExt as _;
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::time::{Duration, SystemTime};
use sweep::scan::walker::{
    walk, EntryInfo, Prune, SkipKind, WalkOptions,
};
use sweep::scan::ScanEvent;

fn pool() -> Arc<rayon::ThreadPool> {
    Arc::new(
        rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .expect("pool"),
    )
}

fn opts() -> WalkOptions {
    WalkOptions::new(pool())
}

fn seed_file(dir: &Path, rel: &str, len: u64, mtime: filetime::FileTime) -> std::path::PathBuf {
    let p = dir.join(rel);
    std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
    std::fs::write(&p, vec![0u8; len as usize]).expect("write");
    filetime::set_file_mtime(&p, mtime).expect("mtime");
    p
}

fn old() -> filetime::FileTime {
    filetime::FileTime::from_system_time(
        SystemTime::now() - Duration::from_secs(10 * 86_400),
    )
}

#[test]
fn symlink_to_file_counted_zero_bytes_target_not_followed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside");
    let target = outside.path().join("big.bin");
    std::fs::write(&target, vec![0u8; 1024 * 1024]).expect("write");
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).expect("mkdir");
    std::os::unix::fs::symlink(&target, root.join("link")).expect("symlink");

    let stats = walk(&root, &opts()).expect("walk");
    assert_eq!(stats.files, 1);
    assert_eq!(stats.bytes, 0, "symlink counts 0 bytes, target not followed");
}

#[test]
fn symlink_to_dir_not_descended() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let outside = tempfile::tempdir().expect("outside");
    std::fs::write(outside.path().join("inner.txt"), b"0123456789").expect("write");
    let root = tmp.path().join("root");
    std::fs::create_dir_all(&root).expect("mkdir");
    std::os::unix::fs::symlink(outside.path(), root.join("dirlink")).expect("symlink");

    let stats = walk(&root, &opts()).expect("walk");
    assert_eq!(stats.files, 1, "only the symlink itself is counted");
    assert_eq!(stats.bytes, 0);
    assert_eq!(stats.dirs, 1, "only the root dir; symlinked dir not descended");
}

#[test]
fn prune_predicate_removes_subtree_without_descending() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    seed_file(&root, "keep.txt", 100, old());
    // Inner "violations": if the walker descended into the pruned dir it
    // would count these files AND bump the skip count again.
    seed_file(&root, "pruned/inner.bin", 5_000, old());
    seed_file(&root, "pruned/deeper/most-inner.bin", 5_000, old());

    let mut o = opts();
    let prune: Arc<sweep::scan::walker::PruneFn> =
        Arc::new(|_p: &Path, info: &EntryInfo| {
            if info.depth == 1 && info.name == "pruned" {
                Prune::Drop(SkipKind::Protected)
            } else {
                Prune::Keep
            }
        });
    o.prune = Some(prune);
    let stats = walk(&root, &o).expect("walk");
    assert_eq!(stats.bytes, 100);
    assert_eq!(stats.files, 1);
    assert_eq!(stats.skipped.get(&SkipKind::Protected), Some(&1));
}

#[test]
fn mount_bounded_entries_pruned_by_dev() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    seed_file(&root, "a/one.bin", 10, old());
    seed_file(&root, "b/two.bin", 20, old());
    seed_file(&root, "three.bin", 30, old());

    // Injected root_dev differs from every real child ⇒ all dirs pruned at
    // depth 1; if subtrees were descended, their files would add to counts.
    let mut o = opts();
    let real = std::fs::metadata(&root).expect("meta").dev();
    o.root_dev = Some(real + 999);
    let stats = walk(&root, &o).expect("walk");
    assert_eq!(stats.dirs, 1, "only the root");
    assert_eq!(stats.skipped.get(&SkipKind::MountBound), Some(&2));
    // three.bin is a FILE — the mount check applies to dirs only, so it is
    // still counted (files at depth 1 don't lead subtrees).
    assert_eq!(stats.files, 1);
    assert_eq!(stats.bytes, 30);

    // Control: root_dev=None counts everything.
    let stats = walk(&root, &opts()).expect("control walk");
    assert_eq!(stats.dirs, 3);
    assert_eq!(stats.files, 3);
    assert_eq!(stats.bytes, 60);
}

#[test]
fn cancel_before_walk_yields_empty_partial() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    seed_file(&root, "a.bin", 10, old());
    seed_file(&root, "sub/b.bin", 10, old());

    let o = opts();
    o.cancel.store(true, Ordering::Relaxed);
    let stats = walk(&root, &o).expect("walk");
    assert!(stats.partial);
    assert_eq!(stats.bytes, 0);
    assert_eq!(stats.files, 0);
    assert_eq!(stats.dirs, 0);
}

#[test]
fn samples_capped_at_20_in_sorted_order() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    for i in 0..25 {
        seed_file(&root, &format!("f{i:02}.bin"), 1, old());
    }
    let stats = walk(&root, &opts()).expect("walk");
    assert_eq!(stats.files, 25, "all files counted");
    assert_eq!(stats.samples.len(), 20, "samples capped at max_samples");
    let names: Vec<String> = stats
        .samples
        .iter()
        .map(|p| p.file_name().expect("name").to_string_lossy().into_owned())
        .collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(names, sorted, "samples in sorted (walk) order");
    assert_eq!(names[0], "f00.bin");
}

#[test]
fn newest_mtime_tracked_across_entries() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    let now = SystemTime::now();
    let m_old = filetime::FileTime::from_system_time(now - Duration::from_secs(200));
    let m_mid = filetime::FileTime::from_system_time(now - Duration::from_secs(100));
    let m_new = filetime::FileTime::from_system_time(now - Duration::from_secs(10));
    seed_file(&root, "a.bin", 1, m_old);
    seed_file(&root, "m.bin", 1, m_mid);
    let newest_path = seed_file(&root, "n.bin", 1, m_new);
    // The root dir is a counted entry too — pin its mtime old so the three
    // files decide `newest`.
    filetime::set_file_mtime(&root, m_old).expect("mtime");

    // StatInfo stores mtime as whole unix seconds, so compare truncated.
    let sec = |t: SystemTime| -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(t.duration_since(SystemTime::UNIX_EPOCH).expect("epoch").as_secs())
    };

    // No gate: newest == max mtime (truncated to seconds).
    let stats = walk(&root, &opts()).expect("walk");
    assert_eq!(stats.newest, Some(sec(now - Duration::from_secs(10))));

    // With a gate excluding the newest entry: newest drops to the mid mtime
    // (gated entries never influence newest — plan settled decision 4).
    let mut o = opts();
    let gate: Arc<sweep::scan::walker::GateFn> = Arc::new(move |p: &Path, _i: &EntryInfo| {
        if p.ends_with("n.bin") {
            Err(SkipKind::KeptNewer)
        } else {
            Ok(())
        }
    });
    o.gate = Some(gate);
    let stats = walk(&root, &o).expect("walk");
    assert_eq!(stats.newest, Some(sec(now - Duration::from_secs(100))));
    assert_eq!(stats.skipped.get(&SkipKind::KeptNewer), Some(&1));
    assert!(!stats.samples.contains(&newest_path));
}

#[test]
fn progress_events_emitted_every_256_dirs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    for i in 0..300 {
        std::fs::create_dir_all(root.join(format!("d{i:03}"))).expect("mkdir");
    }
    let (tx, rx) = mpsc::channel::<ScanEvent>();
    let mut o = opts();
    o.progress = Some(tx);
    let stats = walk(&root, &o).expect("walk");
    assert_eq!(stats.dirs, 301);
    let walks = rx.try_iter().filter(|e| matches!(e, ScanEvent::Walk { .. })).count();
    assert!(walks >= 1, "expected at least one Walk event, got {walks}");
}

#[test]
fn missing_root_returns_missing_error() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let err = walk(&tmp.path().join("nope"), &opts()).expect_err("missing");
    assert!(matches!(err, sweep::scan::ScanError::Missing(_)));
}

#[test]
fn errors_on_unreadable_entry_do_not_fail_walk() {
    // Root can bypass permissions; nothing to observe there.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("root");
    seed_file(&root, "ok.bin", 10, old());
    let secret = root.join("secret");
    std::fs::create_dir_all(&secret).expect("mkdir");
    std::fs::write(secret.join("hidden.bin"), vec![0u8; 100]).expect("write");
    std::fs::set_permissions(&secret, std::os::unix::fs::PermissionsExt::from_mode(0o000))
        .expect("chmod");

    let o = opts();
    let stats = walk(&root, &o).expect("walk survives per-entry errors");
    assert_eq!(stats.files, 1, "readable file still counted");
    assert!(stats.errors >= 1, "unreadable subtree recorded as error");

    std::fs::set_permissions(&secret, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .expect("chmod back");
}
