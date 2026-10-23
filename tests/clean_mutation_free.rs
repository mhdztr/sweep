//! Safety proof for `sweep clean` wiring (M2, R13): abort and the preview path
//! mutate nothing.
//!
//! Two layers:
//! 1. library-level, scripted abort through `run_interactive_with_source`
//!    (deterministic, no tty), mirroring `tests/scan_mutation_free.rs`;
//! 2. real-PTY abort through the production `run_interactive` (termios raw mode,
//!    the real `poll`-bounded reader), reusing the `openpty`/fork harness shape
//!    from `tests/checklist_pty.rs`.
//!
//! A full end-to-end `assert_cmd` test of `sweep clean` over a PTY is not
//! feasible with the current dev-dependencies (`assert_cmd` gives pipes, not a
//! pty). The binary's no-TTY arg/dispatch path is covered by `tests/cli_smoke.rs`
//! and the loop is driven in-process here.
//!
//! Spec: docs/CLI-UX.md section 3.2/section 6; gates: docs/SAFETY.md section 2 (abort deletes
//! nothing), SAFETY section 10 (no lock for a non-mutating run).

#![cfg(unix)]

mod common;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use sweep::ctx::Ctx;
use sweep::ui::Theme;
use sweep::ui::checklist::{
    Outcome, Selection, Source, run_interactive, run_interactive_with_source,
};
use sweep::ui::render::FrameOpts;
use sweep::ui::tty::Key;

fn plain() -> Theme {
    Theme {
        color: false,
        unicode: false,
    }
}

fn selection_for(home: &std::path::Path) -> Selection {
    let ctx = Ctx::for_test(home.to_path_buf());
    let out = sweep::scan::run(&ctx, Arc::new(AtomicBool::new(false)), None);
    let guards: BTreeMap<&str, String> = BTreeMap::new();
    Selection::new(&out.results, ctx.effective_preset(), false, false, &guards)
}

#[test]
fn clean_abort_leaves_fixture_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    common::scan_fixture(dir.path());
    let state = sweep::state::state_dir(dir.path());
    let before = common::manifest(dir.path());

    let mut sel = selection_for(dir.path());
    let outcome =
        run_interactive_with_source(&mut sel, &plain(), |_: &Selection, _: &FrameOpts| {
            Ok(Source::Key(Key::Abort))
        })
        .expect("scripted loop must not fail");
    assert_eq!(outcome, Outcome::Aborted);

    assert_eq!(
        before,
        common::manifest(dir.path()),
        "abort must not create/modify/delete anything under the home"
    );
    assert!(!state.exists(), "clean must not create the state dir");
}

/// Open a pty pair `(master, slave)` via `posix_openpt(3)`.
fn openpty() -> (OwnedFd, OwnedFd) {
    // SAFETY: the `posix_openpt`/`grantpt`/`unlockpt`/`ptsname`/`open` sequence
    // is the documented libc path to a pty pair. `ptsname` returns a NUL
    // terminated string owned by libc, read before any further libc call that
    // could overwrite it; `open` gets a fresh fd we own.
    unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(master >= 0, "posix_openpt failed");
        assert_eq!(libc::grantpt(master), 0, "grantpt failed");
        assert_eq!(libc::unlockpt(master), 0, "unlockpt failed");
        let name = libc::ptsname(master);
        assert!(!name.is_null(), "ptsname failed");
        let slave = libc::open(name, libc::O_RDWR | libc::O_NOCTTY);
        assert!(slave >= 0, "open(slave) failed");
        (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave))
    }
}

/// Run the production `run_interactive` on a pty in a forked child, send `q`,
/// and return the child's report marker line.
fn run_pty_abort(mut sel: Selection, theme: Theme) -> String {
    let (master, slave) = openpty();
    let slave_raw = std::os::fd::AsRawFd::as_raw_fd(&slave);
    let master_raw = std::os::fd::AsRawFd::as_raw_fd(&master);

    // SAFETY: `fork` in a test binary. The child always `_exit`s through
    // `std::process::exit`, so no destructors run twice and no libtest state is
    // corrupted. The scan ran in the parent (its threads are joined), so the
    // child only runs the event loop.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork failed");

    if pid == 0 {
        // Child: the slave becomes stdin/stdout/stderr, so `isatty` is true and
        // termios raw mode can be set on stdin.
        // SAFETY: each `dup2` targets fd 0/1/2 and cannot fail for a valid fd.
        unsafe {
            libc::dup2(slave_raw, libc::STDIN_FILENO);
            libc::dup2(slave_raw, libc::STDOUT_FILENO);
            libc::dup2(slave_raw, libc::STDERR_FILENO);
            libc::setsid();
            libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY as _, 0);
        }
        drop(master);
        drop(slave);

        let line = match run_interactive(&mut sel, &theme) {
            Ok(Outcome::Aborted) => "ABORTED\n".to_string(),
            Ok(Outcome::Accepted(items)) => format!("ACCEPTED n={}\n", items.len()),
            Err(e) => format!("ERROR {e}\n"),
        };
        let mut out = std::io::stdout();
        let _ = out.write_all(line.as_bytes());
        let _ = out.flush();
        std::process::exit(0);
    }

    // Parent: write `q`, then read the child's output.
    drop(slave);
    // A non-blocking master so a broken child cannot hang the test.
    // SAFETY: `master_raw` is a valid fd we own; F_GETFL takes no argument and
    // F_SETFL applies the int derived below. No memory is touched.
    unsafe {
        let flags = libc::fcntl(master_raw, libc::F_GETFL);
        libc::fcntl(master_raw, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
    let mut master_file = std::fs::File::from(master);
    std::thread::sleep(Duration::from_millis(150));
    let _ = master_file.write_all(b"q");
    let _ = master_file.flush();

    let mut captured = String::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut buf = [0u8; 4096];
    loop {
        if Instant::now() > deadline {
            break;
        }
        match master_file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                captured.push_str(&String::from_utf8_lossy(&buf[..n]));
                if captured.contains("ABORTED")
                    || captured.contains("ACCEPTED")
                    || captured.contains("ERROR")
                {
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => break,
        }
    }

    // SAFETY: `waitpid` on the child we forked; it reaps the zombie.
    let mut status = 0;
    unsafe { libc::waitpid(pid, &mut status, 0) };

    marker_line(&captured)
}

fn marker_line(captured: &str) -> String {
    let clean = strip_ansi(captured);
    for marker in ["ABORTED", "ACCEPTED", "ERROR"] {
        if let Some(idx) = clean.find(marker) {
            return clean[idx..].lines().next().unwrap_or("").trim().to_string();
        }
    }
    String::new()
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.peek() == Some(&'[') {
                it.next();
                for n in it.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

#[test]
fn clean_pty_abort_leaves_fixture_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    common::scan_fixture(dir.path());
    let state = sweep::state::state_dir(dir.path());
    let sel = selection_for(dir.path());
    let before = common::manifest(dir.path());

    let report = run_pty_abort(sel, plain());
    assert_eq!(report, "ABORTED", "q must abort on the real pty path");

    assert_eq!(
        before,
        common::manifest(dir.path()),
        "the production loop must not create/modify/delete anything"
    );
    assert!(!state.exists(), "clean must not create the state dir");
}
