//! OpenRouter's public model list, read into what the model picker shows: name, maker, context length and
//! the price per million tokens. A price the list does not give, or gives as a negative number (OpenRouter's
//! routers report "-1" because their price varies), stays unknown and is never read as zero. The list is kept
//! on this Mac so the picker works offline; it is display data only and is never part of a plan approval.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

/// The model list's own address. It needs no key.
pub const PUBLIC_URL: &str = "https://openrouter.ai/api/v1/models";
/// The same list filtered by the account's provider, privacy and guardrail settings. It needs the key.
pub const USER_URL: &str = "https://openrouter.ai/api/v1/models/user";
/// A saved list older than this is refreshed in the background.
pub const MAX_AGE_SECS: u64 = 24 * 60 * 60;
pub const FILE: &str = "openrouter-models.json";

/// New projects start here. Nitish chose these on Nov 10, 2026.
pub const DEFAULT_WRITING: &str = "anthropic/claude-haiku-5.5";
pub const DEFAULT_CHECKING: &str = "mistralai/mistral-small-2603";

/// The app's own short list at the top of the picker, for each side of the crew.
pub fn recommended(checking: bool) -> &'static [&'static str] {
    if checking {
        &[DEFAULT_CHECKING, "deepseek/deepseek-v4.1-flash"]
    } else {
        &[DEFAULT_WRITING, "openai/gpt-6.1-terra"]
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub maker: String,
    pub context_length: Option<u64>,
    /// Dollars per million input tokens. `None` is unknown.
    pub input_per_mtok: Option<f64>,
    /// Dollars per million output tokens. `None` is unknown.
    pub output_per_mtok: Option<f64>,
    pub free: bool,
    /// Whether the model reads images as well as text, as the list says.
    pub sees_images: bool,
}

/// A price string in dollars per token as dollars per million tokens. Missing, unreadable or negative is unknown.
fn per_million(v: &Value) -> Option<f64> {
    let s = v.as_str()?;
    let per_token: f64 = s.trim().parse().ok()?;
    if !per_token.is_finite() || per_token < 0.0 {
        return None;
    }
    // Rounded to a millionth of a dollar so 0.00000028 reads as 0.28, not 0.27999999.
    Some((per_token * 1_000_000.0 * 1_000_000.0).round() / 1_000_000.0)
}

/// Every model in the list the app can send to. Names the app refuses (by shape, or excluded) are left out.
pub fn parse(list: &Value) -> Vec<Model> {
    let mut models: Vec<Model> = list["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            let id = m["id"].as_str()?.to_string();
            if !crate::route::model_known("R12", &id) {
                return None;
            }
            let input = per_million(&m["pricing"]["prompt"]);
            let output = per_million(&m["pricing"]["completion"]);
            let free = (id.ends_with(":free") || id == "openrouter/free")
                && input == Some(0.0)
                && output == Some(0.0);
            let maker = id.split('/').next().unwrap_or_default().to_string();
            let name = m["name"]
                .as_str()
                .filter(|n| !n.trim().is_empty())
                .unwrap_or(&id)
                .to_string();
            let sees_images = m["architecture"]["input_modalities"]
                .as_array()
                .is_some_and(|a| a.iter().any(|x| x == "image"));
            Some(Model {
                id,
                name,
                maker,
                context_length: m["context_length"].as_u64(),
                input_per_mtok: input,
                output_per_mtok: output,
                free,
                sees_images,
            })
        })
        .collect();
    models.sort_by(|a, b| a.id.cmp(&b.id));
    models.dedup_by(|a, b| a.id == b.id);
    models
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub fetched_at: u64,
    pub models: Vec<Model>,
}

impl Saved {
    pub fn stale(&self, now: u64) -> bool {
        now.saturating_sub(self.fetched_at) > MAX_AGE_SECS
    }

    pub fn find(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.id == id)
    }
}

/// The saved list, or nothing when there is none or it cannot be read. A damaged file is ignored, not trusted.
pub fn load(dir: &Path) -> Option<Saved> {
    let bytes = std::fs::read(dir.join(FILE)).ok()?;
    let mut saved: Saved = serde_json::from_slice(&bytes).ok()?;
    // What is free is worked out again, never taken from the file, by the same rule as `parse`.
    saved
        .models
        .retain(|m| crate::route::model_known("R12", &m.id));
    for m in &mut saved.models {
        m.free = (m.id.ends_with(":free") || m.id == "openrouter/free")
            && m.input_per_mtok == Some(0.0)
            && m.output_per_mtok == Some(0.0);
        if m.input_per_mtok.is_some_and(|p| p < 0.0 || !p.is_finite()) {
            m.input_per_mtok = None;
        }
        if m.output_per_mtok.is_some_and(|p| p < 0.0 || !p.is_finite()) {
            m.output_per_mtok = None;
        }
    }
    (!saved.models.is_empty()).then_some(saved)
}

/// Saves a fresh list. An empty list never replaces a good one.
pub fn store(dir: &Path, saved: &Saved) -> Result<(), String> {
    if saved.models.is_empty() {
        return Err("OpenRouter returned no models".into());
    }
    let tmp = dir.join(format!("{FILE}.tmp"));
    let body = serde_json::to_vec(saved).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, body).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, dir.join(FILE)).map_err(|e| e.to_string())
}

/// A failed fetch is not tried again for this long, so a plan that asks during an outage answers at once.
pub const FAILURE_MEMORY_SECS: u64 = 30;

/// One list fetch at a time, whoever asks. It holds when the last fetch failed, if it did.
static FETCHING: std::sync::Mutex<Option<u64>> = std::sync::Mutex::new(None);

/// A list no older than a day: the saved one, or the one `fetch` gets and saves. Callers that arrive together
/// share one fetch; each that waited finds what the first saved and does not fetch again. `None` when the list
/// is missing or old and the fetch failed, or failed within the last [`FAILURE_MEMORY_SECS`].
pub fn ensure(dir: &Path, now: u64, fetch: impl FnOnce(u64) -> Option<Saved>) -> Option<Saved> {
    let mut failed_at = FETCHING.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(saved) = load(dir).filter(|s| !s.stale(now)) {
        return Some(saved);
    }
    if failed_at.is_some_and(|t| now >= t && now - t < FAILURE_MEMORY_SECS) {
        return None;
    }
    let Some(fresh) = fetch(now) else {
        *failed_at = Some(now);
        return None;
    };
    *failed_at = None;
    let _ = store(dir, &fresh);
    Some(fresh)
}

/// What the picker shows when no list has ever been saved and OpenRouter cannot be reached: the defaults and
/// the free router, all with unknown prices.
pub fn offline() -> Vec<Model> {
    let mut ids: Vec<&str> = recommended(false).to_vec();
    ids.extend(recommended(true));
    ids.push("openrouter/free");
    ids.into_iter()
        .map(|id| Model {
            id: id.into(),
            name: id.into(),
            maker: id.split('/').next().unwrap_or_default().into(),
            context_length: None,
            input_per_mtok: None,
            output_per_mtok: None,
            free: id == "openrouter/free",
            sees_images: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn list() -> Value {
        json!({"data": [
            {"id": "baidu/ernie-5.5-flash-preview", "name": "Baidu: ERNIE 5.5", "context_length": 1024000,
             "architecture": {"input_modalities": ["text", "image"]},
             "pricing": {"prompt": "0.00000028", "completion": "0.00000113"}},
            {"id": "openrouter/auto", "name": "Auto Router", "context_length": 2000000,
             "pricing": {"prompt": "-1", "completion": "-1"}},
            {"id": "openrouter/free", "name": "Free Models Router", "pricing": {"prompt": "0", "completion": "0"}},
            {"id": "meta-llama/llama-4-scout:free", "name": "Llama 4 Scout (free)",
             "pricing": {"prompt": "0", "completion": "0"}},
            {"id": "someone/labelled:free", "name": "Not really free", "pricing": {"prompt": "0.000001", "completion": "0"}},
            {"id": "acme/no-pricing", "name": ""},
            {"id": "Bad Name/Shape", "name": "Refused", "pricing": {"prompt": "0", "completion": "0"}},
            {"name": "no id"}
        ]})
    }

    #[test]
    fn prices_in_dollars_per_token_become_dollars_per_million_tokens() {
        let m = parse(&list());
        let ernie = m
            .iter()
            .find(|x| x.id == "baidu/ernie-5.5-flash-preview")
            .unwrap();
        assert_eq!(ernie.input_per_mtok, Some(0.28));
        assert_eq!(ernie.output_per_mtok, Some(1.13));
        assert_eq!(ernie.context_length, Some(1_024_000));
        assert_eq!(ernie.maker, "baidu");
        assert!(ernie.sees_images);
    }

    #[test]
    fn a_negative_or_missing_price_is_unknown_and_never_zero() {
        let m = parse(&list());
        let auto = m.iter().find(|x| x.id == "openrouter/auto").unwrap();
        assert_eq!((auto.input_per_mtok, auto.output_per_mtok), (None, None));
        assert!(!auto.free);
        let none = m.iter().find(|x| x.id == "acme/no-pricing").unwrap();
        assert_eq!((none.input_per_mtok, none.output_per_mtok), (None, None));
        assert!(!none.free);
        assert_eq!(
            none.name, "acme/no-pricing",
            "an empty name falls back to the ID"
        );
    }

    #[test]
    fn free_means_a_free_label_and_both_prices_zero() {
        let m = parse(&list());
        let free: Vec<&str> = m.iter().filter(|x| x.free).map(|x| x.id.as_str()).collect();
        assert_eq!(free, ["meta-llama/llama-4-scout:free", "openrouter/free"]);
        let labelled = m.iter().find(|x| x.id == "someone/labelled:free").unwrap();
        assert!(!labelled.free, "a :free label with a price is not free");
    }

    #[test]
    fn names_the_app_refuses_are_left_out() {
        let ids: Vec<String> = parse(&list()).into_iter().map(|x| x.id).collect();
        assert!(!ids.iter().any(|i| i.contains("Bad")));
        assert_eq!(ids.len(), 6);
    }

    #[test]
    fn the_saved_list_round_trips_and_a_damaged_or_empty_file_is_ignored() {
        let dir = std::env::temp_dir().join(format!("catalog-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(load(&dir).is_none());
        let saved = Saved {
            fetched_at: 100,
            models: parse(&list()),
        };
        store(&dir, &saved).unwrap();
        assert_eq!(load(&dir).unwrap(), saved);
        assert!(!saved.stale(100 + MAX_AGE_SECS));
        assert!(saved.stale(101 + MAX_AGE_SECS));
        assert!(store(
            &dir,
            &Saved {
                fetched_at: 200,
                models: vec![]
            }
        )
        .is_err());
        assert_eq!(
            load(&dir).unwrap().fetched_at,
            100,
            "an empty list never replaces a good one"
        );
        let mut forged = saved.clone();
        forged.models.iter_mut().for_each(|m| {
            m.free = true;
        });
        store(&dir, &forged).unwrap();
        let back = load(&dir).unwrap();
        assert!(
            !back.find("baidu/ernie-5.5-flash-preview").unwrap().free,
            "a saved file cannot make a paid model free"
        );
        std::fs::write(dir.join(FILE), b"{not json").unwrap();
        assert!(load(&dir).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// `ensure` keeps one process-wide failure memory, so the tests that use it take turns.
    static TAKE_TURNS: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("catalog-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn callers_that_arrive_together_share_one_fetch() {
        let _turn = TAKE_TURNS.lock().unwrap_or_else(|e| e.into_inner());
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = scratch("shared");
        let fetches = AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    let got = ensure(&dir, 1_000, |t| {
                        fetches.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(std::time::Duration::from_millis(60));
                        Some(Saved {
                            fetched_at: t,
                            models: parse(&list()),
                        })
                    });
                    assert!(got.is_some_and(|g| !g.models.is_empty()));
                });
            }
        });
        assert_eq!(
            fetches.load(Ordering::SeqCst),
            1,
            "the others wait and read what the first saved"
        );
        assert!(load(&dir).is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_fetch_gives_nothing_and_a_fresh_saved_list_is_not_fetched_again() {
        let _turn = TAKE_TURNS.lock().unwrap_or_else(|e| e.into_inner());
        let dir = scratch("failed");
        assert!(ensure(&dir, 100_000_000, |_| None).is_none());
        let saved = Saved {
            fetched_at: 1_000,
            models: parse(&list()),
        };
        store(&dir, &saved).unwrap();
        let got = ensure(&dir, 1_000 + MAX_AGE_SECS, |_| {
            panic!("a fresh list needs no fetch")
        });
        assert_eq!(got, Some(saved));
        let stale = ensure(
            &dir,
            100_000_000 + FAILURE_MEMORY_SECS + MAX_AGE_SECS + 1,
            |t| {
                Some(Saved {
                    fetched_at: t,
                    models: parse(&list()),
                })
            },
        );
        assert_eq!(
            stale.unwrap().fetched_at,
            100_000_000 + FAILURE_MEMORY_SECS + MAX_AGE_SECS + 1
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_fetch_is_remembered_for_half_a_minute_and_then_tried_again() {
        let _turn = TAKE_TURNS.lock().unwrap_or_else(|e| e.into_inner());
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = scratch("remembered");
        let tries = AtomicUsize::new(0);
        let t0 = 200_000_000;
        let down = |_| {
            tries.fetch_add(1, Ordering::SeqCst);
            None
        };
        assert!(ensure(&dir, t0, down).is_none());
        assert!(ensure(&dir, t0 + FAILURE_MEMORY_SECS - 1, down).is_none());
        assert_eq!(
            tries.load(Ordering::SeqCst),
            1,
            "the second ask did not fetch"
        );
        let back = ensure(&dir, t0 + FAILURE_MEMORY_SECS, |t| {
            Some(Saved {
                fetched_at: t,
                models: parse(&list()),
            })
        });
        assert!(back.is_some(), "after the window the fetch is tried again");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn offline_the_picker_still_has_the_defaults_with_unknown_prices() {
        let m = offline();
        assert!(m
            .iter()
            .any(|x| x.id == DEFAULT_WRITING && x.input_per_mtok.is_none()));
        assert!(m.iter().any(|x| x.id == DEFAULT_CHECKING));
        assert!(m.iter().all(|x| crate::route::model_known("R12", &x.id)));
    }
}
