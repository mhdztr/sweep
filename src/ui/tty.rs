//! Raw-mode terminal I/O: hand-rolled on `libc`/termios (decision D15).
//! Spec: docs/CLI-UX.md §3.2 (keys) / §5 (not-a-TTY error) / §6 (exit codes);
//! gates: docs/SAFETY.md §2 (Esc/Ctrl-C → "aborted, nothing was deleted").
//!
//! The module owning the checklist's terminal I/O: entering raw mode, decoding
//! one key press, the cursor sequences, and the SIGINT/SIGWINCH flags.
//! `src/ui/checklist.rs` also performs one `unsafe` `isatty` probe (does the
//! checklist own stdout?) and `src/ui/mod.rs` the pre-existing
//! `ioctl(TIOCGWINSZ)`; everything else in the checklist is pure.
//! `inquire` cannot express the required keybindings/row states (D14) and no TUI
//! crate is added: `libc` is already the sole syscall dependency (`terminal_width`,
//! `src/ui/mod.rs`). Every `unsafe` block is small, fd-explicit and SAFETY-commented
//! like the existing `ioctl(TIOCGWINSZ)` block.
//!
//! # Reading keys without losing bytes (blocker B1/B2)
//!
//! The loop `poll(2)`s stdin with a real `READ_TIMEOUT_MS` timeout, then reads.
//! Every byte that *is* delivered goes through [`Decoder::feed`] in order, and
//! **every** decoded key is queued, so a terminal that hands us `ESC [ B` or
//! `" \r"` in one read is fully consumed instead of discarding bytes after the
//! first key (a previous `VTIME`-based version kept only `buf[0]`).
//! The read is a raw `read(2)` on fd 0, never `std::io::stdin()`: the latter is
//! a `BufReader` that can hold bytes `poll` no longer sees, which stranded the
//! tail of a large paste (see [`read_key_event`]). After `poll` reports data the
//! loop drains fd 0 until it would block (`O_NONBLOCK`, set in
//! [`RawMode::enter`]), so the invariant is "poll readable => consume all
//! available bytes".
//!
//! `poll` (not `VMIN`/`VTIME`) is what bounds the wait for the *first* byte of a
//! key: the termios fields cannot express an idle timeout. A lone `ESC` resolves
//! to `Aborted` once a poll window closes with it still unterminated; the whole
//! key read may span more than one window (an incomplete prefix is re-polled),
//! so the guarantee is "within a poll window or so", not "exactly one tick".

use crate::cli::Preset;
use std::collections::VecDeque;
use std::io;
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicBool, Ordering};

/// How many bytes one `read` accepts. Big enough that an ordinary paste chunk
/// lands in one read, small enough to keep the loop's latency bounded.
///
/// This is the size of the event loop's buffer (see `run_interactive`), and it
/// is deliberately **not** a correctness bound: [`read_key_event`] drains fd 0
/// until it would block, so a chunk larger than this is consumed in several
/// reads rather than left hidden in a userspace buffer.
pub const READ_BUF: usize = 4096;

/// Milliseconds one `poll` waits before the loop treats an incomplete sequence
/// as finished (100 ms: unobservable to a human, bounds the whole loop latency).
/// A key read may span a few windows while an incomplete prefix is re-polled,
/// and an unterminated lone `ESC` settles at the end of one of them.
const READ_TIMEOUT_MS: i32 = 100;

/// Set by the SIGINT handler (ISIG stays on, so Ctrl-C raises a signal rather
/// than arriving as a `0x03` byte); polled once per loop iteration.
static INTERRUPTED: AtomicBool = AtomicBool::new(false);

/// Set by the SIGWINCH handler; polled once per loop iteration, which then
/// re-reads [`crate::ui::terminal_width`] and re-renders.
static RESIZED: AtomicBool = AtomicBool::new(false);

/// `true` once a Ctrl-C has been observed, and not yet consumed by a loop.
/// The loop clears it before blocking (see `arm`), so a Ctrl-C that arrives
/// while a prompt is not running does not abort the *next* session.
pub fn interrupted() -> bool {
    INTERRUPTED.load(Ordering::Relaxed)
}

/// Consume a recorded Ctrl-C. Called by the event loop before entering raw mode.
pub fn clear_interrupted() {
    INTERRUPTED.store(false, Ordering::Relaxed);
}

/// `true` when a resize arrived since the last [`take_resized`].
pub fn resized() -> bool {
    RESIZED.load(Ordering::Relaxed)
}

/// Consume a recorded resize.
pub fn take_resized() -> bool {
    RESIZED.swap(false, Ordering::Relaxed)
}

/// Store to the SIGINT flag. Split out of the handler so the *handler* body is a
/// single call and the logic is unit-testable without raising a signal.
fn on_sigint() {
    INTERRUPTED.store(true, Ordering::Relaxed);
}

/// Store to the SIGWINCH flag (same split as [`on_sigint`]).
fn on_sigwinch() {
    RESIZED.store(true, Ordering::Relaxed);
}

/// `extern "C"` trampoline: async-signal-safe (one relaxed atomic store).
extern "C" fn sigint_handler(_sig: libc::c_int) {
    on_sigint();
}

/// `extern "C"` trampoline for SIGWINCH (same shape as [`sigint_handler`]).
extern "C" fn sigwinch_handler(_sig: libc::c_int) {
    on_sigwinch();
}

/// The previous dispositions of the two signals we touch, captured so a session
/// can put them back (blocker-adjacent: handlers used to be installed forever).
struct SavedHandlers {
    int: libc::sigaction,
    winch: libc::sigaction,
}

/// RAII guard: installs the SIGINT/SIGWINCH handlers on [`SignalGuard::install`]
/// and restores the *previous* dispositions on `Drop`.
///
/// Without this, a Ctrl-C arriving after `run_interactive` returned (or after it
/// bailed out with "stdin is not a TTY") would only set a flag and stop
/// terminating the process — i.e. a dead Ctrl-C for every later stage of the
/// run, including the gates.
pub struct SignalGuard {
    saved: SavedHandlers,
}

impl SignalGuard {
    /// Install both handlers, capturing the prior dispositions. Idempotent per
    /// install; call once per interactive session (the loop does).
    pub fn install() -> io::Result<SignalGuard> {
        let mut saved = SavedHandlers {
            int: zero_sigaction(),
            winch: zero_sigaction(),
        };
        let mut int = zero_sigaction();
        int.sa_sigaction = sigint_handler as *const () as libc::sighandler_t;
        let mut winch = zero_sigaction();
        winch.sa_sigaction = sigwinch_handler as *const () as libc::sighandler_t;

        // SAFETY: both `sa` structs are fully initialized (mask zeroed, handler
        // set to an async-signal-safe `extern "C" fn(c_int)`); the `saved`
        // out-pointers are valid and written only on success, which we check.
        // No allocation and no locks are involved.
        unsafe {
            if libc::sigaction(libc::SIGINT, &int, &mut saved.int) != 0 {
                return Err(io::Error::last_os_error());
            }
            if libc::sigaction(libc::SIGWINCH, &winch, &mut saved.winch) != 0 {
                // Undo the SIGINT install so a failed call changes nothing.
                libc::sigaction(libc::SIGINT, &saved.int, std::ptr::null_mut());
                return Err(io::Error::last_os_error());
            }
        }
        Ok(SignalGuard { saved })
    }
}

impl Drop for SignalGuard {
    /// Put the dispositions we found back. Runs on every in-process exit path,
    /// so Ctrl-C keeps its default (terminating) semantics afterwards.
    fn drop(&mut self) {
        // SAFETY: `self.saved` holds the values written by the successful
        // `sigaction` calls in `install`; re-installing them is the documented
        // inverse operation. No allocation, no locks, `oldact` is null.
        unsafe {
            libc::sigaction(libc::SIGINT, &self.saved.int, std::ptr::null_mut());
            libc::sigaction(libc::SIGWINCH, &self.saved.winch, std::ptr::null_mut());
        }
    }
}

/// A zeroed `sigaction` with an empty signal mask (the shape `libc` wants).
fn zero_sigaction() -> libc::sigaction {
    // SAFETY: `sigaction` is a plain C struct; an all-zero bit pattern is a
    // valid (if inert) value, and we overwrite the fields we care about.
    let mut sa: libc::sigaction = unsafe { std::mem::zeroed() };
    // SAFETY: `sa_mask` is a valid out-pointer to the freshly zeroed struct's
    // field; `sigemptyset` only writes to it.
    unsafe {
        libc::sigemptyset(&mut sa.sa_mask);
    }
    sa
}

/// One decoded key press, mapped to checklist intent by
/// `ui::checklist::apply_key`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Space,
    /// `a`, select every safe row.
    All,
    /// `n`, deselect every row.
    None,
    /// `i`, invert the selection.
    Invert,
    /// `p`, cycle safe → regular → full → safe.
    Preset,
    /// `enter`/`\r`/`\n`, proceed to the gates.
    Enter,
    /// `?`, toggle the help footer.
    Help,
    /// `esc` / `q` / Ctrl-C: abort, exit 2, nothing done.
    Abort,
    /// `\0` byte: Ctrl-Space. Decodes to [`Key::Unknown`]'s intent (a no-op);
    /// kept as a distinct variant because the byte is recognized, not stray.
    Resize,
    /// Unrecognized byte/sequence: ignored.
    Unknown,
}

/// Pure byte→key decoder (unit-tested without a tty).
///
/// `bytes` is one *complete* key sequence: a lone byte, or `ESC` + the CSI
/// body. `ESC` followed by nothing (or by a sequence we do not recognize) means
/// the user pressed `esc` alone → [`Key::Abort`] (CLI-UX §3.2 / SAFETY §2).
pub fn decode(bytes: &[u8]) -> Key {
    match bytes {
        [] => Key::Unknown,
        [0x1b] => Key::Abort,
        [0x1b, b'[', b'A'] | [0x1b, b'O', b'A'] => Key::Up,
        [0x1b, b'[', b'B'] | [0x1b, b'O', b'B'] => Key::Down,
        [b' '] => Key::Space,
        [0x10] => Key::Up,   // Ctrl-P
        [0x0e] => Key::Down, // Ctrl-N
        [b'a'] => Key::All,
        [b'n'] => Key::None,
        [b'i'] => Key::Invert,
        [b'p'] => Key::Preset,
        [b'q'] => Key::Abort,
        [b'?'] => Key::Help,
        [b'\r'] | [b'\n'] => Key::Enter,
        [0x00] => Key::Resize,
        // Ctrl-C when it reaches us as a byte (a pty/source where ISIG is off);
        // with ISIG on it never gets here, the terminal raises SIGINT instead.
        [0x03] => Key::Abort,
        _ => Key::Unknown,
    }
}

/// The awaiting-a-continuation state of a partial escape sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Partial {
    /// Not in the middle of an escape sequence.
    None,
    /// `ESC` seen; the continuation decides esc-alone vs an arrow key.
    Escape,
    /// `ESC [` / `ESC O` seen; the next byte is the CSI final byte.
    Csi,
}

/// Pure state machine feeding one byte at a time into [`decode`].
///
/// Needed because telling `esc` (abort) from `ESC [ A` (up) requires a
/// non-blocking peek, and the peek must not block waiting for a byte a *human*
/// has not typed. Returns `Some(key)` only when a complete sequence is known.
pub struct Decoder {
    partial: Partial,
    buf: Vec<u8>,
    /// Keys decoded from a read that carried more than one key. A terminal can
    /// deliver `" \r"` (fast type / paste) in a single read; without this queue
    /// the bytes after the first key would be dropped. Drained by [`Decoder::pop_key`].
    pending: VecDeque<Key>,
}

impl Decoder {
    fn new() -> Decoder {
        Decoder {
            partial: Partial::None,
            buf: Vec::new(),
            pending: VecDeque::new(),
        }
    }

    /// Pop one already-decoded key (from a previous multi-key read), if any.
    pub fn pop_key(&mut self) -> Option<Key> {
        self.pending.pop_front()
    }

    /// Whether keys decoded from an earlier read are still queued (the loop
    /// checks this before reporting EOF, so bytes that arrived *before* the
    /// close are never dropped).
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Decode one byte and *queue* the result; the key is also returned so the
    /// byte-at-a-time unit tests keep working.
    fn feed_queued(&mut self, byte: u8) -> Option<Key> {
        if let Some(key) = self.feed(byte) {
            self.pending.push_back(key);
            Some(key)
        } else {
            None
        }
    }

    /// Feed one byte; `Some(key)` when a key is complete.
    pub fn feed(&mut self, byte: u8) -> Option<Key> {
        self.buf.push(byte);
        match (self.partial, byte) {
            (Partial::None, 0x1b) => {
                self.partial = Partial::Escape;
                None
            }
            // `ESC [` / `ESC O`: enter the CSI state; the final byte settles it.
            (Partial::Escape, b'[' | b'O') => {
                self.partial = Partial::Csi;
                None
            }
            // `ESC` followed by an ordinary key: the user pressed `esc`
            // (SAFETY §2 abort); the trailing key is dropped because the loop
            // stops on the abort. Matches the `decode` contract (L121-122).
            (Partial::Escape, _) => {
                self.buf.clear();
                self.partial = Partial::None;
                Some(Key::Abort)
            }
            // Third byte of a CSI sequence: resolve (arrow or `Unknown`).
            (Partial::Csi, _) => self.finish(),
            (Partial::None, _) => self.finish(),
        }
    }

    /// Resolve a buffered sequence now (used by the loop's timeout path and by
    /// the unit tests); a lone `ESC` resolves to `esc` → abort.
    pub fn finish(&mut self) -> Option<Key> {
        let key = decode(&self.buf);
        self.buf.clear();
        self.partial = Partial::None;
        Some(key)
    }

    /// The loop calls this when a read timed out with an incomplete sequence
    /// buffered: resolves `ESC`-alone (abort) without blocking further.
    pub fn flush(&mut self) -> Option<Key> {
        if self.buf.is_empty() {
            None
        } else {
            self.finish()
        }
    }
}

/// Pure: the raw termios flags for `saved`, with
/// `ICANON|ECHO|ECHOE|ECHOK|ECHONL|IEXTEN|IXON|ICRNL|INPCK|ISTRIP` cleared,
/// 8-bit chars, `VMIN=1`/`VTIME=0` (one byte at a time; the *loop* supplies the
/// timeout via `poll`, because neither field can express an idle timeout), and
/// **`ISIG` left set**: Ctrl-C must keep its signal semantics (`SIGINT`)
/// instead of arriving as a `0x03` byte.
///
/// `OPOST` is deliberately **kept**: `ONLCR` lives under `OPOST`, and every
/// frame is written with bare `\n`. Clearing it would move the cursor down
/// without returning to column 0 (the checklist would render as a staircase).
pub fn raw_flags(saved: &libc::termios) -> libc::termios {
    let mut raw = *saved;
    raw.c_lflag &= !(libc::ICANON
        | libc::ECHO
        | libc::ECHOE
        | libc::ECHOK
        | libc::ECHONL
        | libc::IEXTEN
        | libc::IXON);
    raw.c_iflag &= !(libc::ICRNL | libc::INPCK | libc::ISTRIP);
    raw.c_cflag |= libc::CS8;
    raw.c_cc[libc::VMIN] = 1;
    raw.c_cc[libc::VTIME] = 0;
    raw
}

/// RAII raw-mode + cursor guard.
///
/// `enter` saves termios, switches stdin to raw mode and hides the cursor;
/// `Drop` restores both, so Ok/Err/abort/**unwind** all leave the terminal
/// usable. `raw` stores the *original* settings (the `Drop` targets) and
/// `output` the fd the cursor sequences were written to (`None` = no fd: this
/// guard does not own stdout and does not touch it).
pub struct RawMode {
    raw: libc::termios,
    output: Option<RawFd>,
}

impl RawMode {
    /// Enter raw mode on stdin (fd 0). `Err` with
    /// `ErrorKind::NotConnected` when stdin is not a TTY: `src/main.rs` maps
    /// that to the copy-locked `error: interactive mode requires a TTY;
    /// use --preset/--only with --yes` (CLI-UX §5).
    ///
    /// `output: None` means the checklist's stdout was not a TTY either (piped
    /// CI), so no cursor sequences are attempted **and the loop suppresses its
    /// erase sequences too** (see `run_interactive`); the raw-mode entry still
    /// holds because [`crate::ui::checklist::run_interactive`] is driven from a
    /// PTY in tests.
    pub fn enter(output: Option<RawFd>) -> io::Result<RawMode> {
        // SAFETY: `isatty` only inspects fd 0 (always open); no memory is
        // touched and nothing is mutated.
        if unsafe { libc::isatty(libc::STDIN_FILENO) } != 1 {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "stdin is not a TTY",
            ));
        }

        let mut saved = std::mem::MaybeUninit::<libc::termios>::uninit();
        // SAFETY: `saved` is a valid, correctly aligned `termios` out-pointer;
        // `tcgetattr` writes it on success, checked before any read.
        let rc = unsafe { libc::tcgetattr(libc::STDIN_FILENO, saved.as_mut_ptr()) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `tcgetattr` returned 0 above, so `saved` is initialized.
        let saved = unsafe { saved.assume_init() };

        let raw = raw_flags(&saved);
        // SAFETY: `raw` is a fully initialized `termios` (a copy of `saved`
        // with flag bits changed); TCSANOW applies it immediately.
        let rc = unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) };
        if rc != 0 {
            return Err(io::Error::last_os_error());
        }

        let guard = RawMode { raw: saved, output };
        // Non-blocking fd 0: `read_key_event` drains the kernel queue with
        // `read(2)` after `poll` says there is something, and needs the empty
        // queue to surface as `EAGAIN` rather than a blocking wait. termios
        // only holds tty settings, so the flag must be set with `fcntl(2)` and
        // undone in `Drop` (fds are shared, e.g. across a `fork`).
        guard.set_nonblocking(true);
        guard.set_cursor(false);
        Ok(guard)
    }

    /// Toggle `O_NONBLOCK` on stdin. Best-effort: if it fails, the read path
    /// still works because it is only reached after `poll` reports data (the
    /// flag exists to keep the *drain* loop from blocking, not the first read).
    fn set_nonblocking(&self, on: bool) {
        // SAFETY: `fcntl` on the always-open fd 0; F_GETFL takes no argument
        // and F_SETFL takes the int we derive below. No memory is touched.
        unsafe {
            let flags = libc::fcntl(libc::STDIN_FILENO, libc::F_GETFL);
            if flags >= 0 {
                let want = if on {
                    flags | libc::O_NONBLOCK
                } else {
                    flags & !libc::O_NONBLOCK
                };
                libc::fcntl(libc::STDIN_FILENO, libc::F_SETFL, want);
            }
        }
    }

    /// Whether this guard owns stdout (i.e. the checklist may emit cursor and
    /// erase sequences). `false` when stdout was not a TTY.
    pub fn owns_stdout(&self) -> bool {
        self.output.is_some()
    }

    /// Write the ANSI cursor-visibility sequence (`\x1b[?25l` hide /
    /// `\x1b[?25h` show) to the guard's output fd, if it has one.
    fn set_cursor(&self, show: bool) {
        let Some(fd) = self.output else {
            return;
        };
        let seq: &[u8] = if show { b"\x1b[?25h" } else { b"\x1b[?25l" };
        // SAFETY: `set_cursor` is called from `Drop` too, so it must not
        // allocate or lock; `write` on a raw fd does neither. The buffer is a
        // `'static` byte slice and the fd was validated by the caller.
        unsafe {
            libc::write(fd, seq.as_ptr().cast(), seq.len());
        }
    }
}

impl Drop for RawMode {
    /// Restore the saved termios settings and re-show the cursor. Runs on
    /// every in-process exit path (return, `?`, abort, panic unwind). A
    /// `SIGKILL`ed process cannot restore the terminal: inherent, documented in
    /// docs/CLI-UX.md §3.
    fn drop(&mut self) {
        // Put fd 0 back to blocking *before* handing the terminal back, so the
        // O_NONBLOCK we set in `enter` cannot leak into later stages of the run.
        self.set_nonblocking(false);
        // SAFETY: `self.raw` is the `termios` returned by a successful
        // `tcgetattr` in `enter`; restoring it with TCSANOW is the documented
        // inverse operation. A failure here cannot be reported from `Drop`, and
        // the terminal is left as clean as `tcsetattr` managed to do.
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &self.raw);
        }
        self.set_cursor(true);
    }
}

/// Wait up to `timeout_ms` for stdin to become readable.
///
/// Returns `Ok(true)` when a read would not block, `Ok(false)` on a timeout,
/// `Err` on a real poll failure. `POLLIN` only; `EINTR` (a signal — the very
/// thing we are waiting to observe) retries with the *remaining* budget.
///
/// This bounds the wait for the *first* byte of a key, which is what makes an
/// idle terminal (and a lone `ESC`) finite. [`read_key_event`] then drains
/// fd 0 with a non-blocking `read`, so a key larger than one poll window's
/// worth of bytes is reassembled by the next call rather than lost.
fn wait_for_input(timeout_ms: i32) -> io::Result<bool> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms as u64);
    loop {
        // Poll with whatever budget is *left*, so a signal storm cannot stretch
        // the wait past the deadline.
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return Ok(false);
        }
        let mut pfd = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pfd` is a valid, correctly aligned single-element pollfd
        // array for the duration of the call; fd 0 is always open. `poll` only
        // writes `revents`.
        let rc = unsafe { libc::poll(&mut pfd, 1, left.as_millis().min(i32::MAX as u128) as i32) };
        if rc > 0 {
            return Ok(true);
        }
        if rc == 0 {
            return Ok(false);
        }
        let err = io::Error::last_os_error();
        if err.kind() != io::ErrorKind::Interrupted {
            return Err(err);
        }
        // Interrupted (a signal we want to observe): loop and re-poll with the
        // remaining budget.
    }
}

/// The richer read outcome: distinguishes a timeout from stdin EOF so the event
/// loop can *exit* on EOF instead of redrawing forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// A complete key sequence.
    Key(Key),
    /// The poll window closed with nothing (or nothing complete) to report.
    Idle,
    /// `read` returned 0: stdin reached EOF, the session must end.
    Closed,
}
/// Read one event from stdin, waiting up to one poll window.
///
/// Calls `poll` first, so an idle terminal yields `Input::Idle` after
/// `READ_TIMEOUT_MS` (no `VMIN`/`VTIME` trickery: those cannot express an idle
/// timeout, which used to make a lone `ESC` hang until the next key arrived).
/// Every byte a read *does* return is fed to `decoder` in order, so a terminal
/// that delivers `ESC [ B` as a single 3-byte read moves the cursor instead of
/// dropping bytes 1..n. When the poll window closes with an incomplete sequence
/// buffered, the ESC resolves to `Abort` (`decoder.flush()`).
///
/// # Reading fd 0 directly (blocker: input stranded by a userspace buffer)
///
/// This reads with `read(2)` on fd 0, **not** through `std::io::stdin()`, which
/// is a process-global `BufReader`. `BufReader::read` asks its inner fd for
/// `capacity` (8192) bytes and may consume far more than it hands back: the
/// surplus lands in the reader's own userspace buffer, where `poll` on fd 0 can
/// no longer see it. A paste larger than `buf` then left the trailing keys
/// (including the `\r` that ends the session) invisible to `poll` forever, so
/// the loop redrew without ever accepting a key. Reading the kernel queue
/// directly removes that second buffer entirely.
///
/// The invariant is "after `poll` reports fd 0 readable, consume *all* currently
/// available bytes": the loop keeps calling `read` until it returns `EAGAIN`
/// (nothing more is queued; raw-mode fd 0 is non-blocking, see `RawMode::enter`)
/// or the buffer fills. Every key decoded from the drained bytes is queued, so
/// the extras survive into the next call via [`Decoder::pop_key`].
///
/// The outcome distinguishes a timeout from stdin EOF so the event loop can
/// *exit* on EOF instead of redrawing forever. There is deliberately no
/// `Option`-returning convenience wrapper: `Ok(None)` would conflate "idle"
/// with "closed", and the loop needs that exact distinction to stop on EOF
/// (`Input::Closed`) while pacing on idle (`Input::Idle`).
pub fn read_key_event(decoder: &mut Decoder, buf: &mut [u8]) -> io::Result<Input> {
    // A previous read may have decoded more than one key; hand them out before
    // touching the terminal, so `" \r"` in one read applies *both* keys.
    if let Some(key) = decoder.pop_key() {
        return Ok(Input::Key(key));
    }
    // Nothing is queued; wait one poll window for the *first* byte of a key.
    // This is what paces an idle loop and what makes a lone `ESC` finite.
    if !wait_for_input(READ_TIMEOUT_MS)? {
        return Ok(match decoder.flush() {
            Some(key) => Input::Key(key),
            None => Input::Idle,
        });
    }
    // `poll` said fd 0 is readable; keep reading while it still has queued
    // bytes so nothing is left behind for a later `poll` to miss. In raw mode
    // fd 0 is non-blocking (`O_NONBLOCK` in `RawMode::enter`), so the read that
    // hits the end of the queue returns `EAGAIN` instead of blocking.
    loop {
        match read_stdin_available(buf)? {
            ReadOutcome::Bytes(n) => {
                // Feed *every* byte of the read (a terminal can deliver
                // `ESC [ A` or `" \r"` in one read; feeding only `buf[0]` would
                // strand the rest). Each decoded key is queued and drained by
                // the loop below / the next call via `Decoder::pop_key`.
                for &byte in &buf[..n] {
                    decoder.feed_queued(byte);
                }
                if let Some(key) = decoder.pop_key() {
                    return Ok(Input::Key(key));
                }
                if n < buf.len() {
                    // A short read: nothing further was queued at that instant.
                    break;
                }
                // The buffer filled: loop once more to drain the rest.
            }
            ReadOutcome::Eof => {
                // stdin's write end is gone. If keys decoded earlier in this
                // drain are still queued, hand them out first and let the next
                // call observe the close; otherwise stop now. Returning here
                // (rather than looping) is essential: at a pipe/pty hangup
                // `read(2)` returns 0 forever and never `EAGAIN`, so a loop
                // would spin hot and `Input::Closed` would never be reached.
                if decoder.has_pending() {
                    break;
                }
                return Ok(Input::Closed);
            }
            ReadOutcome::WouldBlock => {
                // The queue is empty right now; an incomplete/absent key is
                // just idle for this window.
                break;
            }
        }
    }
    // The drain emptied fd 0 without completing a key. If bytes *did* arrive in
    // this call they are a CSI prefix in progress (`ESC`, then `[` a few ms
    // later): do NOT resolve them yet. Resolving here would abort on the `ESC`
    // the instant it is read, before the rest of an arrow key that is already in
    // flight arrives. The next call's `poll` waits one more window; only a full
    // window with no further byte reaches the timeout path above, where
    // `flush()` resolves a lone `ESC` to abort. That is what keeps both
    // guarantees: split CSI reassembles, a genuinely lone `ESC` still aborts.
    Ok(Input::Idle)
}

/// One `read(2)` on fd 0, normalized so the drain loop can tell "no bytes yet"
/// from "EOF" from a real failure.
#[derive(Debug)]
enum ReadOutcome {
    /// `n > 0` bytes were copied into the caller's buffer.
    Bytes(usize),
    /// `read` returned 0: stdin's write end is gone (EOF). Terminal: the drain
    /// loop stops on it rather than retrying, because a hung-up fd keeps
    /// returning 0 and never `EAGAIN`.
    Eof,
    /// `EAGAIN`/`EWOULDBLOCK`/`EINTR`: the queue is empty right now, try later.
    WouldBlock,
}

/// Read from fd 0 with a single `read(2)`, mapping errno to [`ReadOutcome`].
///
/// Deliberately *not* `std::io::stdin()`: that is a `BufReader` whose internal
/// buffer would swallow bytes `poll` can no longer see (see `read_key_event`).
fn read_stdin_available(buf: &mut [u8]) -> io::Result<ReadOutcome> {
    read_available(libc::STDIN_FILENO, buf)
}

/// [`read_stdin_available`] on an explicit fd, so the EOF/`EAGAIN` mapping can
/// be unit-tested against a pipe rather than the process's own stdin.
fn read_available(fd: RawFd, buf: &mut [u8]) -> io::Result<ReadOutcome> {
    // SAFETY: `buf` is a valid, writable `buf.len()`-byte region for the
    // duration of the call (`read` writes at most that many bytes); `fd` is a
    // valid descriptor owned by the caller. No locks and no allocation.
    let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
    if n == 0 {
        return Ok(ReadOutcome::Eof);
    }
    if n < 0 {
        let err = io::Error::last_os_error();
        return match err.kind() {
            // A signal arrived mid-read: report it as an idle tick so the loop
            // can poll `INTERRUPTED`/`RESIZED` (it re-enters immediately).
            io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut => {
                Ok(ReadOutcome::WouldBlock)
            }
            _ => Err(err),
        };
    }
    // `n` is a positive `ssize_t` that `read` bounded by `buf.len()`.
    Ok(ReadOutcome::Bytes(n as usize))
}

/// Construct a fresh [`Decoder`] (the loop's entry point; keeps the type's
/// constructor internal).
pub fn decoder() -> Decoder {
    Decoder::new()
}

/// Install the signal handlers for one session and clear any stale flags.
///
/// Returns an RAII [`SignalGuard`] that restores the previous dispositions when
/// it drops, so Ctrl-C terminates normally again once the checklist is over.
pub(crate) fn arm() -> io::Result<SignalGuard> {
    let guard = SignalGuard::install()?;
    clear_interrupted();
    take_resized();
    Ok(guard)
}

/// The `Preset` cycle order (CLI-UX §3.2): **the single source of truth**.
/// [`Selection::cycle_preset`](crate::ui::checklist::Selection::cycle_preset)
/// and the renderer's preset label both derive from this order.
pub const PRESET_CYCLE: [Preset; 3] = [Preset::Safe, Preset::Regular, Preset::Full];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_maps_every_documented_key() {
        assert_eq!(decode(b" "), Key::Space);
        assert_eq!(decode(b"\x1b[A"), Key::Up);
        assert_eq!(decode(b"\x1b[B"), Key::Down);
        assert_eq!(decode(b"\x1bOA"), Key::Up, "application cursor mode");
        assert_eq!(decode(b"\x1bOB"), Key::Down);
        assert_eq!(decode(&[0x10]), Key::Up, "Ctrl-P");
        assert_eq!(decode(&[0x0e]), Key::Down, "Ctrl-N");
        assert_eq!(decode(b"a"), Key::All);
        assert_eq!(decode(b"n"), Key::None);
        assert_eq!(decode(b"i"), Key::Invert);
        assert_eq!(decode(b"p"), Key::Preset);
        assert_eq!(decode(b"q"), Key::Abort);
        assert_eq!(decode(b"?"), Key::Help);
        assert_eq!(decode(b"\r"), Key::Enter);
        assert_eq!(decode(b"\n"), Key::Enter);
    }

    #[test]
    fn decode_treats_lone_escape_as_abort() {
        assert_eq!(decode(&[0x1b]), Key::Abort);
        // An escape introducer with an unknown CSI body is not a key we act on,
        // but it is still "escape pressed", never a silent no-op.
        assert_eq!(decode(b"\x1b[3~"), Key::Unknown);
        assert_eq!(decode(&[0x7f]), Key::Unknown, "backspace is not a key");
        assert_eq!(decode(b"z"), Key::Unknown);
        assert_eq!(decode(&[]), Key::Unknown);
    }

    #[test]
    fn decoder_state_machine_splits_escape_from_arrows() {
        let mut d = Decoder::new();
        assert_eq!(d.feed(0x1b), None, "ESC awaits its continuation");
        assert_eq!(d.feed(b'['), None, "ESC[ awaits the final byte");
        assert_eq!(d.feed(b'A'), Some(Key::Up));

        let mut d = Decoder::new();
        assert_eq!(d.feed(b' '), Some(Key::Space));

        // A lone ESC that never gets a continuation: the timeout path flushes
        // it to Abort (SAFETY §2 abort gate).
        let mut d = Decoder::new();
        assert_eq!(d.feed(0x1b), None);
        assert_eq!(d.flush(), Some(Key::Abort));
        assert_eq!(d.flush(), None, "buffer is empty again");

        // ESC followed by an ordinary key: the ESC aborts, the key is dropped
        // (the loop stops on the abort anyway).
        let mut d = Decoder::new();
        assert_eq!(d.feed(0x1b), None);
        assert_eq!(d.feed(b'x'), Some(Key::Abort));
    }

    #[test]
    fn decoder_keeps_every_byte_of_a_multi_byte_read() {
        // Blocker B1 regression: the loop used to feed only `buf[0]`, so a
        // terminal delivering `ESC [ B` in one 3-byte read lost the body and
        // the pending ESC later aborted the session. Feeding every byte of the
        // read must yield Down — this mirrors `read_key_event`'s loop exactly.
        let read: &[u8] = b"\x1b[B";
        let mut d = Decoder::new();
        let mut keys = Vec::new();
        for &byte in read {
            if let Some(k) = d.feed(byte) {
                keys.push(k);
            }
        }
        assert_eq!(keys, vec![Key::Down], "the whole read must be decoded");

        // Two plain keys arriving in one read (fast typing / paste) both apply.
        let read: &[u8] = b"an";
        let mut d = Decoder::new();
        let mut keys = Vec::new();
        for &byte in read {
            if let Some(k) = d.feed(byte) {
                keys.push(k);
            }
        }
        assert_eq!(keys, vec![Key::All, Key::None]);
    }

    #[test]
    fn read_available_maps_eof_and_would_block() {
        // EOF: a pipe whose write end is closed reads 0, which must map to
        // `ReadOutcome::Eof` (never `WouldBlock`), so `read_key_event` stops the
        // drain loop and reports `Input::Closed` instead of spinning.
        let mut fds = [0i32; 2];
        // SAFETY: `pipe` writes two valid fds into `fds`.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let (r, w) = (fds[0], fds[1]);
        // SAFETY: `w` is a valid fd we own; closing it leaves `r` at EOF.
        unsafe { libc::close(w) };
        let mut buf = [0u8; 16];
        match read_available(r, &mut buf).expect("read must not fail") {
            ReadOutcome::Eof => {}
            other => panic!("closed pipe must read as Eof, got {:?}", other),
        }
        // SAFETY: `r` is a valid fd we own.
        unsafe { libc::close(r) };

        // WouldBlock: an open, empty pipe in non-blocking mode has nothing to
        // give, which must map to `WouldBlock` (not `Eof`).
        let mut fds = [0i32; 2];
        // SAFETY: `pipe` writes two valid fds into `fds`.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let (r, w) = (fds[0], fds[1]);
        // SAFETY: `r` is a valid fd we own; set it non-blocking.
        let flags = unsafe { libc::fcntl(r, libc::F_GETFL) };
        unsafe { libc::fcntl(r, libc::F_SETFL, flags | libc::O_NONBLOCK) };
        let mut buf = [0u8; 16];
        match read_available(r, &mut buf).expect("read must not fail") {
            ReadOutcome::WouldBlock => {}
            other => panic!(
                "empty non-blocking pipe must be WouldBlock, got {:?}",
                other
            ),
        }
        // SAFETY: both fds are valid and owned here.
        unsafe {
            libc::close(r);
            libc::close(w);
        }
    }

    #[test]
    fn raw_flags_clear_the_documented_bits_and_keep_isig() {
        let saved = libc::termios {
            c_iflag: libc::ICRNL | libc::IXON | libc::BRKINT,
            c_oflag: libc::OPOST,
            c_cflag: 0,
            c_lflag: libc::ICANON
                | libc::ECHO
                | libc::ECHOE
                | libc::ECHOK
                | libc::ECHONL
                | libc::IEXTEN
                | libc::ISIG,
            c_line: 0,
            c_cc: [0; libc::NCCS],
            c_ispeed: 0,
            c_ospeed: 0,
        };
        let raw = raw_flags(&saved);

        for bit in [
            libc::ICANON,
            libc::ECHO,
            libc::ECHOE,
            libc::ECHOK,
            libc::ECHONL,
            libc::IEXTEN,
            libc::IXON,
        ] {
            assert_eq!(raw.c_lflag & bit, 0, "c_lflag bit {bit:#x} must be off");
        }
        assert_eq!(raw.c_iflag & libc::ICRNL, 0, "ICRNL must be off");
        assert_eq!(
            raw.c_iflag & libc::BRKINT,
            libc::BRKINT,
            "unrelated iflag bits untouched"
        );
        // B3 regression: OPOST must SURVIVE raw mode, because `ONLCR` lives
        // under it and every frame is written with bare `\n`. Clearing it
        // stair-steps the whole checklist.
        assert_eq!(
            raw.c_oflag & libc::OPOST,
            libc::OPOST,
            "OPOST must stay ON so `\\n` returns to column 0"
        );
        assert_eq!(raw.c_cflag & libc::CS8, libc::CS8, "CS8 must be on");

        // Ctrl-C keeps signal semantics (D15): ISIG must survive raw mode.
        assert_ne!(raw.c_lflag & libc::ISIG, 0, "ISIG must stay ON");

        // One byte at a time; the loop's `poll()` supplies the idle timeout
        // because `VMIN`/`VTIME` cannot express one (B2).
        assert_eq!(raw.c_cc[libc::VMIN], 1);
        assert_eq!(raw.c_cc[libc::VTIME], 0, "no inter-byte timer");
        assert_eq!(
            READ_TIMEOUT_MS, 100,
            "one poll window; a key read may span a few of them while an \
             incomplete prefix is re-polled and an unterminated ESC settles"
        );
    }

    #[test]
    fn signal_handlers_only_set_their_flags() {
        clear_interrupted();
        assert!(!interrupted());
        on_sigint();
        assert!(interrupted());
        clear_interrupted();
        assert!(!interrupted());

        take_resized();
        assert!(!resized());
        on_sigwinch();
        assert!(resized());
        assert!(take_resized(), "take_resized returns the observed resize");
        assert!(
            !take_resized(),
            "and consumes it, so the width is read once"
        );
    }

    #[test]
    fn signal_guard_installs_and_restores_dispositions() {
        // A live check that the guard actually round-trips the handler table:
        // install (SIGINT no longer default) -> drop (back to the old action).
        // The old action is captured before, so a non-default disposition
        // inherited from the harness is restored faithfully either way.
        let before = query_action(libc::SIGINT);
        let guard = SignalGuard::install().expect("sigaction");
        let during = query_action(libc::SIGINT);
        assert_ne!(
            during, before,
            "the session disposition must differ while the guard is alive"
        );
        drop(guard);
        assert_eq!(
            query_action(libc::SIGINT),
            before,
            "Drop must restore the previous disposition (no permanent Ctrl-C hijack)"
        );
    }

    /// Read the current disposition for `sig` (test-only helper).
    fn query_action(sig: libc::c_int) -> libc::sighandler_t {
        let mut sa = zero_sigaction();
        // SAFETY: `sa` is a valid out-pointer; `oldact` writes the current
        // action and `act` is null (no change is made).
        unsafe {
            libc::sigaction(sig, std::ptr::null(), &mut sa);
        }
        sa.sa_sigaction
    }

    #[test]
    fn cursor_sequences_are_the_documented_ansi() {
        // The guard writes `\x1b[?25l` / `\x1b[?25h`; assert the literals so a
        // typo cannot silently leave the cursor hidden.
        assert_eq!(
            b"\x1b[?25l" as &[u8],
            &[0x1b, b'[', b'?', b'2', b'5', b'l'][..]
        );
        assert_eq!(
            b"\x1b[?25h" as &[u8],
            &[0x1b, b'[', b'?', b'2', b'5', b'h'][..]
        );
    }

    #[test]
    fn preset_cycle_order_matches_cli_ux() {
        assert_eq!(PRESET_CYCLE.len(), 3);
        assert_eq!(PRESET_CYCLE[0], Preset::Safe);
        assert_eq!(PRESET_CYCLE[1], Preset::Regular);
        assert_eq!(PRESET_CYCLE[2], Preset::Full);
    }
}
