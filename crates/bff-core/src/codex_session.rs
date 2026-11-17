//! A Codex `app-server` stdio session. Keeps reading while it waits, answers server requests with
//! a refusal, and collects notifications and stderr in memory. Nothing raw is ever written to disk.

use crate::codex::{notification_line, parse_message, request_line, Message};
use crate::process::{Graces, Line, ProcId, Supervised};
use serde_json::{json, Value};
use std::io::Write;
use std::sync::mpsc::RecvTimeoutError;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub enum CallError {
    Timeout,
    Closed,
    Rpc { code: i64, message: String },
}

pub struct PumpEnd {
    pub matched: bool,
    pub closed: bool,
    pub timed_out: bool,
}

pub struct CodexSession {
    sup: Supervised,
    next_id: u64,
    /// Notifications in arrival order: method and params.
    pub notes: Vec<(String, Value)>,
    /// Methods of server-to-client requests, which m0 refuses.
    pub server_requests: Vec<String>,
    pub stderr: Vec<String>,
    pub bad_lines: usize,
    /// Notifications before this index have been through a `pump_until` predicate.
    cursor: usize,
}

enum Inbound {
    Response {
        id: Value,
        result: Result<Value, CallError>,
    },
    Note,
}

impl CodexSession {
    pub fn new(sup: Supervised) -> Self {
        CodexSession {
            sup,
            next_id: 1,
            notes: Vec::new(),
            server_requests: Vec::new(),
            stderr: Vec::new(),
            bad_lines: 0,
            cursor: 0,
        }
    }

    pub fn pid_identity(&self) -> ProcId {
        self.sup.id.clone()
    }

    pub fn supervised(&mut self) -> &mut Supervised {
        &mut self.sup
    }

    fn write(&mut self, text: &str) -> Result<(), CallError> {
        let stdin = self.sup.stdin.as_mut().ok_or(CallError::Closed)?;
        stdin
            .write_all(text.as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|_| CallError::Closed)
    }

    fn handle(&mut self, line: Line) -> Option<Inbound> {
        match line {
            Line::Err(s) => {
                self.stderr.push(s);
                None
            }
            Line::Out(s) => match parse_message(&s) {
                Ok(None) => None,
                Err(_) => {
                    self.bad_lines += 1;
                    None
                }
                Ok(Some(Message::Notification { method, params })) => {
                    self.notes.push((method, params));
                    Some(Inbound::Note)
                }
                Ok(Some(Message::Request { id, method, .. })) => {
                    self.server_requests.push(method);
                    let refusal =
                        json!({"id": id, "error": {"code": -32601, "message": "refused by m0"}});
                    let _ = self.write(&format!("{refusal}\n"));
                    None
                }
                Ok(Some(Message::Response { id, result })) => Some(Inbound::Response {
                    id,
                    result: Ok(result),
                }),
                Ok(Some(Message::Error { id, code, message })) => Some(Inbound::Response {
                    id,
                    result: Err(CallError::Rpc { code, message }),
                }),
            },
        }
    }

    pub fn call(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, CallError> {
        let id = self.next_id;
        self.next_id += 1;
        self.write(&request_line(id, method, &params))?;
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.sup.lines.recv_timeout(left) {
                Ok(line) => {
                    if let Some(Inbound::Response { id: got, result }) = self.handle(line) {
                        if got == json!(id) {
                            while let Ok(l) = self.sup.lines.try_recv() {
                                self.handle(l);
                            }
                            return result;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => return Err(CallError::Timeout),
                Err(RecvTimeoutError::Disconnected) => return Err(CallError::Closed),
            }
        }
    }

    pub fn notify(&mut self, method: &str, params: Option<Value>) {
        let _ = self.write(&notification_line(method, params.as_ref()));
    }

    /// Reads until `done(method, params)` is true for a notification, the child closes, or time runs out.
    /// Notifications already collected by earlier calls are tested first, each at most once.
    pub fn pump_until(
        &mut self,
        timeout: Duration,
        mut done: impl FnMut(&str, &Value) -> bool,
    ) -> PumpEnd {
        let deadline = Instant::now() + timeout;
        loop {
            while self.cursor < self.notes.len() {
                let (m, p) = &self.notes[self.cursor];
                self.cursor += 1;
                if done(m, p) {
                    return PumpEnd {
                        matched: true,
                        closed: false,
                        timed_out: false,
                    };
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match self.sup.lines.recv_timeout(left) {
                Ok(line) => {
                    self.handle(line);
                }
                Err(RecvTimeoutError::Timeout) => {
                    return PumpEnd {
                        matched: false,
                        closed: false,
                        timed_out: true,
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return PumpEnd {
                        matched: false,
                        closed: true,
                        timed_out: false,
                    }
                }
            }
        }
    }

    /// Closes stdin and walks the cancel ladder.
    pub fn shutdown(&mut self) {
        self.sup.stdin = None;
        let g = Graces {
            after_interrupt: Duration::from_millis(800),
            after_term: Duration::from_millis(800),
        };
        self.sup.cancel(|_| {}, g);
        while let Ok(l) = self.sup.lines.try_recv() {
            self.handle(l);
        }
    }
}
