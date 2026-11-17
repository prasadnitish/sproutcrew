//! OS delivery is advisory. Decisions and approvals remain in the Rust inbox.
use crate::Shared;
use bff_core::{attention::Tracker, decision_inbox::Item, policy::Tier};
use serde::Serialize;
use std::{path::Path, sync::Mutex, time::Duration};
use tauri::{Manager, State};
use tauri_plugin_notification::NotificationExt;

#[derive(Clone, Default, Serialize)]
pub struct Status {
    enabled: bool,
    count: Option<usize>,
    errors: usize,
    last_error: Option<String>,
    open_inbox: bool,
    /// Whether run notifications may name the project.
    names: bool,
}
pub struct Attention(Mutex<Status>);

#[tauri::command]
pub fn attention_status(state: State<'_, Attention>, take_open: Option<bool>) -> Status {
    let mut state = state.0.lock().expect("attention lock");
    let result = state.clone();
    if take_open == Some(true) {
        state.open_inbox = false;
    }
    result
}

#[tauri::command]
pub fn set_notifications(
    sh: State<'_, Shared>,
    state: State<'_, Attention>,
    on: bool,
) -> Result<Status, String> {
    let mut state = state.0.lock().expect("attention lock");
    let temporary = sh.data.join("notifications.json.tmp");
    std::fs::write(
        &temporary,
        if on {
            b"true".as_slice()
        } else {
            b"false".as_slice()
        },
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, sh.data.join("notifications.json")).map_err(|e| e.to_string())?;
    state.enabled = on;
    state.last_error = None;
    Ok(state.clone())
}

#[tauri::command]
pub fn test_notification(app: tauri::AppHandle, state: State<'_, Attention>) -> Result<(), String> {
    if !state.0.lock().expect("attention lock").enabled {
        return Err("Turn on Mac notifications first".into());
    }
    app.notification()
        .builder()
        .id(201)
        .title("SproutCrew test notification")
        .body("Notifications open your Decision inbox. They never approve work.")
        .show()
        .map_err(|_| {
            "The notification could not be requested. Check macOS notification settings".into()
        })
}

/// Names the project in run notifications, or stops naming it. Kept in `notifications-names.json`.
#[tauri::command]
pub fn set_notification_names(
    sh: State<'_, Shared>,
    state: State<'_, Attention>,
    on: bool,
) -> Result<Status, String> {
    let mut state = state.0.lock().expect("attention lock");
    let temporary = sh.data.join("notifications-names.json.tmp");
    std::fs::write(&temporary, if on { "true" } else { "false" }).map_err(|e| e.to_string())?;
    std::fs::rename(&temporary, sh.data.join("notifications-names.json"))
        .map_err(|e| e.to_string())?;
    state.names = on;
    Ok(state.clone())
}

static LAST_WAITING: Mutex<Option<std::time::Instant>> = Mutex::new(None);

/// Tells the person how a run went, when notifications are on and SproutCrew is not in front. A waiting notice is
/// sent at most once every five minutes. Returns whether a notification was requested.
#[tauri::command]
pub fn notify_run(
    app: tauri::AppHandle,
    sh: State<'_, Shared>,
    state: State<'_, Attention>,
    outcome: String,
) -> Result<bool, String> {
    let outcome = bff_core::attention::RunOutcome::from_word(&outcome)
        .ok_or("that is not a way a run can end")?;
    let (enabled, names) = {
        let s = state.0.lock().expect("attention lock");
        (s.enabled, s.names)
    };
    let foreground = app
        .get_webview_window("main")
        .map(|w| w.is_focused().unwrap_or(true) && w.is_visible().unwrap_or(true))
        .unwrap_or(false);
    if !enabled || foreground {
        return Ok(false);
    }
    if outcome == bff_core::attention::RunOutcome::Waiting {
        let mut last = LAST_WAITING.lock().expect("waiting lock");
        if last.is_some_and(|t| t.elapsed() < Duration::from_secs(300)) {
            return Ok(false);
        }
        *last = Some(std::time::Instant::now());
    }
    let (project, kind) = {
        let a = sh.app.lock().expect("app lock");
        (a.open_name(), a.latest_failure_kind())
    };
    let (title, body) = bff_core::attention::run_text(
        outcome,
        kind.as_deref(),
        project.as_deref().filter(|_| names),
    );
    app.notification()
        .builder()
        .id(202)
        .title(title)
        .body(body)
        .show()
        .map(|_| true)
        .map_err(|_| {
            "The notification could not be requested. Check macOS notification settings".into()
        })
}

pub fn start(app: &tauri::AppHandle, data: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut initial = Status {
        names: std::fs::read(data.join("notifications-names.json"))
            .ok()
            .and_then(|v| serde_json::from_slice::<bool>(&v).ok())
            .unwrap_or(false),
        ..Status::default()
    };
    let file = data.join("notifications.json");
    if file.exists() {
        match std::fs::read(&file)
            .ok()
            .and_then(|v| serde_json::from_slice::<bool>(&v).ok())
        {
            Some(enabled) => initial.enabled = enabled,
            None => {
                initial.last_error = Some(
                    "Saved notification preference could not be read. Mac alerts are off".into(),
                )
            }
        }
    }
    app.manage(Attention(Mutex::new(initial)));
    let clicked = app.clone();
    app.notification().on_action(move |_| {
        clicked
            .state::<Attention>()
            .0
            .lock()
            .expect("attention lock")
            .open_inbox = true;
        if let Some(w) = clicked.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    })?;
    let app = app.clone();
    std::thread::spawn(move || {
        let mut tracker = Tracker::default();
        let start = std::time::Instant::now();
        loop {
            // Never hold the application lock during OS delivery, sleep, or UI callbacks.
            let snapshot = app
                .state::<Shared>()
                .app
                .try_lock()
                .ok()
                .map(|a| a.decision_inbox());
            if let Some(snapshot) = snapshot {
                let errors = snapshot["errors"].as_array().cloned().unwrap_or_default();
                let parsed = serde_json::from_value::<Vec<Item>>(snapshot["items"].clone());
                let Ok(mut items) = parsed else {
                    app.state::<Attention>()
                        .0
                        .lock()
                        .expect("attention lock")
                        .last_error =
                        Some("Pending decisions could not be read. Open the inbox to check".into());
                    std::thread::sleep(Duration::from_secs(3));
                    continue;
                };
                let enabled = {
                    let attention = app.state::<Attention>();
                    let mut state = attention.0.lock().expect("attention lock");
                    state.count = Some(items.len());
                    state.errors = errors.len();
                    state.enabled
                };
                for error in errors {
                    items.push(Item {
                        revision: String::new(),
                        project: error["project"].as_str().unwrap_or_default().into(),
                        stage: String::new(),
                        id: "inbox-unreadable".into(),
                        tier: Tier::Consequential,
                        kind: "error".into(),
                        title: "Pending decisions could not be read".into(),
                        anchor: String::new(),
                    });
                }
                let foreground = app
                    .get_webview_window("main")
                    .map(|w| w.is_focused().unwrap_or(true) && w.is_visible().unwrap_or(true))
                    .unwrap_or(false);
                let now = start.elapsed().as_secs();
                if let Some(notice) = tracker.observe(&items, foreground || !enabled, now) {
                    let accepted = app
                        .notification()
                        .builder()
                        .id(200)
                        .title("SproutCrew needs your input")
                        .body(notice.body())
                        .show()
                        .is_ok();
                    tracker.attempted(&items, now, accepted);
                    app.state::<Attention>().0.lock().expect("attention lock").last_error = (!accepted).then(|| "A Mac notification could not be requested. Decisions remain in your inbox".into());
                }
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
    Ok(())
}
