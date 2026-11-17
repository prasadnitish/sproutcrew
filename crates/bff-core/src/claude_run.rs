//! Runs one `claude -p` child under supervision. Raw output stays in memory.

use crate::claude::{check_event, check_init, parse_line, Event, Policy, Violation};
use crate::process::{signal_leader, spawn, EndedBy, Graces, Line, ProcId, Sig};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

pub struct RunSpec<'a> {
    pub program: &'a Path,
    pub args: &'a [String],
    pub env: &'a [(String, String)],
    pub cwd: &'a Path,
    /// Sent on stdin after the child is spawned and its identity is known, then stdin is closed.
    pub stdin_text: Option<&'a str>,
    /// Wait this long for `system/init` before sending the prompt, and check it first.
    pub defer_init_wait: Option<Duration>,
    /// Interrupt (not a violation) the first time this returns true for an event.
    pub interrupt_when: Option<&'a dyn Fn(&Event) -> bool>,
    /// Polled every 100 ms, also while the child is silent. When it returns true the run is interrupted.
    pub cancel_when: Option<&'a dyn Fn() -> bool>,
    pub policy: &'a Policy,
    pub timeout: Duration,
    pub grace: Duration,
}

impl<'a> RunSpec<'a> {
    pub fn new(
        program: &'a Path,
        args: &'a [String],
        env: &'a [(String, String)],
        cwd: &'a Path,
        policy: &'a Policy,
        timeout: Duration,
        grace: Duration,
    ) -> Self {
        RunSpec {
            program,
            args,
            env,
            cwd,
            stdin_text: None,
            defer_init_wait: None,
            interrupt_when: None,
            cancel_when: None,
            policy,
            timeout,
            grace,
        }
    }

    pub fn stdin(mut self, text: &'a str) -> Self {
        self.stdin_text = Some(text);
        self
    }

    pub fn defer_until_init(mut self, wait: Duration) -> Self {
        self.defer_init_wait = Some(wait);
        self
    }

    pub fn interrupt_when(mut self, f: &'a dyn Fn(&Event) -> bool) -> Self {
        self.interrupt_when = Some(f);
        self
    }

    pub fn cancel_when(mut self, f: &'a dyn Fn() -> bool) -> Self {
        self.cancel_when = Some(f);
        self
    }
}

pub struct ClaudeRun {
    pub id: ProcId,
    pub raw: Vec<String>,
    pub events: Vec<Event>,
    pub stderr: Vec<String>,
    pub bad_lines: usize,
    pub violations: Vec<Violation>,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub ended_by: Option<EndedBy>,
    /// `Some(true)` if `system/init` arrived before the prompt was sent. `None` if not tested.
    pub init_before_prompt: Option<bool>,
    pub interrupted_by_harness: bool,
}

pub fn run(spec: &RunSpec) -> io::Result<ClaudeRun> {
    run_recorded(spec, &|_| {})
}

/// Like [`run`], and calls `record` with the child's identity before anything is sent to it, so a crash can
/// be swept on the next launch.
pub fn run_recorded(spec: &RunSpec, record: &dyn Fn(&ProcId)) -> io::Result<ClaudeRun> {
    let mut sup = spawn(spec.program, spec.args, spec.env, spec.cwd)?;
    record(&sup.id);
    sup.arm_watchdog(spec.timeout, spec.grace);
    let mut r = ClaudeRun {
        id: sup.id.clone(),
        raw: Vec::new(),
        events: Vec::new(),
        stderr: Vec::new(),
        bad_lines: 0,
        violations: Vec::new(),
        exit_code: None,
        timed_out: false,
        ended_by: None,
        init_before_prompt: None,
        interrupted_by_harness: false,
    };

    // Returns true if this line produced a policy violation.
    let take = |r: &mut ClaudeRun, line: Line, check: bool| -> bool {
        match line {
            Line::Err(s) => r.stderr.push(s),
            Line::Out(s) => {
                r.raw.push(s.clone());
                match parse_line(&s) {
                    Ok(None) => {}
                    Err(_) => r.bad_lines += 1,
                    Ok(Some(ev)) => {
                        let found = match &ev {
                            Event::Init(i) => check_init(i, spec.policy),
                            other => check_event(other, spec.policy),
                        };
                        r.events.push(ev);
                        if check && !found.is_empty() {
                            r.violations.extend(found);
                            return true;
                        }
                    }
                }
            }
        }
        false
    };
    let graces = Graces {
        after_interrupt: spec.grace,
        after_term: spec.grace,
    };
    let sigint = |s: &mut crate::process::Supervised| {
        let _ = signal_leader(&s.id, Sig::Int);
    };

    let mut cancel = false;
    // Optionally hold the prompt until init has been seen and checked.
    if let Some(wait) = spec.defer_init_wait {
        let deadline = Instant::now() + wait;
        let mut seen_init = false;
        while !cancel && !seen_init && Instant::now() < deadline {
            match sup.lines.recv_timeout(Duration::from_millis(50)) {
                Ok(line) => {
                    cancel = take(&mut r, line, true);
                    seen_init = r.events.iter().any(|e| matches!(e, Event::Init(_)));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        r.init_before_prompt = Some(seen_init);
    }
    if !cancel {
        if let (Some(text), Some(stdin)) = (spec.stdin_text, sup.stdin.as_mut()) {
            stdin.write_all(text.as_bytes())?;
            stdin.write_all(b"\n")?;
        }
    }
    sup.stdin = None;

    while !cancel {
        if sup.timed_out.load(Ordering::SeqCst) {
            r.timed_out = true;
            cancel = true;
            break;
        }
        if spec.cancel_when.is_some_and(|f| f()) {
            r.interrupted_by_harness = true;
            cancel = true;
            break;
        }
        match sup.lines.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                if take(&mut r, line, true) {
                    cancel = true;
                } else if let (Some(f), Some(ev)) = (spec.interrupt_when, r.events.last()) {
                    if !r.interrupted_by_harness && f(ev) {
                        r.interrupted_by_harness = true;
                        cancel = true;
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    if cancel {
        r.ended_by = Some(sup.cancel(sigint, graces));
    } else {
        // Output is closed. Give the child a moment to exit on its own, then end it.
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end && matches!(sup.try_wait(), Ok(None)) {
            std::thread::sleep(Duration::from_millis(20));
        }
        if matches!(sup.try_wait(), Ok(None)) {
            r.ended_by = Some(sup.cancel(sigint, graces));
        }
    }
    // Late output after the interrupt still counts: usage and the final result can arrive here.
    while let Ok(line) = sup.lines.recv_timeout(Duration::from_millis(300)) {
        take(&mut r, line, false);
    }
    r.exit_code = sup.child.wait().ok().and_then(|s| s.code());
    Ok(r)
}
