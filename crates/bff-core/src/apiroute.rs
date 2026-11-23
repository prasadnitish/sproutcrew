//! Requests for the direct API routes (R3 Anthropic, R5 OpenAI, R8 xAI, R9 Gemini, and others) and their answers. A request is built
//! only within stated bounds: a conservative input estimate, an output limit, a tool-call limit, and a
//! worst-case price that fits the allowance. Every price must be set. The key is never part of a built
//! request: it is named by reference and added at the moment of sending. Only one fixed address exists per
//! provider. Field names follow each provider's current reference.

use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    OpenAi,
    Xai,
    Gemini,
    DeepSeek,
    Kimi,
    OpenRouter,
    /// The person's own service at an address they typed. See [`crate::custom_ai`].
    Custom,
}

impl Provider {
    pub fn for_route(route: &str) -> Option<Provider> {
        match route {
            "R3" => Some(Provider::Anthropic),
            "R5" => Some(Provider::OpenAi),
            "R8" => Some(Provider::Xai),
            "R9" => Some(Provider::Gemini),
            "R10" => Some(Provider::DeepSeek),
            "R11" => Some(Provider::Kimi),
            "R12" => Some(Provider::OpenRouter),
            "R13" => Some(Provider::Custom),
            _ => None,
        }
    }

    pub fn route(self) -> &'static str {
        match self {
            Provider::Anthropic => "R3",
            Provider::OpenAi => "R5",
            Provider::Xai => "R8",
            Provider::Gemini => "R9",
            Provider::DeepSeek => "R10",
            Provider::Kimi => "R11",
            Provider::OpenRouter => "R12",
            Provider::Custom => "R13",
        }
    }

    /// The name of the credential to look up.
    pub fn credential(self) -> &'static str {
        match self {
            Provider::Anthropic => "anthropic-api",
            Provider::OpenAi => "openai-api",
            Provider::Xai => "xai-api",
            Provider::Gemini => "gemini-api",
            Provider::DeepSeek => "deepseek-api",
            Provider::Kimi => "kimi-api",
            Provider::OpenRouter => "openrouter-api",
            Provider::Custom => "custom-ai-api",
        }
    }

    /// The only host this provider's requests go to. The person's own service has none fixed here: its host is
    /// the one in its saved address, and its client is built for that host alone.
    pub fn host(self) -> &'static str {
        match self {
            Provider::Anthropic => "api.anthropic.com",
            Provider::OpenAi => "api.openai.com",
            Provider::Xai => "api.x.ai",
            Provider::Gemini => "generativelanguage.googleapis.com",
            Provider::DeepSeek => "api.deepseek.com",
            Provider::Kimi => "api.moonshot.ai",
            Provider::OpenRouter => "openrouter.ai",
            Provider::Custom => "",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic",
            Provider::OpenAi => "OpenAI",
            Provider::Xai => "xAI",
            Provider::Gemini => "Gemini",
            Provider::DeepSeek => "DeepSeek",
            Provider::Kimi => "Kimi",
            Provider::OpenRouter => "OpenRouter",
            Provider::Custom => "your own AI service",
        }
    }
}

/// Micro-dollars (millionths of a dollar) per million tokens, and per tool call. All must be set.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Prices {
    pub input_per_mtok: u64,
    pub output_per_mtok: u64,
    pub tool_call: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bounds {
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_tool_calls: u32,
    /// The most one request may cost in the worst case, in micro-dollars.
    pub max_cost_micro: u64,
}

pub const OUTPUT_HARD_CAP: u64 = 32_768;

/// Results asked of OpenRouter's web plugin for one search. Up to 10 are covered by its per-search price.
pub const OPENROUTER_SEARCH_RESULTS: u32 = 8;
/// OpenRouter's price for one Exa web search, in dollars (read Nov 10, 2026). Shown before approval as an estimate.
pub const OPENROUTER_SEARCH_USD: f64 = 0.007;

/// The Claude API version sent on every Anthropic request.
pub const ANTHROPIC_VERSION: &str = "2026-10-21";

/// Suggestions from OpenRouter's public catalog. A free label alone is not enough: both prices must be zero.
pub fn free_models(catalog: &Value) -> Vec<String> {
    let mut models = vec!["openrouter/free".to_string()];
    for entry in catalog["data"].as_array().into_iter().flatten() {
        let Some(id) = entry["id"].as_str() else {
            continue;
        };
        let zero = |field: &str| {
            entry["pricing"][field]
                .as_str()
                .and_then(|s| s.parse::<f64>().ok())
                == Some(0.0)
        };
        if id.ends_with(":free")
            && crate::route::model_known("R12", id)
            && zero("prompt")
            && zero("completion")
        {
            models.push(id.to_string());
        }
    }
    models.sort();
    models.dedup();
    models.truncate(64);
    models
}

#[derive(Debug, Clone)]
pub struct Job {
    pub model: String,
    pub effort: String,
    pub system: Option<String>,
    pub prompt: String,
    pub web_search: bool,
    /// Send only where nothing is kept: OpenRouter's zero-retention endpoints. Other providers are refused.
    pub no_retention: bool,
    /// Pictures as `data:` URLs, attached for a model that can see images. Empty for text only.
    pub images: Vec<String>,
    /// The base address of the person's own service. Used only by [`Provider::Custom`].
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    EmptyPrompt,
    UnknownModel(String),
    NoPrices(&'static str),
    InputTooLarge {
        estimate: u64,
        max: u64,
    },
    BadOutputBound(u64),
    ToolsNotAllowed(&'static str),
    NoToolCalls,
    CostOverBound {
        worst: u64,
        max: u64,
    },
    /// The plan says providers keep nothing, and this request could not promise it.
    CannotKeepNothing(&'static str),
    /// The person's own service has no saved address.
    NoAddress,
}

impl Refusal {
    /// The reason in plain words for the person, saying what to do about it.
    pub fn plain(&self) -> String {
        match self {
            Refusal::EmptyPrompt => "the step had nothing to send".into(),
            Refusal::UnknownModel(m) => format!("{m} is not a model this service lists. Choose another model"),
            Refusal::NoPrices(what) => format!("this service has no {what} price set, so its cost could not be bounded"),
            Refusal::InputTooLarge { estimate, max } => format!(
                "what this step sends is too long: it counts {estimate} against a limit of {max} for this service. Remove a long source, or ask for a change that splits the step"
            ),
            Refusal::BadOutputBound(n) => format!("the answer length set for this service ({n}) is not usable"),
            Refusal::ToolsNotAllowed(what) => format!("this service cannot use {what} here"),
            Refusal::NoToolCalls => "this service allows no web searches".into(),
            Refusal::CostOverBound { worst, max } => format!(
                "this request could cost up to ${:.2}, more than the ${:.2} allowed for one request",
                *worst as f64 / 1e6,
                *max as f64 / 1e6
            ),
            Refusal::CannotKeepNothing(what) => format!("{what} cannot promise to keep nothing, as this plan asks"),
            Refusal::NoAddress => "your own AI service has no address saved. Add it in Settings".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    pub url: String,
    /// Content type and the like. Never a credential.
    pub headers: Vec<(String, String)>,
    /// How the key is added when sending: the header name and whether it carries a `Bearer ` prefix.
    pub auth_header: &'static str,
    pub auth_bearer: bool,
    pub credential: &'static str,
    pub body: Value,
    pub worst_case_micro: u64,
}

/// A byte-level upper bound for text tokenization, plus message framing. Counting Unicode characters
/// underestimates CJK, emoji, and poorly compressed text and cannot safely bound a request.
pub fn estimate_input_tokens(text: &str) -> u64 {
    text.len() as u64 + 64
}

fn per_mtok(tokens: u64, price: u64) -> u64 {
    (tokens as u128 * price as u128).div_ceil(1_000_000) as u64
}

pub fn worst_case_micro(input_tokens: u64, b: &Bounds, p: &Prices, tools: bool) -> u64 {
    let calls = if tools {
        b.max_tool_calls as u64 * p.tool_call
    } else {
        0
    };
    per_mtok(input_tokens, p.input_per_mtok)
        + per_mtok(b.max_output_tokens, p.output_per_mtok)
        + calls
}

pub fn build(provider: Provider, job: &Job, b: &Bounds, p: &Prices) -> Result<Built, Refusal> {
    if job.prompt.trim().is_empty() {
        return Err(Refusal::EmptyPrompt);
    }
    if !crate::route::model_known(provider.route(), &job.model) {
        return Err(Refusal::UnknownModel(job.model.clone()));
    }
    if job.web_search
        && !matches!(
            provider,
            Provider::OpenAi | Provider::Anthropic | Provider::OpenRouter
        )
    {
        return Err(Refusal::ToolsNotAllowed(provider.name()));
    }
    // OpenRouter charges for search even on free models, and a free model's zero price cap would refuse it.
    if job.web_search
        && provider == Provider::OpenRouter
        && (job.model == "openrouter/free" || job.model.ends_with(":free"))
    {
        return Err(Refusal::ToolsNotAllowed("a free OpenRouter model"));
    }
    if job.no_retention && provider != Provider::OpenRouter {
        return Err(Refusal::CannotKeepNothing(provider.name()));
    }
    // OpenRouter's zero-retention routing does not cover plugins such as its web search.
    if job.no_retention && job.web_search {
        return Err(Refusal::CannotKeepNothing("a web search"));
    }
    if b.max_output_tokens == 0 || b.max_output_tokens > OUTPUT_HARD_CAP {
        return Err(Refusal::BadOutputBound(b.max_output_tokens));
    }
    if p.input_per_mtok == 0 {
        return Err(Refusal::NoPrices("input"));
    }
    if p.output_per_mtok == 0 {
        return Err(Refusal::NoPrices("output"));
    }
    // OpenRouter runs one search per request through its web plugin and reports the fee in the answer's cost, so
    // its search needs no tool-call price or count from the route settings.
    if job.web_search && provider != Provider::OpenRouter {
        if p.tool_call == 0 {
            return Err(Refusal::NoPrices("tool call"));
        }
        if b.max_tool_calls == 0 {
            return Err(Refusal::NoToolCalls);
        }
    }
    let estimate = estimate_input_tokens(&format!(
        "{}{}",
        job.system.as_deref().unwrap_or(""),
        job.prompt
    )) + job.images.len() as u64 * crate::vision::TOKENS_PER_IMAGE;
    if estimate > b.max_input_tokens {
        return Err(Refusal::InputTooLarge {
            estimate,
            max: b.max_input_tokens,
        });
    }
    let worst = worst_case_micro(estimate, b, p, job.web_search);
    if worst > b.max_cost_micro {
        return Err(Refusal::CostOverBound {
            worst,
            max: b.max_cost_micro,
        });
    }
    let headers = vec![("Content-Type".to_string(), "application/json".to_string())];
    let system = job.system.as_deref().filter(|s| !s.trim().is_empty());
    let mut headers = headers;
    let (url, body, auth_header, auth_bearer) = match provider {
        Provider::Anthropic => {
            headers.push((
                "anthropic-version".to_string(),
                ANTHROPIC_VERSION.to_string(),
            ));
            // No sampling fields: this API version refuses them. Thinking is on by default and counts as output,
            // so `max_tokens` bounds it too.
            let mut body = json!({
                "model": job.model,
                "max_tokens": b.max_output_tokens,
                "messages": [{"role": "user", "content": job.prompt}],
                "output_config": {"effort": job.effort},
            });
            if let Some(s) = system {
                body["system"] = json!(s);
            }
            if job.web_search {
                // Haiku has only the basic search tool. The newer one filters results before they are read.
                let kind = if job.model.starts_with("claude-haiku") {
                    "web_search_20250305"
                } else {
                    "web_search_20260209"
                };
                body["tools"] =
                    json!([{"type": kind, "name": "web_search", "max_uses": b.max_tool_calls}]);
            }
            (
                "https://api.anthropic.com/v1/messages".to_string(),
                body,
                "x-api-key",
                false,
            )
        }
        Provider::OpenAi => {
            let mut body = json!({"model": job.model, "input": job.prompt, "max_output_tokens": b.max_output_tokens, "store": false});
            if let Some(s) = system {
                body["instructions"] = json!(s);
            }
            if ["low", "medium", "high"].contains(&job.effort.as_str()) {
                body["reasoning"] = json!({"effort": job.effort});
            }
            if job.web_search {
                body["tools"] = json!([{"type": "web_search"}]);
                body["max_tool_calls"] = json!(b.max_tool_calls);
            }
            (
                "https://api.openai.com/v1/responses".to_string(),
                body,
                "Authorization",
                true,
            )
        }
        Provider::Xai
        | Provider::DeepSeek
        | Provider::Kimi
        | Provider::OpenRouter
        | Provider::Custom => {
            let mut messages = Vec::new();
            if let Some(s) = system {
                messages.push(json!({"role": "system", "content": s}));
            }
            if job.images.is_empty() || provider != Provider::OpenRouter {
                messages.push(json!({"role": "user", "content": job.prompt}));
            } else {
                // The text first, then each picture, in OpenRouter's multi-part message form.
                let mut parts = vec![json!({"type": "text", "text": job.prompt})];
                parts.extend(
                    job.images
                        .iter()
                        .map(|u| json!({"type": "image_url", "image_url": {"url": u}})),
                );
                messages.push(json!({"role": "user", "content": parts}));
            }
            let mut body = json!({"model": job.model, "messages": messages, "stream": false});
            // Kimi has deprecated `max_tokens` in favor of `max_completion_tokens`.
            let limit = if provider == Provider::Kimi {
                "max_completion_tokens"
            } else {
                "max_tokens"
            };
            body[limit] = json!(b.max_output_tokens);
            if provider == Provider::OpenRouter {
                // No silent rerouting to another provider, and no provider that keeps the data.
                body["provider"] = json!({"allow_fallbacks": false, "data_collection": "deny"});
                if job.no_retention {
                    // Only endpoints that keep no prompts or answers (OpenRouter's zero data retention routing).
                    body["provider"]["zdr"] = json!(true);
                }
                if job.model == "openrouter/free" || job.model.ends_with(":free") {
                    body["provider"]["max_price"] =
                        json!({"prompt": 0, "completion": 0, "request": 0});
                }
                if job.web_search {
                    // Exa has a fixed price per search (0.007 dollars for up to 10 results, Nov 10, 2026).
                    body["plugins"] = json!([{"id": "web", "engine": "exa", "max_results": OPENROUTER_SEARCH_RESULTS}]);
                }
            }
            let path = match provider {
                Provider::Xai => "https://api.x.ai/v1/chat/completions".to_string(),
                Provider::DeepSeek => "https://api.deepseek.com/chat/completions".to_string(),
                Provider::Kimi => "https://api.moonshot.ai/v1/chat/completions".to_string(),
                Provider::Custom => {
                    // Checked again here, so a hand-edited settings file cannot point somewhere unchecked.
                    let a = job
                        .endpoint
                        .as_deref()
                        .and_then(|e| crate::custom_ai::check_address(e).ok())
                        .ok_or(Refusal::NoAddress)?;
                    format!("{}/chat/completions", a.base)
                }
                _ => "https://openrouter.ai/api/v1/chat/completions".to_string(),
            };
            (path, body, "Authorization", true)
        }
        Provider::Gemini => {
            let mut body = json!({
                "contents": [{"role": "user", "parts": [{"text": job.prompt}]}],
                "generationConfig": {"maxOutputTokens": b.max_output_tokens},
                "store": false,
            });
            if let Some(s) = system {
                body["systemInstruction"] = json!({"parts": [{"text": s}]});
            }
            (
                format!(
                    "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
                    job.model
                ),
                body,
                "x-goog-api-key",
                false,
            )
        }
    };
    Ok(Built {
        url,
        headers,
        auth_header,
        auth_bearer,
        credential: provider.credential(),
        body,
        worst_case_micro: worst,
    })
}

/// Checks a pasted key with the provider's list of models, which costs nothing, before it is saved. Only the
/// providers with a key screen (Anthropic and OpenAI) are checked this way.
pub fn check_key(
    provider: Provider,
    secret: &str,
    transport: &mut dyn crate::connector::Transport,
) -> Result<(), String> {
    let (url, mut headers) = match provider {
        Provider::Anthropic => (
            "https://api.anthropic.com/v1/models?limit=1",
            vec![("x-api-key".to_string(), secret.to_string())],
        ),
        Provider::OpenAi => (
            "https://api.openai.com/v1/models",
            vec![("Authorization".to_string(), format!("Bearer {secret}"))],
        ),
        Provider::OpenRouter => (
            "https://openrouter.ai/api/v1/key",
            vec![("Authorization".to_string(), format!("Bearer {secret}"))],
        ),
        // Listing one model reads nothing about the account and costs nothing.
        Provider::Gemini => (
            "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1",
            vec![("x-goog-api-key".to_string(), secret.to_string())],
        ),
        other => return Err(format!("{} keys are not checked here", other.name())),
    };
    if provider == Provider::Anthropic {
        headers.push((
            "anthropic-version".to_string(),
            ANTHROPIC_VERSION.to_string(),
        ));
    }
    let req = crate::connector::Request {
        url: url.to_string(),
        headers,
    };
    match transport.get(&req) {
        Ok(r) if (200..300).contains(&r.status) => Ok(()),
        Ok(r) if r.status == 401 || r.status == 403 => Err(format!(
            "{} refused this key. Check you copied all of it, and that it has not been revoked.",
            provider.name()
        )),
        Ok(r) => Err(format!(
            "{} could not check the key right now (status {}). Try again in a minute.",
            provider.name(),
            r.status
        )),
        Err(crate::connector::TransportError::Network(e)) => {
            Err(format!("{} could not be reached: {e}", provider.name()))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Citation {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub text: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    /// A dollar figure the provider reported itself, in micro-dollars.
    pub reported_micro: Option<u64>,
    pub citations: Vec<Citation>,
    /// Web searches the provider ran, which are charged per call.
    pub tool_calls: u32,
}

pub fn parse(provider: Provider, body: &Value) -> Result<Parsed, String> {
    if !body.is_object() {
        return Err("the answer is not a JSON object".into());
    }
    let u = |v: &Value| v.as_u64();
    let mut citations = Vec::new();
    let mut tool_calls = 0;
    let (text, input_tokens, output_tokens, reported_micro) = match provider {
        Provider::Anthropic => {
            if body["type"] == "error" {
                return Err(format!(
                    "the API returned an error: {}",
                    body["error"]["message"].as_str().unwrap_or("no message")
                ));
            }
            match body["stop_reason"].as_str() {
                Some("max_tokens") => {
                    return Err("the answer was cut off at the output limit".into())
                }
                Some("refusal") => {
                    let why = body["stop_details"]["category"]
                        .as_str()
                        .unwrap_or("no category");
                    return Err(format!("the model declined the request ({why})"));
                }
                Some("pause_turn") => return Err("the model paused before finishing".into()),
                _ => {}
            }
            let mut text = String::new();
            for block in body["content"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|b| b["type"] == "text")
            {
                text.push_str(block["text"].as_str().unwrap_or(""));
                for c in block["citations"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|c| c["type"] == "web_search_result_location")
                {
                    citations.push(Citation {
                        url: c["url"].as_str().unwrap_or("").to_string(),
                        title: c["title"].as_str().unwrap_or("").to_string(),
                    });
                }
            }
            let usage = &body["usage"];
            tool_calls = u(&usage["server_tool_use"]["web_search_requests"]).unwrap_or(0) as u32;
            // Cache reads and writes are listed apart from `input_tokens`. They are charged at the input price,
            // which is never less than their own.
            let input = u(&usage["input_tokens"]).map(|i| {
                i + u(&usage["cache_read_input_tokens"]).unwrap_or(0)
                    + u(&usage["cache_creation_input_tokens"]).unwrap_or(0)
            });
            (text, input, u(&usage["output_tokens"]), None)
        }
        Provider::OpenAi => {
            if let Some(e) = body.get("error").filter(|e| !e.is_null()) {
                return Err(format!(
                    "the API returned an error: {}",
                    e["message"].as_str().unwrap_or("no message")
                ));
            }
            if let Some(st @ ("failed" | "incomplete" | "cancelled")) = body["status"].as_str() {
                return Err(format!("the response status is {st}"));
            }
            let mut text = String::new();
            tool_calls = body["output"]
                .as_array()
                .map(|o| o.iter().filter(|i| i["type"] == "web_search_call").count() as u32)
                .unwrap_or(0);
            for item in body["output"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|i| i["type"] == "message")
            {
                for part in item["content"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|c| c["type"] == "output_text")
                {
                    text.push_str(part["text"].as_str().unwrap_or(""));
                    for a in part["annotations"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|a| a["type"] == "url_citation")
                    {
                        citations.push(Citation {
                            url: a["url"].as_str().unwrap_or("").to_string(),
                            title: a["title"].as_str().unwrap_or("").to_string(),
                        });
                    }
                }
            }
            (
                text,
                u(&body["usage"]["input_tokens"]),
                u(&body["usage"]["output_tokens"]),
                None,
            )
        }
        Provider::Xai
        | Provider::DeepSeek
        | Provider::Kimi
        | Provider::OpenRouter
        | Provider::Custom => {
            let text = body["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_string();
            // OpenRouter's web plugin reports the pages it used in the OpenAI annotation shape.
            for a in body["choices"][0]["message"]["annotations"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|a| a["type"] == "url_citation")
            {
                let c = &a["url_citation"];
                if let Some(url) = c["url"].as_str() {
                    citations.push(Citation {
                        url: url.to_string(),
                        title: c["title"].as_str().unwrap_or("").to_string(),
                    });
                }
            }
            let usage = &body["usage"];
            let reported = match provider {
                Provider::Xai => u(&usage["cost_in_usd_ticks"]).map(|t| t / 10_000),
                Provider::OpenRouter => usage["cost"]
                    .as_f64()
                    .filter(|c| *c >= 0.0)
                    .map(|c| (c * 1_000_000.0).round() as u64),
                _ => None,
            };
            (
                text,
                u(&usage["prompt_tokens"]),
                u(&usage["completion_tokens"]),
                reported,
            )
        }
        Provider::Gemini => {
            let parts = body["candidates"][0]["content"]["parts"].as_array();
            if parts.is_none() {
                let why = body["promptFeedback"]["blockReason"]
                    .as_str()
                    .unwrap_or("no candidate");
                return Err(format!("the answer has no content ({why})"));
            }
            let text: String = parts
                .into_iter()
                .flatten()
                .filter_map(|p| p["text"].as_str())
                .collect();
            let m = &body["usageMetadata"];
            (
                text,
                u(&m["promptTokenCount"]),
                u(&m["responseTokenCount"]).or_else(|| u(&m["candidatesTokenCount"])),
                None,
            )
        }
    };
    if text.trim().is_empty() {
        return Err("the answer has no text".into());
    }
    Ok(Parsed {
        text,
        input_tokens,
        output_tokens,
        reported_micro,
        citations,
        tool_calls,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostBasis {
    /// The provider reported it.
    Reported,
    /// Computed from usage and the price table.
    Estimated,
}

/// The cost of a finished call, or `None` if neither a reported figure nor usage exists.
pub fn actual_micro(parsed: &Parsed, p: &Prices, tool_calls: u32) -> Option<(u64, CostBasis)> {
    if let Some(r) = parsed.reported_micro {
        return Some((r, CostBasis::Reported));
    }
    let (i, o) = (parsed.input_tokens?, parsed.output_tokens?);
    Some((
        per_mtok(i, p.input_per_mtok)
            + per_mtok(o, p.output_per_mtok)
            + tool_calls as u64 * p.tool_call,
        CostBasis::Estimated,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test-only numbers, not real prices.
    const PRICES: Prices = Prices {
        input_per_mtok: 2_000_000,
        output_per_mtok: 8_000_000,
        tool_call: 10_000,
    };
    const BOUNDS: Bounds = Bounds {
        max_input_tokens: 10_000,
        max_output_tokens: 1000,
        max_tool_calls: 3,
        max_cost_micro: 100_000,
    };

    fn job(model: &str) -> Job {
        Job {
            model: model.into(),
            effort: "low".into(),
            system: Some("Be brief.".into()),
            prompt: "x".repeat(1000),
            web_search: false,
            no_retention: false,
            images: vec![],
            endpoint: None,
        }
    }

    #[test]
    fn the_input_estimate_counts_utf8_bytes_and_framing_conservatively() {
        assert_eq!(estimate_input_tokens(""), 64);
        assert_eq!(estimate_input_tokens(&"x".repeat(1000)), 1064);
        assert_eq!(estimate_input_tokens("abc"), 67, "bytes plus framing");
        assert_eq!(
            estimate_input_tokens("héllo wörld"),
            64 + "héllo wörld".len() as u64
        );
        assert_eq!(estimate_input_tokens("你好🪴"), 74);
    }

    #[test]
    fn catalog_suggestions_require_valid_free_ids_and_both_zero_prices() {
        let catalog = json!({"data": [
            {"id":"vendor/free:free", "pricing":{"prompt":"0", "completion":"0"}},
            {"id":"vendor/paid:free", "pricing":{"prompt":"0", "completion":"0.01"}},
            {"id":"vendor/unknown:free", "pricing":{"prompt":"0"}},
            {"id":"vendor/not-free", "pricing":{"prompt":"0", "completion":"0"}},
            {"id":"vendor/free:free", "pricing":{"prompt":"0", "completion":"0"}},
            {"id":"../escape:free", "pricing":{"prompt":"0", "completion":"0"}}
        ]});
        assert_eq!(
            free_models(&catalog),
            vec!["openrouter/free", "vendor/free:free"]
        );
        assert_eq!(free_models(&json!({})), vec!["openrouter/free"]);
    }

    #[test]
    fn the_worst_case_adds_input_output_and_tool_fees_rounding_up() {
        assert_eq!(worst_case_micro(564, &BOUNDS, &PRICES, false), 1128 + 8000);
        assert_eq!(
            worst_case_micro(564, &BOUNDS, &PRICES, true),
            1128 + 8000 + 30_000
        );
        let odd = Prices {
            input_per_mtok: 1,
            output_per_mtok: 1,
            tool_call: 0,
        };
        assert_eq!(
            worst_case_micro(
                1,
                &Bounds {
                    max_output_tokens: 1,
                    ..BOUNDS
                },
                &odd,
                false
            ),
            2,
            "each part rounds up to 1"
        );
    }

    #[test]
    fn openai_requests_go_to_the_fixed_address_with_every_bound_in_the_body() {
        let b = build(Provider::OpenAi, &job("gpt-6.1-terra"), &BOUNDS, &PRICES).unwrap();
        assert_eq!(b.url, "https://api.openai.com/v1/responses");
        assert_eq!(b.body["model"], "gpt-6.1-terra");
        assert_eq!(b.body["max_output_tokens"], 1000);
        assert_eq!(b.body["store"], false);
        assert_eq!(b.body["instructions"], "Be brief.");
        assert_eq!(b.body["input"].as_str().unwrap().len(), 1000);
        assert_eq!(b.body["reasoning"]["effort"], "low");
        assert!(
            b.body.get("tools").is_none()
                && b.body.get("max_tool_calls").is_none()
                && b.body.get("stream").is_none()
        );
        assert_eq!(
            (b.auth_header, b.auth_bearer, b.credential),
            ("Authorization", true, "openai-api")
        );
        assert_eq!(b.worst_case_micro, 10_146);
    }

    #[test]
    fn free_models_cannot_fall_back_to_paid_inference_or_collect_data() {
        for model in ["openrouter/free", "vendor/model:free"] {
            let r = build(Provider::OpenRouter, &job(model), &BOUNDS, &PRICES).unwrap();
            assert_eq!(
                r.body["provider"]["max_price"],
                json!({"prompt":0,"completion":0,"request":0})
            );
            assert_eq!(r.body["provider"]["allow_fallbacks"], false);
            assert_eq!(r.body["provider"]["data_collection"], "deny");
        }
        let paid = build(Provider::OpenRouter, &job("vendor/model"), &BOUNDS, &PRICES).unwrap();
        assert!(paid.body["provider"].get("max_price").is_none());
    }

    #[test]
    fn web_search_is_allowed_only_for_openai_and_brings_a_tool_call_limit_and_fee() {
        let mut j = job("gpt-6.1-terra");
        j.web_search = true;
        let b = build(Provider::OpenAi, &j, &BOUNDS, &PRICES).unwrap();
        assert_eq!(b.body["tools"], json!([{"type": "web_search"}]));
        assert_eq!(b.body["max_tool_calls"], 3);
        assert_eq!(b.worst_case_micro, 40_146);
        assert_eq!(
            build(
                Provider::OpenAi,
                &j,
                &Bounds {
                    max_tool_calls: 0,
                    ..BOUNDS
                },
                &PRICES
            ),
            Err(Refusal::NoToolCalls)
        );
        let no_fee = Prices {
            tool_call: 0,
            ..PRICES
        };
        assert_eq!(
            build(Provider::OpenAi, &j, &BOUNDS, &no_fee),
            Err(Refusal::NoPrices("tool call"))
        );
        let mut x = job("grok-4.7");
        x.web_search = true;
        assert_eq!(
            build(Provider::Xai, &x, &BOUNDS, &PRICES),
            Err(Refusal::ToolsNotAllowed("xAI"))
        );
        let mut g = job("gemini-3.6-flash");
        g.web_search = true;
        assert_eq!(
            build(Provider::Gemini, &g, &BOUNDS, &PRICES),
            Err(Refusal::ToolsNotAllowed("Gemini"))
        );
    }

    #[test]
    fn xai_and_gemini_requests_have_their_own_shapes_and_fixed_addresses() {
        let x = build(Provider::Xai, &job("grok-4.7"), &BOUNDS, &PRICES).unwrap();
        assert_eq!(x.url, "https://api.x.ai/v1/chat/completions");
        assert_eq!(
            x.body["messages"][0],
            json!({"role": "system", "content": "Be brief."})
        );
        assert_eq!(x.body["messages"][1]["role"], "user");
        assert_eq!(x.body["max_tokens"], 1000);
        assert_eq!(x.body["stream"], false);
        assert_eq!((x.auth_header, x.auth_bearer), ("Authorization", true));
        let g = build(Provider::Gemini, &job("gemini-3.6-flash"), &BOUNDS, &PRICES).unwrap();
        assert_eq!(g.url, "https://generativelanguage.googleapis.com/v1beta/models/gemini-3.6-flash:generateContent");
        assert_eq!(g.body["generationConfig"]["maxOutputTokens"], 1000);
        assert_eq!(g.body["systemInstruction"]["parts"][0]["text"], "Be brief.");
        assert_eq!(g.body["contents"][0]["role"], "user");
        assert_eq!(g.body["store"], false);
        assert_eq!(
            (g.auth_header, g.auth_bearer, g.credential),
            ("x-goog-api-key", false, "gemini-api")
        );
    }

    #[test]
    fn an_anthropic_request_uses_the_messages_api_with_its_key_header_and_no_sampling_fields() {
        let mut j = job("claude-sonnet-5-5");
        j.effort = "high".into();
        let a = build(Provider::Anthropic, &j, &BOUNDS, &PRICES).unwrap();
        assert_eq!(a.url, "https://api.anthropic.com/v1/messages");
        assert!(a
            .headers
            .contains(&("anthropic-version".into(), "2026-10-21".into())));
        assert_eq!(
            (a.auth_header, a.auth_bearer, a.credential),
            ("x-api-key", false, "anthropic-api")
        );
        assert_eq!(a.body["max_tokens"], 1000);
        assert_eq!(a.body["system"], "Be brief.");
        assert_eq!(a.body["messages"][0]["role"], "user");
        assert_eq!(a.body["output_config"]["effort"], "high");
        for f in ["temperature", "top_p", "top_k", "tools", "stream"] {
            assert!(a.body.get(f).is_none(), "{f}");
        }
        assert!(build(
            Provider::Anthropic,
            &job("claude-fable-5-1"),
            &BOUNDS,
            &PRICES
        )
        .is_err());
        assert!(build(Provider::Anthropic, &job("gpt-6-luna"), &BOUNDS, &PRICES).is_err());

        j.web_search = true;
        let s = build(Provider::Anthropic, &j, &BOUNDS, &PRICES).unwrap();
        assert_eq!(
            s.body["tools"],
            json!([{"type": "web_search_20260209", "name": "web_search", "max_uses": 3}])
        );
        let mut h = job("claude-haiku-5-5");
        h.web_search = true;
        let s = build(Provider::Anthropic, &h, &BOUNDS, &PRICES).unwrap();
        assert_eq!(
            s.body["tools"][0]["type"], "web_search_20250305",
            "Haiku has only the basic tool"
        );
    }

    #[test]
    fn an_anthropic_answer_joins_text_counts_searches_and_refuses_unfinished_ones() {
        let body = json!({
            "type": "message", "stop_reason": "end_turn",
            "content": [
                {"type": "thinking", "thinking": ""},
                {"type": "server_tool_use", "name": "web_search"},
                {"type": "web_search_tool_result", "content": []},
                {"type": "text", "text": "Rates rose ", "citations": [
                    {"type": "web_search_result_location", "url": "https://a.example", "title": "A", "cited_text": "x"}
                ]},
                {"type": "text", "text": "in May."}
            ],
            "usage": {"input_tokens": 100, "output_tokens": 50, "cache_read_input_tokens": 10,
                      "cache_creation_input_tokens": 5, "server_tool_use": {"web_search_requests": 2}}
        });
        let p = parse(Provider::Anthropic, &body).unwrap();
        assert_eq!(p.text, "Rates rose in May.");
        assert_eq!(
            p.input_tokens,
            Some(115),
            "cache reads and writes count as input"
        );
        assert_eq!(p.output_tokens, Some(50));
        assert_eq!(p.tool_calls, 2);
        assert_eq!(
            p.citations,
            vec![Citation {
                url: "https://a.example".into(),
                title: "A".into()
            }]
        );

        for (stop, why) in [
            ("max_tokens", "output limit"),
            ("refusal", "declined"),
            ("pause_turn", "paused"),
        ] {
            let mut b = body.clone();
            b["stop_reason"] = json!(stop);
            assert!(
                parse(Provider::Anthropic, &b).unwrap_err().contains(why),
                "{stop}"
            );
        }
        let err = json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}});
        assert!(parse(Provider::Anthropic, &err)
            .unwrap_err()
            .contains("Overloaded"));
    }

    struct Answer(
        u16,
        std::sync::Arc<std::sync::Mutex<Vec<crate::connector::Request>>>,
    );
    impl crate::connector::Transport for Answer {
        fn get(
            &mut self,
            req: &crate::connector::Request,
        ) -> Result<crate::connector::Response, crate::connector::TransportError> {
            self.1.lock().unwrap().push(req.clone());
            Ok(crate::connector::Response {
                status: self.0,
                body: String::new(),
            })
        }
    }

    #[test]
    fn keys_are_checked_with_read_only_provider_endpoints_before_they_are_kept() {
        let seen = std::sync::Arc::default();
        let mut ok = Answer(200, std::sync::Arc::clone(&seen));
        assert!(check_key(Provider::Anthropic, "sk-ant-x", &mut ok).is_ok());
        assert!(check_key(Provider::OpenAi, "sk-proj-x", &mut ok).is_ok());
        assert!(check_key(Provider::OpenRouter, "sk-or-test", &mut ok).is_ok());
        let reqs = seen.lock().unwrap().clone();
        assert_eq!(reqs[0].url, "https://api.anthropic.com/v1/models?limit=1");
        assert!(reqs[0]
            .headers
            .contains(&("x-api-key".into(), "sk-ant-x".into())));
        assert!(reqs[0]
            .headers
            .contains(&("anthropic-version".into(), ANTHROPIC_VERSION.into())));
        assert_eq!(reqs[1].url, "https://api.openai.com/v1/models");
        assert_eq!(reqs[2].url, "https://openrouter.ai/api/v1/key");
        assert!(reqs[1]
            .headers
            .contains(&("Authorization".into(), "Bearer sk-proj-x".into())));
        let mut refused = Answer(401, std::sync::Arc::default());
        assert!(check_key(Provider::OpenAi, "sk-x", &mut refused)
            .unwrap_err()
            .contains("refused"));
        let mut down = Answer(529, std::sync::Arc::default());
        assert!(check_key(Provider::Anthropic, "sk-x", &mut down)
            .unwrap_err()
            .contains("529"));
        assert!(check_key(Provider::Gemini, "AIza-test", &mut ok).is_ok());
        let last = seen.lock().unwrap().last().cloned().unwrap();
        assert_eq!(
            last.url,
            "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1"
        );
        assert!(last
            .headers
            .contains(&("x-goog-api-key".into(), "AIza-test".into())));
        assert!(
            !last.url.contains("AIza"),
            "the key never goes in the address"
        );
        assert!(check_key(Provider::Xai, "x", &mut ok).is_err());
    }

    #[test]
    fn a_built_request_never_carries_a_credential_or_another_host() {
        for (p, m) in [
            (Provider::OpenAi, "gpt-6.1-terra"),
            (Provider::Xai, "grok-4.7"),
            (Provider::Gemini, "gemini-3.6-flash"),
        ] {
            let b = build(p, &job(m), &BOUNDS, &PRICES).unwrap();
            assert!(
                b.headers
                    .iter()
                    .all(|(k, _)| !k.eq_ignore_ascii_case("authorization")
                        && !k.eq_ignore_ascii_case("x-goog-api-key")),
                "{p:?}"
            );
            assert!(
                b.url.starts_with("https://") && !b.url.contains('@') && !b.url.contains("key="),
                "{}",
                b.url
            );
            let host = b
                .url
                .trim_start_matches("https://")
                .split('/')
                .next()
                .unwrap();
            assert!([
                "api.openai.com",
                "api.x.ai",
                "generativelanguage.googleapis.com"
            ]
            .contains(&host));
        }
    }

    #[test]
    fn a_model_must_be_on_the_providers_route_so_a_name_cannot_change_the_address() {
        for (p, m) in [
            (Provider::Gemini, "gemini-3.6-flash/../../x"),
            (Provider::Gemini, "gemini-3.6-flash?key=1"),
            (Provider::Gemini, "grok-4.7"),
            (Provider::Xai, "gpt-6.1-terra"),
            (Provider::OpenAi, "claude-sonnet-5-5"),
            (Provider::OpenAi, ""),
        ] {
            assert_eq!(
                build(p, &job(m), &BOUNDS, &PRICES),
                Err(Refusal::UnknownModel(m.into())),
                "{p:?} {m:?}"
            );
        }
    }

    #[test]
    fn requests_outside_the_bounds_are_refused_before_anything_is_built() {
        let mut big = job("gpt-6.1-terra");
        big.prompt = "x".repeat(30_000);
        assert!(matches!(
            build(Provider::OpenAi, &big, &BOUNDS, &PRICES),
            Err(Refusal::InputTooLarge {
                estimate: 30_073,
                max: 10_000
            })
        ));
        let mut empty = job("gpt-6.1-terra");
        empty.prompt = "   ".into();
        assert_eq!(
            build(Provider::OpenAi, &empty, &BOUNDS, &PRICES),
            Err(Refusal::EmptyPrompt)
        );
        for out in [0, OUTPUT_HARD_CAP + 1] {
            assert_eq!(
                build(
                    Provider::OpenAi,
                    &job("gpt-6.1-terra"),
                    &Bounds {
                        max_output_tokens: out,
                        ..BOUNDS
                    },
                    &PRICES
                ),
                Err(Refusal::BadOutputBound(out))
            );
        }
        assert_eq!(
            build(
                Provider::OpenAi,
                &job("gpt-6.1-terra"),
                &Bounds {
                    max_cost_micro: 9137,
                    ..BOUNDS
                },
                &PRICES
            ),
            Err(Refusal::CostOverBound {
                worst: 10_146,
                max: 9137
            })
        );
        assert!(
            build(
                Provider::OpenAi,
                &job("gpt-6.1-terra"),
                &Bounds {
                    max_cost_micro: 10_146,
                    ..BOUNDS
                },
                &PRICES
            )
            .is_ok(),
            "exactly at the bound is allowed"
        );
    }

    #[test]
    fn every_price_must_be_set() {
        for p in [
            Prices {
                input_per_mtok: 0,
                ..PRICES
            },
            Prices {
                output_per_mtok: 0,
                ..PRICES
            },
        ] {
            assert!(matches!(
                build(Provider::OpenAi, &job("gpt-6.1-terra"), &BOUNDS, &p),
                Err(Refusal::NoPrices(_))
            ));
        }
        assert_eq!(
            build(
                Provider::OpenAi,
                &job("gpt-6.1-terra"),
                &BOUNDS,
                &Prices::default()
            ),
            Err(Refusal::NoPrices("input"))
        );
    }

    #[test]
    fn an_openai_answer_gives_text_usage_and_citations() {
        let body = json!({"status": "completed", "output": [
            {"type": "web_search_call", "id": "ws_1", "status": "completed"},
            {"type": "message", "role": "assistant", "content": [
                {"type": "output_text", "text": "The pilot costs $5 per month.", "annotations": [
                    {"type": "url_citation", "url": "https://example.test/pricing", "title": "Pricing", "start_index": 0, "end_index": 5},
                    {"type": "file_citation", "file_id": "f"}]}]}],
            "usage": {"input_tokens": 8438, "output_tokens": 398, "total_tokens": 8836}});
        let p = parse(Provider::OpenAi, &body).unwrap();
        assert_eq!(p.text, "The pilot costs $5 per month.");
        assert_eq!(
            (p.input_tokens, p.output_tokens, p.reported_micro),
            (Some(8438), Some(398), None)
        );
        assert_eq!(
            p.citations,
            vec![Citation {
                url: "https://example.test/pricing".into(),
                title: "Pricing".into()
            }]
        );
    }

    #[test]
    fn an_xai_answer_gives_text_usage_and_the_dollar_figure_the_provider_reported() {
        let body = json!({"choices": [{"index": 0, "message": {"role": "assistant", "content": "303"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 32, "completion_tokens": 9, "total_tokens": 135, "cost_in_usd_ticks": 123_450_000}});
        let p = parse(Provider::Xai, &body).unwrap();
        assert_eq!(p.text, "303");
        assert_eq!((p.input_tokens, p.output_tokens), (Some(32), Some(9)));
        assert_eq!(
            p.reported_micro,
            Some(12_345),
            "10,000,000,000 ticks to the dollar"
        );
    }

    #[test]
    fn a_gemini_answer_joins_parts_and_reads_either_usage_field_name() {
        let body = json!({"candidates": [{"content": {"role": "model", "parts": [{"text": "Hello "}, {"text": "world"}]}, "finishReason": "STOP"}],
            "usageMetadata": {"promptTokenCount": 10, "responseTokenCount": 4, "totalTokenCount": 14}});
        let p = parse(Provider::Gemini, &body).unwrap();
        assert_eq!(p.text, "Hello world");
        assert_eq!((p.input_tokens, p.output_tokens), (Some(10), Some(4)));
        let older = json!({"candidates": [{"content": {"parts": [{"text": "x"}]}}], "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 2}});
        assert_eq!(
            parse(Provider::Gemini, &older).unwrap().output_tokens,
            Some(2)
        );
    }

    #[test]
    fn missing_usage_is_unknown_and_unusable_answers_are_errors() {
        let p = parse(
            Provider::Xai,
            &json!({"choices": [{"message": {"content": "hi"}}]}),
        )
        .unwrap();
        assert_eq!(
            (p.input_tokens, p.output_tokens, p.reported_micro),
            (None, None, None)
        );
        assert_eq!(actual_micro(&p, &PRICES, 0), None, "unknown is not zero");
        assert!(parse(Provider::OpenAi, &json!({"error": {"message": "bad"}})).is_err());
        assert!(parse(
            Provider::OpenAi,
            &json!({"status": "incomplete", "output": []})
        )
        .is_err());
        assert!(parse(Provider::Xai, &json!({"choices": []})).is_err());
        assert!(parse(
            Provider::Gemini,
            &json!({"candidates": [], "promptFeedback": {"blockReason": "SAFETY"}})
        )
        .is_err());
        assert!(parse(Provider::Gemini, &json!("nope")).is_err());
    }

    #[test]
    fn the_cost_prefers_the_reported_figure_and_otherwise_comes_from_usage_and_prices() {
        let reported = Parsed {
            text: String::new(),
            input_tokens: Some(5),
            output_tokens: Some(5),
            reported_micro: Some(777),
            citations: vec![],
            tool_calls: 0,
        };
        assert_eq!(
            actual_micro(&reported, &PRICES, 0),
            Some((777, CostBasis::Reported))
        );
        let usage = Parsed {
            reported_micro: None,
            input_tokens: Some(1_000_000),
            output_tokens: Some(500_000),
            ..reported.clone()
        };
        assert_eq!(
            actual_micro(&usage, &PRICES, 2),
            Some((2_000_000 + 4_000_000 + 20_000, CostBasis::Estimated))
        );
        let half = Parsed {
            input_tokens: Some(10),
            output_tokens: None,
            reported_micro: None,
            ..reported
        };
        assert_eq!(
            actual_micro(&half, &PRICES, 0),
            None,
            "half the usage is not enough to price it"
        );
    }

    const OPEN: [(Provider, &str, &str, &str); 3] = [
        (
            Provider::DeepSeek,
            "deepseek-v4-flash",
            "https://api.deepseek.com/chat/completions",
            "deepseek-api",
        ),
        (
            Provider::Kimi,
            "kimi-k3",
            "https://api.moonshot.ai/v1/chat/completions",
            "kimi-api",
        ),
        (
            Provider::OpenRouter,
            "deepseek/deepseek-v4-flash",
            "https://openrouter.ai/api/v1/chat/completions",
            "openrouter-api",
        ),
    ];

    #[test]
    fn openrouter_searches_through_its_web_plugin_on_a_paid_model_only_and_reports_the_pages() {
        let mut j = job("anthropic/claude-haiku-5.5");
        j.web_search = true;
        let token_bounds = Bounds {
            max_tool_calls: 0,
            ..BOUNDS
        };
        let b = build(Provider::OpenRouter, &j, &token_bounds, &PRICES).unwrap();
        assert_eq!(b.body["plugins"][0]["id"], "web");
        assert_eq!(b.body["plugins"][0]["engine"], "exa");
        assert_eq!(b.body["provider"]["data_collection"], "deny");
        for free in ["openrouter/free", "meta-llama/llama-4-scout:free"] {
            let mut f = job(free);
            f.web_search = true;
            assert!(
                matches!(
                    build(Provider::OpenRouter, &f, &BOUNDS, &PRICES),
                    Err(Refusal::ToolsNotAllowed(_))
                ),
                "{free}"
            );
        }
        assert!(build(
            Provider::OpenRouter,
            &job("anthropic/claude-haiku-5.5"),
            &BOUNDS,
            &PRICES
        )
        .unwrap()
        .body
        .get("plugins")
        .is_none());
        let answer = json!({"choices": [{"message": {"content": "Findings.", "annotations": [
            {"type": "url_citation", "url_citation": {"url": "https://example.com/a", "title": "A", "content": "x", "start_index": 0, "end_index": 3}},
            {"type": "other"}]}}], "usage": {"prompt_tokens": 10, "completion_tokens": 5, "cost": 0.0091}});
        let p = parse(Provider::OpenRouter, &answer).unwrap();
        assert_eq!(
            p.citations,
            vec![Citation {
                url: "https://example.com/a".into(),
                title: "A".into()
            }]
        );
        assert_eq!(
            p.reported_micro,
            Some(9100),
            "the search fee is in the reported cost"
        );
    }

    #[test]
    fn deepseek_kimi_and_openrouter_have_fixed_addresses_routes_and_credentials() {
        for (p, model, url, cred) in OPEN {
            let b = build(p, &job(model), &BOUNDS, &PRICES).unwrap();
            assert_eq!(
                (b.url.as_str(), b.credential, b.auth_header, b.auth_bearer),
                (url, cred, "Authorization", true),
                "{p:?}"
            );
            assert_eq!(Provider::for_route(p.route()), Some(p));
            assert_eq!(
                b.body["messages"][0],
                json!({"role": "system", "content": "Be brief."})
            );
            assert_eq!(b.body["stream"], false);
            assert!(
                b.url.starts_with(&format!("https://{}/", p.host())),
                "{}",
                b.url
            );
            assert!(b
                .headers
                .iter()
                .all(|(k, _)| !k.eq_ignore_ascii_case("authorization")));
        }
        assert_eq!(
            build(
                Provider::DeepSeek,
                &job("deepseek-v4-flash"),
                &BOUNDS,
                &PRICES
            )
            .unwrap()
            .body["max_tokens"],
            1000
        );
        let k = build(Provider::Kimi, &job("kimi-k3"), &BOUNDS, &PRICES).unwrap();
        assert_eq!(k.body["max_completion_tokens"], 1000);
        assert!(
            k.body.get("max_tokens").is_none(),
            "max_tokens is deprecated on this API"
        );
        let hosts: std::collections::BTreeSet<&str> = [
            Provider::OpenAi,
            Provider::Xai,
            Provider::Gemini,
            Provider::DeepSeek,
            Provider::Kimi,
            Provider::OpenRouter,
        ]
        .iter()
        .map(|p| p.host())
        .collect();
        assert_eq!(hosts.len(), 6, "each provider has its own host");
    }

    #[test]
    fn keeping_nothing_asks_openrouter_for_zero_retention_and_refuses_every_other_provider_and_search(
    ) {
        let mut j = job("moonshotai/kimi-k3");
        j.no_retention = true;
        let b = build(Provider::OpenRouter, &j, &BOUNDS, &PRICES).unwrap();
        assert_eq!(
            b.body["provider"],
            json!({"allow_fallbacks": false, "data_collection": "deny", "zdr": true})
        );
        let mut a = job("claude-sonnet-5-5");
        a.no_retention = true;
        assert_eq!(
            build(Provider::Anthropic, &a, &BOUNDS, &PRICES),
            Err(Refusal::CannotKeepNothing("Anthropic"))
        );
        j.web_search = true;
        assert_eq!(
            build(Provider::OpenRouter, &j, &BOUNDS, &PRICES),
            Err(Refusal::CannotKeepNothing("a web search"))
        );
    }

    #[test]
    fn pictures_go_to_openrouter_as_image_parts_and_count_toward_the_worst_case() {
        let text_only = build(
            Provider::OpenRouter,
            &job("moonshotai/kimi-k3"),
            &BOUNDS,
            &PRICES,
        )
        .unwrap();
        let mut j = job("moonshotai/kimi-k3");
        j.images = vec!["data:image/png;base64,iVBORw==".into()];
        let b = build(Provider::OpenRouter, &j, &BOUNDS, &PRICES).unwrap();
        let user = b.body["messages"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .clone();
        assert_eq!(user["content"][0]["type"], "text");
        assert_eq!(
            user["content"][1]["image_url"]["url"],
            "data:image/png;base64,iVBORw=="
        );
        assert!(
            b.worst_case_micro > text_only.worst_case_micro,
            "a picture raises the worst case"
        );
    }

    #[test]
    fn openrouter_requests_forbid_rerouting_and_data_collection() {
        let b = build(
            Provider::OpenRouter,
            &job("moonshotai/kimi-k3"),
            &BOUNDS,
            &PRICES,
        )
        .unwrap();
        assert_eq!(
            b.body["provider"],
            json!({"allow_fallbacks": false, "data_collection": "deny"})
        );
        assert_eq!(b.body["model"], "moonshotai/kimi-k3");
        assert_eq!(b.body["max_tokens"], 1000);
    }

    #[test]
    fn openrouter_model_names_are_checked_and_excluded_models_cannot_be_reached_through_it() {
        for bad in [
            "openai/gpt-6-astra",
            "anthropic/claude-fable-5",
            "x",
            "a/b/c",
            "a b/c",
            "a/../c",
            "/x",
            "x/",
            "UPPER/Case",
            "a/b?x=1",
            "a/b\nc",
        ] {
            assert_eq!(
                build(Provider::OpenRouter, &job(bad), &BOUNDS, &PRICES),
                Err(Refusal::UnknownModel(bad.into())),
                "{bad:?}"
            );
        }
        for good in [
            "deepseek/deepseek-v4-flash",
            "moonshotai/kimi-k3",
            "meta-llama/llama-4-maverick",
            "qwen/qwen3-coder:free",
        ] {
            assert!(
                build(Provider::OpenRouter, &job(good), &BOUNDS, &PRICES).is_ok(),
                "{good}"
            );
        }
    }

    #[test]
    fn the_new_providers_take_no_tools_and_need_models_on_their_own_route() {
        for (p, model, _, _) in OPEN
            .into_iter()
            .filter(|(p, ..)| *p != Provider::OpenRouter)
        {
            let mut j = job(model);
            j.web_search = true;
            assert!(
                matches!(
                    build(p, &j, &BOUNDS, &PRICES),
                    Err(Refusal::ToolsNotAllowed(_))
                ),
                "{p:?}"
            );
        }
        assert_eq!(
            build(Provider::DeepSeek, &job("kimi-k3"), &BOUNDS, &PRICES),
            Err(Refusal::UnknownModel("kimi-k3".into()))
        );
        assert_eq!(
            build(Provider::Kimi, &job("deepseek-v4-flash"), &BOUNDS, &PRICES),
            Err(Refusal::UnknownModel("deepseek-v4-flash".into()))
        );
    }

    #[test]
    fn answers_from_the_new_providers_parse_and_openrouter_reports_its_own_cost() {
        let plain = json!({"choices": [{"message": {"role": "assistant", "content": "hi"}}], "usage": {"prompt_tokens": 9, "completion_tokens": 12, "total_tokens": 21}});
        for p in [Provider::DeepSeek, Provider::Kimi, Provider::OpenRouter] {
            let r = parse(p, &plain).unwrap();
            assert_eq!(
                (
                    r.text.as_str(),
                    r.input_tokens,
                    r.output_tokens,
                    r.reported_micro
                ),
                ("hi", Some(9), Some(12), None),
                "{p:?}"
            );
        }
        let with_cost = json!({"choices": [{"message": {"content": "hi"}}], "usage": {"prompt_tokens": 1, "completion_tokens": 1, "cost": 0.00123}});
        assert_eq!(
            parse(Provider::OpenRouter, &with_cost)
                .unwrap()
                .reported_micro,
            Some(1230)
        );
        assert_eq!(
            parse(Provider::DeepSeek, &with_cost)
                .unwrap()
                .reported_micro,
            None,
            "only OpenRouter reports a cost this way"
        );
        let reasoning_only =
            json!({"choices": [{"message": {"content": "", "reasoning_content": "thinking..."}}]});
        assert!(
            parse(Provider::DeepSeek, &reasoning_only).is_err(),
            "reasoning is not an answer"
        );
        assert!(parse(Provider::Kimi, &json!({"choices": []})).is_err());
    }
}
