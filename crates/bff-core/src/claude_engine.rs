//! The Claude engine (route R2): `claude -p --restricted` on the plan login, one process per step. Before a
//! step it checks the login is `claude.ai` and reads the plan windows with `/usage` (no tokens), and refuses
//! at 50 percent in any window. Workers get file tools inside their own task folder. Reviewers get read
//! tools only. Usage comes from the event stream, and a missing figure is unknown, never zero.

use crate::claude::{
    launch_args, mcp_config_json, settings_for, Event, Launch, Policy, Session, SystemPrompt,
};
use crate::claude_run::{run_recorded, ClaudeRun, RunSpec};
use crate::claude_usage::parse_usage;
use crate::claude_usage::UsageReading;
use crate::engine::{not_this_route, Engine, Request, ReviewRun, WorkerResult};
use crate::env::{build_child_env, EnvSpec};
use crate::ledger::{Baseline, ClaudeLedger, Status as LedgerStatus};
use crate::meters::{Entry, Meter, Status};
use crate::plan::Plan;
use crate::stop::Stopper;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const CLAUDE_EXTRA: &[(&str, &str)] = &[
    ("CLAUDE_CODE_DISABLE_AUTO_MEMORY", "1"),
    ("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1"),
    ("CLAUDE_AGENT_SDK_DISABLE_BUILTIN_AGENTS", "1"),
];
const WRITER_TOOLS: &[&str] = &["Read", "Write", "Edit", "Glob", "Grep"];
const READER_TOOLS: &[&str] = &["Read", "Glob", "Grep"];
/// A web step can write its result and read the web. It has no tool that reads local files.
const WEB_TOOLS: &[&str] = &["Write", "WebSearch", "WebFetch"];

pub struct ClaudeEngine {
    pub claude: PathBuf,
    pub home: String,
    /// Per-step run files and `TMPDIR`.
    pub scratch: PathBuf,
    /// Holds `children.jsonl`, the record the orphan sweep reads.
    pub state_dir: PathBuf,
    pub timeout_secs: u64,
    /// Set to stop before the next step and to interrupt the running one.
    pub cancel: Arc<AtomicBool>,
}

struct Outcome {
    completed: bool,
    note: String,
    structured: Option<Value>,
    usage: Vec<Entry>,
}

fn session_id() -> String {
    use std::io::Read;
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        let _ = f.read_exact(&mut b);
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..]
    )
}

/// The text the model produced, from the raw stream: assistant and user text blocks and the result string.
fn texts_of(raw: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for line in raw {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        match v["type"].as_str() {
            Some("assistant") | Some("user") => match &v["message"]["content"] {
                Value::String(s) => out.push(s.clone()),
                Value::Array(a) => out.extend(
                    a.iter()
                        .filter_map(|b| b["text"].as_str().map(str::to_string)),
                ),
                _ => {}
            },
            Some("result") => out.extend(v["result"].as_str().map(str::to_string)),
            _ => {}
        }
    }
    out
}

fn entries(
    step: &str,
    model: &str,
    tokens: Option<(f64, Status)>,
    window: Option<f64>,
) -> Vec<Entry> {
    let entry = |meter, value: Option<f64>, status, detail: &str| Entry {
        step: step.to_string(),
        meter,
        value,
        status: if value.is_some() {
            status
        } else {
            Status::Unknown
        },
        detail: detail.to_string(),
    };
    vec![
        entry(
            Meter::Tokens,
            tokens.map(|t| t.0),
            tokens.map_or(Status::Unknown, |t| t.1),
            model,
        ),
        entry(Meter::PlanWindow, window, Status::Reported, "claude"),
        // On the plan login the CLI's dollar figure is an estimate, not a charge.
        entry(
            Meter::ApiDollars,
            None,
            Status::Unknown,
            "plan route, no dollar charge",
        ),
    ]
}

impl ClaudeEngine {
    fn env_for(&self, tmp: &Path) -> Result<Vec<(String, String)>, String> {
        let mut spec =
            EnvSpec::from_ambient(&crate::tools::child_path(&self.claude), tmp.to_path_buf());
        spec.home = self.home.clone();
        spec.extra = CLAUDE_EXTRA
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        build_child_env(&spec).map_err(|e| format!("{e:?}"))
    }

    fn record(&self, id: &crate::process::ProcId) {
        let _ = std::fs::create_dir_all(&self.state_dir);
        let _ = crate::sweep::record(&self.state_dir.join("children.jsonl"), "claude", id);
    }

    /// `claude auth status` must report the plan login.
    fn check_login(&self, env: &[(String, String)], cwd: &Path) -> Result<(), String> {
        let out = std::process::Command::new(&self.claude)
            .args(["auth", "status"])
            .env_clear()
            .envs(env.iter().cloned())
            .current_dir(cwd)
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|e| format!("cannot start claude: {e}"))?;
        let v: Value = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
        let mut stopper = Stopper::new();
        stopper.check_claude_auth(v["authMethod"].as_str().unwrap_or(""));
        match stopper.reason() {
            Some(r) => Err(format!("billing route stop: {r:?}")),
            None => Ok(()),
        }
    }

    /// The plan windows right now, from `/usage`. It costs no tokens.
    fn read_usage(&self, env: &[(String, String)], cwd: &Path) -> Result<UsageReading, String> {
        let args: Vec<String> = [
            "-p",
            "--restricted",
            "--tools",
            "",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-hook-events",
            "--model",
            "claude-haiku-5-5",
            "--effort",
            "low",
            "--max-turns",
            "1",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let policy = Policy::default();
        let spec = RunSpec::new(
            &self.claude,
            &args,
            env,
            cwd,
            &policy,
            Duration::from_secs(60),
            Duration::from_secs(3),
        )
        .stdin("/usage");
        let r = run_recorded(&spec, &|id| self.record(id))
            .map_err(|e| format!("usage reading: {e}"))?;
        let now = crate::clock::now();
        texts_of(&r.raw)
            .iter()
            .find_map(|t| parse_usage(t, now))
            .ok_or_else(|| "the plan usage reading could not be parsed".to_string())
    }

    /// The login and plan windows right now, through two short calls that use no tokens.
    pub fn status(&self) -> Result<Value, String> {
        let base = self.scratch.join("status");
        let (tmp, probe) = (base.join("tmp"), base.join("probe"));
        for d in [&tmp, &probe] {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let env = self.env_for(&tmp)?;
        self.check_login(&env, &probe)?;
        let u = self.read_usage(&env, &probe)?;
        Ok(serde_json::json!({
            "login": "claude.ai",
            "windows": u.windows.iter().map(|w| serde_json::json!({"window": w.label, "used_percent": w.percent})).collect::<Vec<_>>(),
        }))
    }

    fn failed(&self, req: &Request, note: String) -> Outcome {
        Outcome {
            completed: false,
            note,
            structured: None,
            usage: entries(req.step, req.model, None, None),
        }
    }

    fn one_run(
        &self,
        req: &Request,
        tools: &[&str],
        role: &str,
        schema: Option<&Value>,
    ) -> Outcome {
        if self.cancel.load(Ordering::SeqCst) {
            return self.failed(
                req,
                "paused: a stop was requested before the step started".into(),
            );
        }
        let base = self.scratch.join(req.step);
        let (run_dir, tmp, probe) = (base.join("run"), base.join("tmp"), base.join("probe"));
        for d in [&run_dir, &tmp, &probe] {
            if let Err(e) = std::fs::create_dir_all(d) {
                return self.failed(req, format!("cannot create {}: {e}", d.display()));
            }
        }
        let env = match self.env_for(&tmp) {
            Ok(e) => e,
            Err(e) => return self.failed(req, e),
        };
        if let Err(e) = self.check_login(&env, &probe) {
            return self.failed(req, e);
        }
        match self.read_usage(&env, &probe).and_then(|u| {
            u.gate(crate::clock::now())
                .map_err(|b| format!("plan usage blocks dispatch: {b:?}"))
        }) {
            Ok(()) => {}
            Err(e) => return self.failed(req, e),
        }

        let files = (
            run_dir.join("settings.json"),
            run_dir.join("mcp.json"),
            run_dir.join("role.md"),
        );
        let wrote = std::fs::write(&files.0, settings_for(tools).to_string())
            .and_then(|_| std::fs::write(&files.1, mcp_config_json(None).to_string()))
            .and_then(|_| std::fs::write(&files.2, role));
        if let Err(e) = wrote {
            return self.failed(req, format!("cannot write the run files: {e}"));
        }
        let (settings_s, mcp_s, role_s) = (
            files.0.to_string_lossy(),
            files.1.to_string_lossy(),
            files.2.to_string_lossy(),
        );
        let schema_s = schema.map(Value::to_string);
        let sid = session_id();
        let args = launch_args(&Launch {
            tools,
            settings_path: &settings_s,
            mcp_config_path: &mcp_s,
            model: req.model,
            effort: req.effort,
            max_turns: 12,
            system_prompt: SystemPrompt::File(&role_s),
            json_schema: schema_s.as_deref(),
            session: Session::New(&sid),
            stream_input: false,
        });
        let policy = Policy {
            tools: tools.iter().map(|t| t.to_string()).collect(),
            mcp_servers: vec![],
            implicit: if schema.is_some() {
                vec!["StructuredOutput".to_string()]
            } else {
                vec![]
            },
        };
        let cancel = self.cancel.clone();
        let stop_now = move || cancel.load(Ordering::SeqCst);
        let spec = RunSpec::new(
            &self.claude,
            &args,
            &env,
            req.task_dir,
            &policy,
            Duration::from_secs(self.timeout_secs),
            Duration::from_secs(5),
        )
        .stdin(req.prompt)
        .cancel_when(&stop_now);
        let r = match run_recorded(&spec, &|id| self.record(id)) {
            Ok(r) => r,
            Err(e) => return self.failed(req, format!("cannot start claude: {e}")),
        };
        let window = self.read_usage(&env, &probe).ok().map(|u| u.max_percent());
        self.outcome(req, &r, schema.is_some(), window)
    }

    fn outcome(
        &self,
        req: &Request,
        r: &ClaudeRun,
        wants_json: bool,
        window: Option<f64>,
    ) -> Outcome {
        let mut ledger = ClaudeLedger::new(Baseline::default());
        let mut result: Option<&Value> = None;
        for e in &r.events {
            match e {
                Event::Assistant {
                    message_id, usage, ..
                } => ledger.on_assistant(message_id, usage),
                Event::Stream(v) => ledger.on_stream_event(v),
                Event::Result(v) => {
                    ledger.on_result(v);
                    result = Some(v);
                }
                _ => {}
            }
        }
        let report = ledger.report();
        let tokens = report.tokens.map(|t| {
            let status = match report.tokens_status {
                LedgerStatus::Reported => Status::Reported,
                LedgerStatus::Estimated => Status::Estimated,
                LedgerStatus::Unknown => Status::Unknown,
            };
            (
                (t.input + t.output + t.cache_creation + t.cache_read) as f64,
                status,
            )
        });
        let usage = entries(req.step, req.model, tokens, window);
        let succeeded = result.is_some_and(|v| v["subtype"] == "success" && v["is_error"] == false);
        let structured = result.and_then(|v| {
            v.get("structured_output").cloned().or_else(|| {
                wants_json
                    .then(|| {
                        v["result"]
                            .as_str()
                            .and_then(|t| serde_json::from_str(t).ok())
                    })
                    .flatten()
            })
        });
        let (completed, note) = if !r.violations.is_empty() {
            (false, format!("policy violation: {:?}", r.violations))
        } else if r.timed_out {
            (false, crate::fallback::timed_out_note(self.timeout_secs))
        } else if r.interrupted_by_harness || self.cancel.load(Ordering::SeqCst) {
            (false, "paused: stopped on request".to_string())
        } else if succeeded {
            (true, "completed".to_string())
        } else {
            (
                false,
                format!("the run did not finish (exit {:?})", r.exit_code),
            )
        };
        Outcome {
            completed,
            note,
            structured,
            usage,
        }
    }
}

impl Engine for ClaudeEngine {
    fn label(&self) -> &'static str {
        "live"
    }

    fn plan_windows(&mut self) -> Option<Vec<(String, f64)>> {
        let v = self.status().ok()?;
        Some(
            v["windows"]
                .as_array()?
                .iter()
                .filter_map(|w| {
                    Some((
                        w["window"].as_str()?.to_string(),
                        w["used_percent"].as_f64()?,
                    ))
                })
                .collect(),
        )
    }

    fn propose_plan(&mut self, _brief: &str) -> Result<(Plan, Vec<Entry>), String> {
        Err("the Claude engine does not plan; the lead runs on Codex".into())
    }

    fn run_worker(&mut self, req: &Request) -> WorkerResult {
        if req.route != "R2" {
            return WorkerResult {
                completed: false,
                note: not_this_route("Claude", req, "R2"),
                usage: entries(req.step, req.model, None, None),
            };
        }
        let role = "You are a worker. Follow the instructions exactly. Write only inside the current directory.";
        let tools = if req.web { WEB_TOOLS } else { WRITER_TOOLS };
        let o = self.one_run(req, tools, role, None);
        WorkerResult {
            completed: o.completed,
            note: o.note,
            usage: o.usage,
        }
    }

    fn run_review(&mut self, req: &Request, schema: &Value) -> ReviewRun {
        if req.route != "R2" {
            return ReviewRun {
                completed: false,
                note: not_this_route("Claude", req, "R2"),
                json: None,
                usage: entries(req.step, req.model, None, None),
            };
        }
        let role = "You are a reviewer. You may read files and must not change any.";
        let o = self.one_run(req, READER_TOOLS, role, Some(schema));
        ReviewRun {
            completed: o.completed && o.structured.is_some(),
            note: o.note,
            json: o.structured,
            usage: o.usage,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fake {
        root: PathBuf,
    }

    /// A stand-in `claude`. `auth` is the authMethod it reports, `pct` the session percent `/usage` shows,
    /// `init_tools` replaces the tool list the worker run reports, and `usage_in_result` controls whether
    /// the run reports token usage at all.
    fn fake(
        tag: &str,
        auth: &str,
        pct: u32,
        init_tools: Option<&str>,
        usage_in_result: bool,
    ) -> Fake {
        let root =
            std::env::temp_dir().join(format!("bff-claude-eng-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(root.join("state")).unwrap();
        let usage = if usage_in_result {
            r#","usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":2,"cache_read_input_tokens":3}"#
        } else {
            ""
        };
        let assistant_usage = if usage_in_result {
            r#","usage":{"input_tokens":10,"output_tokens":5,"cache_creation_input_tokens":2,"cache_read_input_tokens":3}"#
        } else {
            ""
        };
        let writer_tools = init_tools.unwrap_or(r#""Read","Write","Edit","Glob","Grep""#);
        let script = format!(
            r#"#!/bin/sh
D="$(dirname "$0")"
if [ "$1" = "auth" ]; then
  echo "$@" >> "$D/calls.log"
  printf '%s\n' '{{"authMethod":"{auth}","subscriptionType":"max"}}'
  exit 0
fi
echo "-p" >> "$D/calls.log"
prompt=$(cat)
if [ "$prompt" = "/usage" ]; then
  printf '%s\n' '{{"type":"system","subtype":"init","session_id":"u","model":"m","tools":[],"mcp_servers":[],"apiKeySource":"none"}}'
  printf '%s\n' '{{"type":"assistant","message":{{"id":"mu","model":"m","content":[{{"type":"text","text":"Current session: {pct}% used\nCurrent week (all models): 20% used · resets Nov 3 at 5:59am (America/Los_Angeles)"}}],"usage":{{"input_tokens":0,"output_tokens":0}}}}}}'
  printf '%s\n' '{{"type":"result","subtype":"success","is_error":false}}'
  exit 0
fi
case "$prompt" in
  *"You are a reviewer"*)
    h=$(shasum -a 256 artifact.md | cut -d' ' -f1)
    printf '%s\n' '{{"type":"system","subtype":"init","session_id":"r","model":"m","tools":["Read","Glob","Grep","StructuredOutput"],"mcp_servers":[],"apiKeySource":"none"}}'
    printf '%s\n' '{{"type":"assistant","message":{{"id":"m2","model":"m","content":[{{"type":"text","text":"ok"}}]{assistant_usage}}}}}'
    printf '%s\n' '{{"type":"result","subtype":"success","is_error":false,"structured_output":{{"reviewed_sha256":"'"$h"'","findings":[]}}{usage}}}'
    ;;
  *)
    if [ -f "$D/slow" ]; then
      printf '# Partial\n' > artifact.md
      printf '%s\n' '{{"type":"system","subtype":"init","session_id":"w","model":"m","tools":["Read","Write","Edit","Glob","Grep"],"mcp_servers":[],"apiKeySource":"none"}}'
      echo $$ > "$D/child.pid"
      sleep 30
      exit 0
    fi
    printf '# Note\nbody\n' > artifact.md
    printf '%s\n' '{{"type":"system","subtype":"init","session_id":"w","model":"m","tools":[{writer_tools}],"mcp_servers":[],"apiKeySource":"none"}}'
    printf '%s\n' '{{"type":"assistant","message":{{"id":"m1","model":"m","content":[{{"type":"text","text":"done"}}]{assistant_usage}}}}}'
    printf '%s\n' '{{"type":"result","subtype":"success","is_error":false,"result":"done"{usage}}}'
    ;;
esac
"#
        );
        let p = root.join("claude");
        std::fs::write(&p, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        Fake { root }
    }

    fn engine(f: &Fake) -> ClaudeEngine {
        ClaudeEngine {
            claude: f.root.join("claude"),
            home: f.root.to_string_lossy().into_owned(),
            scratch: f.root.join("scratch"),
            state_dir: f.root.join("state"),
            timeout_secs: 20,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    fn task_dir(f: &Fake, name: &str) -> PathBuf {
        let d = f.root.join(name);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn req<'a>(route: &'a str, dir: &'a Path, prompt: &'a str) -> Request<'a> {
        Request {
            web: false,
            route,
            step: "t1",
            task_dir: dir,
            prompt,
            model: "claude-sonnet-5-5",
            effort: "medium",
        }
    }

    fn calls(f: &Fake) -> Vec<String> {
        std::fs::read_to_string(f.root.join("calls.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn value(u: &[Entry], m: Meter) -> (Option<f64>, Status) {
        let e = u.iter().find(|e| e.meter == m).unwrap();
        (e.value, e.status)
    }

    #[test]
    fn a_worker_writes_its_artifact_and_usage_is_reported_per_meter() {
        let f = fake("worker", "claude.ai", 3, None, true);
        let dir = task_dir(&f, "w");
        let r = engine(&f).run_worker(&req("R2", &dir, "Write the note."));
        assert!(r.completed, "{}", r.note);
        assert!(dir.join("artifact.md").is_file());
        assert_eq!(
            value(&r.usage, Meter::Tokens),
            (Some(20.0), Status::Reported)
        );
        assert_eq!(
            value(&r.usage, Meter::PlanWindow),
            (Some(20.0), Status::Reported)
        );
        // Plan route: a client-side dollar estimate is not a charge.
        assert_eq!(value(&r.usage, Meter::ApiDollars).1, Status::Unknown);
        let plan = r
            .usage
            .iter()
            .find(|e| e.meter == Meter::PlanWindow)
            .unwrap();
        assert_eq!(plan.detail, "claude");
    }

    #[test]
    fn the_child_is_recorded_before_it_runs_so_a_crash_can_be_swept() {
        let f = fake("record", "claude.ai", 3, None, true);
        let dir = task_dir(&f, "w");
        engine(&f).run_worker(&req("R2", &dir, "Write."));
        let rec = std::fs::read_to_string(f.root.join("state/children.jsonl")).unwrap();
        assert!(rec.lines().any(|l| l.contains("claude")), "{rec}");
    }

    #[test]
    fn a_reviewer_reports_the_hash_of_the_file_it_read() {
        let f = fake("review", "claude.ai", 3, None, true);
        let dir = task_dir(&f, "r");
        std::fs::write(dir.join("artifact.md"), "hello\n").unwrap();
        let schema = json!({"type": "object"});
        let r = engine(&f).run_review(&req("R2", &dir, "You are a reviewer. Read it."), &schema);
        assert!(r.completed, "{}", r.note);
        let j = r.json.unwrap();
        assert_eq!(j["reviewed_sha256"], crate::engine::sha256_hex(b"hello\n"));
        assert_eq!(j["findings"], json!([]));
        assert_eq!(
            std::fs::read_to_string(dir.join("artifact.md")).unwrap(),
            "hello\n"
        );
    }

    #[test]
    fn a_request_for_the_other_route_is_refused_without_starting_anything() {
        let f = fake("route", "claude.ai", 3, None, true);
        let dir = task_dir(&f, "x");
        let r = engine(&f).run_worker(&req("R1", &dir, "Write."));
        assert!(!r.completed);
        assert!(r.note.contains("R2") && r.note.contains("R1"), "{}", r.note);
        assert!(calls(&f).is_empty());
    }

    #[test]
    fn a_login_that_is_not_the_plan_login_stops_before_any_model_call() {
        let f = fake("auth", "api-key", 3, None, true);
        let dir = task_dir(&f, "a");
        let r = engine(&f).run_worker(&req("R2", &dir, "Write."));
        assert!(!r.completed);
        assert!(r.note.to_lowercase().contains("route"), "{}", r.note);
        assert_eq!(
            calls(&f),
            vec!["auth status".to_string()],
            "only the auth check ran"
        );
        assert!(!dir.join("artifact.md").exists());
    }

    #[test]
    fn a_plan_window_at_fifty_percent_blocks_dispatch() {
        let f = fake("usage", "claude.ai", 50, None, true);
        let dir = task_dir(&f, "u");
        let r = engine(&f).run_worker(&req("R2", &dir, "Write."));
        assert!(!r.completed);
        assert!(
            r.note.contains("50") || r.note.to_lowercase().contains("usage"),
            "{}",
            r.note
        );
        assert!(!dir.join("artifact.md").exists(), "the worker never ran");
        // The probe ran, the worker did not: auth, then one /usage call.
        assert_eq!(calls(&f).iter().filter(|c| *c == "-p").count(), 1);
    }

    #[test]
    fn a_tool_outside_the_allowlist_at_init_cancels_the_run() {
        let f = fake(
            "tool",
            "claude.ai",
            3,
            Some(r#""Read","Write","Bash""#),
            true,
        );
        let dir = task_dir(&f, "t");
        let r = engine(&f).run_worker(&req("R2", &dir, "Write."));
        assert!(!r.completed);
        assert!(r.note.contains("Bash"), "{}", r.note);
    }

    #[test]
    fn a_run_with_no_usage_reads_unknown_not_zero() {
        let f = fake("nousage", "claude.ai", 3, None, false);
        let dir = task_dir(&f, "n");
        let r = engine(&f).run_worker(&req("R2", &dir, "Write."));
        assert_eq!(value(&r.usage, Meter::Tokens), (None, Status::Unknown));
    }

    #[test]
    fn a_pause_request_stops_before_the_next_step_starts() {
        let f = fake("cancel", "claude.ai", 3, None, true);
        let dir = task_dir(&f, "c");
        let e = engine(&f);
        e.cancel.store(true, Ordering::SeqCst);
        let mut e = e;
        let r = e.run_worker(&req("R2", &dir, "Write."));
        assert!(!r.completed);
        assert!(
            r.note.to_lowercase().contains("paus") || r.note.to_lowercase().contains("cancel"),
            "{}",
            r.note
        );
        assert!(calls(&f).is_empty(), "nothing was started");
    }

    #[test]
    fn the_claude_engine_does_not_plan() {
        let f = fake("plan", "claude.ai", 3, None, true);
        assert!(engine(&f).propose_plan("x").is_err());
    }

    #[test]
    fn status_reports_the_login_and_every_plan_window_and_refuses_the_wrong_login() {
        let f = fake("status", "claude.ai", 7, None, true);
        let v = engine(&f).status().unwrap();
        assert_eq!(v["login"], "claude.ai");
        assert_eq!(
            v["windows"][0],
            json!({"window": "session", "used_percent": 7.0})
        );
        assert_eq!(v["windows"][1]["window"], "week (all models)");
        let bad = fake("status-bad", "api-key", 7, None, true);
        assert!(engine(&bad).status().is_err());
    }

    #[test]
    fn a_stop_request_during_a_silent_stretch_ends_the_child_and_keeps_the_partial_work() {
        let f = fake("slow", "claude.ai", 3, None, true);
        std::fs::write(f.root.join("slow"), "").unwrap();
        let dir = task_dir(&f, "s");
        let mut e = engine(&f);
        e.timeout_secs = 60;
        let cancel = e.cancel.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            cancel.store(true, Ordering::SeqCst);
        });
        let started = std::time::Instant::now();
        let r = e.run_worker(&req("R2", &dir, "Write."));
        t.join().unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "the stop took {:?}",
            started.elapsed()
        );
        assert!(!r.completed);
        assert!(r.note.to_lowercase().contains("paus"), "{}", r.note);
        assert_eq!(
            std::fs::read_to_string(dir.join("artifact.md")).unwrap(),
            "# Partial\n",
            "partial work is kept"
        );
        // The usage entries exist and nothing reads as zero.
        assert!(r
            .usage
            .iter()
            .any(|u| u.meter == Meter::Tokens && u.value.is_none()));
        // The child is gone.
        let pid: i32 = std::fs::read_to_string(f.root.join("child.pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .unwrap()
            .success();
        assert!(!alive, "child {pid} is still running");
    }
}
