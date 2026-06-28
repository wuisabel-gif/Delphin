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
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use crate::arbiter::{AgentPhase, Arbiter, Decision, Verdict};
use crate::memory::MemoryLog;
use crate::queue::PromptQueue;

pub struct Settings {
    pub agent_command: Vec<String>,
    pub idle_after_ms: u64,
    pub tick_ms: u64,
    pub submit: Vec<u8>,
    pub interrupt_bytes: Vec<u8>,
    pub interrupt_label: String,
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
    let _master = pair.master;

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

    let flush_agent = |buf: &mut Vec<u8>, memlog: &Option<MemoryLog>| {
        if buf.is_empty() {
            return;
        }
        if let Some(ml) = memlog {
            let text = String::from_utf8_lossy(buf);
            if !text.trim().is_empty() {
                ml.agent(&text);
            }
        }
        buf.clear();
    };

    notice!(
        "orbiting `{}` | arbiter={} | idle>{}ms | interrupt={} | memory={}",
        settings.agent_command.join(" "),
        arbiter.name(),
        settings.idle_after_ms,
        settings.interrupt_label,
        match &memlog {
            Some(ml) => format!("on ({})", ml.db_path().display()),
            None => "off".to_string(),
        }
    );
    notice!("type normally; prompts queue while the agent thinks. Say 'stop'/'wait' to barge in. Ctrl-D to quit.");

    while let Ok(ev) = rx.recv() {
        match ev {
            Event::AgentOutput(bytes) => {
                if phase == AgentPhase::Idle {
                    busy_since = Instant::now();
                }
                phase = AgentPhase::Busy;
                last_activity = Instant::now();
                agent_buf.extend_from_slice(&bytes);
            }
            Event::Tick => {
                if phase == AgentPhase::Busy && last_activity.elapsed() >= idle_after {
                    phase = AgentPhase::Idle;
                    flush_agent(&mut agent_buf, &memlog);
                    if let Some(item) = queue.pop() {
                        notice!("agent idle -> sending queued #{}: {}", item.id, item.text);
                        if let Some(ml) = &memlog {
                            ml.system(&format!("released #{}: {}", item.id, item.text));
                        }
                        send_prompt(&mut writer, item.text.as_bytes(), &settings.submit)?;
                        phase = AgentPhase::Busy;
                        busy_since = Instant::now();
                        last_activity = Instant::now();
                    }
                }
            }
            Event::UserLine(text) => {
                if text.trim().is_empty() {
                    send_prompt(&mut writer, b"", &settings.submit)?;
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
                if let Some(ml) = &memlog {
                    ml.user(&text, verdict.as_str());
                }
                match verdict {
                    Verdict::SendNow => {
                        send_prompt(&mut writer, text.as_bytes(), &settings.submit)?;
                        phase = AgentPhase::Busy;
                        busy_since = Instant::now();
                        last_activity = Instant::now();
                    }
                    Verdict::Interrupt => {
                        notice!("interrupting agent for: {}", text);
                        if !settings.interrupt_bytes.is_empty() {
                            writer.write_all(&settings.interrupt_bytes)?;
                            writer.flush()?;
                            thread::sleep(Duration::from_millis(150));
                        }
                        flush_agent(&mut agent_buf, &memlog);
                        send_prompt(&mut writer, text.as_bytes(), &settings.submit)?;
                        phase = AgentPhase::Busy;
                        busy_since = Instant::now();
                        last_activity = Instant::now();
                    }
                    Verdict::Enqueue => {
                        let id = queue.push(text);
                        notice!("agent busy -> queued #{} ({} waiting)", id, queue.len());
                    }
                }
            }
            Event::UserEof => {
                notice!("stdin closed, shutting down agent");
                break;
            }
            Event::AgentExited => {
                notice!("agent exited");
                break;
            }
        }
    }

    flush_agent(&mut agent_buf, &memlog);
    let _ = child.kill();
    let _ = child.wait();
    Ok(())
}

fn send_prompt(writer: &mut Box<dyn Write + Send>, body: &[u8], submit: &[u8]) -> Result<()> {
    writer.write_all(body)?;
    writer.write_all(submit)?;
    writer.flush()?;
    Ok(())
}

fn spawn_ticker(tx: Sender<Event>, period: Duration) {
    thread::spawn(move || loop {
        thread::sleep(period);
        if tx.send(Event::Tick).is_err() {
            break;
        }
    });
}
