//! Real-PTY event-loop tests (M2 task 2, slice 2).
//!
//! Unlike `tests/checklist_tty.rs` (which injects `Key` values through the
//! `run_interactive_with` seam), these tests run the **production** entry point
//! [`run_interactive`] on a pseudo-terminal, so the real termios raw mode, the
//! real `poll`-bounded `read_key_event`, and the real [`Decoder`] all execute. They
//! exist because two real bugs shipped green without a PTY: a multi-byte read
//! losing bytes 1..n (an arrow key resolving to abort) and a bare `\n` frame on
//! a terminal with `ONLCR` cleared (a staircase render).
//!
//! Spec: docs/CLI-UX.md §3.2/§6; gates: docs/SAFETY.md §2 (a lone `esc` aborts,
//! nothing deleted).

#![cfg(unix)]

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::time::{Duration, Instant};

use sweep::cli::Preset;
use sweep::rules::Tier;
use sweep::scan::{RuleResult, ScanInfo};
use sweep::ui::Theme;
use sweep::ui::checklist::{Outcome, Selection, run_interactive};

fn rr(id: &'static str, tier: Tier, bytes: u64) -> RuleResult {
    RuleResult {
        id,
        title: "Test rule",
        category: "test",
        tier,
        applied: true,
        reason: None,
        needs_sudo: false,
        location: "~/.cache/test".to_string(),
        info: ScanInfo {
            bytes,
            ..Default::default()
        },
        net_of: Vec::new(),
        skipped: BTreeMap::new(),
    }
}

fn fixture() -> Vec<RuleResult> {
    vec![
        rr("row-0", Tier::Safe, 10),
        rr("row-1", Tier::Safe, 20),
        rr("row-2", Tier::Safe, 30),
    ]
}

fn plain() -> Theme {
    Theme {
        color: false,
        unicode: true,
    }
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

/// The child's report, printed to the pty after the loop returns.
fn report(outcome: &Outcome, cursor: usize) -> String {
    match outcome {
        Outcome::Accepted(items) => format!("ACCEPTED n={} cursor={}\n", items.len(), cursor),
        Outcome::Aborted => format!("ABORTED cursor={}\n", cursor),
    }
}

/// Run `run_interactive` on a pty in a forked child; write `keys` to the master
/// as `writes` separate writes; return `(report_line, captured_output)`.
///
/// `gap_ms` is the settle delay before *every* chunk, and between chunks when
/// `writes` is false. It must be read against the production poll window:
/// `tty::READ_TIMEOUT_MS` is 100 ms, and a chunk that is *not* a complete key
/// (a bare `ESC`, or `ESC [`) is resolved as finite at the end of that window.
/// So a test that intends to exercise reassembly **across `read()` calls** must
/// use `gap_ms` well below 100 ms (see [`run_on_pty_gapped`]); a test that
/// intends to prove a lone `ESC` aborts must use a gap *above* it.
fn run_on_pty(keys: &[&[u8]], writes: bool, gap_ms: u64) -> (String, String) {
    let (master, slave) = openpty();
    let slave_raw = std::os::fd::AsRawFd::as_raw_fd(&slave);
    let _master_raw = std::os::fd::AsRawFd::as_raw_fd(&master);

    // SAFETY: `fork` in a test binary. The child does no allocation before its
    // first syscall other than the intents it owns; it always `_exit`s, so no
    // destructors run twice and no libtest state is corrupted.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork failed");

    if pid == 0 {
        // Child: the slave becomes this process's stdin/stdout/stderr, so
        // `isatty` is true and termios raw mode can be set on stdin.
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

        let mut sel = Selection::from_results(&fixture(), Preset::Regular);
        let theme = plain();
        let line = match run_interactive(&mut sel, &theme) {
            Ok(outcome) => report(&outcome, sel.cursor()),
            Err(e) => format!("ERROR {e}\n"),
        };
        // Write the report straight to the tty (fd 1 is the slave) and flush.
        let mut out = std::io::stdout();
        let _ = out.write_all(line.as_bytes());
        let _ = out.flush();
        std::process::exit(0);
    }

    // Parent: write the scripted keys, then read whatever the child produced.
    drop(slave);
    let mut master_file = std::fs::File::from(master);
    for chunk in keys {
        // A small settle delay so `poll` is already waiting (one-write vs
        // split-write is the distinction under test). Kept below the poll
        // window whenever the split itself is the subject.
        std::thread::sleep(Duration::from_millis(gap_ms));
        let _ = master_file.write_all(chunk);
        let _ = master_file.flush();
        if !writes {
            std::thread::sleep(Duration::from_millis(gap_ms));
        }
    }

    // Read the child's tty output (it includes the rendered frames + report).
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
                if captured.contains("ACCEPTED")
                    || captured.contains("ABORTED")
                    || captured.contains("ERROR")
                {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    // SAFETY: `waitpid` on the child we forked; it reaps the zombie.
    let mut status = 0;
    unsafe { libc::waitpid(pid, &mut status, 0) };

    // The report is written right after the cursor-show sequence with no
    // newline before it, so scan for the marker inside the (ANSI-stripped)
    // final segment rather than relying on the line start.
    let line = captured
        .rsplit("\r\n")
        .find_map(|seg| {
            let clean = strip_ansi(seg);
            let t = clean.trim();
            if t.starts_with("ACCEPTED") || t.starts_with("ABORTED") || t.starts_with("ERROR") {
                Some(t.to_string())
            } else {
                ["ACCEPTED", "ABORTED", "ERROR"]
                    .iter()
                    .find_map(|m| t.find(m))
                    .map(|idx| t[idx..].trim().to_string())
            }
        })
        .unwrap_or_default();
    (line, captured)
}

/// The settle delay used by tests where the gap itself is *not* the subject (a
/// chunk that is a complete key is safe at any gap). Above `READ_TIMEOUT_MS`,
/// matching the historical harness value.
const GAP_MS_SETTLED: u64 = 120;

/// Gap for the split-sequence test: comfortably below `tty::READ_TIMEOUT_MS`
/// (100 ms) so `ESC`, `[`, `B` each arrive inside *one* poll window and the
/// decoder must reassemble them. 15 ms leaves CI scheduler-jitter margin while
/// keeping the writes genuinely separate (1 ms risks coalescing into one read).
const GAP_MS_SPLIT: u64 = 15;

/// [`run_on_pty`] with the split-reassembly gap (see [`GAP_MS_SPLIT`]).
fn run_on_pty_gapped(keys: &[&[u8]]) -> (String, String) {
    run_on_pty(keys, false, GAP_MS_SPLIT)
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
fn large_single_write_is_fully_drained_and_accepts_promptly() {
    // Regression for the `BufReader`-vs-`poll` deadlock: a paste larger than the
    // read buffer (historically `READ_BUF = 64`) delivered in ONE write must be
    // fully drained, and the trailing `\r` must end the session. Before the raw
    // `read(2)` fix the surplus bytes sat in std's userspace buffer where `poll`
    // could not see them, so the loop redrew forever and never accepted.
    //
    // 130 spaces is an even number of toggles on the single cursor row (space
    // does not move the cursor), and the row starts checked under
    // `Preset::Regular`, so it ends checked: n stays 3.
    let mut chunk = vec![b' '; 130];
    chunk.push(b'\r');
    let (report, _out) = run_on_pty(&[&chunk], true, GAP_MS_SETTLED);
    assert_eq!(
        report, "ACCEPTED n=3 cursor=0",
        "a >READ_BUF single write must drain fully and accept"
    );
}

#[test]
fn arrow_key_in_one_write_moves_the_cursor() {
    // `ESC [ B` delivered as a *single* write: the decoder must consume all
    // three bytes and move the cursor down, then `enter` accepts. Before the
    // fix, only `ESC` was fed, so the next byte resolved it to abort.
    let (report, _out) = run_on_pty(&[b"\x1b[B", b"\r"], true, GAP_MS_SETTLED);
    assert_eq!(
        report, "ACCEPTED n=3 cursor=1",
        "one-write arrow must move the cursor"
    );
}

#[test]
fn arrow_key_split_across_writes_still_moves_the_cursor() {
    // The three bytes of `ESC [ B` arrive in three separate writes, each
    // separated by `GAP_MS_SPLIT` (15 ms), which is well inside one
    // `tty::READ_TIMEOUT_MS` (100 ms) poll window. The decoder must therefore
    // buffer the incomplete prefix across `read()` boundaries and reassemble it
    // into `Key::Down` rather than resolving the lone `ESC` to abort. This
    // timing assumption is the whole point of the test: widen the gap past
    // 100 ms and the correct behaviour becomes ABORTED (see `lone_esc_aborts`).
    let (report, _out) = run_on_pty_gapped(&[b"\x1b", b"[", b"B", b"\r"]);
    assert_eq!(
        report, "ACCEPTED n=3 cursor=1",
        "split arrow within one poll window must still move the cursor"
    );
}

#[test]
fn lone_esc_aborts() {
    // A lone `ESC` with no continuation: the poll window closes and the decoder
    // flushes it to abort. The child must NOT hang waiting for another key. The
    // gap before the (absent) continuation is irrelevant here because nothing
    // follows, but the *window itself* is the guarantee: we wait one full
    // `tty::READ_TIMEOUT_MS` (100 ms) with only `ESC` buffered, so the flush
    // path is what produces ABORTED.
    let (report, _out) = run_on_pty(&[b"\x1b"], true, GAP_MS_SETTLED);
    assert_eq!(report, "ABORTED cursor=0", "a lone esc must abort");
}

#[test]
fn q_aborts() {
    let (report, _out) = run_on_pty(&[b"q"], true, GAP_MS_SETTLED);
    assert_eq!(report, "ABORTED cursor=0");
}

#[test]
fn frames_use_crlf_so_the_render_does_not_staircase() {
    // With OPOST kept (ONLCR on), the tty translates `\n` to CRLF; the captured
    // output must therefore contain "\r\n" and not a bare "\n".
    let (_report, out) = run_on_pty(&[b"q"], true, GAP_MS_SETTLED);
    assert!(
        out.contains("\r\n"),
        "frames must reach the tty with CRLF (ONLCR kept): {:?}",
        out.escape_debug().to_string()
    );
}

#[test]
fn space_toggles_the_cursor_row_then_enter_accepts_it() {
    // `\r` is a complete key on its own, so the 120 ms gap is harmless here;
    // `GAP_MS_SETTLED` keeps the two writes unambiguous.
    let (report, _out) = run_on_pty(&[b" ", b"\r"], true, GAP_MS_SETTLED);
    assert_eq!(
        report, "ACCEPTED n=2 cursor=0",
        "space toggles the cursor row (the cursor row starts checked)"
    );
}
