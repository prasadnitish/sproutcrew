//! Process supervision (plan section 11). Each child leads its own process group. Identity is
//! PID + PGID + start time, recorded before dispatch. Nothing is ever killed by executable name.

use std::io::{self, BufRead, BufReader, Read};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcId {
    pub pid: i32,
    pub pgid: i32,
    /// `ps -o lstart` of the process. One-second resolution.
    pub start_time: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sig {
    Int,
    Term,
    Kill,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SignalError {
    /// The PID now belongs to a different process.
    IdentityMismatch,
    /// Neither the leader nor any group member exists.
    Gone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PsRow {
    pub pid: i32,
    pub ppid: i32,
    pub pgid: i32,
    pub zombie: bool,
}

fn ps(args: &[&str]) -> Option<String> {
    let out = Command::new("/bin/ps")
        .args(args)
        .env_clear()
        .stderr(Stdio::null())
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Rows of `ps -A -o pid=,ppid=,pgid=,stat=`. A fourth column starting with `Z` marks a zombie.
pub fn parse_ps_table(text: &str) -> Vec<PsRow> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (pid, ppid, pgid) = (
                f.next()?.parse().ok()?,
                f.next()?.parse().ok()?,
                f.next()?.parse().ok()?,
            );
            Some(PsRow {
                pid,
                ppid,
                pgid,
                zombie: f.next().is_some_and(|s| s.starts_with('Z')),
            })
        })
        .collect()
}

fn snapshot() -> Vec<PsRow> {
    parse_ps_table(&ps(&["-A", "-o", "pid=,ppid=,pgid=,stat="]).unwrap_or_default())
}

/// Every descendant of `root` by parent links, however far.
pub fn descendants_of(rows: &[PsRow], root: i32) -> Vec<i32> {
    let mut out = Vec::new();
    let mut frontier = vec![root];
    while let Some(p) = frontier.pop() {
        for r in rows.iter().filter(|r| r.ppid == p && r.pid != root) {
            if !out.contains(&r.pid) {
                out.push(r.pid);
                frontier.push(r.pid);
            }
        }
    }
    out
}

pub fn start_time_of(pid: i32) -> Option<String> {
    let s = ps(&["-o", "lstart=", "-p", &pid.to_string()])?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// Live (non-zombie) members of a process group, from a `ps` snapshot.
pub fn group_members(pgid: i32) -> Vec<i32> {
    snapshot()
        .into_iter()
        .filter(|r| r.pgid == pgid && !r.zombie)
        .map(|r| r.pid)
        .collect()
}

pub fn is_running(id: &ProcId) -> bool {
    if id.start_time.is_empty() || start_time_of(id.pid).as_deref() != Some(id.start_time.as_str())
    {
        return false;
    }
    !snapshot().iter().any(|r| r.pid == id.pid && r.zombie)
}

fn signo(sig: Sig) -> i32 {
    match sig {
        Sig::Int => libc::SIGINT,
        Sig::Term => libc::SIGTERM,
        Sig::Kill => libc::SIGKILL,
    }
}

fn check_leader(id: &ProcId) -> Result<bool, SignalError> {
    match start_time_of(id.pid) {
        Some(t) if t == id.start_time => Ok(true),
        Some(_) => Err(SignalError::IdentityMismatch),
        None => Ok(false),
    }
}

/// Signals the recorded group. A reused PID is refused. If the leader has gone, the group is
/// still ours while it has members, since the kernel does not reuse a group ID in that case.
pub fn signal_group(id: &ProcId, sig: Sig) -> Result<(), SignalError> {
    let leader_alive = check_leader(id)?;
    if !leader_alive && group_members(id.pgid).is_empty() {
        return Err(SignalError::Gone);
    }
    match unsafe { libc::killpg(id.pgid, signo(sig)) } {
        0 => Ok(()),
        _ => Err(SignalError::Gone),
    }
}

pub fn signal_leader(id: &ProcId, sig: Sig) -> Result<(), SignalError> {
    if !check_leader(id)? {
        return Err(SignalError::Gone);
    }
    match unsafe { libc::kill(id.pid, signo(sig)) } {
        0 => Ok(()),
        _ => Err(SignalError::Gone),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndedBy {
    /// Exited after the provider interrupt.
    Interrupt,
    Term,
    Kill,
    AlreadyExited,
}

#[derive(Debug, Clone, Copy)]
pub struct Graces {
    pub after_interrupt: Duration,
    pub after_term: Duration,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Line {
    Out(String),
    Err(String),
}

pub struct Supervised {
    pub id: ProcId,
    pub child: Child,
    pub stdin: Option<ChildStdin>,
    pub lines: Receiver<Line>,
    pub timed_out: Arc<AtomicBool>,
}

fn pump<R: Read + Send + 'static>(r: R, tx: Sender<Line>, wrap: fn(String) -> Line) {
    std::thread::spawn(move || {
        let mut r = BufReader::new(r);
        let mut buf = Vec::new();
        while matches!(r.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
            let s = String::from_utf8_lossy(&buf)
                .trim_end_matches(['\n', '\r'])
                .to_string();
            buf.clear();
            if tx.send(wrap(s)).is_err() {
                break;
            }
        }
    });
}

/// A child with its pipe handles still owned by the caller, so they can be closed on purpose.
pub struct RawChild {
    pub id: ProcId,
    pub child: Child,
    pub stdin: Option<ChildStdin>,
    pub stdout: Option<std::process::ChildStdout>,
    pub stderr: Option<std::process::ChildStderr>,
}

/// Like [`spawn`], but without reader threads.
pub fn spawn_raw(
    program: &Path,
    args: &[String],
    env: &[(String, String)],
    cwd: &Path,
) -> io::Result<RawChild> {
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .current_dir(cwd)
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = child.id() as i32;
    let pgid = unsafe { libc::getpgid(pid) };
    let id = ProcId {
        pid,
        pgid: if pgid > 0 { pgid } else { pid },
        start_time: start_time_of(pid).unwrap_or_default(),
    };
    let (stdin, stdout, stderr) = (child.stdin.take(), child.stdout.take(), child.stderr.take());
    Ok(RawChild {
        id,
        child,
        stdin,
        stdout,
        stderr,
    })
}

/// Spawns from an empty environment plus `env`, in a new process group, with piped stdio.
pub fn spawn(
    program: &Path,
    args: &[String],
    env: &[(String, String)],
    cwd: &Path,
) -> io::Result<Supervised> {
    let RawChild {
        id,
        child,
        stdin,
        stdout,
        stderr,
    } = spawn_raw(program, args, env, cwd)?;
    let (tx, lines) = channel();
    pump(stdout.expect("piped"), tx.clone(), Line::Out);
    pump(stderr.expect("piped"), tx, Line::Err);
    Ok(Supervised {
        id,
        child,
        stdin,
        lines,
        timed_out: Arc::new(AtomicBool::new(false)),
    })
}

fn sleep_until_exit(child: &mut Child, d: Duration) -> bool {
    let end = Instant::now() + d;
    loop {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return true;
        }
        if Instant::now() >= end {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

impl Supervised {
    pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// An external wall-clock bound that does not depend on the event loop. At `timeout` it sets
    /// `timed_out`, giving the loop `grace` to cancel gracefully, and then ends the group itself.
    pub fn arm_watchdog(&mut self, timeout: Duration, grace: Duration) {
        let (id, flag) = (self.id.clone(), self.timed_out.clone());
        std::thread::spawn(move || {
            std::thread::sleep(timeout);
            if !is_running(&id) && group_members(id.pgid).is_empty() {
                return;
            }
            flag.store(true, Ordering::SeqCst);
            std::thread::sleep(grace);
            for sig in [Sig::Term, Sig::Kill] {
                if signal_group(&id, sig).is_err() {
                    return;
                }
                std::thread::sleep(grace);
            }
        });
    }

    /// Cancel order: provider interrupt, wait, SIGTERM to the group, wait, SIGKILL to the group.
    /// Descendants that left the group are recorded beforehand and ended by identity.
    pub fn cancel(&mut self, interrupt: impl FnOnce(&mut Supervised), graces: Graces) -> EndedBy {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            self.sweep_group(&[], graces);
            return EndedBy::AlreadyExited;
        }
        let escaped = self.escaped_descendants();
        interrupt(self);
        let ended = if sleep_until_exit(&mut self.child, graces.after_interrupt) {
            EndedBy::Interrupt
        } else {
            let _ = signal_group(&self.id, Sig::Term);
            if sleep_until_exit(&mut self.child, graces.after_term) {
                EndedBy::Term
            } else {
                let _ = signal_group(&self.id, Sig::Kill);
                let _ = self.child.wait();
                EndedBy::Kill
            }
        };
        self.sweep_group(&escaped, graces);
        ended
    }

    /// After the leader is gone: end what is left of the group, then the recorded escapees.
    fn sweep_group(&mut self, escaped: &[ProcId], graces: Graces) {
        if !group_members(self.id.pgid).is_empty() {
            let _ = signal_group(&self.id, Sig::Term);
            std::thread::sleep(graces.after_term.min(Duration::from_millis(200)));
            let _ = signal_group(&self.id, Sig::Kill);
        }
        for e in escaped {
            let _ = signal_leader(e, Sig::Kill);
        }
    }

    /// Descendants that left the process group, with identity, from a fresh snapshot.
    pub fn escaped_descendants(&self) -> Vec<ProcId> {
        let rows = snapshot();
        descendants_of(&rows, self.id.pid)
            .into_iter()
            .filter_map(|pid| rows.iter().find(|r| r.pid == pid))
            .filter(|r| r.pgid != self.id.pgid && !r.zombie)
            .filter_map(|r| {
                Some(ProcId {
                    pid: r.pid,
                    pgid: r.pgid,
                    start_time: start_time_of(r.pid)?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn sh(script: &str) -> Supervised {
        let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
        spawn(
            Path::new("/bin/sh"),
            &["-c".into(), script.into()],
            &env,
            Path::new("/private/tmp"),
        )
        .unwrap()
    }

    fn quick() -> Graces {
        Graces {
            after_interrupt: Duration::from_millis(300),
            after_term: Duration::from_millis(300),
        }
    }

    fn wait_gone(id: &ProcId) -> bool {
        let t = Instant::now();
        while t.elapsed() < Duration::from_secs(3) {
            if !is_running(id) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    #[test]
    fn a_raw_child_leads_its_group_and_hands_back_its_pipes() {
        let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
        let mut r = spawn_raw(
            Path::new("/bin/sh"),
            &["-c".into(), "echo hello; sleep 5".into()],
            &env,
            Path::new("/private/tmp"),
        )
        .unwrap();
        assert_eq!(r.id.pid, r.id.pgid);
        assert!(!r.id.start_time.is_empty());
        let mut line = String::new();
        std::io::BufRead::read_line(
            &mut std::io::BufReader::new(r.stdout.take().unwrap()),
            &mut line,
        )
        .unwrap();
        assert_eq!(line.trim(), "hello");
        assert!(r.stdin.is_some() && r.stderr.is_some());
        let _ = signal_group(&r.id, Sig::Kill);
        let _ = r.child.wait();
    }

    #[test]
    fn parses_ps_rows_and_walks_descendants() {
        let rows =
            parse_ps_table("  10   1  10\n  11  10  10\n  12  11  12\n  99   1  99\nbad line\n");
        assert_eq!(rows.len(), 4);
        let mut d = descendants_of(&rows, 10);
        d.sort();
        assert_eq!(d, vec![11, 12]);
        assert!(descendants_of(&rows, 99).is_empty());
    }

    #[test]
    fn child_leads_its_own_group_with_a_recorded_start_time() {
        let mut p = sh("sleep 5");
        assert_eq!(p.id.pid, p.id.pgid);
        assert!(!p.id.start_time.is_empty());
        assert_ne!(p.id.pgid, unsafe { libc::getpgrp() });
        p.cancel(|_| {}, quick());
    }

    #[test]
    fn child_environment_is_exactly_what_was_passed() {
        std::env::set_var("BFF_LEAK_CANARY", "yes");
        let env = vec![
            ("PATH".to_string(), "/usr/bin:/bin".to_string()),
            ("ONLY".to_string(), "1".to_string()),
        ];
        let p = spawn(
            Path::new("/usr/bin/env"),
            &[],
            &env,
            Path::new("/private/tmp"),
        )
        .unwrap();
        let mut seen = Vec::new();
        while let Ok(l) = p.lines.recv_timeout(Duration::from_secs(3)) {
            if let Line::Out(s) = l {
                seen.push(s);
            }
        }
        seen.sort();
        assert!(seen.contains(&"ONLY=1".to_string()), "{seen:?}");
        assert!(
            !seen.iter().any(|s| s.contains("BFF_LEAK_CANARY")),
            "{seen:?}"
        );
    }

    #[test]
    fn stdout_and_stderr_arrive_as_lines() {
        let p = sh("echo out1; echo err1 1>&2");
        let mut got = Vec::new();
        while let Ok(l) = p.lines.recv_timeout(Duration::from_secs(3)) {
            got.push(l);
        }
        assert!(got.contains(&Line::Out("out1".into())));
        assert!(got.contains(&Line::Err("err1".into())));
    }

    #[test]
    fn cancel_ends_the_whole_group_including_grandchildren() {
        let mut p = sh("sleep 60 & sleep 60 & wait");
        std::thread::sleep(Duration::from_millis(300));
        let members = group_members(p.id.pgid);
        assert!(members.len() >= 3, "{members:?}");
        let ended = p.cancel(|_| {}, quick());
        assert_ne!(ended, EndedBy::AlreadyExited);
        let t = Instant::now();
        while !group_members(p.id.pgid).is_empty() && t.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(group_members(p.id.pgid).is_empty());
    }

    #[test]
    fn interrupt_is_tried_first() {
        let mut p = sh("sleep 60");
        let ended = p.cancel(
            |s| {
                let _ = signal_leader(&s.id, Sig::Int);
            },
            quick(),
        );
        assert_eq!(ended, EndedBy::Interrupt);
    }

    #[test]
    fn a_child_that_ignores_term_is_killed() {
        let mut p = sh("trap '' INT TERM; sleep 60 & wait; sleep 60");
        std::thread::sleep(Duration::from_millis(300));
        let ended = p.cancel(
            |s| {
                let _ = signal_leader(&s.id, Sig::Int);
            },
            quick(),
        );
        assert_eq!(ended, EndedBy::Kill);
        assert!(wait_gone(&p.id));
    }

    #[test]
    fn already_exited_child_needs_no_signal() {
        let mut p = sh("true");
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            p.cancel(|_| panic!("no interrupt needed"), quick()),
            EndedBy::AlreadyExited
        );
    }

    #[test]
    fn a_reused_pid_is_never_signalled() {
        let mut p = sh("sleep 60");
        let mut wrong = p.id.clone();
        wrong.start_time = "Mon Jan  1 00:00:00 2001".into();
        assert!(!is_running(&wrong));
        assert_eq!(
            signal_group(&wrong, Sig::Kill),
            Err(SignalError::IdentityMismatch)
        );
        assert_eq!(
            signal_leader(&wrong, Sig::Kill),
            Err(SignalError::IdentityMismatch)
        );
        assert!(is_running(&p.id), "the real process must be untouched");
        p.cancel(|_| {}, quick());
    }

    #[test]
    fn watchdog_sets_the_flag_then_ends_the_group_itself() {
        let mut p = sh("sleep 60");
        p.arm_watchdog(Duration::from_millis(200), Duration::from_millis(300));
        std::thread::sleep(Duration::from_millis(350));
        assert!(p.timed_out.load(Ordering::SeqCst));
        assert!(wait_gone(&p.id));
    }

    #[test]
    fn watchdog_leaves_a_finished_run_alone() {
        let mut p = sh("true");
        p.arm_watchdog(Duration::from_millis(100), Duration::from_millis(100));
        let _ = p.child.wait();
        std::thread::sleep(Duration::from_millis(400));
        assert!(!p.timed_out.load(Ordering::SeqCst));
    }

    #[test]
    fn detects_a_descendant_that_left_the_group() {
        // macOS has no setsid binary, so perl starts a process in a new session.
        let mut p = sh("/usr/bin/perl -MPOSIX -e 'POSIX::setsid(); exec q(sleep), q(60)' & wait");
        std::thread::sleep(Duration::from_millis(700));
        let escaped = p.escaped_descendants();
        assert_eq!(escaped.len(), 1, "{escaped:?}");
        assert_ne!(escaped[0].pgid, p.id.pgid);
        let escapee = escaped[0].clone();
        p.cancel(|_| {}, quick());
        assert!(wait_gone(&escapee), "cancel must end the recorded escapee");
    }
}
