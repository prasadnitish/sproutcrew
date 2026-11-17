//! The live engine: Codex over stdio under the section 12 contract. Before any dispatch it checks the
//! account is `chatgpt`, reads the plan windows and credit balance, and refuses on a stop condition.
//! The credit baseline is saved on the first reading, so a change between sessions is still seen.

use crate::codex::{
    check_account, initialize_params, launch_args, mcp_server_names, read_only_policy,
    shell_set_names, thread_start_params, turn_start_params, writer_policy,
};
use crate::codex_session::CodexSession;
use crate::codex_turn::{run_turn, ItemRec, TurnOpts};
use crate::codex_types::GetAccountRateLimitsResponse;
use crate::decimal::Decimal;
use crate::engine::{not_this_route, Engine, Request, ReviewRun, WorkerResult};
use crate::env::{build_child_env, EnvSpec};
use crate::lead::{parse_plan, plan_prompt, plan_schema, usage_entries};
use crate::ledger::CodexTokenLedger;
use crate::meters::Entry;
use crate::plan::Plan;
use crate::process::spawn;
use crate::ratelimit::RateLimitState;
use crate::stop::Stopper;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const CALL: Duration = Duration::from_secs(30);

pub struct CodexEngine {
    pub codex: PathBuf,
    pub home: String,
    /// Scratch space for the server's own `cwd` and `TMPDIR`.
    pub scratch: PathBuf,
    /// Holds `baselines.json`.
    pub state_dir: PathBuf,
    pub lead_model: String,
    pub lead_effort: String,
    pub timeout_secs: u64,
    /// Set to interrupt the running turn, for a safe pause on quit.
    pub cancel: Arc<AtomicBool>,
}

fn account_hash(id: &str) -> String {
    format!("{:x}", Sha256::digest(id.as_bytes()))[..12].to_string()
}

fn final_text(items: &[ItemRec]) -> Option<String> {
    items
        .iter()
        .rev()
        .find(|i| i.kind == "agentMessage" && i.phase.as_deref() == Some("final_answer"))
        .or_else(|| items.iter().rev().find(|i| i.kind == "agentMessage"))
        .and_then(|i| i.text.clone())
}

#[derive(Clone, Copy)]
struct TurnSpec<'a> {
    step: &'a str,
    name: &'a str,
    cwd: &'a Path,
    model: &'a str,
    effort: &'a str,
    instructions: &'a str,
    prompt: &'a str,
    schema: Option<&'a Value>,
    writable: bool,
}

impl CodexEngine {
    /// Spawns the server, completes the handshake, checks the account, and gates on the limits.
    fn connect(&self, name: &str) -> Result<(CodexSession, RateLimitState), String> {
        let cwd = self.scratch.join(name).join("cwd");
        let tmp = self.scratch.join(name).join("tmp");
        for d in [&cwd, &tmp] {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let toml = std::fs::read_to_string(Path::new(&self.home).join(".codex/config.toml"))
            .unwrap_or_default();
        let args = launch_args(&mcp_server_names(&toml), &shell_set_names(&toml), false);
        drop(toml);
        let mut spec = EnvSpec::from_ambient(&crate::tools::child_path(&self.codex), tmp);
        spec.home = self.home.clone();
        let env = build_child_env(&spec).map_err(|e| format!("{e:?}"))?;
        let sup = spawn(&self.codex, &args, &env, &cwd)
            .map_err(|e| format!("cannot start codex: {e}"))?;
        // The identity is on disk before anything is sent, so a crash can be swept on the next launch.
        let _ = crate::sweep::record(&self.state_dir.join("children.jsonl"), "codex", &sup.id);
        let mut s = CodexSession::new(sup);
        let ready = (|| -> Result<RateLimitState, String> {
            s.call("initialize", initialize_params(), CALL)
                .map_err(|e| format!("initialize: {e:?}"))?;
            s.notify("initialized", None);
            let account = s
                .call("account/read", json!({"refreshToken": false}), CALL)
                .map_err(|e| format!("account/read: {e:?}"))?;
            check_account(&account)
                .map_err(|v| format!("the Codex account is not on the ChatGPT route: {v:?}"))?;
            let rl = s
                .call("account/rateLimits/read", Value::Null, CALL)
                .map_err(|e| format!("rateLimits: {e:?}"))?;
            let id = rl
                .get("accountId")
                .and_then(Value::as_str)
                .map(account_hash);
            let mut resp: GetAccountRateLimitsResponse = serde_json::from_value(rl)
                .map_err(|_| "the rate-limit reading could not be parsed".to_string())?;
            resp.account_id = id.clone();
            self.apply_baseline(&resp, id)
        })();
        match ready {
            Ok(state) => Ok((s, state)),
            Err(e) => {
                s.shutdown();
                Err(e)
            }
        }
    }

    /// The credit balance and plan windows right now, through a short-lived server. Refuses on a stop condition.
    pub fn status(&self) -> Result<Value, String> {
        let (mut s, state) = self.connect("status")?;
        s.shutdown();
        Ok(json!({
            "balance": match state.balance() {
                crate::ratelimit::Balance::Known(d) => d.to_string(),
                crate::ratelimit::Balance::Unlimited => "unlimited".into(),
                crate::ratelimit::Balance::Unknown => "unknown".into(),
            },
            "windows": window_rows(state.buckets()),
        }))
    }

    fn apply_baseline(
        &self,
        resp: &GetAccountRateLimitsResponse,
        id: Option<String>,
    ) -> Result<RateLimitState, String> {
        let path = self.state_dir.join("baselines.json");
        let saved: Value = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null);
        let mut state = RateLimitState::new();
        if let Some(b) = saved["balance"].as_str().and_then(Decimal::parse) {
            state.restore_baseline(b, saved["account"].as_str().map(str::to_string));
        }
        state.apply_read(resp, crate::clock::now());
        if saved.is_null() {
            state
                .freeze_baseline()
                .map_err(|b| format!("the credit balance is unavailable: {b:?}"))?;
            std::fs::create_dir_all(&self.state_dir).map_err(|e| e.to_string())?;
            let doc = json!({"balance": state.baseline().map(|d| d.to_string()), "account": id});
            std::fs::write(&path, doc.to_string()).map_err(|e| e.to_string())?;
        }
        let mut stopper = Stopper::new();
        stopper.check_codex(&state);
        if let Some(r) = stopper.reason() {
            return Err(format!("stopped: {r:?}"));
        }
        state
            .dispatch_gate(crate::clock::now())
            .map_err(|b| format!("the usage reading blocks dispatch: {b:?}"))?;
        Ok(state)
    }

    /// One thread and one turn. Returns the final message text and the usage entries for the step.
    fn one_turn(&self, q: &TurnSpec) -> Result<(Option<String>, Vec<Entry>, String), String> {
        let TurnSpec {
            step,
            name,
            cwd,
            model,
            effort,
            instructions,
            prompt,
            schema,
            writable,
        } = *q;
        if crate::route::is_excluded(model) {
            return Err(format!("the model {model} is excluded and is not run"));
        }
        let (mut s, mut state) = self.connect(name)?;
        let result = (|| {
            let ts = s
                .call(
                    "thread/start",
                    thread_start_params(cwd, model, instructions),
                    CALL,
                )
                .map_err(|e| format!("thread/start: {e:?}"))?;
            if ts["modelProvider"] != "openai" {
                return Err(format!(
                    "the model provider is {}, not openai",
                    ts["modelProvider"]
                ));
            }
            let thread = ts["thread"]["id"]
                .as_str()
                .ok_or("thread/start returned no thread id")?
                .to_string();
            let policy = if writable {
                writer_policy(cwd)
            } else {
                read_only_policy()
            };
            let (mut stopper, mut ledger) = (Stopper::new(), CodexTokenLedger::default());
            let cancel = self.cancel.clone();
            let stop_now = move || cancel.load(Ordering::SeqCst);
            let opts = TurnOpts {
                timeout: Duration::from_secs(self.timeout_secs),
                grace: Duration::from_secs(10),
                interrupt_when: None,
                cancel_when: Some(&stop_now),
                state: &mut state,
                stopper: &mut stopper,
                ledger: &mut ledger,
            };
            let turn = run_turn(
                &mut s,
                &thread,
                turn_start_params(&thread, cwd, effort, prompt, schema, &policy),
                opts,
            )?;
            let percent = state.buckets().get("codex").and_then(highest_percent);
            let usage = usage_entries(step, model, ledger.delta(&thread), percent);
            let note = match (
                turn.status.as_str(),
                stopper.reason(),
                turn.violations.is_empty(),
            ) {
                ("completed", None, true) => "completed".to_string(),
                (st, Some(r), _) => format!("{st}; stopped: {r:?}"),
                (st, None, false) => format!("{st}; route violation: {:?}", turn.violations),
                (_, None, true) if turn.timed_out => {
                    crate::fallback::timed_out_note(self.timeout_secs)
                }
                (st, None, true) => st.to_string(),
            };
            Ok((final_text(&turn.items), usage, note))
        })();
        s.shutdown();
        result
    }
}

/// The highest reading across a bucket's primary and secondary windows.
fn highest_percent(b: &crate::ratelimit::Bucket) -> Option<f64> {
    [b.primary.as_ref(), b.secondary.as_ref()]
        .into_iter()
        .flatten()
        .map(|w| w.used_percent as f64)
        .reduce(f64::max)
}

/// One row for every window of every bucket, so a person checking a stop threshold sees all of them. A bucket's
/// second window is named for its length.
fn window_rows(
    buckets: &std::collections::BTreeMap<String, crate::ratelimit::Bucket>,
) -> Vec<Value> {
    let mut rows = Vec::new();
    for (k, b) in buckets {
        for (n, w) in [b.primary.as_ref(), b.secondary.as_ref()]
            .into_iter()
            .enumerate()
        {
            let Some(w) = w else { continue };
            let name = match (n, w.duration_mins) {
                (0, _) => k.clone(),
                (_, Some(m)) if m % 1440 == 0 => format!("{k} {}d", m / 1440),
                (_, Some(m)) if m % 60 == 0 => format!("{k} {}h", m / 60),
                _ => format!("{k} (second window)"),
            };
            rows.push(json!({
                "bucket": name,
                "used_percent": w.used_percent,
                "window_mins": w.duration_mins,
            }));
        }
    }
    rows
}

impl Engine for CodexEngine {
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
                        w["bucket"].as_str()?.to_string(),
                        w["used_percent"].as_f64()?,
                    ))
                })
                .collect(),
        )
    }

    fn propose_plan(&mut self, brief: &str) -> Result<(Plan, Vec<Entry>), String> {
        if crate::route::is_excluded(&self.lead_model) {
            return Err(format!(
                "the model {} is excluded and is not run",
                self.lead_model
            ));
        }
        let schema = plan_schema();
        let cwd = self.scratch.join("lead-cwd");
        std::fs::create_dir_all(&cwd).map_err(|e| e.to_string())?;
        let prompt = plan_prompt(brief);
        let q = TurnSpec {
            step: "plan",
            name: "lead",
            cwd: &cwd,
            model: &self.lead_model,
            effort: &self.lead_effort,
            instructions: "You are the lead. Follow the instructions exactly.",
            prompt: &prompt,
            schema: Some(&schema),
            writable: false,
        };
        let (text, usage, note) = self.one_turn(&q)?;
        if note != "completed" {
            return Err(format!("the lead did not finish: {note}"));
        }
        let v: Value = serde_json::from_str(&text.ok_or("the lead returned no message")?)
            .map_err(|_| "the lead's answer was not JSON".to_string())?;
        Ok((parse_plan(brief, &v)?, usage))
    }

    fn run_worker(&mut self, req: &Request) -> WorkerResult {
        if req.route != "R1" {
            return WorkerResult {
                completed: false,
                note: not_this_route("Codex", req, "R1"),
                usage: usage_entries(req.step, req.model, None, None),
            };
        }
        if req.web {
            return WorkerResult {
                completed: false,
                note: "web research runs on Claude, the only provider with web tools here".into(),
                usage: usage_entries(req.step, req.model, None, None),
            };
        }
        let name = format!("worker-{}", req.step);
        let q = TurnSpec {
            step: req.step,
            name: &name,
            cwd: req.task_dir,
            model: req.model,
            effort: req.effort,
            instructions: "You are a worker. Follow the instructions exactly.",
            prompt: req.prompt,
            schema: None,
            writable: true,
        };
        match self.one_turn(&q) {
            Ok((_, usage, note)) => WorkerResult {
                completed: note == "completed",
                note,
                usage,
            },
            Err(e) => WorkerResult {
                completed: false,
                note: e,
                usage: usage_entries(req.step, req.model, None, None),
            },
        }
    }

    fn run_review(&mut self, req: &Request, schema: &Value) -> ReviewRun {
        let refused = |note: String| ReviewRun {
            completed: false,
            note,
            json: None,
            usage: usage_entries(req.step, req.model, None, None),
        };
        if req.route != "R1" {
            return refused(not_this_route("Codex", req, "R1"));
        }
        let name = format!("review-{}", req.step);
        let q = TurnSpec {
            step: req.step,
            name: &name,
            cwd: req.task_dir,
            model: req.model,
            effort: req.effort,
            instructions: "You are a reviewer. You may read files and must not change any.",
            prompt: req.prompt,
            schema: Some(schema),
            writable: false,
        };
        match self.one_turn(&q) {
            Ok((text, usage, note)) => {
                let json = text.and_then(|t| serde_json::from_str(&t).ok());
                ReviewRun {
                    completed: note == "completed" && json.is_some(),
                    note,
                    json,
                    usage,
                }
            }
            Err(e) => refused(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_excluded_lead_model_is_refused_before_anything_starts() {
        let d = std::env::temp_dir().join(format!("bff-codex-excl-{}", std::process::id()));
        let mut e = CodexEngine {
            codex: PathBuf::from("/nonexistent/codex"),
            home: String::new(),
            scratch: d.join("s"),
            state_dir: d.join("st"),
            lead_model: "gpt-6-astra".into(),
            lead_effort: "medium".into(),
            timeout_secs: 5,
            cancel: Arc::new(AtomicBool::new(false)),
        };
        let err = e.propose_plan("Write.").unwrap_err();
        assert!(err.contains("excluded"), "{err}");
        assert!(!d.join("s").exists(), "nothing was started or created");
    }

    #[test]
    fn every_plan_window_is_shown_and_the_highest_is_the_one_recorded() {
        use crate::ratelimit::{Bucket, Window};
        let w = |p, m| {
            Some(Window {
                used_percent: p,
                resets_at: None,
                duration_mins: Some(m),
            })
        };
        let b = Bucket {
            primary: w(10, 300),
            secondary: w(46, 10080),
            observed_at: 0,
        };
        assert_eq!(highest_percent(&b), Some(46.0));
        let mut m = std::collections::BTreeMap::new();
        m.insert("codex".to_string(), b);
        let rows = window_rows(&m);
        let names: Vec<_> = rows.iter().map(|r| r["bucket"].as_str().unwrap()).collect();
        assert_eq!(names, ["codex", "codex 7d"]);
        assert_eq!(rows[1]["used_percent"], 46);
    }
}
