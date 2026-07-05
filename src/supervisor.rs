//! Owns the agent child (in a PTY), infers thinking-vs-idle from output
//! activity, reads the user's lines, routes each through the [`Arbiter`], and
//! remembers the conversation flow via [`crate::memory::MemoryLog`].
//!
//! "Is the agent thinking?" is inferred from output silence: after a prompt is
//! sent the agent is Busy; once silent for `idle_after_ms` it is Idle and delphin
//! releases the next queued prompt. This is a heuristic — the main place that
//! benefits from per-agent tuning.

use std::io::{Read, Write};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use owo_colors::OwoColorize;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use crate::arbiter::{AgentPhase, Arbiter, Decision, Verdict};
use crate::memory::MemoryLog;
use crate::queue::PromptQueue;

/// How many idle windows to wait before releasing when the agent's output ends
/// mid-line (still likely drawing). Bounded so a newline-less prompt can't wedge
/// the queue forever.
// ponytail: raise this if a slow token-streaming agent still gets interrupted
// mid-thought; it only affects the no-ready-marker path.
const MIDLINE_GRACE: u32 = 3;

pub struct Settings {
    pub agent_command: Vec<String>,
    pub idle_after_ms: u64,
    /// Minimum time the agent must have been busy before a silence gap counts as
    /// idle — stops a brief early stall from flipping idle instantly. 0 = off.
    pub min_busy_ms: u64,
    pub tick_ms: u64,
    pub submit: Vec<u8>,
    pub interrupt_bytes: Vec<u8>,
    pub interrupt_label: String,
    /// Substrings that, when the agent's (ANSI-stripped) output tail ends with
    /// one, mean it's waiting for input — flip to idle immediately instead of
    /// waiting out the silence timer. Empty = pure silence detection.
    pub ready_markers: Vec<String>,
    pub rows: u16,
    pub cols: u16,
}

enum Event {
    AgentOutput(Vec<u8>),
    AgentExited,
    UserLine(String),
    UserEof,
    Tick,
}

macro_rules! notice {
    ($($arg:tt)*) => {{
        eprintln!("\x1b[2m[delphin]\x1b[0m {}", format!($($arg)*));
    }};
}

pub fn run(
    settings: &Settings,
    arbiter: Box<dyn Arbiter>,
    memlog: Option<MemoryLog>,
) -> Result<()> {
    let program = settings
        .agent_command
        .first()
        .context("no agent command given (pass one after `--`)")?;

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: settings.rows,
            cols: settings.cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("opening pty")?;

    let mut cmd = CommandBuilder::new(program);
    for arg in &settings.agent_command[1..] {
        cmd.arg(arg);
    }
    if let Ok(cwd) = std::env::current_dir() {
        cmd.cwd(cwd);
    }
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }

    let mut child = pair.slave.spawn_command(cmd).context("spawning agent")?;
    let mut reader = pair
        .master
        .try_clone_reader()
        .context("cloning pty reader")?;
    let mut writer = pair.master.take_writer().context("taking pty writer")?;
    drop(pair.slave);
    let master = pair.master; // kept alive for the PTY's lifetime; also used to propagate resizes

    let (tx, rx) = mpsc::channel::<Event>();

    // PTY reader: mirror output to our stdout, forward bytes for memory/idle.
    {
        let tx = tx.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut out = std::io::stdout();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(Event::AgentExited);
                        break;
                    }
                    Ok(n) => {
                        let _ = out.write_all(&buf[..n]);
                        let _ = out.flush();
                        let _ = tx.send(Event::AgentOutput(buf[..n].to_vec()));
                    }
                }
            }
        });
    }

    // stdin reader: one event per line.
    {
        let tx = tx.clone();
        thread::spawn(move || {
            use std::io::BufRead;
            let stdin = std::io::stdin();
            let mut handle = stdin.lock();
            let mut line = String::new();
            loop {
                line.clear();
                match handle.read_line(&mut line) {
                    Ok(0) => {
                        let _ = tx.send(Event::UserEof);
                        break;
                    }
                    Ok(_) => {
                        let trimmed = line.trim_end_matches(['\n', '\r']).to_string();
                        let _ = tx.send(Event::UserLine(trimmed));
                    }
                    Err(_) => {
                        let _ = tx.send(Event::UserEof);
                        break;
                    }
                }
            }
        });
    }

    spawn_ticker(tx.clone(), Duration::from_millis(settings.tick_ms));

    let idle_after = Duration::from_millis(settings.idle_after_ms);
    let mut queue = PromptQueue::new();
    let mut phase = AgentPhase::Busy; // assume booting
    let mut last_activity = Instant::now();
    let mut busy_since = Instant::now();
    let mut agent_buf: Vec<u8> = Vec::new();
    let mut force_idle = false; // set when a ready-marker is seen in the output
    let mut next_group: u64 = 0; // monotonic id minted per user prompt
    let mut current_group: u64 = 0; // group the agent's current output belongs to (0 = boot)
    let mut term_size = (settings.cols, settings.rows); // polled on each Tick to catch a resize

    let flush_agent = |buf: &mut Vec<u8>, memlog: &Option<MemoryLog>, group: u64| {
        if buf.is_empty() {
            return;
        }
        if let Some(ml) = memlog {
            let text = String::from_utf8_lossy(buf);
            if !text.trim().is_empty() {
                ml.agent(&text, group);
            }
        }
        buf.clear();
    };

    print_banner(settings, arbiter.as_ref(), &memlog);

    while let Ok(ev) = rx.recv() {
        match ev {
            Event::AgentOutput(bytes) => {
                if phase == AgentPhase::Idle {
                    busy_since = Instant::now();
                }
                phase = AgentPhase::Busy;
                last_activity = Instant::now();
                agent_buf.extend_from_slice(&bytes);
                // Smarter idle: if the output now ends with a "ready" prompt, the
                // agent is waiting for us — go idle on the next tick instead of
                // waiting out the full silence window.
                if tail_is_ready(&agent_buf, &settings.ready_markers) {
                    force_idle = true;
                }
            }
            Event::Tick => {
                // Cheap poll for a terminal resize (piggybacking on the existing
                // tick rather than a signal handler) and propagate it to the
                // child, so a rich TUI agent redraws at the real size instead
                // of whatever size delphin happened to start at.
                let queried = terminal_size::terminal_size().map(|(w, h)| (w.0, h.0));
                if let Some(new_size) = resized(term_size, queried) {
                    term_size = new_size;
                    let _ = master.resize(PtySize {
                        rows: term_size.1,
                        cols: term_size.0,
                        pixel_width: 0,
                        pixel_height: 0,
                    });
                }

                let idle_now = is_idle_now(
                    force_idle,
                    last_activity.elapsed(),
                    busy_since.elapsed(),
                    idle_after,
                    Duration::from_millis(settings.min_busy_ms),
                    &agent_buf,
                );
                if phase == AgentPhase::Busy && idle_now {
                    force_idle = false;
                    phase = AgentPhase::Idle;
                    flush_agent(&mut agent_buf, &memlog, current_group);
                    if let Some(item) = queue.pop() {
                        notice!("agent idle -> sending queued #{}: {}", item.id, item.text);
                        current_group = item.group;
                        if let Some(ml) = &memlog {
                            ml.system(
                                &format!("released #{}: {}", item.id, item.text),
                                current_group,
                            );
                        }
                        send_prompt(&mut writer, item.text.as_bytes(), &settings.submit);
                        phase = AgentPhase::Busy;
                        busy_since = Instant::now();
                        last_activity = Instant::now();
                    }
                }
            }
            Event::UserLine(text) => {
                if text.trim().is_empty() {
                    send_prompt(&mut writer, b"", &settings.submit);
                    continue;
                }
                let ctx = Decision {
                    phase,
                    text: text.clone(),
                    busy_elapsed_ms: if phase == AgentPhase::Busy {
                        busy_since.elapsed().as_millis()
                    } else {
                        0
                    },
                    queue_len: queue.len(),
                };
                let verdict = arbiter.decide(&ctx);
                next_group += 1;
                let group = next_group;
                if let Some(ml) = &memlog {
                    ml.user(&text, verdict.as_str(), group);
                }
                match verdict {
                    Verdict::SendNow => {
                        current_group = group;
                        send_prompt(&mut writer, text.as_bytes(), &settings.submit);
                        phase = AgentPhase::Busy;
                        busy_since = Instant::now();
                        last_activity = Instant::now();
                    }
                    Verdict::Interrupt => {
                        notice!("interrupting agent for: {}", text);
                        if !settings.interrupt_bytes.is_empty() {
                            let _ = writer.write_all(&settings.interrupt_bytes);
                            let _ = writer.flush();
                            thread::sleep(Duration::from_millis(150));
                        }
                        // the interrupted partial reply belongs to the old prompt
                        flush_agent(&mut agent_buf, &memlog, current_group);
                        current_group = group;
                        send_prompt(&mut writer, text.as_bytes(), &settings.submit);
                        phase = AgentPhase::Busy;
                        busy_since = Instant::now();
                        last_activity = Instant::now();
                    }
                    Verdict::Enqueue => {
                        let id = queue.push(text, group);
                        notice!("agent busy -> queued #{} ({} waiting)", id, queue.len());
                    }
                    Verdict::Stream => {
                        notice!("agent busy -> streaming: {}", text);
                        // ponytail: output attribution in live mode is best-effort —
                        // flush what the prior prompt produced, then the newest prompt
                        // owns what follows.
                        flush_agent(&mut agent_buf, &memlog, current_group);
                        current_group = group;
                        send_prompt(&mut writer, text.as_bytes(), &settings.submit);
                    }
                }
            }
            Event::UserEof => {
                notice!("stdin closed, shutting down agent");
                break;
            }
            Event::AgentExited => {
                // A flight recorder that silently stops when the agent crashes
                // is a worse gap than the crash itself — record it, and name
                // any prompts that were queued but never delivered.
                let msg = if queue.is_empty() {
                    "agent exited".to_string()
                } else {
                    format!(
                        "agent exited ({} prompt(s) still queued, never sent)",
                        queue.len()
                    )
                };
                notice!("{msg}");
                if let Some(ml) = &memlog {
                    ml.system(&msg, current_group);
                }
                break;
            }
        }
    }

    flush_agent(&mut agent_buf, &memlog, current_group);
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

/// Returns the newly-queried terminal size if it differs from `current`, or
/// `None` if it's unchanged or unavailable (e.g. stdout isn't a real tty).
/// Pulled out as a pure function so "did it actually change" is testable
/// without a real terminal.
fn resized(current: (u16, u16), queried: Option<(u16, u16)>) -> Option<(u16, u16)> {
    match queried {
        Some(new) if new != current => Some(new),
        _ => None,
    }
}

const WORDMARK: &str = r#"██████╗ ███████╗██╗     ██████╗ ██╗  ██╗██╗███╗   ██╗
██╔══██╗██╔════╝██║     ██╔══██╗██║  ██║██║████╗  ██║
██║  ██║█████╗  ██║     ██████╔╝███████║██║██╔██╗ ██║
██║  ██║██╔══╝  ██║     ██╔═══╝ ██╔══██║██║██║╚██╗██║
██████╔╝███████╗███████╗██║     ██║  ██║██║██║ ╚████║
╚═════╝ ╚══════╝╚══════╝╚═╝     ╚═╝  ╚═╝╚═╝╚═╝  ╚═══╝"#;

/// Print a startup banner so it's unmistakable delphin is active and wrapping
/// the agent, not just the agent's own splash screen. Printed once, to stderr
/// (like `notice!`), so it never pollutes the agent's real stdout stream.
fn print_banner(settings: &Settings, arbiter: &dyn Arbiter, memlog: &Option<MemoryLog>) {
    eprintln!("{}", WORDMARK.bright_blue());
    eprintln!("\x1b[2m           keep talking while it thinks\x1b[0m");
    eprintln!();
    let rule = "─".repeat(48);
    eprintln!("\x1b[2m{rule}\x1b[0m");
    eprintln!(
        "\x1b[2m  wrapping   \x1b[0m{}",
        settings.agent_command.join(" ")
    );
    eprintln!("\x1b[2m  arbiter    \x1b[0m{}", arbiter.name());
    eprintln!("\x1b[2m  interrupt  \x1b[0m{}", settings.interrupt_label);
    eprintln!("\x1b[2m  idle       \x1b[0m>{}ms", settings.idle_after_ms);
    eprintln!(
        "\x1b[2m  memory     \x1b[0m{}",
        match memlog {
            Some(ml) => format!("on  ({})", ml.db_path().display()),
            None => "off".to_string(),
        }
    );
    eprintln!();
    notice!("type normally; busy prompts may queue or stream depending on the arbiter. Say 'stop'/'wait' to barge in. Ctrl-D to quit.");
}

/// Best-effort write to the agent. Once the agent process is gone (e.g. an
/// interrupt terminated it) writes fail with EIO — that's a normal shutdown, not
/// an error, and the reader thread reports `AgentExited` so we quit cleanly. So a
/// failed write is deliberately ignored rather than propagated.
fn send_prompt(writer: &mut Box<dyn Write + Send>, body: &[u8], submit: &[u8]) {
    let _ = writer.write_all(body);
    let _ = writer.write_all(submit);
    let _ = writer.flush();
}

fn spawn_ticker(tx: Sender<Event>, period: Duration) {
    thread::spawn(move || loop {
        thread::sleep(period);
        if tx.send(Event::Tick).is_err() {
            break;
        }
    });
}

/// Is the agent idle right now? A ready marker (`force_idle`) is definitive;
/// otherwise idle requires silence, past the min-busy floor, with a settled
/// (not mid-line) tail. Takes elapsed durations rather than `Instant`s so it's
/// a pure function — deterministically testable without real sleeping, and the
/// one place golden-transcript tests replay against.
fn is_idle_now(
    force_idle: bool,
    last_activity_elapsed: Duration,
    busy_since_elapsed: Duration,
    idle_after: Duration,
    min_busy: Duration,
    agent_buf: &[u8],
) -> bool {
    force_idle || {
        let quiet = last_activity_elapsed >= idle_after;
        let past_floor = busy_since_elapsed >= min_busy;
        let settled = ends_line(agent_buf) || last_activity_elapsed >= idle_after * MIDLINE_GRACE;
        quiet && past_floor && settled
    }
}

/// True if the (ANSI-stripped) tail of the agent's output ends with one of the
/// configured ready markers — i.e. the agent is showing a prompt and waiting.
fn tail_is_ready(buf: &[u8], markers: &[String]) -> bool {
    if markers.is_empty() {
        return false;
    }
    let start = buf.len().saturating_sub(256);
    let clean = crate::memory::strip_ansi(&String::from_utf8_lossy(&buf[start..]));
    let tail = clean.trim_end();
    markers.iter().any(|m| {
        let m = m.trim_end();
        !m.is_empty() && tail.ends_with(m)
    })
}

/// True if the agent's output tail ends a line (finished a chunk) rather than
/// mid-line (still drawing). ANSI codes and trailing spaces/tabs are ignored; an
/// empty/whitespace tail counts as ended (nothing meaningful is pending).
fn ends_line(buf: &[u8]) -> bool {
    if buf.is_empty() {
        return true;
    }
    let start = buf.len().saturating_sub(256);
    let clean = crate::memory::strip_ansi(&String::from_utf8_lossy(&buf[start..]));
    let tail = clean.trim_end_matches([' ', '\t']);
    tail.is_empty() || tail.ends_with('\n')
}

#[cfg(test)]
mod tests {
    use super::{ends_line, is_idle_now, resized, tail_is_ready, MIDLINE_GRACE};

    #[test]
    fn resize_only_propagates_on_a_real_change() {
        assert_eq!(
            resized((80, 24), Some((80, 24))),
            None,
            "unchanged size -> no resize"
        );
        assert_eq!(
            resized((80, 24), Some((100, 30))),
            Some((100, 30)),
            "a real change should be reported"
        );
        assert_eq!(
            resized((80, 24), None),
            None,
            "not a real terminal (no size available) -> no-op, keep the fallback"
        );
    }

    #[test]
    fn ends_line_detects_mid_line_output() {
        assert!(ends_line(b"a finished line\n"));
        assert!(ends_line(b"trailing spaces then newline\n   "));
        assert!(ends_line(b"")); // nothing pending
                                 // mid-line: no newline -> still drawing
        assert!(!ends_line(b"half a lin"));
        assert!(!ends_line("done\nyou> ".as_bytes()));
        // ANSI colour codes are ignored
        assert!(!ends_line(b"\x1b[32myou> \x1b[0m"));
    }

    #[test]
    fn ready_marker_detection() {
        let markers = vec!["you> ".to_string(), "❯ ".to_string()];
        // prompt at the end (with ANSI + trailing space) -> ready
        assert!(tail_is_ready(
            b"thinking...\n\x1b[32myou> \x1b[0m",
            &markers
        ));
        assert!(tail_is_ready("done\n❯ ".as_bytes(), &markers));
        // mid-output, not a trailing prompt -> not ready
        assert!(!tail_is_ready(
            b"you> typed something then more output",
            &markers
        ));
        // no markers configured -> never ready (pure silence detection)
        assert!(!tail_is_ready(b"you> ", &[]));
    }

    // ---- golden-transcript regression tests -------------------------------
    //
    // Replay real captured agent output through `is_idle_now` using virtual
    // millisecond time instead of real sleeping. Unlike tests/e2e.rs (a real
    // PTY on real wall-clock time), these run in microseconds and can't flake,
    // so the idle-detection logic can be refactored with confidence. The
    // bytes below are exactly what `examples/mock-agent.sh` printed for a real
    // turn, captured via:
    //   printf 'add a login endpoint\n' | bash examples/mock-agent.sh

    enum TranscriptEvent {
        /// Agent output arrives at this virtual ms.
        Output(u64, &'static [u8]),
        /// An idle-detector tick fires at this virtual ms.
        Tick(u64),
    }

    /// Replay `events` (non-decreasing `at_ms`) against `is_idle_now`, Busy
    /// from t=0. Returns the virtual ms of the first Busy->Idle transition, or
    /// `None` if it never goes idle.
    fn first_idle_transition(
        events: &[TranscriptEvent],
        idle_after_ms: u64,
        min_busy_ms: u64,
        ready_markers: &[String],
    ) -> Option<u64> {
        let idle_after = std::time::Duration::from_millis(idle_after_ms);
        let min_busy = std::time::Duration::from_millis(min_busy_ms);
        let mut agent_buf: Vec<u8> = Vec::new();
        let mut last_activity_ms: u64 = 0;
        let mut force_idle = false;

        for ev in events {
            match *ev {
                TranscriptEvent::Output(at_ms, bytes) => {
                    agent_buf.extend_from_slice(bytes);
                    last_activity_ms = at_ms;
                    if tail_is_ready(&agent_buf, ready_markers) {
                        force_idle = true;
                    }
                }
                TranscriptEvent::Tick(at_ms) => {
                    let last_activity_elapsed =
                        std::time::Duration::from_millis(at_ms.saturating_sub(last_activity_ms));
                    // busy_since = 0 in every fixture here (a single turn).
                    let busy_since_elapsed = std::time::Duration::from_millis(at_ms);
                    if is_idle_now(
                        force_idle,
                        last_activity_elapsed,
                        busy_since_elapsed,
                        idle_after,
                        min_busy,
                        &agent_buf,
                    ) {
                        return Some(at_ms);
                    }
                }
            }
        }
        None
    }

    const BANNER: &[u8] = b"mock-agent ready. ask me anything.\nyou> ";
    // No trailing newline -> the agent is still drawing.
    const THINKING_MIDLINE: &[u8] = b"thinking about: add a login endpoint .....";
    // Ends with a newline -> settled, but no ready marker in this chunk.
    const ANSWER_SETTLED: &[u8] =
        b"\nanswer: I considered \"add a login endpoint\" and here is a verbatim-ish reply.\n";

    #[test]
    fn ready_marker_goes_idle_on_the_next_tick() {
        let events = [
            TranscriptEvent::Output(0, BANNER),
            TranscriptEvent::Tick(10),
        ];
        let markers = vec!["you> ".to_string()];
        assert_eq!(
            first_idle_transition(&events, 800, 0, &markers),
            Some(10),
            "a ready marker must flip idle on the next tick, not wait out the silence timer"
        );
    }

    #[test]
    fn mid_line_output_defers_idle_past_the_silence_window() {
        let events = [
            TranscriptEvent::Output(0, THINKING_MIDLINE),
            TranscriptEvent::Tick(850), // idle_after (800ms) elapsed, but tail is mid-line
        ];
        assert_eq!(
            first_idle_transition(&events, 800, 0, &[]),
            None,
            "mid-line output must not release on the first silence tick"
        );
    }

    #[test]
    fn mid_line_output_eventually_releases_via_the_bounded_grace_window() {
        let events = [
            TranscriptEvent::Output(0, THINKING_MIDLINE),
            TranscriptEvent::Tick(800 * (MIDLINE_GRACE as u64) + 10),
        ];
        assert!(
            first_idle_transition(&events, 800, 0, &[]).is_some(),
            "the mid-line guard must be bounded, or a newline-less agent wedges the queue forever"
        );
    }

    #[test]
    fn min_busy_floor_holds_off_release_even_once_output_settles() {
        let events = [
            TranscriptEvent::Output(0, ANSWER_SETTLED),
            TranscriptEvent::Tick(850), // idle_after elapsed & tail settled -> would release without the floor
        ];
        assert_eq!(
            first_idle_transition(&events, 800, 1000, &[]),
            None,
            "min-busy-ms must hold off release even once idle_after elapses and the tail is settled"
        );
    }

    #[test]
    fn min_busy_floor_releases_once_the_floor_elapses() {
        let events = [
            TranscriptEvent::Output(0, ANSWER_SETTLED),
            TranscriptEvent::Tick(850),  // before the floor
            TranscriptEvent::Tick(1010), // past the floor
        ];
        assert_eq!(first_idle_transition(&events, 800, 1000, &[]), Some(1010));
    }

    #[test]
    fn ready_marker_is_definitive_even_before_the_min_busy_floor() {
        // Documented behavior: a ready marker means "the agent is waiting for
        // input," a hard signal, so it bypasses --min-busy-ms entirely — even
        // a multi-second floor won't hold off release once it's seen.
        let events = [
            TranscriptEvent::Output(0, BANNER),
            TranscriptEvent::Tick(10),
        ];
        let markers = vec!["you> ".to_string()];
        assert_eq!(
            first_idle_transition(&events, 800, 5000, &markers),
            Some(10),
            "a ready marker should bypass --min-busy-ms, per its documented 'definitive' semantics"
        );
    }

    // ---- multi-agent conformance: a silent (zero-output) worker ------------
    //
    // Real bytes captured from `examples/silent-agent.sh` — an agent shape
    // meaningfully different from mock-agent.sh: it produces NO output at all
    // while working (no dots, no streaming) and never prints a ready marker.
    // Captured via: printf 'add a login endpoint\n' | bash examples/silent-agent.sh

    // Ends with '\n' from startup, then nothing more arrives for the whole
    // (simulated) work period below — the mid-line guard has nothing to see.
    const SILENT_AGENT_BANNER: &[u8] = b"silent-agent ready.\n";

    #[test]
    fn silent_tool_call_releases_prematurely_without_a_min_busy_floor() {
        // Honest, named limitation (roadmap 0.1: "tool-call pauses that look
        // idle but aren't"): with no output at all during a silent work
        // period, the mid-line guard can't help — the buffer's tail was
        // already settled before the silence began. Default settings release
        // at idle_after, even though the agent is still genuinely working.
        let events = [
            TranscriptEvent::Output(0, SILENT_AGENT_BANNER),
            TranscriptEvent::Tick(850), // idle_after (800ms) elapsed; the real work runs ~3s
        ];
        assert_eq!(
            first_idle_transition(&events, 800, 0, &[]),
            Some(850),
            "known gap: zero-output work is indistinguishable from idle without a min-busy floor"
        );
    }

    #[test]
    fn min_busy_floor_protects_a_silent_tool_call_when_tuned_to_it() {
        // The existing mechanism (--min-busy-ms) does cover this, once a user
        // tunes it to how long this particular agent's silent work actually
        // takes — the fix a --agent preset for a known silent CLI would set.
        let events = [
            TranscriptEvent::Output(0, SILENT_AGENT_BANNER),
            TranscriptEvent::Tick(850), // still within the known ~3s work window
            TranscriptEvent::Tick(4010), // past a 4s floor tuned to this agent
        ];
        assert_eq!(
            first_idle_transition(&events, 800, 4000, &[]),
            Some(4010),
            "--min-busy-ms, tuned to the agent, should hold off release until the real work is likely done"
        );
    }
}
