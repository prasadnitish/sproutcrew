//! Read-only OpenRouter allowance projection. Credentials and raw account responses never leave here.
use crate::connector::{Credentials, Request, Transport};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct DailyRequests {
    pub used: u64,
    pub limit: u64,
    pub remaining: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Limits {
    pub checked_at: u64,
    pub daily: Option<DailyRequests>,
}

fn daily(v: &Value) -> Option<DailyRequests> {
    let d = &v["data"]["free_model_daily_requests"];
    let used = d["used"].as_u64()?;
    let limit = d["limit"].as_u64()?;
    let remaining = d["remaining"].as_u64()?;
    if remaining != limit.saturating_sub(used) {
        return None;
    }
    Some(DailyRequests {
        used,
        limit,
        remaining,
    })
}

/// No model request and no account mutation. A valid key does not prove inference availability.
pub fn fetch(keys: &dyn Credentials, http: &mut dyn Transport, now: u64) -> Result<Limits, String> {
    let key = keys
        .get("openrouter-api")
        .ok_or("Connect OpenRouter in AI connections first")?;
    let request = Request {
        url: "https://openrouter.ai/api/v1/key".into(),
        headers: vec![("Authorization".into(), format!("Bearer {}", key.token()))],
    };
    let response = http
        .get(&request)
        .map_err(|_| "OpenRouter could not be reached for the allowance check")?;
    match response.status {
        200..=299 => {
            let body: Value = serde_json::from_str(&response.body).map_err(|_| "OpenRouter returned unreadable allowance data")?;
            Ok(Limits { checked_at: now, daily: daily(&body) })
        }
        401 | 403 => Err("OpenRouter refused the saved key for the allowance check; inspect the connection in Settings".into()),
        429 => Err("OpenRouter rate limited the allowance check. Wait before checking again".into()),
        status => Err(format!("OpenRouter allowance check was unavailable (status {status})")),
    }
}
