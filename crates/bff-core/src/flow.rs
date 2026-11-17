//! The studio flow: propose a plan, approve it, dispatch a worker, collect its artifact. Every step is
//! journaled, and the state is rebuilt from the journal on reopen. No worker starts before approval, and
//! a changed plan is no longer approved.

use crate::journal::Journal;
use crate::meters::{record, Entry, Ledger, Meter};
use crate::plan::{plan_hash, Approval, Gate, Plan};
use crate::reserve::{Charge, Refusal as ReserveRefusal, Reservations};
use crate::route;
use crate::workspace::Project;
use serde_json::json;
use std::path::PathBuf;

const METER: &str = "tokens";

fn budget_message(r: ReserveRefusal) -> String {
    match r {
        ReserveRefusal::Insufficient { available, wanted, .. } => format!(
            "the token budget has room for {available} and this task reserves {wanted}. Raise the budget or wait for a running task to finish"
        ),
        ReserveRefusal::Paused { .. } => "the token budget is paused: it is spent. Raise it to continue".into(),
        other => format!("{other:?}"),
    }
}

pub struct Studio {
    pub project: Project,
    pub journal: Journal,
    plan: Option<Plan>,
    approval: Option<Approval>,
    tasks: u32,
    reserve: Reservations,
    /// Stages a person must unblock before they can start.
    blocked: Vec<(String, String)>,
    /// Set once a token budget exists. Each task then reserves `per_task` before it starts.
    per_task: Option<u64>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BudgetStatus {
    pub limit: u64,
    pub spent: u64,
    pub available: u64,
    pub paused: bool,
    /// Tasks whose usage was unknown and were charged at their reservation.
    pub unknown_charges: u32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct RouteCost {
    pub tokens: Option<f64>,
    pub api_dollars: Option<f64>,
    /// Steps on this route whose tokens were not reported.
    pub unknown_token_steps: usize,
}

/// Tokens one model used on one route.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelTokens {
    pub route: String,
    pub model: String,
    pub tokens: Option<f64>,
    /// Steps on this model whose tokens were not reported.
    pub unknown_token_steps: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Timing {
    pub task: String,
    pub secs: f64,
    pub route: String,
    pub model: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Dispatch {
    pub task: String,
    pub stage: String,
    pub dir: PathBuf,
    pub checkpoint: u32,
}

/// The longest a step id may be.
const MAX_STAGE_ID: usize = 64;

/// Everything wrong with a plan, in words. Empty means valid.
/// Why a plan cannot keep its promise that providers keep nothing: a step or stand-in on a provider with no way
/// to ask for that on one request, or a web search, which OpenRouter's promise does not cover.
fn no_retention_problems(plan: &Plan) -> Vec<String> {
    let mut out = Vec::new();
    let name = |route: &str| {
        crate::apiroute::Provider::for_route(route)
            .map(|p| p.name().to_string())
            .unwrap_or_else(|| route.to_string())
    };
    for s in &plan.stages {
        let route = s.uses.as_ref().map(|u| u.route.clone()).or_else(|| {
            plan.team
                .iter()
                .find(|a| a.role == s.owner_role)
                .map(|a| a.route.clone())
        });
        if let Some(r) = route.filter(|r| r != "R12") {
            out.push(format!(
                "step {} runs on {}, which cannot promise to keep nothing for one request. Move it to OpenRouter, or allow providers to keep requests",
                s.id,
                name(&r)
            ));
        }
        if crate::workflows::uses_web(&s.owner_role) {
            out.push(format!(
                "step {} searches the web, and OpenRouter's promise to keep nothing does not cover its search. Turn web research off, or allow providers to keep requests",
                s.id
            ));
        }
    }
    for f in plan.fallbacks.iter().filter(|f| f.route != "R12") {
        out.push(format!(
            "the stand-in for {} runs on {}, which cannot promise to keep nothing for one request",
            f.role,
            name(&f.route)
        ));
    }
    out
}

pub fn validate_plan(plan: &Plan) -> Vec<String> {
    let mut out = Vec::new();
    if plan.brief.trim().is_empty() {
        out.push("the brief is empty".to_string());
    }
    if plan.stages.is_empty() {
        out.push("the plan has no stages".to_string());
    }
    if plan.team.is_empty() {
        out.push("the team is empty".to_string());
    }
    let mut roles: Vec<&str> = Vec::new();
    for a in &plan.team {
        if roles.contains(&a.role.as_str()) {
            out.push(format!("duplicate role {}", a.role));
        }
        roles.push(&a.role);
        if let Err(e) = route::validate(&a.route, &a.model, &a.effort) {
            out.push(format!("{}: {}", a.role, e.0));
        }
    }
    let mut covered: Vec<&str> = Vec::new();
    for f in &plan.fallbacks {
        let Some(primary) = plan.team.iter().find(|a| a.role == f.role) else {
            out.push(format!("fallback for {}, who is not on the team", f.role));
            continue;
        };
        if covered.contains(&f.role.as_str()) {
            out.push(format!("{} has more than one fallback", f.role));
        }
        covered.push(&f.role);
        if f.route == primary.route {
            out.push(format!(
                "fallback for {} must be on the other route",
                f.role
            ));
        }
        if let Err(e) = route::validate(&f.route, &f.model, &f.effort) {
            out.push(format!("fallback for {}: {}", f.role, e.0));
        }
    }
    let mut ids: Vec<&str> = Vec::new();
    let mut files: Vec<(String, &str)> = Vec::new();
    for s in &plan.stages {
        if ids.contains(&s.id.as_str()) {
            out.push(format!("duplicate stage id {}", s.id));
        }
        ids.push(&s.id);
        if s.id.trim().is_empty() {
            out.push("a stage id is empty".to_string());
        } else if s.id.chars().count() > MAX_STAGE_ID {
            out.push(format!(
                "stage id {:?} is longer than {MAX_STAGE_ID} characters",
                s.id
            ));
        } else {
            // The result, the chat and the notes of a step are stored under a file name made from its id.
            let file = crate::app::artifact_name(&s.id);
            match files.iter().find(|(f, id)| f == &file && *id != s.id) {
                Some((_, other)) => out.push(format!(
                    "stage ids {other} and {} make the same file name",
                    s.id
                )),
                None => files.push((file, &s.id)),
            }
        }
        if !roles.contains(&s.owner_role.as_str()) {
            out.push(format!(
                "stage {} is owned by {}, who is not on the team",
                s.id, s.owner_role
            ));
        }
        if let Some(u) = &s.uses {
            if let Err(e) = route::validate(&u.route, &u.model, &u.effort) {
                out.push(format!("step {}: {}", s.id, e.0));
            }
        }
        if crate::workflows::uses_web(&s.owner_role) {
            let route = s.uses.as_ref().map(|u| u.route.as_str()).or(plan
                .team
                .iter()
                .find(|a| a.role == s.owner_role)
                .map(|a| a.route.as_str()));
            let model = s.uses.as_ref().map(|u| u.model.as_str()).or(plan
                .team
                .iter()
                .find(|a| a.role == s.owner_role)
                .map(|a| a.model.as_str()));
            if !route.is_some_and(|r| crate::workflows::WEB_ROUTES.contains(&r)) {
                out.push(format!(
                    "step {} searches the web, so it runs on OpenRouter, Anthropic or OpenAI",
                    s.id
                ));
            } else if route == Some("R12")
                && model.is_some_and(|m| m == "openrouter/free" || m.ends_with(":free"))
            {
                out.push(format!(
                    "step {} searches the web, which OpenRouter charges for even on free models, so it needs a paid model",
                    s.id
                ));
            }
        }
    }
    if plan.privacy == crate::plan::Privacy::NoRetention {
        out.extend(no_retention_problems(plan));
    }
    for (i, s) in plan.stages.iter().enumerate() {
        let mut seen: Vec<&str> = Vec::new();
        for a in &s.after {
            if a == &s.id {
                out.push(format!("stage {} waits for itself", s.id));
            } else if seen.contains(&a.as_str()) {
                out.push(format!("stage {} waits for {a} more than once", s.id));
            } else if !plan.stages.iter().any(|x| &x.id == a) {
                // A step that was taken out: the waiting step goes on without it, or is skipped when it has
                // nothing left to work on. The review board says which.
            } else if !plan.stages[..i].iter().any(|x| &x.id == a) {
                out.push(format!(
                    "stage {} waits for {a}, which comes after it. Put {a} first",
                    s.id
                ));
            }
            seen.push(a);
        }
    }
    out
}

impl Studio {
    pub fn open(project: Project) -> Result<Studio, String> {
        let journal = Journal::open(&project.journal_path())?;
        let (mut plan, mut approval, mut tasks) = (None, None, 0);
        let mut blocked: Vec<(String, String)> = Vec::new();
        let mut budget: Option<(u64, u64)> = None;
        let mut unfinished: Vec<String> = Vec::new();
        for e in journal.events()? {
            let task = e.payload["task"].as_str().unwrap_or_default().to_string();
            match e.kind.as_str() {
                "budget.set" => {
                    budget = Some((
                        e.payload["limit"].as_u64().unwrap_or(0),
                        e.payload["per_task"].as_u64().unwrap_or(0),
                    ))
                }
                "budget.raised" => {
                    if let Some((limit, _)) = budget.as_mut() {
                        *limit = e.payload["limit"].as_u64().unwrap_or(*limit);
                    }
                }
                "worker.dispatched" => unfinished.push(task),
                "worker.finished" | "worker.failed" => unfinished.retain(|t| *t != task),
                _ => {}
            }
            match e.kind.as_str() {
                "plan.set" => {
                    plan = Some(
                        serde_json::from_value(e.payload["plan"].clone())
                            .map_err(|x| format!("plan.set {}: {x}", e.id))?,
                    )
                }
                "plan.approved" => {
                    approval = Some(Approval {
                        plan_hash: e.payload["hash"].as_str().unwrap_or_default().to_string(),
                        approved_by: e.payload["by"].as_str().unwrap_or_default().to_string(),
                    })
                }
                "worker.dispatched" => tasks += 1,
                "stage.blocked" => blocked.push((
                    e.payload["stage"].as_str().unwrap_or_default().to_string(),
                    e.payload["reason"].as_str().unwrap_or_default().to_string(),
                )),
                "stage.unblocked" => {
                    blocked.retain(|(s, _)| Some(s.as_str()) != e.payload["stage"].as_str())
                }
                _ => {}
            }
        }
        let mut studio = Studio {
            project,
            journal,
            plan,
            approval,
            tasks,
            reserve: Reservations::default(),
            blocked,
            per_task: None,
        };
        // The project's token budget is rebuilt from its journal. A task that was running when the app stopped
        // may have used tokens nobody saw, so it counts at the per-task amount until recovery records it.
        if let Some((limit, per_task)) = budget.filter(|(l, p)| *p > 0 && p <= l) {
            studio.apply_token_budget(limit, per_task, unfinished.len() as u64)?;
        }
        Ok(studio)
    }

    /// Sets a token budget. Tokens already in the journal count against it, and a task whose usage was
    /// never reported is counted at `per_task`. Each new task reserves `per_task` before it starts.
    pub fn set_token_budget(&mut self, limit: u64, per_task: u64) -> Result<(), String> {
        if per_task == 0 || per_task > limit {
            return Err("the per-task amount must be above zero and fit inside the budget".into());
        }
        self.apply_token_budget(limit, per_task, 0)?;
        self.journal.append(
            "budget.set",
            &json!({"limit": limit, "per_task": per_task}),
            crate::clock::now(),
        )?;
        Ok(())
    }

    fn apply_token_budget(
        &mut self,
        limit: u64,
        per_task: u64,
        unfinished: u64,
    ) -> Result<(), String> {
        let agg = self.ledger()?.aggregate(Meter::Tokens);
        let mut r = Reservations::default();
        r.set_budget(METER, limit);
        r.seed_spent(
            METER,
            agg.total.unwrap_or(0.0) as u64 + (agg.unknown as u64 + unfinished) * per_task,
        )
        .map_err(|e| format!("{e:?}"))?;
        self.reserve = r;
        self.per_task = Some(per_task);
        Ok(())
    }

    /// How long a task's model work took, for planning later sessions.
    pub fn record_timing(
        &mut self,
        task: &str,
        secs: f64,
        route: &str,
        model: &str,
        now: u64,
    ) -> Result<(), String> {
        self.journal.append(
            "worker.timing",
            &json!({"task": task, "secs": secs, "route": route, "model": model}),
            now,
        )?;
        Ok(())
    }

    pub fn timings(&self) -> Result<Vec<Timing>, String> {
        Ok(self
            .journal
            .events()?
            .into_iter()
            .filter(|e| e.kind == "worker.timing")
            .map(|e| Timing {
                task: e.payload["task"].as_str().unwrap_or("").to_string(),
                secs: e.payload["secs"].as_f64().unwrap_or(0.0),
                route: e.payload["route"].as_str().unwrap_or("").to_string(),
                model: e.payload["model"].as_str().unwrap_or("").to_string(),
            })
            .collect())
    }

    /// Stops one stage from starting, with the reason, until a person unblocks it. Other stages still run.
    pub fn block_stage(&mut self, stage: &str, reason: &str, now: u64) -> Result<(), String> {
        let plan = self.plan.as_ref().ok_or("there is no plan")?;
        if !plan.stages.iter().any(|s| s.id == stage) {
            return Err(format!("no stage {stage:?} in the plan"));
        }
        if self.blocked.iter().any(|(s, _)| s == stage) {
            return Ok(());
        }
        self.journal.append(
            "stage.blocked",
            &json!({"stage": stage, "reason": reason}),
            now,
        )?;
        self.blocked.push((stage.to_string(), reason.to_string()));
        Ok(())
    }

    pub fn unblock_stage(&mut self, stage: &str, now: u64) -> Result<(), String> {
        if !self.blocked.iter().any(|(s, _)| s == stage) {
            return Err(format!("stage {stage:?} is not blocked"));
        }
        self.journal
            .append("stage.unblocked", &json!({"stage": stage}), now)?;
        self.blocked.retain(|(s, _)| s != stage);
        Ok(())
    }

    /// `(stage, reason)` for each step whose latest attempt did not finish, such as one that failed, was stopped, or
    /// was interrupted when the app closed. The reason is kept in the journal, so it is still here after a restart. A
    /// step that finished afterwards is not listed.
    pub fn failed_stages(&self) -> Result<Vec<(String, String)>, String> {
        let mut stage_of: std::collections::BTreeMap<String, String> = Default::default();
        let mut failed: Vec<(String, String)> = Vec::new();
        for e in self.journal.events()? {
            let task = e.payload["task"].as_str().unwrap_or_default().to_string();
            match e.kind.as_str() {
                "worker.dispatched" if e.payload["kind"] == "stage" => {
                    if let Some(st) = e.payload["stage"].as_str() {
                        stage_of.insert(task, st.to_string());
                    }
                }
                "worker.finished" => {
                    if let Some(st) = stage_of.get(&task) {
                        failed.retain(|(s, _)| s != st);
                    }
                }
                "worker.failed" => {
                    if let Some(st) = stage_of.get(&task) {
                        failed.retain(|(s, _)| s != st);
                        let why = e.payload["reason"].as_str().unwrap_or("it did not finish");
                        failed.push((st.clone(), why.chars().take(300).collect()));
                    }
                }
                _ => {}
            }
        }
        Ok(failed)
    }

    /// `(stage, what moved)` for each step whose latest run moved to its stand-in. A later run of the step clears it.
    pub fn stand_ins_used(&self) -> Result<Vec<(String, serde_json::Value)>, String> {
        let mut used: Vec<(String, serde_json::Value)> = Vec::new();
        let mut moving: Vec<String> = Vec::new();
        for e in self.journal.events()? {
            let Some(stage) = e.payload["stage"].as_str().map(str::to_string) else {
                continue;
            };
            match e.kind.as_str() {
                "fallback.used" => {
                    used.retain(|(s, _)| *s != stage);
                    used.push((stage.clone(), e.payload.clone()));
                    moving.push(stage);
                }
                "worker.dispatched" => match moving.iter().position(|s| *s == stage) {
                    Some(i) => {
                        moving.remove(i);
                    }
                    None => used.retain(|(s, _)| *s != stage),
                },
                _ => {}
            }
        }
        Ok(used)
    }

    /// `(stage, reason)` for each blocked stage.
    pub fn blocked_stages(&self) -> Vec<(String, String)> {
        self.blocked.clone()
    }

    /// Usage grouped by the route that ran each task. Tokens and dollars stay separate fields, and a step with
    /// no timing record (the lead's planning turn) goes under `unattributed`.
    pub fn cost_by_route(&self) -> Result<std::collections::BTreeMap<String, RouteCost>, String> {
        let events = self.journal.events()?;
        let route_of: std::collections::BTreeMap<String, String> = events
            .iter()
            .filter(|e| e.kind == "worker.timing")
            .filter_map(|e| {
                Some((
                    e.payload["task"].as_str()?.to_string(),
                    e.payload["route"].as_str()?.to_string(),
                ))
            })
            .collect();
        let mut out: std::collections::BTreeMap<String, RouteCost> = Default::default();
        for e in events.iter().filter(|e| e.kind == "usage") {
            let step = e.payload["step"].as_str().unwrap_or("");
            let route = route_of
                .get(step)
                .cloned()
                .unwrap_or_else(|| "unattributed".into());
            let cost = out.entry(route).or_default();
            let value = e.payload["value"].as_f64();
            match e.payload["meter"].as_str() {
                Some("tokens") => match value {
                    Some(v) => *cost.tokens.get_or_insert(0.0) += v,
                    None => cost.unknown_token_steps += 1,
                },
                Some("api_dollars") => {
                    if let Some(v) = value {
                        *cost.api_dollars.get_or_insert(0.0) += v;
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    /// Tokens grouped by the model and route that ran each task, biggest first. A step whose tokens were not
    /// reported is counted as unknown and never as zero, and the planning turn has no timing record, so it is
    /// listed under the model `planning`.
    pub fn tokens_by_model(&self) -> Result<Vec<ModelTokens>, String> {
        let events = self.journal.events()?;
        let ran: std::collections::BTreeMap<String, (String, String)> = events
            .iter()
            .filter(|e| e.kind == "worker.timing")
            .filter_map(|e| {
                Some((
                    e.payload["task"].as_str()?.to_string(),
                    (
                        e.payload["route"].as_str()?.to_string(),
                        e.payload["model"].as_str()?.to_string(),
                    ),
                ))
            })
            .collect();
        let mut rows: Vec<ModelTokens> = Vec::new();
        for e in events
            .iter()
            .filter(|e| e.kind == "usage" && e.payload["meter"].as_str() == Some("tokens"))
        {
            let step = e.payload["step"].as_str().unwrap_or("");
            let (route, model) = ran
                .get(step)
                .cloned()
                .unwrap_or_else(|| (String::new(), "planning".into()));
            let at = match rows
                .iter()
                .position(|r| r.route == route && r.model == model)
            {
                Some(i) => i,
                None => {
                    rows.push(ModelTokens {
                        route,
                        model,
                        tokens: None,
                        unknown_token_steps: 0,
                    });
                    rows.len() - 1
                }
            };
            match e.payload["value"].as_f64() {
                Some(v) => *rows[at].tokens.get_or_insert(0.0) += v,
                None => rows[at].unknown_token_steps += 1,
            }
        }
        rows.sort_by(|a, b| {
            b.tokens
                .unwrap_or(0.0)
                .total_cmp(&a.tokens.unwrap_or(0.0))
                .then_with(|| a.model.cmp(&b.model))
        });
        Ok(rows)
    }

    /// Marks every task that was dispatched and never finished or failed as failed, because the app stopped
    /// or crashed before it ended. The folder stays as it is. Its tokens are recorded as unknown, which a
    /// budget charges at the per-task amount. Safe to call again.
    pub fn recover(&mut self, now: u64) -> Result<Vec<String>, String> {
        let mut open: Vec<String> = Vec::new();
        for e in self.journal.events()? {
            let task = e.payload["task"].as_str().unwrap_or("").to_string();
            match e.kind.as_str() {
                "worker.dispatched" => open.push(task),
                "worker.finished" | "worker.failed" => open.retain(|t| *t != task),
                _ => {}
            }
        }
        for task in &open {
            let unknown = Entry {
                step: task.clone(),
                meter: Meter::Tokens,
                value: None,
                status: crate::meters::Status::Unknown,
                detail: "interrupted".into(),
            };
            record(&self.journal, &unknown, now)?;
            self.journal.append(
                "worker.failed",
                &json!({"task": task, "reason": "interrupted: the app stopped before this task finished"}),
                now,
            )?;
            self.settle(task, &[unknown]);
        }
        Ok(open)
    }

    pub fn budget_status(&self) -> Option<BudgetStatus> {
        self.per_task?;
        let r = &self.reserve;
        Some(BudgetStatus {
            limit: r.limit(METER).ok()?,
            spent: r.spent(METER).ok()?,
            available: r.available(METER).ok()?,
            paused: r.is_paused(METER).ok()?,
            unknown_charges: r.unknown_charges(METER).ok()?,
        })
    }

    /// Nitish raises the limit. The budget resumes if the new limit leaves room.
    pub fn raise_token_budget(&mut self, new_limit: u64) -> Result<(), String> {
        let current = self
            .budget_status()
            .ok_or("there is no token budget")?
            .limit;
        if new_limit <= current {
            return Err(format!("the new limit must be above the current {current}"));
        }
        self.reserve
            .raise_and_resume(METER, new_limit)
            .map_err(|e| format!("{e:?}"))?;
        self.journal.append(
            "budget.raised",
            &json!({"limit": new_limit}),
            crate::clock::now(),
        )?;
        Ok(())
    }

    pub fn plan(&self) -> Option<&Plan> {
        self.plan.as_ref()
    }

    /// Effective policy is durable; adding/removing roles changes the defaults and invalidates approval.
    pub fn policy(&self) -> Result<crate::policy::Matrix, String> {
        let plan = self.plan.as_ref().ok_or("there is no plan")?;
        let events = self.journal.events()?;
        match events.iter().rev().find(|e| e.kind == "policy.set") {
            Some(e) => {
                serde_json::from_value(e.payload["matrix"].clone()).map_err(|e| e.to_string())
            }
            None => Ok(crate::policy::defaults(plan)),
        }
    }

    pub fn set_policy(&mut self, matrix: crate::policy::Matrix, now: u64) -> Result<(), String> {
        crate::policy::validate(self.plan.as_ref().ok_or("there is no plan")?, &matrix)?;
        self.journal
            .append("policy.set", &json!({"matrix": matrix}), now)?;
        Ok(())
    }

    /// Canonical scope covers plan, policy revision, source snapshots, source roles, selected tools,
    /// exercise requirements and the current per-project budget. Old plan-only approvals fail closed.
    pub fn approval_hash(&self) -> Result<String, String> {
        let plan = self.plan.as_ref().ok_or("there is no plan")?;
        let matrix = self.policy()?;
        crate::policy::validate(plan, &matrix)?;
        let revision = self
            .journal
            .events()?
            .iter()
            .filter(|e| e.kind == "policy.set")
            .map(|e| e.id)
            .max()
            .unwrap_or(0);
        let root = &self.project.root;
        let sources = if root.join("sources").exists() {
            format!(
                "{:?}",
                crate::fshash::hash_tree(&root.join("sources")).map_err(|e| e.to_string())?
            )
        } else {
            "{}".into()
        };
        let mut context = std::collections::BTreeMap::new();
        for name in ["source_roles.json", "selection.json", "exercise.json"] {
            let value = match std::fs::read(root.join(name)) {
                Ok(v) => crate::engine::sha256_hex(&v),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
                Err(e) => return Err(e.to_string()),
            };
            context.insert(name, value);
        }
        // API routing/prices/bounds are configuration, not usage; changing them invalidates scope.
        let api_config = root
            .parent()
            .and_then(std::path::Path::parent)
            .map(|p| p.join("api-routes.json"));
        let api_revision = match api_config.map(std::fs::read) {
            Some(Ok(bytes)) => crate::engine::sha256_hex(&bytes),
            Some(Err(e)) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.to_string()),
            _ => String::new(),
        };
        let budget_revision = self
            .journal
            .events()?
            .iter()
            .filter(|e| matches!(e.kind.as_str(), "budget.set" | "budget.raised"))
            .map(|e| e.id)
            .max()
            .unwrap_or(0);
        Ok(crate::engine::sha256_hex(json!({"version": 1, "plan": plan, "policy": matrix, "revision": revision, "sources": sources, "context": context, "budget_revision": budget_revision, "api_revision": api_revision}).to_string().as_bytes()))
    }

    pub fn gate(&self) -> Gate {
        match &self.approval {
            None => Gate::NotApproved,
            Some(a) if self.approval_hash().is_ok_and(|h| h == a.plan_hash) => Gate::Allowed,
            Some(_) => Gate::PlanChanged,
        }
    }

    pub fn authorize_action(&self, action: &crate::policy::Action, now: u64) -> Result<(), String> {
        self.open_gate()?;
        let scope = self.approval_hash()?;
        let role = &self
            .plan
            .as_ref()
            .ok_or("no plan")?
            .stages
            .iter()
            .find(|s| s.id == action.stage)
            .ok_or("no such step")?
            .owner_role;
        let permission = crate::policy::evaluate(&self.policy()?, role, action);
        let id = action.id(&scope);
        let events = self.journal.events()?;
        if permission == crate::policy::Permission::Ask {
            let approved = events.iter().rev().find(|e| {
                e.payload["id"] == id
                    && matches!(
                        e.kind.as_str(),
                        "policy.approved" | "policy.consumed" | "policy.rejected"
                    )
            });
            if approved.is_some_and(|e| e.kind == "policy.approved") {
                self.journal
                    .append("policy.consumed", &json!({"id": id, "scope": scope}), now)?;
                return Ok(());
            }
        }
        let outcome = match permission {
            crate::policy::Permission::Allow => "allowed",
            crate::policy::Permission::Ask => "asked",
            crate::policy::Permission::Deny => "denied",
        };
        // One pending row per exact action; repeated model requests cannot flood or alter it.
        if outcome != "asked"
            || !crate::policy::pending(&self.journal, &scope)?
                .iter()
                .any(|p| p["id"] == id)
        {
            self.journal.append("policy.action", &json!({"id": id, "scope": scope, "action": action, "tier": action.tier(), "outcome": outcome, "role": role}), now)?;
        }
        if permission == crate::policy::Permission::Allow {
            Ok(())
        } else {
            Err(format!(
                "Policy {outcome}: {:?} at {}",
                action.tool, action.stage
            ))
        }
    }

    pub fn decide_actions(
        &self,
        ids: &[String],
        approve: bool,
        by: &str,
        now: u64,
    ) -> Result<(), String> {
        self.open_gate()?;
        if by.trim().is_empty() || ids.is_empty() {
            return Err("Name the decision maker and actions".into());
        }
        let pending = crate::policy::pending(&self.journal, &self.approval_hash()?)?;
        let mut seen = std::collections::BTreeSet::new();
        for id in ids {
            if !seen.insert(id) {
                return Err("Duplicate action".into());
            }
            let row = pending
                .iter()
                .find(|p| p["id"] == *id)
                .ok_or("This action is stale or already decided")?;
            if approve && ids.len() > 1 && row["tier"] != "reversible" {
                return Err(
                    "Consequential and destructive actions require individual approval".into(),
                );
            }
        }
        for id in ids {
            self.journal.append(
                if approve {
                    "policy.approved"
                } else {
                    "policy.rejected"
                },
                &json!({"id": id, "by": by.trim(), "trigger": "explicit user decision"}),
                now,
            )?;
        }
        Ok(())
    }

    pub fn policy_view(&self) -> Result<serde_json::Value, String> {
        if self.plan.is_none() {
            return Ok(serde_json::Value::Null);
        }
        let scope = self.approval_hash()?;
        let mut counts = json!({"allowed": 0, "asked": 0, "denied": 0});
        for event in self
            .journal
            .events()?
            .iter()
            .filter(|e| e.kind == "policy.action")
        {
            if let Some(k) = event.payload["outcome"].as_str() {
                counts[k] = json!(counts[k].as_u64().unwrap_or(0) + 1);
            }
        }
        Ok(
            json!({"matrix": self.policy()?, "guardrails": crate::policy::guardrails(&self.policy()?), "scope_hash": scope, "pending": crate::policy::pending(&self.journal, &scope)?, "counts": counts, "assurance": "Guarded"}),
        )
    }

    /// Stores a plan, whether it is the lead's proposal or a human edit. An invalid plan is refused whole.
    pub fn set_plan(&mut self, plan: Plan, now: u64) -> Result<(), Vec<String>> {
        let problems = validate_plan(&plan);
        if !problems.is_empty() {
            return Err(problems);
        }
        let payload = json!({"plan": plan, "hash": plan_hash(&plan)});
        self.journal
            .append("plan.set", &payload, now)
            .map_err(|e| vec![e])?;
        self.plan = Some(plan);
        if self
            .journal
            .events()
            .map_err(|e| vec![e])?
            .iter()
            .any(|e| e.kind == "policy.set")
        {
            let plan = self.plan.as_ref().expect("just set");
            let matrix = self.policy().map_err(|e| vec![e])?;
            if crate::policy::validate(plan, &matrix).is_err() {
                self.set_policy(crate::policy::defaults(plan), now)
                    .map_err(|e| vec![e])?;
            }
        }
        Ok(())
    }

    pub fn approve(&mut self, by: &str, now: u64) -> Result<(), String> {
        let plan = self.plan.as_ref().ok_or("there is no plan to approve")?;
        let _ = plan;
        if by.trim().is_empty() {
            return Err("Name the person approving".into());
        }
        let a = Approval {
            plan_hash: self.approval_hash()?,
            approved_by: by.trim().into(),
        };
        self.journal.append(
            "plan.approved",
            &json!({"hash": a.plan_hash, "by": a.approved_by, "plan": self.plan(), "policy": self.policy()?, "trigger": "explicit plan approval"}),
            now,
        )?;
        self.approval = Some(a);
        Ok(())
    }

    /// Charges a finished task against the budget. A task with no reported tokens is charged at its
    /// reservation, because unknown is never free.
    fn settle(&mut self, task: &str, usage: &[Entry]) {
        if self.per_task.is_none() {
            return;
        }
        let tokens: Vec<&Entry> = usage.iter().filter(|e| e.meter == Meter::Tokens).collect();
        let charge = if tokens.is_empty() || tokens.iter().any(|e| e.value.is_none()) {
            Charge::Unknown
        } else {
            Charge::Known(tokens.iter().filter_map(|e| e.value).sum::<f64>() as u64)
        };
        let _ = self.reserve.settle(METER, task, charge);
    }

    fn open_gate(&self) -> Result<(), String> {
        match self.gate() {
            Gate::Allowed => Ok(()),
            Gate::NotApproved => Err("the plan is not approved".into()),
            Gate::PlanChanged => {
                Err("the plan changed after it was approved. Approve it again".into())
            }
        }
    }

    fn start_task(&mut self, stage: &str, kind: &str, now: u64) -> Result<Dispatch, String> {
        let task = format!("t{}", self.tasks + 1);
        if let Some(per) = self.per_task {
            self.reserve
                .reserve(METER, &task, per)
                .map_err(budget_message)?;
        }
        let started = self.begin_task(&task, stage, kind, now);
        if started.is_err() && self.per_task.is_some() {
            let _ = self.reserve.release(METER, &task);
        }
        started
    }

    fn begin_task(
        &mut self,
        task: &str,
        stage: &str,
        kind: &str,
        now: u64,
    ) -> Result<Dispatch, String> {
        let plan = self.plan.as_ref().ok_or("there is no plan")?;
        let _ = plan;
        let hash = self.approval_hash()?;
        let task = task.to_string();
        let dir = self.project.new_task_dir(&task)?;
        let (checkpoint, _) = self.project.checkpoint(&task)?;
        self.journal.append(
            "worker.dispatched",
            &json!({"task": task, "stage": stage, "kind": kind, "plan_hash": hash, "checkpoint": checkpoint}),
            now,
        )?;
        self.tasks += 1;
        Ok(Dispatch {
            task,
            stage: stage.to_string(),
            dir,
            checkpoint,
        })
    }

    /// Allocates a task folder, takes a checkpoint, and journals the dispatch. Refused unless approved.
    pub fn dispatch(&mut self, stage_id: &str, now: u64) -> Result<Dispatch, String> {
        self.open_gate()?;
        let plan = self.plan.as_ref().ok_or("there is no plan")?;
        if !plan.stages.iter().any(|s| s.id == stage_id) {
            return Err(format!("no stage {stage_id:?} in the plan"));
        }
        if let Some((_, why)) = self.blocked.iter().find(|(s, _)| s == stage_id) {
            return Err(format!("stage {stage_id} is blocked: {why}"));
        }
        self.start_task(stage_id, "stage", now)
    }

    /// A task that is not one of the plan's stages, such as a review or a revision. It passes the same
    /// approval gate, gets its own folder and checkpoint, and is journaled as a dispatch of `kind`.
    pub fn dispatch_task(&mut self, kind: &str, label: &str, now: u64) -> Result<Dispatch, String> {
        self.open_gate()?;
        self.start_task(label, kind, now)
    }

    /// A task that talks about the plan and runs nothing of it, such as the coordinator's conversation. It needs a
    /// plan but not an approval, so a person can reshape the steps before approving them. It has its own folder and
    /// checkpoint and is journaled as a dispatch of `kind`. The caller may use only a reader's tools.
    pub fn dispatch_planning_task(
        &mut self,
        kind: &str,
        label: &str,
        now: u64,
    ) -> Result<Dispatch, String> {
        if self.plan.is_none() {
            return Err("there is no plan".into());
        }
        self.start_task(label, kind, now)
    }

    /// Promotes the worker's file as an artifact and journals the finish with its usage.
    pub fn complete(
        &mut self,
        d: &Dispatch,
        rel: &str,
        name: &str,
        usage: &[Entry],
        now: u64,
    ) -> Result<PathBuf, String> {
        let artifact = self.project.promote(&d.task, rel, name)?;
        for e in usage {
            record(&self.journal, e, now)?;
        }
        let file = artifact
            .file_name()
            .map(|f| f.to_string_lossy().into_owned());
        self.journal.append(
            "worker.finished",
            &json!({"task": d.task, "artifact": file}),
            now,
        )?;
        self.settle(&d.task, usage);
        Ok(artifact)
    }

    /// Finishes a task that promotes no file, such as a review: records its usage, journals the finish, and
    /// settles its budget reservation.
    pub fn finish_task(
        &mut self,
        d: &Dispatch,
        kind: &str,
        usage: &[Entry],
        now: u64,
    ) -> Result<(), String> {
        for e in usage {
            record(&self.journal, e, now)?;
        }
        self.journal.append(
            "worker.finished",
            &json!({"task": d.task, "kind": kind}),
            now,
        )?;
        self.settle(&d.task, usage);
        Ok(())
    }

    /// The task folder stays as it is, and the failure and any usage are journaled.
    pub fn fail(
        &mut self,
        d: &Dispatch,
        reason: &str,
        usage: &[Entry],
        now: u64,
    ) -> Result<(), String> {
        for e in usage {
            record(&self.journal, e, now)?;
        }
        self.journal.append(
            "worker.failed",
            &json!({"task": d.task, "reason": reason}),
            now,
        )?;
        self.settle(&d.task, usage);
        Ok(())
    }

    /// Ends a dispatch whose worker never started, so nothing was sent to a model: the failure is journaled and the
    /// budget held for the task is given back.
    pub fn abandon(&mut self, d: &Dispatch, reason: &str, now: u64) -> Result<(), String> {
        self.journal.append(
            "worker.failed",
            &json!({"task": d.task, "reason": reason}),
            now,
        )?;
        if self.per_task.is_some() {
            let _ = self.reserve.release(METER, &d.task);
        }
        Ok(())
    }

    /// Journals usage that no worker run produced, such as the lead's planning turn.
    pub fn record_usage(&mut self, usage: &[Entry], now: u64) -> Result<(), String> {
        for e in usage {
            record(&self.journal, e, now)?;
        }
        Ok(())
    }

    pub fn approval_by(&self) -> Option<&str> {
        self.approval.as_ref().map(|a| a.approved_by.as_str())
    }

    pub fn ledger(&self) -> Result<Ledger, String> {
        Ledger::from_events(&self.journal.events()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_steps_own_model_must_pass_the_same_checks_as_a_roles() {
        let mut p: Plan = serde_json::from_value(json!({"brief": "b",
            "stages": [{"id": "a", "title": "A", "owner_role": "writer"}],
            "team": [{"role": "writer", "route": "R1", "model": "gpt-6-luna", "effort": "low"}]}))
        .unwrap();
        p.stages[0].uses = Some(crate::plan::StepModel {
            route: "R2".into(),
            model: "claude-sonnet-5-5".into(),
            effort: "high".into(),
        });
        assert!(validate_plan(&p).is_empty());
        p.stages[0].uses.as_mut().unwrap().model = "no-such-model".into();
        assert!(
            validate_plan(&p).iter().any(|x| x.starts_with("step a:")),
            "{:?}",
            validate_plan(&p)
        );
    }
    use crate::meters::{Meter, Status};
    use crate::plan::{Assignment, Stage};

    fn base(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bff-flow-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn studio(tag: &str) -> Studio {
        let p = Project::create(&base(tag), "case", "Write a short plan.", 1).unwrap();
        Studio::open(p).unwrap()
    }

    fn plan() -> Plan {
        Plan {
            brief: "Write a short plan.".into(),
            stages: vec![Stage {
                uses: None,
                after: vec![],
                id: "draft".into(),
                title: "Draft".into(),
                owner_role: "writer".into(),
                guide: String::new(),
            }],
            team: vec![Assignment {
                role: "writer".into(),
                route: "R1".into(),
                model: "gpt-6-luna".into(),
                effort: "medium".into(),
            }],
            fallbacks: vec![],
            privacy: Default::default(),
            keynote: false,
        }
    }

    #[test]
    fn a_step_may_wait_only_for_steps_that_come_before_it() {
        let step = |id: &str, after: &[&str]| Stage {
            uses: None,
            id: id.into(),
            title: id.into(),
            owner_role: "writer".into(),
            guide: String::new(),
            after: after.iter().map(|x| x.to_string()).collect(),
        };
        let with = |stages: Vec<Stage>| Plan { stages, ..plan() };
        assert!(validate_plan(&with(vec![
            step("a", &[]),
            step("b", &["a"]),
            step("c", &["a"]),
            step("d", &["b", "c"])
        ]))
        .is_empty());
        let bad = |stages: Vec<Stage>| validate_plan(&with(stages)).join(" | ");
        // A step that was taken out is not an error: the waiting step is skipped or goes on without it.
        assert!(bad(vec![step("a", &["ghost"])]).is_empty());
        assert!(bad(vec![step("a", &["a"])]).contains("itself"));
        assert!(bad(vec![step("a", &["b"]), step("b", &[])]).contains("after it"));
        assert!(bad(vec![step("a", &[]), step("b", &["a", "a"])]).contains("more than once"));
    }

    #[test]
    fn a_step_id_must_be_usable_and_must_not_collide_with_another_after_naming_files() {
        let step = |id: &str| Stage {
            uses: None,
            id: id.into(),
            title: "t".into(),
            owner_role: "writer".into(),
            guide: String::new(),
            after: vec![],
        };
        let bad = |ids: &[&str]| {
            let stages = ids.iter().map(|i| step(i)).collect();
            validate_plan(&Plan { stages, ..plan() }).join(" | ")
        };
        assert!(
            bad(&["a/b", "a?b"]).contains("same file name"),
            "{}",
            bad(&["a/b", "a?b"])
        );
        assert!(bad(&[""]).contains("empty"));
        assert!(bad(&["  "]).contains("empty"));
        assert!(bad(&[&"x".repeat(65)]).contains("64"));
        assert!(bad(&["ok_one", "ok-two", "Three3"]).is_empty());
    }

    #[test]
    fn invalid_plans_are_refused_with_every_problem_listed() {
        let mut p = plan();
        p.stages.push(Stage {
            uses: None,
            after: vec![],
            id: "draft".into(),
            title: "Dup".into(),
            owner_role: "ghost".into(),
            guide: String::new(),
        });
        p.team[0].model = "gpt-9".into();
        let problems = validate_plan(&p);
        let all = problems.join(" | ");
        assert!(
            all.contains("duplicate stage") && all.contains("ghost") && all.contains("gpt-9"),
            "{all}"
        );
        assert!(validate_plan(&plan()).is_empty());
        let empty = Plan {
            brief: " ".into(),
            stages: vec![],
            team: vec![],
            fallbacks: vec![],
            privacy: Default::default(),
            keynote: false,
        };
        assert!(validate_plan(&empty).len() >= 3);
        let mut s = studio("invalid");
        assert!(s.set_plan(p, 2).is_err());
        assert!(s.plan().is_none(), "an invalid plan is not stored");
    }

    #[test]
    fn nothing_dispatches_before_approval_and_no_folder_is_made() {
        let mut s = studio("noapproval");
        s.set_plan(plan(), 2).unwrap();
        assert_eq!(s.gate(), Gate::NotApproved);
        assert!(s.dispatch("draft", 3).is_err());
        assert_eq!(
            std::fs::read_dir(s.project.root.join("tasks"))
                .unwrap()
                .count(),
            0
        );
        assert!(s.approve("Nitish", 3).is_ok());
        assert_eq!(s.gate(), Gate::Allowed);
    }

    #[test]
    fn approval_cannot_precede_a_plan() {
        let mut s = studio("noplan");
        assert!(s.approve("Nitish", 1).is_err());
        assert!(s.dispatch("draft", 1).is_err());
    }

    #[test]
    fn an_approved_plan_dispatches_with_a_checkpoint_and_journal_entry() {
        let mut s = studio("dispatch");
        s.set_plan(plan(), 2).unwrap();
        s.approve("Nitish", 3).unwrap();
        let d = s.dispatch("draft", 4).unwrap();
        assert_eq!(
            (d.task.as_str(), d.stage.as_str(), d.checkpoint),
            ("t1", "draft", 1)
        );
        assert!(d.dir.is_dir());
        assert!(s.project.root.join("checkpoints/t1-1.json").exists());
        assert!(s
            .journal
            .events()
            .unwrap()
            .iter()
            .any(|e| e.kind == "worker.dispatched" && e.payload["task"] == "t1"));
        assert_eq!(s.dispatch("draft", 5).unwrap().task, "t2");
        assert!(s.dispatch("no-such-stage", 6).is_err());
    }

    #[test]
    fn editing_the_plan_after_approval_blocks_dispatch_until_it_is_approved_again() {
        let mut s = studio("edit");
        s.set_plan(plan(), 2).unwrap();
        s.approve("Nitish", 3).unwrap();
        let mut edited = plan();
        edited.team[0].effort = "high".into();
        s.set_plan(edited, 4).unwrap();
        assert_eq!(s.gate(), Gate::PlanChanged);
        assert!(s.dispatch("draft", 5).is_err());
        assert_eq!(
            std::fs::read_dir(s.project.root.join("tasks"))
                .unwrap()
                .count(),
            0
        );
        s.approve("Nitish", 6).unwrap();
        assert!(s.dispatch("draft", 7).is_ok());
    }

    #[test]
    fn state_is_rebuilt_from_the_journal_on_reopen() {
        let b = base("reopen");
        let root = {
            let mut s = Studio::open(Project::create(&b, "case", "Write.", 1).unwrap()).unwrap();
            s.set_plan(plan(), 2).unwrap();
            s.approve("Nitish", 3).unwrap();
            s.dispatch("draft", 4).unwrap();
            s.project.root.clone()
        };
        let mut s = Studio::open(Project::open(&root).unwrap()).unwrap();
        assert_eq!(s.plan(), Some(&plan()));
        assert_eq!(s.gate(), Gate::Allowed);
        assert_eq!(
            s.dispatch("draft", 9).unwrap().task,
            "t2",
            "the task counter survives a relaunch"
        );
        let mut edited = plan();
        edited.brief.push('!');
        s.set_plan(edited, 10).unwrap();
        let s2 = Studio::open(Project::open(&root).unwrap()).unwrap();
        assert_eq!(s2.gate(), Gate::PlanChanged);
    }

    #[test]
    fn completing_promotes_the_artifact_and_records_usage_without_inventing_any() {
        let mut s = studio("complete");
        s.set_plan(plan(), 2).unwrap();
        s.approve("Nitish", 3).unwrap();
        let d = s.dispatch("draft", 4).unwrap();
        std::fs::write(d.dir.join("artifact.md"), "# Plan").unwrap();
        let usage = [
            Entry {
                step: "t1".into(),
                meter: Meter::Tokens,
                value: Some(8705.0),
                status: Status::Reported,
                detail: "gpt-6-luna".into(),
            },
            Entry {
                step: "t1".into(),
                meter: Meter::PlanWindow,
                value: None,
                status: Status::Unknown,
                detail: "codex".into(),
            },
        ];
        let a = s.complete(&d, "artifact.md", "plan", &usage, 5).unwrap();
        assert!(a.exists());
        let l = s.ledger().unwrap();
        assert_eq!(l.display(Meter::Tokens), "8705");
        assert_eq!(l.display(Meter::PlanWindow), "codex unknown");
        assert_eq!(l.display(Meter::ApiDollars), "unknown");
        assert!(s
            .journal
            .events()
            .unwrap()
            .iter()
            .any(|e| e.kind == "worker.finished" && e.payload["task"] == "t1"));
    }

    #[test]
    fn a_failed_worker_keeps_its_folder_and_journals_the_reason() {
        let mut s = studio("fail");
        s.set_plan(plan(), 2).unwrap();
        s.approve("Nitish", 3).unwrap();
        let d = s.dispatch("draft", 4).unwrap();
        std::fs::write(d.dir.join("partial.md"), "half").unwrap();
        s.fail(&d, "timed out", &[], 5).unwrap();
        assert_eq!(
            std::fs::read_to_string(d.dir.join("partial.md")).unwrap(),
            "half"
        );
        let ev = s.journal.events().unwrap();
        assert!(ev
            .iter()
            .any(|e| e.kind == "worker.failed" && e.payload["reason"] == "timed out"));
        assert!(s.complete(&d, "missing.md", "x", &[], 6).is_err());
    }

    #[test]
    fn a_side_task_needs_the_approved_plan_gets_the_next_task_number_and_is_journaled_with_its_kind(
    ) {
        let mut s = studio("side");
        assert!(
            s.dispatch_task("review", "review:note", 1).is_err(),
            "no plan yet"
        );
        s.set_plan(plan(), 2).unwrap();
        assert!(
            s.dispatch_task("review", "review:note", 3).is_err(),
            "not approved"
        );
        s.approve("Nitish", 4).unwrap();
        let a = s.dispatch("draft", 5).unwrap();
        let b = s.dispatch_task("review", "review:note", 6).unwrap();
        assert_eq!((a.task.as_str(), b.task.as_str()), ("t1", "t2"));
        assert!(b.dir.is_dir());
        let ev = s
            .journal
            .events()
            .unwrap()
            .into_iter()
            .find(|e| e.payload["task"] == "t2")
            .unwrap();
        assert_eq!(ev.kind, "worker.dispatched");
        assert_eq!(ev.payload["kind"], "review");
        assert_eq!(ev.payload["stage"], "review:note");
        // A changed plan closes the gate for side tasks too.
        let mut p2 = plan();
        p2.brief = "different".into();
        s.set_plan(p2, 7).unwrap();
        assert!(s.dispatch_task("review", "x", 8).is_err());
        // Numbering survives a reopen.
        let root = s.project.root.clone();
        let mut s = Studio::open(Project::open(&root).unwrap()).unwrap();
        s.approve("Nitish", 9).unwrap();
        assert_eq!(
            s.dispatch_task("revise", "revise:note", 10).unwrap().task,
            "t3"
        );
    }

    fn tokens(step: &str, n: Option<f64>) -> Vec<Entry> {
        vec![Entry {
            step: step.into(),
            meter: Meter::Tokens,
            value: n,
            status: if n.is_some() {
                Status::Reported
            } else {
                Status::Unknown
            },
            detail: "m".into(),
        }]
    }

    fn approved(tag: &str) -> Studio {
        let mut s = studio(tag);
        s.set_plan(plan(), 1).unwrap();
        s.approve("Nitish", 2).unwrap();
        s
    }

    #[test]
    fn a_second_task_that_does_not_fit_the_budget_is_refused_and_leaves_no_trace() {
        let mut s = approved("budget-fit");
        s.set_token_budget(100, 60).unwrap();
        s.approve("Fixture owner", 3).unwrap();
        let a = s.dispatch("draft", 3).unwrap();
        let tasks_before = std::fs::read_dir(s.project.root.join("tasks"))
            .unwrap()
            .count();
        let events_before = s.journal.events().unwrap().len();
        let err = s.dispatch_task("review", "review:x", 4).unwrap_err();
        assert!(err.to_lowercase().contains("budget"), "{err}");
        assert_eq!(
            std::fs::read_dir(s.project.root.join("tasks"))
                .unwrap()
                .count(),
            tasks_before
        );
        assert_eq!(s.journal.events().unwrap().len(), events_before);
        // Finishing the first with less than its reservation frees room for the second.
        std::fs::write(a.dir.join("artifact.md"), "x").unwrap();
        s.complete(&a, "artifact.md", "note", &tokens(&a.task, Some(20.0)), 5)
            .unwrap();
        assert_eq!(s.budget_status().unwrap().spent, 20);
        assert!(s.dispatch_task("review", "review:x", 6).is_ok());
    }

    #[test]
    fn an_overrun_pauses_the_budget_until_it_is_raised() {
        let mut s = approved("budget-pause");
        s.set_token_budget(100, 50).unwrap();
        s.approve("Fixture owner", 3).unwrap();
        let a = s.dispatch("draft", 3).unwrap();
        std::fs::write(a.dir.join("artifact.md"), "x").unwrap();
        s.complete(&a, "artifact.md", "note", &tokens(&a.task, Some(130.0)), 4)
            .unwrap();
        let st = s.budget_status().unwrap();
        assert!(st.paused && st.spent == 130 && st.available == 0, "{st:?}");
        let err = s.dispatch_task("review", "r", 5).unwrap_err();
        assert!(err.to_lowercase().contains("paused"), "{err}");
        s.raise_token_budget(300).unwrap();
        s.approve("Fixture owner", 3).unwrap();
        assert!(!s.budget_status().unwrap().paused);
        assert!(s.dispatch_task("review", "r", 6).is_ok());
        s.raise_token_budget(130).unwrap_err();
    }

    #[test]
    fn unknown_usage_is_charged_at_the_reservation_and_counted() {
        let mut s = approved("budget-unknown");
        s.set_token_budget(200, 60).unwrap();
        s.approve("Fixture owner", 3).unwrap();
        let a = s.dispatch("draft", 3).unwrap();
        s.fail(&a, "timed out", &tokens(&a.task, None), 4).unwrap();
        let st = s.budget_status().unwrap();
        assert_eq!((st.spent, st.unknown_charges, st.available), (60, 1, 140));
    }

    #[test]
    fn a_failed_task_with_no_usage_entry_at_all_is_also_unknown_not_free() {
        let mut s = approved("budget-none");
        s.set_token_budget(200, 60).unwrap();
        s.approve("Fixture owner", 3).unwrap();
        let a = s.dispatch("draft", 3).unwrap();
        s.fail(&a, "never started", &[], 4).unwrap();
        assert_eq!(s.budget_status().unwrap().spent, 60);
    }

    #[test]
    fn earlier_journal_usage_counts_when_the_budget_is_set_after_a_restart() {
        let mut s = approved("budget-reopen");
        let a = s.dispatch("draft", 3).unwrap();
        std::fs::write(a.dir.join("artifact.md"), "x").unwrap();
        s.complete(&a, "artifact.md", "note", &tokens(&a.task, Some(70.0)), 4)
            .unwrap();
        let b = s.dispatch_task("review", "r", 5).unwrap();
        s.fail(&b, "no usage reported", &tokens(&b.task, None), 6)
            .unwrap();
        let root = s.project.root.clone();
        let mut s = Studio::open(Project::open(&root).unwrap()).unwrap();
        s.set_token_budget(500, 40).unwrap();
        s.approve("Fixture owner", 3).unwrap();
        let st = s.budget_status().unwrap();
        assert_eq!(
            st.spent,
            70 + 40,
            "known tokens plus one unknown step at the per-task amount"
        );
        assert_eq!(st.unknown_charges, 0, "seeded, not newly charged");
    }

    #[test]
    fn without_a_budget_nothing_is_reserved_and_status_is_none() {
        let mut s = approved("nobudget");
        assert!(s.budget_status().is_none());
        for i in 0..5 {
            s.dispatch_task("review", &format!("r{i}"), 3).unwrap();
        }
    }

    #[test]
    fn a_budget_must_have_a_task_amount_that_fits_in_it() {
        let mut s = approved("budget-bad");
        assert!(s.set_token_budget(50, 60).is_err());
        assert!(s.set_token_budget(50, 0).is_err());
        assert!(s.budget_status().is_none());
    }

    #[test]
    fn keeping_nothing_needs_every_step_on_openrouter_and_old_plans_keep_their_saved_form() {
        let mut p = plan();
        assert!(
            !serde_json::to_string(&p).unwrap().contains("privacy"),
            "a plan with the default leaves it out, so its approval hash does not change"
        );
        p.privacy = crate::plan::Privacy::NoRetention;
        assert!(serde_json::to_string(&p)
            .unwrap()
            .contains("\"privacy\":\"no_retention\""));
        let problems = validate_plan(&p);
        assert!(
            problems.iter().any(|x| x.contains("step draft runs on")
                && x.contains("cannot promise to keep nothing")),
            "{problems:?}"
        );
        p.team[0] = Assignment {
            role: "writer".into(),
            route: "R12".into(),
            model: "anthropic/claude-haiku-5.5".into(),
            effort: "medium".into(),
        };
        assert!(validate_plan(&p).is_empty(), "{:?}", validate_plan(&p));
    }

    #[test]
    fn fallbacks_in_a_plan_are_validated_like_the_team() {
        let mut p = plan();
        p.fallbacks = vec![Assignment {
            role: "writer".into(),
            route: "R2".into(),
            model: "claude-haiku-5-5".into(),
            effort: "low".into(),
        }];
        assert!(validate_plan(&p).is_empty(), "{:?}", validate_plan(&p));
        let mut same_route = p.clone();
        same_route.fallbacks[0] = Assignment {
            role: "writer".into(),
            route: "R1".into(),
            model: "gpt-6.1-terra".into(),
            effort: "low".into(),
        };
        assert!(
            validate_plan(&same_route)
                .iter()
                .any(|x| x.contains("other route")),
            "{:?}",
            validate_plan(&same_route)
        );
        let mut stranger = p.clone();
        stranger.fallbacks[0].role = "ghost".into();
        assert!(validate_plan(&stranger)
            .iter()
            .any(|x| x.contains("ghost") && x.contains("not on the team")));
        let mut bad_model = p.clone();
        bad_model.fallbacks[0].model = "gpt-6-luna".into();
        assert!(validate_plan(&bad_model)
            .iter()
            .any(|x| x.contains("fallback")));
        let mut twice = p.clone();
        twice.fallbacks.push(twice.fallbacks[0].clone());
        assert!(validate_plan(&twice)
            .iter()
            .any(|x| x.contains("more than one fallback")));
    }

    #[test]
    fn a_task_left_unfinished_by_a_crash_is_marked_failed_once_with_unknown_tokens_and_its_folder_kept(
    ) {
        let mut s = approved("recover");
        let a = s.dispatch("draft", 3).unwrap();
        std::fs::write(a.dir.join("partial.md"), "half").unwrap();
        let b = s.dispatch_task("review", "r", 4).unwrap();
        s.fail(&b, "done", &[], 5).unwrap();
        // The app stops here. A new launch opens the project again.
        let root = s.project.root.clone();
        let mut s = Studio::open(Project::open(&root).unwrap()).unwrap();
        assert_eq!(
            s.recover(10).unwrap(),
            vec!["t1".to_string()],
            "only the unfinished task"
        );
        assert_eq!(
            s.recover(11).unwrap(),
            Vec::<String>::new(),
            "a second call changes nothing"
        );
        assert_eq!(
            std::fs::read_to_string(a.dir.join("partial.md")).unwrap(),
            "half"
        );
        let ev = s.journal.events().unwrap();
        let failed = ev
            .iter()
            .find(|e| e.kind == "worker.failed" && e.payload["task"] == "t1")
            .unwrap();
        assert!(failed.payload["reason"]
            .as_str()
            .unwrap()
            .contains("interrupted"));
        assert_eq!(s.ledger().unwrap().display(Meter::Tokens), "unknown");
        // A budget set afterwards charges the lost task at its reservation.
        s.set_token_budget(500, 100).unwrap();
        s.approve("Fixture owner", 3).unwrap();
        assert_eq!(
            s.budget_status().unwrap().spent,
            100,
            "t1 was recovered as unknown tokens"
        );
    }

    #[test]
    fn nothing_to_recover_on_a_clean_project() {
        let mut s = approved("recover-clean");
        let a = s.dispatch("draft", 3).unwrap();
        std::fs::write(a.dir.join("artifact.md"), "x").unwrap();
        s.complete(&a, "artifact.md", "note", &[], 4).unwrap();
        assert!(s.recover(5).unwrap().is_empty());
    }

    fn two_stage_plan() -> Plan {
        let mut p = plan();
        p.stages.push(Stage {
            uses: None,
            after: vec![],
            id: "review".into(),
            title: "Review".into(),
            owner_role: "writer".into(),
            guide: String::new(),
        });
        p
    }

    #[test]
    fn a_blocked_stage_cannot_start_while_the_others_can_and_the_block_survives_a_reopen() {
        let mut s = studio("blocked");
        s.set_plan(two_stage_plan(), 1).unwrap();
        s.approve("Nitish", 2).unwrap();
        s.block_stage(
            "draft",
            "github is blocked: the credential was refused with status 401",
            3,
        )
        .unwrap();
        let err = s.dispatch("draft", 4).unwrap_err();
        assert!(err.contains("blocked") && err.contains("401"), "{err}");
        assert!(
            s.dispatch("review", 5).is_ok(),
            "another stage is not affected"
        );
        assert_eq!(s.blocked_stages().len(), 1);
        // Task folders were not created for the refused dispatch.
        assert_eq!(
            std::fs::read_dir(s.project.root.join("tasks"))
                .unwrap()
                .count(),
            1
        );
        let root = s.project.root.clone();
        let mut s = Studio::open(Project::open(&root).unwrap()).unwrap();
        assert_eq!(
            s.blocked_stages(),
            vec![(
                "draft".to_string(),
                "github is blocked: the credential was refused with status 401".to_string()
            )]
        );
        assert!(s.dispatch("draft", 6).is_err());
        s.unblock_stage("draft", 7).unwrap();
        assert!(s.dispatch("draft", 8).is_ok());
        assert!(s.blocked_stages().is_empty());
    }

    #[test]
    fn blocking_needs_a_real_stage_and_unblocking_needs_a_block() {
        let mut s = studio("blocked-bad");
        s.set_plan(plan(), 1).unwrap();
        assert!(s.block_stage("nope", "x", 2).is_err());
        assert!(s.unblock_stage("draft", 3).is_err(), "it was not blocked");
        s.block_stage("draft", "x", 4).unwrap();
        s.block_stage("draft", "second reason", 5).unwrap();
        assert_eq!(
            s.blocked_stages(),
            vec![("draft".to_string(), "x".to_string())],
            "the first reason stands"
        );
    }

    #[test]
    fn cost_is_attributed_to_the_route_that_ran_each_task_and_meters_are_never_added_together() {
        let mut s = approved("by-route");
        let ent = |step: &str, meter: Meter, v: Option<f64>| Entry {
            step: step.into(),
            meter,
            value: v,
            status: if v.is_some() {
                Status::Reported
            } else {
                Status::Unknown
            },
            detail: "x".into(),
        };
        s.record_usage(&[ent("plan", Meter::Tokens, Some(1000.0))], 1)
            .unwrap();
        s.record_usage(
            &[
                ent("t1", Meter::Tokens, Some(100.0)),
                ent("t1", Meter::PlanWindow, Some(2.0)),
            ],
            2,
        )
        .unwrap();
        s.record_timing("t1", 1.0, "R1", "gpt-6-luna", 2).unwrap();
        s.record_usage(
            &[
                ent("t2", Meter::Tokens, Some(50.0)),
                ent("t2", Meter::ApiDollars, Some(0.01)),
            ],
            3,
        )
        .unwrap();
        s.record_timing("t2", 1.0, "R5", "gpt-6.1-terra", 3)
            .unwrap();
        s.record_usage(
            &[
                ent("t3", Meter::Tokens, None),
                ent("t3", Meter::ApiDollars, Some(0.02)),
            ],
            4,
        )
        .unwrap();
        s.record_timing("t3", 1.0, "R5", "gpt-6.1-terra", 4)
            .unwrap();
        s.record_usage(&[ent("t4", Meter::Tokens, Some(70.0))], 5)
            .unwrap();
        s.record_timing("t4", 1.0, "R1", "gpt-6-luna", 5).unwrap();
        let c = s.cost_by_route().unwrap();
        assert_eq!(
            c["R1"],
            RouteCost {
                tokens: Some(170.0),
                api_dollars: None,
                unknown_token_steps: 0
            }
        );
        assert_eq!(c["R5"].tokens, Some(50.0));
        assert!((c["R5"].api_dollars.unwrap() - 0.03).abs() < 1e-12);
        assert_eq!(
            c["R5"].unknown_token_steps, 1,
            "t3 reported no tokens, so it is counted as unknown and not as zero"
        );
        assert_eq!(
            c["unattributed"],
            RouteCost {
                tokens: Some(1000.0),
                api_dollars: None,
                unknown_token_steps: 0
            }
        );
        assert_eq!(c.len(), 3);
    }

    #[test]
    fn tokens_are_counted_per_model_and_a_model_on_two_routes_is_two_rows() {
        let mut s = approved("by-model");
        let ent = |step: &str, v: Option<f64>| Entry {
            step: step.into(),
            meter: Meter::Tokens,
            value: v,
            status: if v.is_some() {
                Status::Reported
            } else {
                Status::Unknown
            },
            detail: "x".into(),
        };
        s.record_usage(&[ent("plan", Some(1000.0))], 1).unwrap();
        for (task, route, model, tokens, at) in [
            ("t1", "R1", "gpt-6.1-terra", Some(100.0), 2),
            ("t2", "R5", "gpt-6.1-terra", Some(50.0), 3),
            ("t3", "R5", "gpt-6.1-terra", None, 4),
            ("t4", "R1", "gpt-6.1-terra", Some(70.0), 5),
            ("t5", "R2", "claude-haiku-5-5", Some(9.0), 6),
        ] {
            s.record_usage(&[ent(task, tokens)], at).unwrap();
            s.record_timing(task, 1.0, route, model, at).unwrap();
        }
        let rows = s.tokens_by_model().unwrap();
        let find = |route: &str, model: &str| {
            rows.iter()
                .find(|r| r.route == route && r.model == model)
                .unwrap()
        };
        assert_eq!(find("R1", "gpt-6.1-terra").tokens, Some(170.0), "Codex");
        assert_eq!(
            find("R5", "gpt-6.1-terra").tokens,
            Some(50.0),
            "the same model on the API is its own row"
        );
        assert_eq!(
            find("R5", "gpt-6.1-terra").unknown_token_steps,
            1,
            "unknown is counted, never zero"
        );
        assert_eq!(find("R2", "claude-haiku-5-5").tokens, Some(9.0));
        assert_eq!(
            find("", "planning").tokens,
            Some(1000.0),
            "the planning turn has no timing record"
        );
        assert_eq!(rows.len(), 4);
        // Largest first, so the biggest user is at the top.
        assert!(rows
            .windows(2)
            .all(|w| w[0].tokens.unwrap_or(0.0) >= w[1].tokens.unwrap_or(0.0)));
    }

    #[test]
    fn the_token_budget_and_its_raises_survive_closing_and_reopening() {
        let b = base("budget-survives");
        let root = {
            let mut s = Studio::open(Project::create(&b, "case", "Write.", 1).unwrap()).unwrap();
            s.set_plan(plan(), 2).unwrap();
            s.approve("Nitish", 3).unwrap();
            s.set_token_budget(500, 100).unwrap();
            s.approve("Fixture owner", 3).unwrap();
            s.raise_token_budget(700).unwrap();
            s.approve("Fixture owner", 3).unwrap();
            // A task is running when the app stops: nobody saw its usage.
            s.dispatch("draft", 4).unwrap();
            s.project.root.clone()
        };
        let s = Studio::open(Project::open(&root).unwrap()).unwrap();
        let st = s.budget_status().expect("the budget is still there");
        assert_eq!((st.limit, st.spent), (700, 100), "{st:?}");
    }

    #[test]
    fn the_reason_a_step_did_not_finish_is_kept_until_it_finishes_and_survives_a_reopen() {
        let b = base("failures");
        let root = {
            let mut s = Studio::open(Project::create(&b, "case", "Write.", 1).unwrap()).unwrap();
            s.set_plan(plan(), 2).unwrap();
            s.approve("Nitish", 3).unwrap();
            let d = s.dispatch("draft", 4).unwrap();
            s.fail(&d, "paused: stopped on request", &[], 5).unwrap();
            assert_eq!(
                s.failed_stages().unwrap(),
                vec![(
                    "draft".to_string(),
                    "paused: stopped on request".to_string()
                )]
            );
            s.project.root.clone()
        };
        let mut s = Studio::open(Project::open(&root).unwrap()).unwrap();
        assert_eq!(
            s.failed_stages().unwrap().len(),
            1,
            "still there after a reopen"
        );
        let d = s.dispatch("draft", 6).unwrap();
        std::fs::write(d.dir.join("artifact.md"), "# Done").unwrap();
        s.complete(&d, "artifact.md", "draft", &[], 7).unwrap();
        assert!(
            s.failed_stages().unwrap().is_empty(),
            "a step that finished is not listed"
        );
    }
}
