//! Orphan sweep (plan section 11). Child identities are recorded before dispatch. After a crash or a
//! force quit, the next launch ends only recorded process groups whose start time still matches, so a
//! reused PID is never touched and nothing is ever killed by name.

use crate::process::{signal_group, ProcId, Sig};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::time::Duration;

/// Appends one identity to the record file.
pub fn record(path: &Path, label: &str, id: &ProcId) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(
        f,
        "{}",
        json!({"label": label, "pid": id.pid, "pgid": id.pgid, "start_time": id.start_time})
    )
}

#[derive(Debug, PartialEq, Eq)]
pub struct Swept {
    pub label: String,
    pub pid: i32,
    /// `true` if it was still running and has been ended.
    pub ended: bool,
}

/// Ends every recorded process that is still the same process, then empties the record.
pub fn sweep(path: &Path, grace: Duration) -> std::io::Result<Vec<Swept>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for v in text
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
    {
        let (Some(pid), Some(pgid), Some(start)) = (
            v["pid"].as_i64(),
            v["pgid"].as_i64(),
            v["start_time"].as_str(),
        ) else {
            continue;
        };
        let id = ProcId {
            pid: pid as i32,
            pgid: pgid as i32,
            start_time: start.to_string(),
        };
        // signal_group refuses a reused PID, and reports Gone when nothing of the group remains.
        let ended = if signal_group(&id, Sig::Term).is_ok() {
            std::thread::sleep(grace);
            let _ = signal_group(&id, Sig::Kill);
            true
        } else {
            false
        };
        out.push(Swept {
            label: v["label"].as_str().unwrap_or_default().to_string(),
            pid: id.pid,
            ended,
        });
    }
    std::fs::write(path, "")?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::{is_running, spawn, Graces};

    fn tmp(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("bff-sweep-{}-{tag}.jsonl", std::process::id()))
    }

    fn sleeper() -> crate::process::Supervised {
        let env = vec![("PATH".to_string(), "/usr/bin:/bin".to_string())];
        spawn(
            Path::new("/bin/sh"),
            &["-c".into(), "sleep 60 & sleep 60 & wait".into()],
            &env,
            Path::new("/private/tmp"),
        )
        .unwrap()
    }

    fn wait_gone(id: &ProcId) -> bool {
        let t = std::time::Instant::now();
        while t.elapsed() < Duration::from_secs(4) {
            if !is_running(id) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        false
    }

    #[test]
    fn a_recorded_orphan_is_ended_with_its_whole_group_and_the_record_is_emptied() {
        let p = tmp("orphan");
        let mut s = sleeper();
        std::thread::sleep(Duration::from_millis(300));
        record(&p, "codex", &s.id).unwrap();
        let id = s.id.clone();
        // The app "crashes": nothing cancels the child, which keeps running in its own group.
        assert!(is_running(&id));
        let swept = sweep(&p, Duration::from_millis(300)).unwrap();
        assert_eq!(
            swept,
            vec![Swept {
                label: "codex".into(),
                pid: id.pid,
                ended: true
            }]
        );
        assert!(wait_gone(&id));
        let t = std::time::Instant::now();
        while !crate::process::group_members(id.pgid).is_empty()
            && t.elapsed() < Duration::from_secs(3)
        {
            std::thread::sleep(Duration::from_millis(30));
        }
        assert!(
            crate::process::group_members(id.pgid).is_empty(),
            "the whole group must be gone"
        );
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "");
        let _ = s.cancel(
            |_| {},
            Graces {
                after_interrupt: Duration::from_millis(100),
                after_term: Duration::from_millis(100),
            },
        );
    }

    #[test]
    fn a_reused_pid_is_left_alone() {
        let p = tmp("reuse");
        let mut s = sleeper();
        std::thread::sleep(Duration::from_millis(200));
        let mut wrong = s.id.clone();
        wrong.start_time = "Mon Jan  1 00:00:00 2001".into();
        record(&p, "impostor", &wrong).unwrap();
        let swept = sweep(&p, Duration::from_millis(200)).unwrap();
        assert_eq!(
            swept,
            vec![Swept {
                label: "impostor".into(),
                pid: wrong.pid,
                ended: false
            }]
        );
        assert!(is_running(&s.id), "the real process must be untouched");
        let _ = s.cancel(
            |_| {},
            Graces {
                after_interrupt: Duration::from_millis(100),
                after_term: Duration::from_millis(100),
            },
        );
    }

    #[test]
    fn an_already_finished_child_is_reported_as_not_ended_and_a_missing_record_is_fine() {
        let p = tmp("done");
        let mut s = sleeper();
        let id = s.id.clone();
        record(&p, "done", &id).unwrap();
        let _ = s.cancel(
            |_| {},
            Graces {
                after_interrupt: Duration::from_millis(100),
                after_term: Duration::from_millis(100),
            },
        );
        assert!(wait_gone(&id));
        assert!(!sweep(&p, Duration::from_millis(100)).unwrap()[0].ended);
        assert_eq!(
            sweep(&tmp("never-written"), Duration::from_millis(100)).unwrap(),
            vec![]
        );
    }

    #[test]
    fn a_damaged_line_is_skipped_not_fatal() {
        let p = tmp("damaged");
        std::fs::write(&p, "{ not json\n").unwrap();
        let s = sleeper();
        record(&p, "ok", &s.id).unwrap();
        let swept = sweep(&p, Duration::from_millis(200)).unwrap();
        assert_eq!(swept.len(), 1);
        assert_eq!(swept[0].label, "ok");
    }
}
