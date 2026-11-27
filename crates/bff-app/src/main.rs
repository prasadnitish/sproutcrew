//! The BFF shell: a thin command layer over `bff_core::app::App`. Application logic does not live here.

use bff_core::app::App;
use bff_core::clock::now;
use bff_core::engine::{Engine, RoutedEngine, SubscriptionUnavailable};
use bff_core::route;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder};
use tauri::{Manager, State, WindowEvent};
mod attention;
use attention::{
    attention_status, notify_run, set_notification_names, set_notifications, test_notification,
};

struct Shared {
    app: Arc<Mutex<App>>,
    /// The person signed in to this Mac, read once at launch. Every decision records it; no screen chooses it.
    approver: String,
    /// The first safety stop on any route, held for all of them until BFF restarts.
    latch: bff_core::stop::StopLatch,
    data: PathBuf,
    /// Set to interrupt the running turn.
    cancel: Arc<AtomicBool>,
    /// Number of engine runs in flight.
    active: Arc<AtomicUsize>,
    /// Held for the whole of a run of steps, so nothing treats the moment between two steps as idle.
    hold: bff_core::app::RunHold,
    /// The running prototype preview, if any.
    preview: Mutex<Option<bff_core::preview::Preview>>,
    /// GitHub and Notion. A refused credential blocks one until a person unblocks it.
    connectors: Arc<Mutex<bff_core::connector::Connectors>>,
    /// The dollar cap on API spending, across every project and route.
    spend: Arc<Mutex<bff_core::spend::SpendBook>>,
    /// The token cap for API routes that count tokens and have no prices.
    tokens: Arc<Mutex<bff_core::spend::SpendBook>>,
}

/// Only API routes with settings and a key are runnable in the public beta.
/// A plan SproutCrew makes is moved onto these before it is approved.
fn refresh_runnable(sh: &Shared) {
    use bff_core::apiconfig::{states, RouteState};
    let keys = bff_core::keychain::Keychain::new();
    let mut routes = Vec::new();
    routes.extend(
        states(&api_config(&sh.data), &|n| keys.present(n))
            .into_iter()
            .filter(|(_, st)| *st == RouteState::Ready)
            .map(|(r, _)| r),
    );
    sh.app.lock().expect("app lock").set_runnable(routes);
}

#[derive(Serialize, Clone)]
#[serde(tag = "event", rename_all = "snake_case")]
enum RunEvent {
    Started { engine: String, what: String },
    Progress { line: String },
    Finished { ok: bool, note: String },
}

/// What an engine is waiting for before it retries, such as "Retrying in 4 seconds". The pulse shows it.
static RETRY_NOTICE: std::sync::LazyLock<Arc<Mutex<Option<String>>>> =
    std::sync::LazyLock::new(Arc::default);

/// Sends a progress line at least every 30 seconds while a run is in flight, so a long turn never looks stuck.
/// A retry wait is shown as it counts down.
struct Pulse(Arc<AtomicBool>);

impl Pulse {
    fn start(on_event: Channel<RunEvent>, what: String) -> Pulse {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            let begun = std::time::Instant::now();
            let mut hb = bff_core::heartbeat::Heartbeat::new(&what, 0);
            let mut shown: Option<String> = None;
            while !flag.load(Ordering::SeqCst) {
                let waiting = RETRY_NOTICE.lock().expect("notice").clone();
                if waiting.is_some() && waiting != shown {
                    let _ = on_event.send(RunEvent::Progress {
                        line: waiting.clone().unwrap_or_default(),
                    });
                } else if waiting.is_none() && shown.is_some() {
                    // The countdown ended, so the last line would otherwise keep saying "Retrying in 1 second".
                    let _ = on_event.send(RunEvent::Progress {
                        line: format!("Trying again: {what}"),
                    });
                } else if let Some(line) = hb.line(begun.elapsed().as_secs()) {
                    let _ = on_event.send(RunEvent::Progress { line });
                }
                shown = waiting;
                std::thread::sleep(Duration::from_millis(500));
            }
        });
        Pulse(stop)
    }
}

impl Drop for Pulse {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn engine_for(mode: &str, sh: &Shared) -> Result<Box<dyn Engine>, String> {
    engine_for_at(mode, sh, 0)
}

/// An engine for one worker among several that run at the same time. Each gets its own scratch folders, so two
/// of them never share a working directory. Everything else, such as the stop flag and the spend books, is shared.
fn engine_for_at(mode: &str, sh: &Shared, _branch: usize) -> Result<Box<dyn Engine>, String> {
    if mode != "live" {
        return Err(format!("unknown engine {mode:?}"));
    }
    // Every request this engine makes follows the open project's plan on what providers may keep.
    let keep_nothing =
        sh.app.lock().expect("app lock").plan_privacy() == bff_core::plan::Privacy::NoRetention;
    let mut engines = api_engines(&sh.data, &sh.spend, &sh.tokens, &sh.cancel, keep_nothing);
    if engines.is_empty() {
        return Err("Connect an API key in Settings → AI connections before running AI work. Drafts and local project setup remain available.".into());
    }
    // OpenRouter first; a single key can both write and perform a clearly labelled checking pass.
    engines.sort_by_key(|(r, _)| if *r == "R12" { 0 } else { 1 });
    let planner = engines.first().map(|(route, _)| *route);
    let mut routed = RoutedEngine::new(
        Box::new(SubscriptionUnavailable),
        Box::new(SubscriptionUnavailable),
    )
    .with_latch(sh.latch.clone())
    .with_cancel(sh.cancel.clone());
    for (route, engine) in engines {
        routed = routed.with_route(route, engine);
    }
    if let Some(route) = planner {
        routed = routed.with_api_planner(route);
    }
    Ok(Box::new(routed))
}

fn api_config(
    data: &std::path::Path,
) -> std::collections::BTreeMap<String, bff_core::apiconfig::RouteConfig> {
    std::fs::read_to_string(data.join("api-routes.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| bff_core::apiconfig::parse(&v).ok())
        .unwrap_or_default()
}

/// Engines for the API routes that have both settings and a key. None unless the person set both up.
fn api_engines(
    data: &std::path::Path,
    dollars: &Arc<Mutex<bff_core::spend::SpendBook>>,
    tokens: &Arc<Mutex<bff_core::spend::SpendBook>>,
    cancel: &Arc<AtomicBool>,
    keep_nothing: bool,
) -> Vec<(&'static str, Box<dyn Engine>)> {
    use bff_core::api_engine::ApiEngine;
    use bff_core::http::HttpClient;
    use bff_core::keychain::Keychain;
    let cfg = api_config(data);
    let keys = Keychain::new();
    bff_core::apiconfig::ready_engines(&cfg, &|n| keys.present(n), &mut |p, c| {
        // The person's own service talks only to the host in its saved, checked address.
        let own = (p == bff_core::apiroute::Provider::Custom)
            .then(|| {
                c.base_url
                    .as_deref()
                    .and_then(|b| bff_core::custom_ai::check_address(b).ok())
            })
            .flatten();
        let client = match &own {
            Some(a) => HttpClient::for_own_service(a),
            None => HttpClient::for_models(&[p.host()]),
        };
        let engine = ApiEngine::new(
            p,
            Box::new(client),
            Box::new(Keychain::new()),
            c.bounds,
            c.prices,
            c.allowance_micro,
        )
        .with_waits(cancel.clone(), RETRY_NOTICE.clone())
        .with_no_retention(keep_nothing);
        let engine = match own {
            Some(a) => engine.with_endpoint(a.base),
            None => engine,
        };
        // Paid OpenRouter models count against each project's own dollar limit, priced from the saved model list.
        // The list is read at each request, so a list fetched after the run started is used.
        let engine = if p == bff_core::apiroute::Provider::OpenRouter {
            let dir = data.to_path_buf();
            let seeing = data.to_path_buf();
            engine
                .with_project_limit(Arc::new(move |model: &str| {
                    let m = bff_core::catalog::load(&dir)?.find(model)?.clone();
                    Some((m.input_per_mtok?, m.output_per_mtok?))
                }))
                // Pictures go only to models the saved list says can read images.
                .with_image_models(Arc::new(move |model: &str| {
                    bff_core::catalog::load(&seeing)
                        .and_then(|c| c.find(model).map(|m| m.sees_images))
                        .unwrap_or(false)
                }))
        } else {
            engine
        };
        // A route with no prices counts tokens against the token cap. A priced route counts dollars.
        if c.token_mode {
            engine.token_mode().with_book(tokens.clone())
        } else {
            engine.with_book(dollars.clone())
        }
    })
}

/// The refusal of a second run in the same project.
const RUN_GOING: &str = "A run is already going in this project.";

/// Clears the stop flag for work that is about to start. When a piece of work is still in flight it may be one the
/// person stopped, so the flag is left alone and a later start cannot un-cancel it.
fn clear_stop(sh: &Shared) {
    if sh.hold.steps_in_flight() == 0 {
        sh.cancel.store(false, Ordering::SeqCst);
    }
}

/// Counts one piece of work as in progress until it is dropped.
struct ActiveGuard(Arc<AtomicUsize>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Runs `f` on a blocking thread while counting it as an active run.
async fn blocking<T: Send + 'static>(
    sh: &Shared,
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    let active = sh.active.clone();
    active.fetch_add(1, Ordering::SeqCst);
    let out = tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string());
    active.fetch_sub(1, Ordering::SeqCst);
    out
}

#[tauri::command]
fn app_info(app: tauri::AppHandle) -> Value {
    json!({
        "version": app.package_info().version.to_string(),
        "stress_mode": std::env::var_os("BFF_STRESS_REPORT").is_some(),
        "built_at": built_at(),
    })
}

/// When the running program was built, in seconds, from its own file. It shows which copy of the app is open.
fn built_at() -> Option<u64> {
    std::fs::metadata(std::env::current_exe().ok()?)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

/// For the WKWebView stress check only. It writes the report to the path in `BFF_STRESS_REPORT` and
/// refuses when that variable is not set, so the page can never choose where to write.
#[tauri::command]
fn write_stress_report(report: Value) -> Result<(), String> {
    let path = std::env::var_os("BFF_STRESS_REPORT").ok_or("stress reporting is not enabled")?;
    std::fs::write(path, report.to_string()).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_projects(sh: State<'_, Shared>) -> Vec<String> {
    sh.app.lock().expect("app lock").projects()
}

/// The projects with a line of brief and their progress, for the home screen.
#[tauri::command]
fn projects_overview(sh: State<'_, Shared>) -> Value {
    sh.app.lock().expect("app lock").projects_overview()
}

#[tauri::command(async)]
fn create_project(
    sh: State<'_, Shared>,
    name: String,
    brief: String,
    allow_key_shape: Option<bool>,
) -> Result<(), String> {
    sh.app.lock().expect("app lock").create_project_allowing(
        &name,
        &brief,
        now(),
        allow_key_shape.unwrap_or(false),
    )
}

#[tauri::command]
async fn create_project_from(
    sh: State<'_, Shared>,
    from: String,
    name: String,
    brief: String,
    source_ids: Vec<String>,
    allow_key_shape: Option<bool>,
) -> Result<(), String> {
    if sh.active.load(Ordering::SeqCst) > 0 {
        return Err("Finish or stop the current work before starting over".into());
    }
    let app = sh.app.clone();
    let active = sh.active.clone();
    // Source files can be large. Keep the webview responsive, and count this copy for safe Quit.
    blocking(&sh, move || {
        let mut app = app
            .try_lock()
            .map_err(|_| "Finish or stop the current work before starting over".to_string())?;
        if active.load(Ordering::SeqCst) != 1 {
            return Err("Finish or stop the current work before starting over".into());
        }
        app.create_project_from_allowing(
            &from,
            &name,
            &brief,
            &source_ids,
            now(),
            allow_key_shape.unwrap_or(false),
        )
    })
    .await?
}

/// Hides a project from the list and keeps all of its work. Not while a run is going, since a run may be
/// writing to the project it would close.
#[tauri::command]
fn archive_project(sh: State<'_, Shared>, name: String) -> Result<(), String> {
    if sh.active.load(Ordering::SeqCst) > 0 {
        return Err("Finish or stop the current work before you archive a project.".into());
    }
    sh.app
        .lock()
        .expect("app lock")
        .archive_project(&name, now())
}

#[tauri::command]
fn restore_project(sh: State<'_, Shared>, name: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").restore_project(&name)
}

#[tauri::command]
fn open_project(sh: State<'_, Shared>, name: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").open_project(&name)
}

/// Runs a read off the main thread. The interface asks for these every few seconds, and a read that waits for
/// the project lock on the main thread would freeze the window for as long as an export or a save holds it.
async fn off_main<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_state(sh: State<'_, Shared>, engine: String) -> Result<Value, String> {
    let app = sh.app.clone();
    off_main(move || app.lock().expect("app lock").state(&engine)).await?
}

#[tauri::command]
async fn propose_plan(
    sh: State<'_, Shared>,
    engine: String,
    on_event: Channel<RunEvent>,
) -> Result<(), String> {
    let mut e = engine_for(&engine, &sh)?;
    let app = sh.app.clone();
    clear_stop(&sh);
    let _ = on_event.send(RunEvent::Started {
        engine: engine.clone(),
        what: "planning the steps".into(),
    });
    // The project is locked to read the brief and to keep the plan, never while the request is out.
    let r = blocking(&sh, move || {
        let (brief, asked) = app.lock().expect("app lock").begin_propose()?;
        let (plan, usage) = e.propose_plan(&asked)?;
        app.lock()
            .expect("app lock")
            .finish_propose(brief, plan, &usage, now())
    })
    .await?;
    let _ = on_event.send(RunEvent::Finished {
        ok: r.is_ok(),
        note: r.clone().err().unwrap_or_default(),
    });
    r
}

/// Gives the writing roles a stand-in on the other provider, or takes them away.
#[tauri::command]
fn set_stand_ins(sh: State<'_, Shared>, on: bool) -> Result<(), Vec<String>> {
    sh.app.lock().expect("app lock").set_stand_ins(on, now())
}

#[tauri::command]
fn save_plan(sh: State<'_, Shared>, plan: Value) -> Result<(), Vec<String>> {
    let allowed = bff_core::running_code::allowed(&sh.data);
    let mut app = sh.app.lock().expect("app lock");
    app.set_code_allowed(allowed);
    app.save_plan(plan, now())
}

/// What the person decided across projects, newest first (the board's Decided).
#[tauri::command]
fn decided_list(sh: State<'_, Shared>) -> Value {
    sh.app.lock().expect("app lock").decided(200)
}

/// The policy defaults and how each project compares with them (the Policy page).
#[tauri::command]
fn policy_overview(sh: State<'_, Shared>) -> Value {
    sh.app.lock().expect("app lock").policy_overview()
}

/// Saves the policy defaults for new projects, and tightens the projects named in `tighten` to them.
#[tauri::command]
fn set_policy_defaults(
    sh: State<'_, Shared>,
    defaults: bff_core::policy_defaults::Defaults,
    tighten: Vec<String>,
) -> Result<Vec<String>, String> {
    sh.app
        .lock()
        .expect("app lock")
        .set_policy_defaults(defaults, &tighten, now())
}

/// Marks the open project's finished results reviewed, as the person saw them (`version`).
#[tauri::command]
fn mark_reviewed(sh: State<'_, Shared>, project: String, version: String) -> Result<(), String> {
    sh.app
        .lock()
        .expect("app lock")
        .mark_reviewed(&project, &version, &sh.approver, now())
}

/// Approves the open plan only if it is the named project and still the version the person read (`hash`).
#[tauri::command]
fn approve(sh: State<'_, Shared>, project: String, hash: String) -> Result<(), String> {
    sh.app
        .lock()
        .expect("app lock")
        .approve_seen(&project, &hash, &sh.approver, now())
}

#[tauri::command]
async fn run_stage(
    sh: State<'_, Shared>,
    engine: String,
    stage_id: String,
    on_event: Channel<RunEvent>,
) -> Result<String, String> {
    let mut e = engine_for(&engine, &sh)?;
    let app = sh.app.clone();
    clear_stop(&sh);
    let _ = on_event.send(RunEvent::Started {
        engine: engine.clone(),
        what: format!("stage {stage_id}"),
    });
    // The progress lines name the step by its title. The start event above keeps the id, which the screen reads.
    let title = app.lock().expect("app lock").stage_title(&stage_id);
    let _pulse = Pulse::start(on_event.clone(), title);
    let step = stage_id.clone();
    if app
        .lock()
        .expect("app lock")
        .skip_steps_without_input(now())?
        .contains(&stage_id)
    {
        let note = "Skipped: the step it works on is not in the plan.".to_string();
        let _ = on_event.send(RunEvent::Finished {
            ok: true,
            note: note.clone(),
        });
        return Ok(note);
    }
    // The project is locked only to start a step and to record it. The worker runs with no lock held, so the rest of
    // the app (reading a finished step, asking, scrolling) stays usable while a step is running.
    let r = blocking(
        &sh,
        move || -> Result<bff_core::engine::StageOutcome, String> {
            let (mut who, mut used) = (None, 0);
            loop {
                let p = app.lock().expect("app lock").begin_stage(
                    &stage_id,
                    who.take(),
                    used,
                    now(),
                )?;
                let (result, secs) = bff_core::engine::run_prepared(e.as_mut(), &p);
                match app
                    .lock()
                    .expect("app lock")
                    .end_stage(p, result, secs, now())?
                {
                    bff_core::engine::Next::Over(o) => return Ok(o),
                    bff_core::engine::Next::Again {
                        who: next,
                        fallbacks_used,
                    } => {
                        who = Some(next);
                        used = fallbacks_used;
                    }
                }
            }
        },
    )
    .await?;
    let (ok, note) = match &r {
        Ok(o) => (o.artifact.is_some(), o.note.clone()),
        Err(e) => (false, e.clone()),
    };
    let _ = on_event.send(RunEvent::Finished {
        ok,
        note: note.clone(),
    });
    if !ok {
        // Only the kind of failure is logged, never the step's words.
        app_log(
            &sh.data,
            "step.failed",
            bff_core::applog::failure_kind(&note),
        );
    }
    // A step that made no result did not finish. The screen shows why instead of treating it as done.
    r.and_then(|o| {
        if o.artifact.is_some() {
            Ok(o.note)
        } else {
            let title = sh.app.lock().expect("app lock").stage_title(&step);
            Err(format!("“{title}” did not finish. {}", plain_note(&o.note)))
        }
    })
}

/// A step's failure note for the screen: without the `stopped:` or `paused:` label, starting with a capital, and
/// ending with one full stop.
fn plain_note(note: &str) -> String {
    let n = note
        .strip_prefix("stopped:")
        .or_else(|| note.strip_prefix("paused:"))
        .unwrap_or(note)
        .trim()
        .trim_end_matches(['.', ' ']);
    let mut c = n.chars();
    match c.next() {
        Some(f) => format!("{}{}.", f.to_uppercase(), c.as_str()),
        None => String::new(),
    }
}

/// The steps that can start now: not done, and everything they wait for is done.
#[tauri::command]
fn ready_stages(sh: State<'_, Shared>) -> Result<Vec<String>, String> {
    let mut app = sh.app.lock().expect("app lock");
    // Steps left with nothing to work on are skipped first, so the steps after them become ready.
    app.skip_steps_without_input(now())?;
    app.ready_stages()
}

/// Runs steps that are ready together, each on its own engine and thread. Returns what happened to each.
#[tauri::command]
async fn run_wave(
    sh: State<'_, Shared>,
    engine: String,
    stage_ids: Vec<String>,
    on_event: Channel<RunEvent>,
) -> Result<Value, String> {
    let engines = (0..stage_ids.len())
        .map(|i| engine_for_at(&engine, &sh, i))
        .collect::<Result<Vec<_>, _>>()?;
    let what = stage_ids
        .iter()
        .map(|s| format!("stage {s}"))
        .collect::<Vec<_>>()
        .join(" and ");
    let app = sh.app.clone();
    clear_stop(&sh);
    let _ = on_event.send(RunEvent::Started {
        engine: engine.clone(),
        what: what.clone(),
    });
    let titles = {
        let app = app.lock().expect("app lock");
        stage_ids
            .iter()
            .map(|s| app.stage_title(s))
            .collect::<Vec<_>>()
            .join(" and ")
    };
    let _pulse = Pulse::start(on_event.clone(), titles);
    let ids = stage_ids.clone();
    let r = blocking(&sh, move || {
        let jobs = app.lock().expect("app lock").begin_wave(&ids, now())?;
        let runs = App::run_wave_workers(&jobs, engines);
        app.lock().expect("app lock").end_wave(jobs, runs, now())
    })
    .await?;
    let rows: Vec<Value> = match &r {
        Ok(v) => v
            .iter()
            .map(|(id, o)| match o {
                Ok(o) => json!({"stage": id, "ok": o.artifact.is_some(), "note": o.note}),
                Err(e) => json!({"stage": id, "ok": false, "note": e}),
            })
            .collect(),
        Err(_) => vec![],
    };
    let ok = r.is_ok() && rows.iter().all(|x| x["ok"] == true);
    let _ = on_event.send(RunEvent::Finished {
        ok,
        note: r.as_ref().err().cloned().unwrap_or_default(),
    });
    r.map(|_| Value::Array(rows))
}

/// Runs a call that makes one model request, announcing it on the channel. The project is locked to get ready and to record
/// the answer, and the request itself runs with no lock held, so the rest of the app stays usable while it waits.
async fn engine_phased<P: Send + 'static, R: Send + 'static, T: Send + 'static>(
    sh: &Shared,
    engine: String,
    what: String,
    on_event: Channel<RunEvent>,
    begin: impl FnOnce(&mut App) -> Result<P, String> + Send + 'static,
    run: impl FnOnce(&mut dyn Engine, &P) -> R + Send + 'static,
    finish: impl FnOnce(&mut App, P, R) -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let mut e = engine_for(&engine, sh)?;
    let app = sh.app.clone();
    clear_stop(sh);
    let _ = on_event.send(RunEvent::Started {
        engine,
        what: what.clone(),
    });
    let _pulse = Pulse::start(on_event.clone(), what);
    let r = blocking(sh, move || -> Result<T, String> {
        let p = begin(&mut app.lock().expect("app lock"))?;
        let out = run(e.as_mut(), &p);
        finish(&mut app.lock().expect("app lock"), p, out)
    })
    .await?;
    let (ok, note) = match &r {
        Ok(_) => (true, "done".to_string()),
        Err(e) => (false, e.clone()),
    };
    let _ = on_event.send(RunEvent::Finished { ok, note });
    r
}

#[tauri::command]
async fn review(
    sh: State<'_, Shared>,
    engine: String,
    artifact: String,
    reviewer: String,
    on_event: Channel<RunEvent>,
) -> Result<Value, String> {
    // Work other than a plan step is named by the step's title, never by its id.
    let what = format!(
        "a second check of {}",
        sh.app.lock().expect("app lock").stage_title(&artifact)
    );
    engine_phased(
        &sh,
        engine,
        what,
        on_event,
        move |app| app.begin_review(&artifact, &reviewer, now()),
        |e, p| bff_core::peer::Peer::run_review_call(e, p),
        move |app, p, (run, secs)| app.finish_review(p, run, secs, now()),
    )
    .await
}

#[tauri::command]
fn decide(
    sh: State<'_, Shared>,
    project: String,
    artifact: String,
    finding: String,
    decision: String,
    reason: String,
) -> Result<(), String> {
    let mut app = sh.app.lock().expect("app lock");
    app.require_open(&project)?;
    app.decide(&artifact, &finding, &decision, &reason, now())
}

#[tauri::command]
async fn revise(
    sh: State<'_, Shared>,
    engine: String,
    artifact: String,
    by: String,
    base: String,
    on_event: Channel<RunEvent>,
) -> Result<u32, String> {
    let what = format!(
        "a revision of {}",
        sh.app.lock().expect("app lock").stage_title(&artifact)
    );
    engine_phased(
        &sh,
        engine,
        what,
        on_event,
        move |app| app.begin_revise(&artifact, &by, &base, now()),
        |e, p| bff_core::peer::Peer::run_revise_call(e, p),
        move |app, p, run| app.finish_revise(p, run, now()),
    )
    .await
}

#[tauri::command]
fn set_budget(sh: State<'_, Shared>, limit: u64, per_task: u64) -> Result<(), String> {
    sh.app.lock().expect("app lock").set_budget(limit, per_task)
}

#[tauri::command]
fn set_project_limit(sh: State<'_, Shared>, dollars: f64) -> Result<Value, String> {
    sh.app.lock().expect("app lock").set_project_limit(dollars)
}

#[tauri::command]
fn raise_budget(sh: State<'_, Shared>, new_limit: u64) -> Result<(), String> {
    sh.app.lock().expect("app lock").raise_budget(new_limit)
}

fn spend_view(b: &bff_core::spend::SpendBook) -> Value {
    let s = b.status();
    // Dollars are shown in dollars, and tokens as they are.
    let n = |v: u64| match b.unit() {
        bff_core::spend::Unit::Dollars => json!(v as f64 / 1_000_000.0),
        bff_core::spend::Unit::Tokens => json!(v),
    };
    json!({"cap": n(s.cap), "spent": n(s.spent), "held": n(s.held), "available": n(s.available), "unknown_charges": s.unknown_charges})
}

/// The API caps. Tokens are always shown. Dollars are shown only when a route has prices.
#[tauri::command]
fn spend_status(sh: State<'_, Shared>) -> Value {
    let priced = api_config(&sh.data).values().any(|c| !c.token_mode);
    json!({
        "tokens": spend_view(&sh.tokens.lock().expect("tokens lock")),
        "dollars": if priced { spend_view(&sh.spend.lock().expect("spend lock")) } else { Value::Null },
    })
}

/// Approves a higher cap. This is the person's approval to keep going past the old one. `unit` is `tokens`
/// or `dollars`.
#[tauri::command]
fn spend_raise(sh: State<'_, Shared>, unit: String, new_cap: f64) -> Result<Value, String> {
    if !new_cap.is_finite() || new_cap <= 0.0 {
        return Err("type a cap as a number, such as 1000000".into());
    }
    match unit.as_str() {
        "tokens" => sh
            .tokens
            .lock()
            .expect("tokens lock")
            .raise_cap(new_cap.round() as u64),
        "dollars" => sh
            .spend
            .lock()
            .expect("spend lock")
            .raise_cap((new_cap * 1_000_000.0).round() as u64),
        other => Err(format!("{other:?} is not a unit. Use tokens or dollars")),
    }?;
    // The approval is the way on from a stop held for this cap, so the next request goes ahead without a
    // restart (Q2-B10). A stop for any other reason stays held.
    let cap = if unit == "tokens" {
        "token_cap"
    } else {
        "spending_cap"
    };
    if sh.latch.clear_cap(cap) {
        app_log(&sh.data, "stop.cleared", cap);
    }
    Ok(spend_status(sh))
}

/// Which API routes are ready, missing a key, or not set up.
#[tauri::command]
fn api_routes(sh: State<'_, Shared>) -> Value {
    use bff_core::apiconfig::{states, RouteState};
    let keys = bff_core::keychain::Keychain::new();
    let cfg = api_config(&sh.data);
    Value::Array(
        states(&cfg, &|n| keys.present(n))
            .into_iter()
            .map(|(route, st)| {
                let mut v = json!({"route": route, "state": match st {
                    RouteState::Ready => "ready",
                    RouteState::NoKey => "no_key",
                    RouteState::NotConfigured => "not_configured",
                }});
                // The person's own service shows its address and model, never its key.
                if let Some(c) = cfg.get(&route).filter(|_| route == "R13") {
                    v["address"] = json!(c.base_url);
                    v["model"] = json!(c.model);
                    v["has_key"] = json!(keys.present("custom-ai-api"));
                }
                v
            })
            .collect(),
    )
}

/// The routes whose key can be pasted in Settings.
const KEY_ROUTES: [&str; 4] = ["R12", "R3", "R5", "R9"];

/// Checks a pasted key with a read-only provider endpoint, saves it in the Keychain, and switches the route on
/// with token limits if it had no settings. The key is never logged or returned.
#[tauri::command]
async fn save_key(sh: State<'_, Shared>, route: String, key: String) -> Result<Value, String> {
    let data = sh.data.clone();
    let app = sh.app.clone();
    blocking(&sh, move || {
        let provider = KEY_ROUTES
            .contains(&route.as_str())
            .then(|| bff_core::apiroute::Provider::for_route(&route))
            .flatten()
            .ok_or("Keys for that provider are not set here")?;
        let key = key.trim();
        if !bff_core::keychain::valid_secret(key) {
            return Err(
                "That does not look like an API key. Paste the whole key, with no spaces."
                    .to_string(),
            );
        }
        let mut http = bff_core::http::HttpClient::new(&[provider.host()]);
        bff_core::apiroute::check_key(provider, key, &mut http)?;
        let _settings_guard = app.lock().map_err(|_| "Settings are unavailable")?;
        let path = data.join("api-routes.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => Some(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(format!(
                    "Request limits could not be read; the key was not changed: {e}"
                ))
            }
        };
        let next = bff_core::apiconfig::with_key_defaults(text.as_deref(), &route)?;
        bff_core::apiconfig::parse(
            &serde_json::from_str(&next).map_err(|e| format!("Request limits: {e}"))?,
        )
        .map_err(|problems| problems.join("; "))?;
        if text.as_deref() != Some(next.as_str()) {
            let temporary = path.with_extension("json.tmp");
            std::fs::write(&temporary, next).map_err(|e| e.to_string())?;
            std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
        }
        bff_core::keychain::Keychain::new().save(provider.credential(), key)?;
        Ok(())
    })
    .await??;
    refresh_runnable(&sh);
    Ok(api_routes(sh))
}

/// Saves the person's own AI service: checks the address and model, asks the service for its model list (which
/// costs nothing) to confirm it answers, then saves the address in the request limits and the key, if any, in the
/// Keychain. A key saved for an earlier address is removed, so it is never sent to a new one.
#[tauri::command]
async fn save_own_ai(
    sh: State<'_, Shared>,
    address: String,
    model: String,
    key: String,
) -> Result<Value, String> {
    let data = sh.data.clone();
    let app = sh.app.clone();
    let models = blocking(&sh, move || -> Result<Vec<String>, String> {
        let a = bff_core::custom_ai::check_address(&address)?;
        let key = key.trim();
        if !key.is_empty() && !bff_core::keychain::valid_secret(key) {
            return Err(
                "That does not look like a key. Paste the whole key, with no spaces.".into(),
            );
        }
        let mut http = bff_core::http::HttpClient::for_own_service(&a);
        let models =
            bff_core::custom_ai::check_service(&a, (!key.is_empty()).then_some(key), &mut http)?;
        let _settings_guard = app.lock().map_err(|_| "Settings are unavailable")?;
        let path = data.join("api-routes.json");
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => Some(t),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                return Err(format!(
                    "Request limits could not be read; nothing was changed: {e}"
                ))
            }
        };
        let next = bff_core::apiconfig::with_custom_ai(text.as_deref(), &a.base, &model)?;
        bff_core::apiconfig::parse(
            &serde_json::from_str(&next).map_err(|e| format!("Request limits: {e}"))?,
        )
        .map_err(|problems| problems.join("; "))?;
        // The address is saved first, and put back if the key cannot be, so a key is never left beside an
        // address it was not typed for.
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, next).map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
        let keys = bff_core::keychain::Keychain::new();
        let saved = if key.is_empty() {
            keys.remove("custom-ai-api")
        } else {
            keys.save("custom-ai-api", key)
        };
        if let Err(e) = saved {
            match &text {
                Some(old) => {
                    let _ = std::fs::write(&temporary, old)
                        .and_then(|()| std::fs::rename(&temporary, &path));
                }
                None => {
                    let _ = std::fs::remove_file(&path);
                }
            }
            return Err(e);
        }
        Ok(models)
    })
    .await??;
    refresh_runnable(&sh);
    Ok(json!({"routes": api_routes(sh), "models": models}))
}

/// Removes the person's own AI service and its key.
#[tauri::command]
fn remove_own_ai(sh: State<'_, Shared>) -> Result<Value, String> {
    {
        let _settings_guard = sh.app.lock().map_err(|_| "Settings are unavailable")?;
        let path = sh.data.join("api-routes.json");
        if let Ok(text) = std::fs::read_to_string(&path) {
            let next = bff_core::apiconfig::without_custom_ai(&text)?;
            let temporary = path.with_extension("json.tmp");
            std::fs::write(&temporary, next).map_err(|e| e.to_string())?;
            std::fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
        }
        bff_core::keychain::Keychain::new().remove("custom-ai-api")?;
    }
    refresh_runnable(&sh);
    Ok(api_routes(sh))
}

/// Fetches only allowlisted daily request counters; no secret or raw account response is returned.
#[tauri::command]
async fn openrouter_limits(
    sh: State<'_, Shared>,
) -> Result<bff_core::openrouter_limits::Limits, String> {
    blocking(&sh, move || {
        let keys = bff_core::keychain::Keychain::new();
        let mut http = bff_core::http::HttpClient::new(&["openrouter.ai"]);
        bff_core::openrouter_limits::fetch(&keys, &mut http, now())
    })
    .await?
}

/// Deletes a saved key. It still works at the provider until it is revoked there.
#[tauri::command]
fn remove_key(sh: State<'_, Shared>, route: String) -> Result<Value, String> {
    let provider = KEY_ROUTES
        .contains(&route.as_str())
        .then(|| bff_core::apiroute::Provider::for_route(&route))
        .flatten()
        .ok_or("Keys for that provider are not set here")?;
    bff_core::keychain::Keychain::new().remove(provider.credential())?;
    refresh_runnable(&sh);
    Ok(api_routes(sh))
}

/// Reads one path from GitHub or Notion and keeps it as a source. GET only, to that service's own host.
#[tauri::command]
async fn connector_read(sh: State<'_, Shared>, id: String, path: String) -> Result<Value, String> {
    let host = match id.as_str() {
        "github" => "api.github.com",
        "notion" => "api.notion.com",
        other => {
            return Err(format!(
                "{other:?} is not a connector. Use github or notion"
            ))
        }
    };
    let (app, connectors) = (sh.app.clone(), sh.connectors.clone());
    blocking(&sh, move || {
        let mut client = bff_core::http::HttpClient::new(&[host]);
        let keys = bff_core::keychain::Keychain::new();
        let mut guard = connectors.lock().expect("connectors lock");
        let r = app.lock().expect("app lock").read_connector(
            &mut guard,
            &keys,
            &mut client,
            &id,
            &path,
        );
        r
    })
    .await?
}

/// Reads a Notion page by its link and keeps it as a Markdown source.
#[tauri::command]
async fn read_notion_page(sh: State<'_, Shared>, link: String) -> Result<Value, String> {
    let (app, connectors) = (sh.app.clone(), sh.connectors.clone());
    blocking(&sh, move || {
        let mut client = bff_core::http::HttpClient::new(&["api.notion.com"]);
        let keys = bff_core::keychain::Keychain::new();
        let mut guard = connectors.lock().expect("connectors lock");
        let r =
            app.lock()
                .expect("app lock")
                .read_notion_page(&mut guard, &keys, &mut client, &link);
        r
    })
    .await?
}

/// Whether Google Docs is offered in this version, and whether this Mac is signed in to it.
#[tauri::command]
fn google_docs_status() -> Value {
    use bff_core::connector::Credentials;
    let keys = bff_core::keychain::Keychain::new();
    json!({"available": bff_core::google_docs::available(), "connected": keys.get(bff_core::google_docs::KEY_ITEM).is_some()})
}

/// Signs in to Google Docs in the browser, view-only, and keeps the lasting sign-in in the Keychain. It listens
/// once on 127.0.0.1 for Google's answer and gives up after three minutes.
#[tauri::command]
async fn connect_google_docs(sh: State<'_, Shared>) -> Result<Value, String> {
    blocking(&sh, move || -> Result<(), String> {
        use bff_core::google_docs as g;
        let id = g::CLIENT_ID.ok_or("Google Docs is not available in this version yet")?;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        let redirect = format!(
            "http://127.0.0.1:{}",
            listener.local_addr().map_err(|e| e.to_string())?.port()
        );
        let (state, verifier) = (g::random_token()?, g::random_token()?);
        let url = g::auth_url(id, &redirect, &state, &verifier);
        std::process::Command::new("/usr/bin/open")
            .arg(&url)
            .env_clear()
            .spawn()
            .map_err(|e| e.to_string())?;
        let code = g::wait_for_code(&listener, &state, std::time::Duration::from_secs(180))?;
        let mut client = bff_core::http::HttpClient::new(&[g::TOKEN_HOST]);
        let refresh = g::exchange(&mut client, &code, &verifier, &redirect)?;
        bff_core::keychain::Keychain::new().save(g::KEY_ITEM, &g::to_store(&refresh))
    })
    .await??;
    Ok(google_docs_status())
}

/// Forgets the Google Docs sign-in on this Mac. Access can also be removed in the Google account.
#[tauri::command]
fn disconnect_google_docs() -> Result<Value, String> {
    bff_core::keychain::Keychain::new().remove(bff_core::google_docs::KEY_ITEM)?;
    Ok(google_docs_status())
}

/// Reads a Google Doc by its link and keeps it as a Markdown source.
#[tauri::command]
async fn read_google_doc(sh: State<'_, Shared>, link: String) -> Result<Value, String> {
    let app = sh.app.clone();
    blocking(&sh, move || {
        use bff_core::google_docs as g;
        let mut client = bff_core::http::HttpClient::new(&[g::TOKEN_HOST, g::DOCS_HOST]);
        let keys = bff_core::keychain::Keychain::new();
        let r = app
            .lock()
            .expect("app lock")
            .read_google_doc(&keys, &mut client, &link);
        r
    })
    .await?
}

#[tauri::command]
fn unblock_connector(sh: State<'_, Shared>, id: String) -> bool {
    sh.connectors.lock().expect("connectors lock").unblock(&id)
}

// Commands that take the app lock while a project is being made run off the main thread, so a slow call can never
// freeze the window, Cancel or Quit (QA Q2-B1).
#[tauri::command(async)]
fn use_workflow(
    sh: State<'_, Shared>,
    name: String,
    author: (String, String, String),
    reviewer: (String, String, String),
) -> Result<(), String> {
    if name == "app_development" && !bff_core::running_code::allowed(&sh.data) {
        return Err(bff_core::running_code::NOT_ALLOWED.into());
    }
    sh.app.lock().expect("app lock").use_workflow(
        &name,
        (&author.0, &author.1, &author.2),
        (&reviewer.0, &reviewer.1, &reviewer.2),
        now(),
    )
}

#[tauri::command]
fn use_cases(sh: State<'_, Shared>) -> Value {
    let mut cases = App::use_cases();
    // App building is offered only once the person has allowed projects that write running code.
    if !bff_core::running_code::allowed(&sh.data) {
        if let Some(items) = cases.as_array_mut() {
            items.retain(|c| c["id"] != "app_development");
        }
    }
    cases
}

/// Whether projects may write running code (app building and clickable prototypes).
#[tauri::command]
fn running_code_allowed(sh: State<'_, Shared>) -> bool {
    bff_core::running_code::allowed(&sh.data)
}

/// The person's choice to allow projects that write running code, or not.
#[tauri::command]
fn set_running_code(sh: State<'_, Shared>, on: bool) -> Result<bool, String> {
    bff_core::running_code::set(&sh.data, on)?;
    Ok(on)
}

/// Adds a clickable prototype step to the open project's plan, or takes it out.
#[tauri::command(async)]
fn set_prototype(sh: State<'_, Shared>, on: bool) -> Result<(), Vec<String>> {
    let allowed = bff_core::running_code::allowed(&sh.data);
    sh.app
        .lock()
        .expect("app lock")
        .set_prototype(on, allowed, now())
}

/// Adds the file or folder at a typed or pasted path. A picture or a scan takes seconds to read, so the files
/// are read and checked on a blocking thread without holding the app, and only the keeping takes the lock.
#[tauri::command]
async fn add_source_path(sh: State<'_, Shared>, path: String) -> Result<Value, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    bff_core::ocr::allow_reading();
    let started = std::time::Instant::now();
    let read = blocking(&sh, move || {
        let (files, folder) = App::read_source_path(&path, &home)?;
        Ok::<_, String>((App::prepare_files(files), folder))
    })
    .await?;
    // How long reading took, never what was read, so a slow file shows in a log for a bug report.
    let secs = started.elapsed().as_secs();
    app_log(
        &sh.data,
        "source.read",
        &format!("{} in {secs} s", if read.is_ok() { "ok" } else { "failed" }),
    );
    let (prepared, folder) = read?;
    let kept = sh.app.lock().expect("app lock").keep_prepared(prepared)?;
    App::folder_result(kept, folder)
}

/// The person pressed Cancel while files were being added: any picture or scan being read is stopped at once.
#[tauri::command]
fn stop_reading() {
    bff_core::ocr::stop_reading();
}

/// Reads a web page or a shared Google document and keeps its text as a source. The page is fetched on a
/// blocking thread without holding the app, so the window stays usable while a slow site answers.
#[tauri::command]
async fn add_source_url(sh: State<'_, Shared>, url: String) -> Result<Value, String> {
    let asked = url.clone();
    let fetched = blocking(&sh, move || {
        bff_core::webpage::fetch_page(&asked, &mut bff_core::webpage::PublicWeb::new())
    })
    .await?;
    sh.app.lock().expect("app lock").keep_page(&url, fetched)
}

/// Proposes one new Notion page, or text added at the end of an existing one. Nothing is written until it is
/// approved and sent.
#[tauri::command]
fn propose_notion(
    sh: State<'_, Shared>,
    parent: String,
    title: String,
    body: String,
    append: Option<bool>,
) -> Result<String, String> {
    sh.app
        .lock()
        .expect("app lock")
        .propose_notion(&parent, &title, &body, append.unwrap_or(false))
}

/// Proposes the project's code as new files on a new branch of a repository.
#[tauri::command]
fn propose_github(
    sh: State<'_, Shared>,
    repo: String,
    branch: String,
    folder: String,
) -> Result<String, String> {
    sh.app
        .lock()
        .expect("app lock")
        .propose_github(&repo, &branch, &folder, now())
}

#[tauri::command]
fn approve_write(sh: State<'_, Shared>, project: String, id: String) -> Result<(), String> {
    let mut app = sh.app.lock().expect("app lock");
    app.require_open(&project)?;
    app.approve_write(&id, &sh.approver)
}

/// The pages a web research step's search used, for the person to add as sources if they choose. Adding one is an
/// ordinary source change, so the plan is approved again before the next step runs.
#[tauri::command]
fn cited_pages(sh: State<'_, Shared>, stage: String) -> Result<Vec<String>, String> {
    sh.app.lock().expect("app lock").cited_pages(&stage, 5)
}

#[tauri::command]
fn cancel_write(sh: State<'_, Shared>, id: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").cancel_write(&id)
}

#[tauri::command]
fn retry_write(sh: State<'_, Shared>, id: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").retry_write(&id)
}

/// Records what the person found at the destination after a send was interrupted: the address of what arrived,
/// or nothing when it did not.
#[tauri::command]
fn resolve_write(sh: State<'_, Shared>, id: String, arrived: Option<String>) -> Result<(), String> {
    sh.app
        .lock()
        .expect("app lock")
        .resolve_write(&id, arrived.as_deref())
}

/// The exact text of one file in a GitHub proposal.
#[tauri::command]
fn write_file(sh: State<'_, Shared>, id: String, path: String) -> Result<String, String> {
    sh.app.lock().expect("app lock").write_file(&id, &path)
}

/// Writes an approved proposal to Notion or GitHub with the key in the Keychain item for writing. The app is
/// not held while the request is in flight.
#[tauri::command]
async fn send_write(sh: State<'_, Shared>, id: String) -> Result<String, String> {
    let root = {
        let app = sh.app.lock().expect("app lock");
        // The action policy is checked here, with the app held; the request then runs without it. The send counts
        // as work in progress before the app is let go, so the guardrails cannot change between check and send.
        app.authorize_send(&id)?;
        let root = app.project_root()?;
        sh.active.fetch_add(1, Ordering::SeqCst);
        root
    };
    let _held = ActiveGuard(sh.active.clone());
    blocking(&sh, move || {
        let mut transport = bff_core::http::HttpClient::new(&["api.notion.com", "api.github.com"]);
        let keys = bff_core::keychain::Keychain::new();
        let mut remote = bff_core::writes::Remote {
            transport: &mut transport,
            creds: &keys,
        };
        bff_core::writes::Writes::open(&root)?.send(&id, &mut remote)
    })
    .await?
}

/// The questions the run is waiting on, from the step that checks what is missing. Empty once they are answered.
#[tauri::command]
fn fact_questions(sh: State<'_, Shared>) -> Result<Vec<bff_core::facts::Question>, String> {
    sh.app.lock().expect("app lock").fact_questions()
}

/// Keeps the answers to those questions as notes for the steps that follow, and lets the run go on. A skipped
/// question is `null`. An answer that looks like a password is refused and nothing is saved.
#[tauri::command]
fn answer_facts(sh: State<'_, Shared>, answers: Vec<Option<String>>) -> Result<(), String> {
    sh.app
        .lock()
        .expect("app lock")
        .answer_facts(answers, now())
}

/// Adds a note for a step, or for every step when `stage` is empty. The step's next run is told it.
#[tauri::command]
fn add_note(
    sh: State<'_, Shared>,
    stage: String,
    text: String,
    replace_id: Option<String>,
) -> Result<Value, String> {
    let mut app = sh.app.lock().expect("app lock");
    match replace_id {
        Some(id) => app.replace_note(&id, &text, now()),
        None => app.add_note(&stage, &text, now()),
    }
}

#[tauri::command]
fn remove_note(sh: State<'_, Shared>, id: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").remove_note(&id, now())
}

/// The notes kept for every project.
#[tauri::command]
fn memory_list(sh: State<'_, Shared>) -> Result<Value, String> {
    sh.app.lock().expect("app lock").memory_list()
}

#[tauri::command]
fn memory_save(
    sh: State<'_, Shared>,
    text: String,
    replace_id: Option<String>,
) -> Result<Value, String> {
    sh.app
        .lock()
        .expect("app lock")
        .memory_save(&text, replace_id.as_deref(), now())
}

#[tauri::command]
fn memory_remove(sh: State<'_, Shared>, id: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").memory_remove(&id)
}

/// Sets the rules for the exercise, or clears them with empty text.
#[tauri::command(async)]
fn set_exercise_rules(sh: State<'_, Shared>, text: String) -> Result<Value, String> {
    sh.app
        .lock()
        .expect("app lock")
        .set_exercise_rules(&text, now())
}

/// What the newest result of a step was given, from the record made when it ran.
#[tauri::command]
fn step_inputs(sh: State<'_, Shared>, stage: String) -> Result<Value, String> {
    sh.app.lock().expect("app lock").step_inputs(&stage)
}

/// Asks a model about the newest result of a step. Read-only.
#[tauri::command]
async fn ask_step(
    sh: State<'_, Shared>,
    engine: String,
    stage: String,
    role: String,
    question: String,
    on_event: Channel<RunEvent>,
) -> Result<Value, String> {
    let what = format!(
        "a question about {}",
        sh.app.lock().expect("app lock").stage_title(&stage)
    );
    engine_phased(
        &sh,
        engine,
        what,
        on_event,
        move |app| app.begin_ask_step(&stage, &role, &question, now()),
        |e, p| bff_core::ask::run_ask(e, p),
        move |app, p, (run, secs)| app.finish_ask_step(p, run, secs, now()),
    )
    .await
}

#[tauri::command]
fn step_chat(sh: State<'_, Shared>, stage: String) -> Result<Value, String> {
    sh.app.lock().expect("app lock").step_chat(&stage)
}

/// Says something to the coordinator. It may answer with a proposal, which changes nothing until applied.
#[tauri::command]
async fn coordinator_say(
    sh: State<'_, Shared>,
    engine: String,
    role: String,
    message: String,
    on_event: Channel<RunEvent>,
) -> Result<Value, String> {
    engine_phased(
        &sh,
        engine,
        "your request for a change".into(),
        on_event,
        move |app| app.begin_coordinator_say(&role, &message, now()),
        |e, p| bff_core::coordinator::run_say(e, p),
        move |app, p, (run, secs)| app.finish_coordinator_say(p, run, secs, now()),
    )
    .await
}

#[tauri::command]
fn coordinator_chat(sh: State<'_, Shared>) -> Result<Value, String> {
    sh.app.lock().expect("app lock").coordinator_chat()
}

/// Applies a proposal. A plan change is saved and approved by `by`.
#[tauri::command]
fn coordinator_apply(sh: State<'_, Shared>, project: String, id: String) -> Result<Value, String> {
    let allowed = bff_core::running_code::allowed(&sh.data);
    let mut app = sh.app.lock().expect("app lock");
    app.require_open(&project)?;
    app.set_code_allowed(allowed);
    app.coordinator_apply(&id, &sh.approver, now())
}

#[tauri::command]
fn coordinator_dismiss(sh: State<'_, Shared>, id: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").coordinator_dismiss(&id)
}

/// The tools for making files on this Mac. Keynote is offered when it is installed; the core still uses it only
/// for an approved plan that allows it.
fn file_tools(home: &str) -> bff_core::clientfiles::Tools {
    let mut t = bff_core::clientfiles::Tools::detect(home);
    t.keynote = bff_core::keynote::find_app().is_some();
    t
}

/// Lets Keynote build the deck for this project, or stops it. It changes the plan, so it needs a new approval.
#[tauri::command]
fn set_keynote(sh: State<'_, Shared>, on: bool) -> Result<(), Vec<String>> {
    sh.app.lock().expect("app lock").set_keynote(on, now())
}

/// The name, size and kind of each path the person dropped or typed, for the list on the new project screen.
/// It reads file information only, never the contents, and changes nothing.
#[tauri::command]
fn path_info(paths: Vec<String>) -> Vec<Value> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    paths.iter().map(|p| App::path_info(p, &home)).collect()
}

/// Whether a step is running anywhere, so a screen that was refused because of it can clear its message when the
/// run ends. It reads a count only.
#[tauri::command]
fn work_running(sh: State<'_, Shared>) -> bool {
    sh.active.load(Ordering::SeqCst) > 0
}

/// Whether Keynote is installed on this Mac.
#[tauri::command]
fn keynote_installed() -> bool {
    bff_core::keynote::find_app().is_some()
}

/// Makes the Word file, the PDF, and the deck for the client from two steps' results.
#[tauri::command]
async fn export_client_files(
    sh: State<'_, Shared>,
    document_step: Option<String>,
    deck_step: Option<String>,
    subtitle: String,
) -> Result<Value, String> {
    let app = sh.app.clone();
    blocking(&sh, move || {
        let home = std::env::var("HOME").unwrap_or_default();
        let tools = file_tools(&home);
        // The copy to Documents can wait on a macOS permission prompt, so it runs after the lock is let go.
        let (mut shown, job) = app.lock().expect("app lock").export_client_files_pending(
            document_step.as_deref(),
            deck_step.as_deref(),
            &subtitle,
            &tools,
            now(),
        )?;
        if let Some(job) = job {
            job.run(&mut shown);
        }
        Ok(shown)
    })
    .await?
}

/// The Word file, PDF, and deck to hand over, made once every step has a result.
#[tauri::command]
async fn final_files(sh: State<'_, Shared>) -> Result<Value, String> {
    let app = sh.app.clone();
    blocking(&sh, move || {
        let home = std::env::var("HOME").unwrap_or_default();
        let tools = file_tools(&home);
        // The copy to Documents can wait on a macOS permission prompt, so it runs after the lock is let go.
        let (mut shown, job) = app.lock().expect("app lock").final_files_pending(&tools)?;
        if let Some(job) = job {
            job.run(&mut shown);
        }
        Ok(shown)
    })
    .await?
}

/// Writes the decision log: what the checkers said, what the person changed, and what changed in the work.
#[tauri::command]
fn set_action_policy(
    sh: State<'_, Shared>,
    matrix: bff_core::policy::Matrix,
) -> Result<(), String> {
    if sh.active.load(Ordering::SeqCst) > 0 {
        return Err("Stop the current work before changing policy".into());
    }
    sh.app
        .lock()
        .expect("app lock")
        .set_action_policy(matrix, now())
}

/// Sets one guardrail for every writing role: sending outside, deleting or installing, to "ask each time" or "never".
#[tauri::command]
fn set_guardrail(
    sh: State<'_, Shared>,
    tool: bff_core::policy::Tool,
    ask: bool,
) -> Result<(), String> {
    if sh.active.load(Ordering::SeqCst) > 0 {
        return Err("Stop the current work before changing what it may do".into());
    }
    sh.app
        .lock()
        .expect("app lock")
        .set_guardrail(tool, ask, now())
}

#[tauri::command]
fn decide_actions(
    sh: State<'_, Shared>,
    project: String,
    ids: Vec<String>,
    approve: bool,
) -> Result<(), String> {
    let app = sh.app.lock().expect("app lock");
    app.require_open(&project)?;
    app.decide_actions(&ids, approve, &sh.approver, now())
}

#[tauri::command]
fn escalate_finding(
    sh: State<'_, Shared>,
    project: String,
    artifact: String,
    finding: String,
    base: String,
    reason: String,
) -> Result<(), String> {
    let mut app = sh.app.lock().expect("app lock");
    app.require_open(&project)?;
    app.escalate_finding(&artifact, &finding, &base, &reason, &sh.approver, now())
}

#[tauri::command]
fn stage_code_draft(sh: State<'_, Shared>, task: String) -> Result<(), String> {
    sh.app
        .lock()
        .expect("app lock")
        .stage_code_draft(&task, now())
}
#[tauri::command]
fn decide_code(
    sh: State<'_, Shared>,
    project: String,
    task: String,
    patch_hash: String,
    approve: bool,
) -> Result<(), String> {
    let app = sh.app.lock().expect("app lock");
    app.require_open(&project)?;
    app.decide_code(&task, &patch_hash, approve, &sh.approver, now())
}
#[tauri::command]
fn reconcile_code(
    sh: State<'_, Shared>,
    task: String,
    base: String,
    bundle: bff_core::code_review::Bundle,
) -> Result<String, String> {
    sh.app
        .lock()
        .expect("app lock")
        .reconcile_code(&task, &base, bundle, now())
}
#[tauri::command]
fn code_bundle(sh: State<'_, Shared>, task: String) -> Result<Value, String> {
    sh.app.lock().expect("app lock").code_bundle(&task)
}
#[tauri::command]
fn request_dependency_install(sh: State<'_, Shared>, task: String) -> Result<(), String> {
    sh.app
        .lock()
        .expect("app lock")
        .request_dependency_install(&task, now())
}

#[tauri::command]
async fn decision_inbox(sh: State<'_, Shared>) -> Result<Value, String> {
    let app = sh.app.clone();
    off_main(move || app.lock().expect("app lock").decision_inbox()).await
}

#[tauri::command]
async fn export_run_report(sh: State<'_, Shared>) -> Result<Value, String> {
    let app = sh.app.clone();
    blocking(&sh, move || {
        let home = std::env::var("HOME").unwrap_or_default();
        app.lock()
            .expect("app lock")
            .export_run_report(&file_tools(&home), now())
    })
    .await?
}

/// The raw records behind the run report, as an advanced export.
#[tauri::command]
fn export_run_report_raw(sh: State<'_, Shared>) -> Result<Value, String> {
    sh.app
        .lock()
        .expect("app lock")
        .export_run_report_raw(now())
}

#[tauri::command]
fn export_decision_log(sh: State<'_, Shared>) -> Result<Value, String> {
    sh.app.lock().expect("app lock").export_decision_log(now())
}

#[tauri::command]
fn template_preview(sh: State<'_, Shared>, source_id: String) -> Result<Value, String> {
    sh.app
        .lock()
        .expect("app lock")
        .template_preview(&source_id)
}

#[tauri::command]
fn use_template(
    sh: State<'_, Shared>,
    source_id: String,
    look: bool,
    outline: bool,
    style: bool,
) -> Result<Value, String> {
    sh.app
        .lock()
        .expect("app lock")
        .use_template(&source_id, look, outline, style, now())
}

#[tauri::command]
fn edit_template(
    sh: State<'_, Shared>,
    name: String,
    outline: Vec<String>,
    style: String,
    keep_look: bool,
) -> Result<Value, String> {
    sh.app
        .lock()
        .expect("app lock")
        .edit_template(&name, outline, &style, keep_look, now())
}

#[tauri::command]
fn templates(sh: State<'_, Shared>) -> Value {
    sh.app.lock().expect("app lock").templates()
}

#[tauri::command]
fn use_starter(sh: State<'_, Shared>, id: String) -> Result<Value, String> {
    sh.app.lock().expect("app lock").use_starter(&id, now())
}

#[tauri::command]
fn share_template(sh: State<'_, Shared>) -> Result<String, String> {
    sh.app.lock().expect("app lock").share_template()
}

#[tauri::command]
fn use_shared_template(sh: State<'_, Shared>, id: String) -> Result<Value, String> {
    sh.app
        .lock()
        .expect("app lock")
        .use_shared_template(&id, now())
}

#[tauri::command]
fn delete_shared_template(sh: State<'_, Shared>, id: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").delete_shared_template(&id)
}

#[tauri::command]
fn clear_template_look(sh: State<'_, Shared>) -> Result<(), String> {
    sh.app.lock().expect("app lock").clear_template_look(now())
}

/// Saves a step's result as a new Markdown file in a folder the person names, such as an Obsidian vault.
#[tauri::command]
fn save_result_to_folder(
    sh: State<'_, Shared>,
    stage: String,
    folder: String,
) -> Result<String, String> {
    let home = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default());
    sh.app
        .lock()
        .expect("app lock")
        .save_result_to_folder(&stage, &folder, &home, now())
}

#[tauri::command]
fn export_usage_csv(sh: State<'_, Shared>) -> Result<String, String> {
    sh.app.lock().expect("app lock").export_usage_csv(now())
}

/// How each crew has done across every project on this Mac.
#[tauri::command]
fn reliability(sh: State<'_, Shared>) -> Value {
    sh.app.lock().expect("app lock").reliability()
}

/// Usage across every project, for the all-projects total on Home.
#[tauri::command]
fn usage_everywhere(sh: State<'_, Shared>) -> Value {
    sh.app.lock().expect("app lock").usage_everywhere()
}

/// `path` if it is inside the open project's exports folder, or else inside the SproutCrew folder in Documents where
/// the finished files are copied. The exports folder is checked first, so showing a file made in the project never
/// looks inside Documents. Nothing else in Documents can be opened or shown.
fn exported(sh: &Shared, path: &str) -> Result<std::path::PathBuf, String> {
    let p = std::fs::canonicalize(path).map_err(|_| "that file is not there".to_string())?;
    let exports = sh
        .app
        .lock()
        .expect("app lock")
        .project_root()
        .map(|root| root.join("exports"));
    if exports.is_ok_and(|e| bff_core::documents::within(&e, &p)) {
        return Ok(p);
    }
    let copies = std::env::var("HOME")
        .map(|h| std::path::Path::new(&h).join("Documents/SproutCrew"))
        .unwrap_or_default();
    if bff_core::documents::within(&copies, &p) {
        return Ok(p);
    }
    Err("only files made by SproutCrew can be shown".into())
}

/// Opens a finished Word file, PDF, deck (PowerPoint or Keynote) or workbook in the app the person uses for it.
#[tauri::command]
fn open_export(sh: State<'_, Shared>, path: String) -> Result<(), String> {
    let p = exported(&sh, &path)?;
    if !matches!(
        p.extension().and_then(|e| e.to_str()),
        Some("docx" | "pdf" | "pptx" | "xlsx" | "key")
    ) {
        return Err("only a Word file, a PDF, a deck, or a workbook can be opened".into());
    }
    std::process::Command::new("/usr/bin/open")
        .arg(&p)
        .env_clear()
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Shows a file made for the client in Finder, or opens the folder when it is one. Only files inside the open
/// project's exports folder or the SproutCrew folder in Documents.
#[tauri::command]
fn reveal_export(sh: State<'_, Shared>, path: String) -> Result<(), String> {
    let p = exported(&sh, &path)?;
    let mut open = std::process::Command::new("/usr/bin/open");
    if !p.is_dir() {
        open.arg("-R");
    }
    open.arg(&p)
        .env_clear()
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// The menu bar item: today's cost and tokens across every project and how many items wait for the person,
/// refreshed every 30 seconds, with a way to open SproutCrew. It reads the same records as the Usage screen.
fn menu_bar(app: &tauri::AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    use tauri::tray::TrayIconBuilder;
    let today = MenuItem::with_id(app, "today", "Today: ...", false, None::<&str>)?;
    let waiting = MenuItem::with_id(app, "waiting", "...", false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open SproutCrew", true, None::<&str>)?;
    let gap = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&today, &waiting, &gap, &open])?;
    let mut tray = TrayIconBuilder::with_id("usage")
        .tooltip("SproutCrew")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            if event.id().as_ref() == "open" {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.unminimize();
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    let handle = app.clone();
    std::thread::spawn(move || loop {
        // Never wait on the project lock: when it is busy, the lines are refreshed next time.
        let lines = handle
            .state::<Shared>()
            .app
            .try_lock()
            .ok()
            .map(|a| a.menu_bar_lines(now()));
        if let Some((t, w)) = lines {
            let _ = today.set_text(t);
            let _ = waiting.set_text(w);
        }
        std::thread::sleep(std::time::Duration::from_secs(30));
    });
    Ok(())
}

/// Writes one line to the local log. The log is for the person's own bug reports, and writing it never stops the app.
fn app_log(data: &std::path::Path, kind: &str, text: &str) {
    let home = std::env::var("HOME").unwrap_or_default();
    let _ = bff_core::applog::AppLog::new(data, &home).write(kind, text, now());
}

/// Writes the local log to a file the person can attach to a bug report, and shows it in Finder. It holds the app
/// and macOS versions, which connections are set up (never their keys), and the log lines. Nothing is sent.
#[tauri::command]
fn export_log(app: tauri::AppHandle, sh: State<'_, Shared>) -> Result<String, String> {
    let macos = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .env_clear()
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".into());
    let keys = bff_core::keychain::Keychain::new();
    let routes = bff_core::apiconfig::states(&api_config(&sh.data), &|n| keys.present(n))
        .into_iter()
        .map(|(r, st)| format!("{r} {st:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let home = std::env::var("HOME").unwrap_or_default();
    let out = bff_core::applog::AppLog::new(&sh.data, &home).export(
        &[
            ("App", app.package_info().version.to_string()),
            ("macOS", macos),
            (
                "Connections",
                if routes.is_empty() {
                    "none".into()
                } else {
                    routes
                },
            ),
        ],
        now(),
    )?;
    std::process::Command::new("/usr/bin/open")
        .arg("-R")
        .arg(&out)
        .env_clear()
        .spawn()
        .map_err(|e| e.to_string())?;
    Ok(out.display().to_string())
}

/// The only addresses the About section opens: SproutCrew's own pages and mail, and the bundled programs' licenses.
const LINKS: [(&str, &str); 10] = [
    ("home", "https://www.sproutcrew.app/"),
    ("privacy", "https://www.sproutcrew.app/privacy"),
    ("terms", "https://www.sproutcrew.app/terms"),
    ("contact", "mailto:hello@sproutcrew.app"),
    ("R12-key", "https://openrouter.ai/keys"),
    ("R12-credits", "https://openrouter.ai/settings/credits"),
    ("R12-activity", "https://openrouter.ai/activity"),
    ("R3-key", "https://platform.claude.com/settings/keys"),
    ("R5-key", "https://platform.openai.com/api-keys"),
    ("R9-key", "https://aistudio.google.com/app/apikey"),
];

/// Opens one of `LINKS` in the default browser or mail app, or the licenses folder in Finder. Nothing else.
#[tauri::command]
fn open_link(app: tauri::AppHandle, link: String) -> Result<(), String> {
    let target = if link == "licenses" {
        app.path()
            .resource_dir()
            .map_err(|e| e.to_string())?
            .join("licenses")
            .into_os_string()
    } else {
        LINKS
            .iter()
            .find(|(k, _)| *k == link)
            .map(|(_, url)| (*url).into())
            .ok_or("that link is not one SproutCrew opens")?
    };
    std::process::Command::new("/usr/bin/open")
        .arg(target)
        .env_clear()
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Opens the project's code folder in Finder.
#[tauri::command]
fn reveal_code(sh: State<'_, Shared>) -> Result<(), String> {
    let root = sh.app.lock().expect("app lock").project_root()?;
    let code = std::fs::canonicalize(root.join(bff_core::code::DIR))
        .map_err(|_| "this project has no code yet".to_string())?;
    std::process::Command::new("/usr/bin/open")
        .arg(&code)
        .env_clear()
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Records that the open project was built from another one.
#[tauri::command]
fn link_project(sh: State<'_, Shared>, from: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").link_project(&from)
}

/// Takes a link away. Results already in the sources stay.
#[tauri::command]
fn unlink_project(sh: State<'_, Shared>, from: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").unlink_project(&from)
}

/// What another project has produced, to offer as material for this one.
#[tauri::command]
fn project_results(sh: State<'_, Shared>, name: String) -> Result<Value, String> {
    sh.app.lock().expect("app lock").project_results(&name)
}

/// Keeps results of another project as sources of the open one.
#[tauri::command]
fn add_source_results(
    sh: State<'_, Shared>,
    from: String,
    stages: Vec<String>,
) -> Result<Value, String> {
    sh.app
        .lock()
        .expect("app lock")
        .add_source_results(&from, &stages)
}

/// A file chosen in the file dialog. The bytes come as the raw request body and the name in a header, so a
/// large file is not turned into a JSON list of numbers.
#[tauri::command]
async fn add_source_bytes(
    sh: State<'_, Shared>,
    request: tauri::ipc::Request<'_>,
) -> Result<Value, String> {
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("the file did not arrive as bytes".into());
    };
    let bytes = bytes.clone();
    let name = request
        .headers()
        .get("name")
        .and_then(|v| v.to_str().ok())
        .map(bff_core::app::percent_decode)
        .ok_or("the file had no name")?;
    let prepared = blocking(&sh, move || App::prepare_files(vec![(name, Ok(bytes))])).await?;
    sh.app.lock().expect("app lock").keep_prepared(prepared)
}

/// Says what a file is for: evidence, the person's requirements, or a template to fill.
#[tauri::command]
fn set_source_role(sh: State<'_, Shared>, id: String, role: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").set_source_role(&id, &role)
}

#[tauri::command]
fn remove_source(sh: State<'_, Shared>, id: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").remove_source(&id)
}

/// A run of steps begins. Until `end_run`, no other project is opened or made, even between two steps. A stop
/// from an earlier run is cleared here, when no step is still in flight. Refused when a run is already going, which
/// is what a project left and opened again would otherwise start a second time (a second paid run of the same step).
#[tauri::command]
fn begin_run(sh: State<'_, Shared>) -> Result<(), String> {
    if !sh.hold.begin() {
        return Err(RUN_GOING.into());
    }
    clear_stop(&sh);
    Ok(())
}

/// A run of steps is over, however it ended. Also called when the window loads, when no run can still be going.
#[tauri::command]
fn end_run(sh: State<'_, Shared>) {
    sh.hold.end();
}

/// Asks the run in progress to stop. The child process is ended and the step is recorded as stopped. `steps` is
/// true when the run is a run of the plan's steps: the stop is then written down, so the project says "Stopped by
/// you" and the Decision inbox lists it until a step starts again. A stop of anything else is only a stop.
#[tauri::command]
fn stop_run(sh: State<'_, Shared>, steps: Option<bool>) {
    sh.cancel.store(true, Ordering::SeqCst);
    if steps == Some(true) {
        let _ = sh.app.lock().expect("app lock").record_stop(now());
    }
}

/// Makes outbox drafts from the posts in a step's newest result. Drafts only: each still needs approval.
#[tauri::command]
fn queue_to_drafts(
    sh: State<'_, Shared>,
    stage: String,
    start: u64,
    spacing: u64,
) -> Result<Value, String> {
    sh.app
        .lock()
        .expect("app lock")
        .queue_to_drafts(&stage, start, spacing)
}

#[tauri::command]
fn outbox_draft(sh: State<'_, Shared>, text: String, at: u64) -> Result<String, String> {
    sh.app.lock().expect("app lock").outbox_draft(&text, at)
}

#[tauri::command]
fn outbox_approve(sh: State<'_, Shared>, project: String, id: String) -> Result<(), String> {
    let mut app = sh.app.lock().expect("app lock");
    app.require_open(&project)?;
    app.outbox_approve(&id, &sh.approver)
}

#[tauri::command]
fn outbox_cancel(sh: State<'_, Shared>, id: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").outbox_cancel(&id)
}

/// Sends what a person approved and is due. Only the fake destination exists in this build.
#[tauri::command]
fn outbox_send_due(sh: State<'_, Shared>) -> Result<Vec<Value>, String> {
    sh.app.lock().expect("app lock").outbox_send_due(now())
}

#[tauri::command]
fn set_presentation(sh: State<'_, Shared>, on: bool) {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut app = sh.app.lock().expect("app lock");
    // The approver's name is personal too, so it is hidden with the home folder and email addresses.
    app.set_presentation_name(&sh.approver);
    app.set_presentation(on, &home);
}

#[tauri::command]
async fn export_package(sh: State<'_, Shared>, run_instructions: String) -> Result<Value, String> {
    let app = sh.app.clone();
    blocking(&sh, move || {
        let home = std::env::var("HOME").unwrap_or_default();
        let tools = file_tools(&home);
        let (mut shown, target) = {
            let mut a = app.lock().expect("app lock");
            (a.export_package(&run_instructions, &tools, now())?, a.documents_target())
        };
        // The set goes to Documents/SproutCrew/<project>, where the other files go, and not only to the app's hidden
        // folder (QA case 6). The copy runs after the lock is let go, as macOS may ask first.
        if let (Some((base, project)), Some(made)) = (target, shown["path"].as_str().map(PathBuf::from)) {
            match bff_core::documents::copy_folder_into(&base, &project, &made) {
                Ok(copy) => shown["path"] = json!(copy.to_string_lossy()),
                Err(e) => {
                    if let Some(notes) = shown["notes"].as_array_mut() {
                        notes.push(json!(format!("The set could not be copied to your Documents folder, so it is in the app's own folder: {e}")));
                    }
                }
            }
        }
        Ok::<_, String>(shown)
    })
    .await?
}

#[tauri::command]
fn discover_tools(sh: State<'_, Shared>) -> Result<Value, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    sh.app
        .lock()
        .expect("app lock")
        .discover_tools(std::path::Path::new(&home))
}

/// The skills that suit these roles, for setup to show before a project exists.
#[tauri::command(async)]
fn recommended_skills(roles: Vec<String>) -> Value {
    let home = std::env::var("HOME").unwrap_or_default();
    json!(App::recommended_skills(std::path::Path::new(&home), &roles))
}

/// Adds web research as the plan's first step, or takes it away.
#[tauri::command(async)]
fn set_web_research(sh: State<'_, Shared>, on: bool) -> Result<(), Vec<String>> {
    sh.app.lock().expect("app lock").set_web_research(on, now())
}

/// Adds or removes the last step that removes the signs of machine writing.
#[tauri::command(async)]
fn set_humaniser(sh: State<'_, Shared>, on: bool) -> Result<(), Vec<String>> {
    sh.app.lock().expect("app lock").set_humaniser(on, now())
}

#[tauri::command(async)]
fn select_tool(sh: State<'_, Shared>, kind: String, name: String) -> Result<(), String> {
    let home = std::env::var("HOME").unwrap_or_default();
    sh.app
        .lock()
        .expect("app lock")
        .select_tool(std::path::Path::new(&home), &kind, &name)
}

#[tauri::command]
fn deselect_tool(sh: State<'_, Shared>, name: String) -> Result<(), String> {
    sh.app.lock().expect("app lock").deselect_tool(&name)
}

/// Serves the project's web page on 127.0.0.1 and opens it in the system browser.
#[tauri::command]
fn start_preview(sh: State<'_, Shared>) -> Result<String, String> {
    let root = sh.app.lock().expect("app lock").project_root()?;
    let folder = bff_core::kit::ready(&root)?;
    let p = bff_core::preview::Preview::start(&folder).map_err(|e| e.to_string())?;
    let url = p.url();
    if let Some(old) = sh.preview.lock().expect("preview lock").replace(p) {
        old.stop();
    }
    std::process::Command::new("/usr/bin/open")
        .arg(&url)
        .spawn()
        .map_err(|e| format!("could not open the browser: {e}"))?;
    Ok(url)
}

/// Checks before a run: API connections, export tools, and disk.
#[tauri::command]
async fn preflight(sh: State<'_, Shared>) -> Result<Value, String> {
    let data = sh.data.clone();
    blocking(&sh, move || preflight_checks(&data))
        .await?
        .inspect(|_| refresh_runnable(&sh))
}

fn preflight_checks(data: &std::path::Path) -> Result<Value, String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let keys = bff_core::keychain::Keychain::new();
    let ready = bff_core::apiconfig::states(&api_config(data), &|n| keys.present(n))
        .iter()
        .any(|(_, st)| *st == bff_core::apiconfig::RouteState::Ready);
    let typst = bff_core::typst::detect_in(&bff_core::typst::candidate_paths(&home));
    let free_bytes = std::process::Command::new("/bin/df")
        .args(["-k"])
        .arg(data)
        .output()
        .ok()
        .and_then(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .nth(1)?
                .split_whitespace()
                .nth(3)?
                .parse::<u64>()
                .ok()
        })
        .map(|k| k * 1024);
    let disk_ok = free_bytes.is_some_and(|n| n >= 500 * 1024 * 1024);
    Ok(json!({
        "can_start": ready && disk_ok,
        "checks": [
            {"name": "API connection", "status": if ready { "pass" } else { "fail" }, "detail": if ready { "A key and request limits are configured. Provider availability is checked when a request runs." } else { "Connect OpenRouter, Anthropic, or OpenAI in AI connections." }},
            {"name": "Subscription access", "status": "pass", "detail": "Disabled in this beta. No Claude or ChatGPT subscription credentials are used."},
            {"name": "PDF export", "status": if typst.is_some() { "pass" } else { "warn" }, "detail": if typst.is_some() { "Typst is available." } else { "PDF export is unavailable; Markdown exports remain available." }},
            {"name": "Free disk space", "status": if disk_ok { "pass" } else { "fail" }, "detail": if disk_ok { "At least 500 MB is available." } else { "At least 500 MB of free space is needed, or space could not be checked." }}
        ]
    }))
}

#[tauri::command]
fn read_artifact(sh: State<'_, Shared>, file: String) -> Result<String, String> {
    sh.app.lock().expect("app lock").read_artifact(&file)
}

#[tauri::command]
fn check_assignment(route: String, model: String, effort: String) -> Result<(), String> {
    if bff_core::apiroute::Provider::for_route(&route).is_none() {
        return Err(
            "Subscription routes are unavailable in this beta. Choose a connected API provider."
                .into(),
        );
    }
    App::check_assignment(&route, &model, &effort)
}

/// The reviewer used when none is chosen, for an author on this route: `[route, model, effort]`.
#[tauri::command]
fn default_reviewer(sh: State<'_, Shared>, route: String) -> [&'static str; 3] {
    let keys = bff_core::keychain::Keychain::new();
    let ready = bff_core::apiconfig::states(&api_config(&sh.data), &|n| keys.present(n));
    let chosen = ready
        .iter()
        .find(|(r, st)| r != &route && *st == bff_core::apiconfig::RouteState::Ready)
        .or_else(|| {
            ready
                .iter()
                .find(|(r, st)| r == &route && *st == bff_core::apiconfig::RouteState::Ready)
        });
    let provider = chosen.and_then(|(r, _)| bff_core::apiroute::Provider::for_route(r));
    let r = provider.map_or("R12", |p| p.route());
    // The checking side shown on a new crew. The planner keeps the free router (`route::api_default`), since it
    // runs before anything is approved.
    let model = if r == "R12" {
        bff_core::catalog::DEFAULT_CHECKING
    } else {
        route::api_default(r).unwrap_or(bff_core::catalog::DEFAULT_CHECKING)
    };
    [r, model, "high"]
}

/// Fetches OpenRouter's model list: the account's own filtered list when `with_key` and a key is saved, else the
/// public one. It gives up on each address after a few seconds. The launch-time refresh passes `false`, so the
/// Keychain is read only when a person's own action needs the list.
fn fetch_catalog(now: u64, with_key: bool) -> Option<bff_core::catalog::Saved> {
    use bff_core::connector::{Credentials, Transport};
    let mut http = bff_core::http::HttpClient::quick(&["openrouter.ai"]);
    let keys = bff_core::keychain::Keychain::new();
    let mut tries = Vec::new();
    if let Some(key) = with_key.then(|| keys.get("openrouter-api")).flatten() {
        tries.push(bff_core::connector::Request {
            url: bff_core::catalog::USER_URL.into(),
            headers: vec![("Authorization".into(), format!("Bearer {}", key.token()))],
        });
    }
    tries.push(bff_core::connector::Request {
        url: bff_core::catalog::PUBLIC_URL.into(),
        headers: vec![],
    });
    tries.iter().find_map(|request| {
        let response = http.get(request).ok().filter(|r| r.status == 200)?;
        let list = serde_json::from_str::<Value>(&response.body).ok()?;
        let models = bff_core::catalog::parse(&list);
        (!models.is_empty()).then_some(bff_core::catalog::Saved {
            fetched_at: now,
            models,
        })
    })
}

/// One background refresh at a time.
static CATALOG_REFRESHING: AtomicBool = AtomicBool::new(false);

/// The saved list, fetched first when there is none. A list older than a day is returned as it is and
/// refreshed in the background. With no list and no connection, the defaults come back marked offline.
/// Every fetch goes through `catalog::ensure`, so the plan, the picker and the start-up refresh share one.
fn catalog(data: &std::path::Path) -> (Vec<bff_core::catalog::Model>, Option<u64>, bool) {
    catalog_with(data, true)
}

/// [`catalog`], reading the Keychain for the account's own list only when `with_key`.
fn catalog_with(
    data: &std::path::Path,
    with_key: bool,
) -> (Vec<bff_core::catalog::Model>, Option<u64>, bool) {
    match bff_core::catalog::load(data) {
        Some(saved) => {
            if saved.stale(now()) && !CATALOG_REFRESHING.swap(true, Ordering::SeqCst) {
                let dir = data.to_path_buf();
                std::thread::spawn(move || {
                    let _ = bff_core::catalog::ensure(&dir, now(), |t| fetch_catalog(t, with_key));
                    CATALOG_REFRESHING.store(false, Ordering::SeqCst);
                });
            }
            (saved.models, Some(saved.fetched_at), false)
        }
        None => match bff_core::catalog::ensure(data, now(), |t| fetch_catalog(t, with_key)) {
            Some(fresh) => (fresh.models, Some(fresh.fetched_at), false),
            None => (bff_core::catalog::offline(), None, true),
        },
    }
}

/// What the plan screen shows: the estimate, and why its prices are missing when the list could not be loaded.
#[derive(serde::Serialize)]
struct PlanPreflight {
    #[serde(flatten)]
    estimate: bff_core::estimate::Estimate,
    prices_error: Option<&'static str>,
}

/// What the open plan would ask of the providers: steps, models, requests on the free allowance against what is
/// left today, and the estimated and maximum cost, unknown where a price is. Read only; it decides nothing.
#[tauri::command]
async fn plan_preflight(sh: State<'_, Shared>) -> Result<PlanPreflight, String> {
    let data = sh.data.clone();
    let app = sh.app.clone();
    let tokens = sh.tokens.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let routes = api_config(&data);
        // The prices come first: with no saved list, wait for the fetch the picker would start, so a new Mac does
        // not show "unknown" for a list that is a second away.
        let saved = bff_core::catalog::load(&data)
            .or_else(|| bff_core::catalog::ensure(&data, now(), |t| fetch_catalog(t, true)));
        let prices_error = saved
            .is_none()
            .then_some("Couldn't load model prices; check the connection.");
        let needs_free = {
            let a = app.lock().expect("app lock");
            a.plan_estimate(&routes, saved.as_ref(), None)?
                .free_requests
                > 0
        };
        // Asked only when the plan uses the free allowance. A failure here leaves it unknown, never zero.
        let left = needs_free
            .then(|| {
                let keys = bff_core::keychain::Keychain::new();
                let mut http = bff_core::http::HttpClient::new(&["openrouter.ai"]);
                bff_core::openrouter_limits::fetch(&keys, &mut http, now())
                    .ok()
                    .and_then(|l| l.daily.map(|d| d.remaining))
            })
            .flatten();
        let a = app.lock().expect("app lock");
        let mut estimate = a.plan_estimate(&routes, saved.as_ref(), left)?;
        // A plan on token-counted routes is checked against what the shared token cap has left (Q2-B9).
        if estimate.estimated_tokens > 0 {
            let left = tokens.lock().expect("tokens lock").status().available;
            estimate = estimate.against_token_cap(left);
        }
        Ok(PlanPreflight {
            estimate,
            prices_error,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Every OpenRouter model the picker can offer, with prices per million tokens where known.
#[tauri::command]
async fn model_catalog(sh: State<'_, Shared>) -> Result<Value, String> {
    let data = sh.data.clone();
    let (models, fetched_at, offline) =
        tauri::async_runtime::spawn_blocking(move || catalog(&data))
            .await
            .map_err(|e| e.to_string())?;
    Ok(json!({
        "models": models,
        "fetched_at": fetched_at,
        "offline": offline,
        "recommended": {
            "writing": bff_core::catalog::recommended(false),
            "checking": bff_core::catalog::recommended(true),
        },
    }))
}

#[tauri::command]
async fn route_models(sh: State<'_, Shared>, route: String) -> Result<Vec<String>, String> {
    if route == "R12" {
        let data = sh.data.clone();
        return tauri::async_runtime::spawn_blocking(move || {
            let (models, _, _) = catalog(&data);
            let mut ids: Vec<String> = models
                .into_iter()
                .filter(|m| m.free)
                .map(|m| m.id)
                .collect();
            if !ids.iter().any(|m| m == "openrouter/free") {
                ids.insert(0, "openrouter/free".into());
            }
            ids
        })
        .await
        .map_err(|e| e.to_string());
    }
    if route == "R13" {
        // The person's own service: the model they saved for it.
        return Ok(api_config(&sh.data)
            .get("R13")
            .and_then(|c| c.model.clone())
            .into_iter()
            .collect());
    }
    Ok(route_models_fixed(&route))
}

fn route_models_fixed(route: &str) -> Vec<String> {
    if bff_core::apiroute::Provider::for_route(route).is_some() {
        route::models_for(route)
            .into_iter()
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            // The first picture or scan after an update is slow while macOS readies its text model.
            // Doing that now means it is already done when a person adds one.
            bff_core::ocr::warm_up();
            let data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data)?;
            app_log(
                &data,
                "app.start",
                &format!("version {}", app.package_info().version),
            );
            // If a saved book cannot be read, the history that would justify any spending is lost, so that book
            // allows no spending until the person fixes the file. It is never replaced by a fresh allowance.
            let spend =
                bff_core::spend::SpendBook::open(&data.join("spend.json")).unwrap_or_else(|e| {
                    bff_core::spend::SpendBook::unreadable(bff_core::spend::Unit::Dollars, &e)
                });
            let tokens = bff_core::spend::SpendBook::open_unit(
                &data.join("tokens.json"),
                bff_core::spend::Unit::Tokens,
            )
            .unwrap_or_else(|e| {
                bff_core::spend::SpendBook::unreadable(bff_core::spend::Unit::Tokens, &e)
            });
            let mut projects = App::new(data.join("projects"));
            // Finished files are copied to the SproutCrew folder inside Documents, where the person finds them.
            if let Ok(home) = std::env::var("HOME") {
                projects.set_documents_dir(std::path::Path::new(&home).join("Documents"));
            }
            // One count of engine runs in flight, so the project list and the shell agree on what is running.
            let active = projects.running();
            app.manage(Shared {
                app: Arc::new(Mutex::new(projects)),
                approver: bff_core::approver::current(),
                latch: bff_core::stop::StopLatch::new(),
                spend: Arc::new(Mutex::new(spend)),
                tokens: Arc::new(Mutex::new(tokens)),
                data,
                cancel: Arc::new(AtomicBool::new(false)),
                hold: bff_core::app::RunHold::new(active.clone()),
                active,
                preview: Mutex::new(None),
                connectors: Arc::new(Mutex::new({
                    let mut c = bff_core::connector::Connectors::default();
                    c.register(bff_core::connector::github());
                    c.register(bff_core::connector::notion());
                    c
                })),
            });
            refresh_runnable(&app.state::<Shared>());
            // Prices for the plan screen: fetches the public model list when there is none, and refreshes one over a
            // day old. Done here so the list is usually saved before anyone reaches a plan. No key is read here.
            let list_dir = app.state::<Shared>().data.clone();
            std::thread::spawn(move || {
                catalog_with(&list_dir, false);
            });
            attention::start(app.handle(), &app.state::<Shared>().data)?;
            menu_bar(app.handle())?;
            // The predefined Quit item appears to bypass ExitRequested, so Quit is a custom item.
            let quit = MenuItemBuilder::with_id("quit", "Quit SproutCrew")
                .accelerator("CmdOrCtrl+Q")
                .build(app)?;
            let app_menu = SubmenuBuilder::new(app, "SproutCrew")
                .item(&PredefinedMenuItem::about(app, None, None)?)
                .separator()
                .item(&PredefinedMenuItem::hide(app, None)?)
                .separator()
                .item(&quit)
                .build()?;
            let edit = SubmenuBuilder::new(app, "Edit")
                .item(&PredefinedMenuItem::undo(app, None)?)
                .item(&PredefinedMenuItem::redo(app, None)?)
                .separator()
                .item(&PredefinedMenuItem::cut(app, None)?)
                .item(&PredefinedMenuItem::copy(app, None)?)
                .item(&PredefinedMenuItem::paste(app, None)?)
                .item(&PredefinedMenuItem::select_all(app, None)?)
                .build()?;
            app.set_menu(MenuBuilder::new(app).item(&app_menu).item(&edit).build()?)?;
            Ok(())
        })
        .on_menu_event(|app, event| {
            if event.id().as_ref() != "quit" {
                return;
            }
            // Safe pause: interrupt the running turn, wait for it to be journaled, then exit. A picture or scan being
            // read is stopped at once, and the window goes away now, so Quit is seen to work.
            let sh = app.state::<Shared>();
            sh.cancel.store(true, Ordering::SeqCst);
            bff_core::ocr::stop_reading();
            for (_, w) in app.webview_windows() {
                let _ = w.hide();
            }
            let (active, handle) = (sh.active.clone(), app.clone());
            std::thread::spawn(move || {
                let end = std::time::Instant::now() + Duration::from_secs(20);
                while active.load(Ordering::SeqCst) > 0 && std::time::Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(100));
                }
                handle.exit(0);
            });
        })
        // Closing the window keeps the app running. The Dock icon is the indicator, and it reopens the window.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            stop_reading,
            attention_status,
            set_notifications,
            test_notification,
            list_projects,
            projects_overview,
            create_project,
            create_project_from,
            open_project,
            archive_project,
            set_stand_ins,
            restore_project,
            get_state,
            propose_plan,
            save_plan,
            approve,
            mark_reviewed,
            decided_list,
            policy_overview,
            set_policy_defaults,
            run_stage,
            ready_stages,
            run_wave,
            read_artifact,
            check_assignment,
            route_models,
            model_catalog,
            plan_preflight,
            set_guardrail,
            export_log,
            cited_pages,
            default_reviewer,
            write_stress_report,
            review,
            decide,
            use_workflow,
            use_cases,
            add_source_path,
            add_source_url,
            project_results,
            export_client_files,
            final_files,
            save_key,
            save_own_ai,
            remove_own_ai,
            openrouter_limits,
            remove_key,
            open_link,
            open_export,
            reveal_export,
            reveal_code,
            link_project,
            unlink_project,
            fact_questions,
            answer_facts,
            add_note,
            remove_note,
            set_exercise_rules,
            step_inputs,
            ask_step,
            step_chat,
            coordinator_say,
            coordinator_chat,
            coordinator_apply,
            coordinator_dismiss,
            propose_notion,
            propose_github,
            approve_write,
            cancel_write,
            retry_write,
            resolve_write,
            write_file,
            send_write,
            add_source_results,
            add_source_bytes,
            remove_source,
            set_source_role,
            stop_run,
            begin_run,
            end_run,
            api_routes,
            spend_status,
            set_project_limit,
            export_usage_csv,
            save_result_to_folder,
            notify_run,
            set_notification_names,
            read_notion_page,
            google_docs_status,
            connect_google_docs,
            disconnect_google_docs,
            read_google_doc,
            template_preview,
            use_template,
            edit_template,
            templates,
            use_starter,
            share_template,
            use_shared_template,
            delete_shared_template,
            clear_template_look,
            usage_everywhere,
            reliability,
            memory_list,
            memory_save,
            memory_remove,
            spend_raise,
            connector_read,
            unblock_connector,
            outbox_draft,
            queue_to_drafts,
            outbox_approve,
            outbox_cancel,
            outbox_send_due,
            set_presentation,
            export_package,
            export_decision_log,
            export_run_report,
            export_run_report_raw,
            decision_inbox,
            stage_code_draft,
            decide_code,
            reconcile_code,
            code_bundle,
            request_dependency_install,
            escalate_finding,
            set_action_policy,
            decide_actions,
            discover_tools,
            select_tool,
            recommended_skills,
            set_web_research,
            set_humaniser,
            running_code_allowed,
            set_running_code,
            set_prototype,
            set_keynote,
            keynote_installed,
            work_running,
            path_info,
            deselect_tool,
            start_preview,
            preflight,
            revise,
            set_budget,
            raise_budget
        ])
        .build(tauri::generate_context!())
        .expect("build the BFF shell")
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { api, code, .. } if code.is_none() => {
                api.prevent_exit()
            }
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            _ => {}
        });
}
