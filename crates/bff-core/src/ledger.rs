//! Usage accounting by adapter (plan section 9). Unknown is not zero.

use crate::codex_types::TokenUsageBreakdown;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Reported,
    Estimated,
    Unknown,
}

// ---------------------------------------------------------------- Codex

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CodexTokens {
    pub input: i64,
    /// A breakdown of `input`, not an addition to it.
    pub cached_input: i64,
    pub output: i64,
    /// A breakdown of `output`, not an addition to it.
    pub reasoning_output: i64,
    pub total: i64,
}

impl CodexTokens {
    pub fn uncached_input(&self) -> i64 {
        self.input - self.cached_input
    }
}

fn tokens_of(b: &TokenUsageBreakdown) -> CodexTokens {
    CodexTokens {
        input: b.input_tokens,
        cached_input: b.cached_input_tokens,
        output: b.output_tokens,
        reasoning_output: b.reasoning_output_tokens,
        total: b.total_tokens,
    }
}

#[derive(Debug, Default)]
pub struct CodexTokenLedger {
    baseline: BTreeMap<String, CodexTokens>,
    latest: BTreeMap<String, CodexTokens>,
    reroutes: BTreeMap<String, Vec<(String, String)>>,
}

impl CodexTokenLedger {
    /// The cumulative total a thread already carried when the attempt began, such as a resume replay.
    pub fn set_baseline(&mut self, thread: &str, total: &TokenUsageBreakdown) {
        self.baseline.insert(thread.to_string(), tokens_of(total));
    }

    /// `thread/tokenUsage/updated`. Only the cumulative `total` is used. A repeat replaces the last one.
    pub fn on_update(&mut self, thread: &str, total: &TokenUsageBreakdown) {
        self.latest.insert(thread.to_string(), tokens_of(total));
    }

    /// `None` means no usage was ever reported, or the totals went backwards. Both are unknown.
    pub fn delta(&self, thread: &str) -> Option<CodexTokens> {
        let now = self.latest.get(thread)?;
        let base = self.baseline.get(thread).copied().unwrap_or_default();
        let d = CodexTokens {
            input: now.input - base.input,
            cached_input: now.cached_input - base.cached_input,
            output: now.output - base.output,
            reasoning_output: now.reasoning_output - base.reasoning_output,
            total: now.total - base.total,
        };
        let backwards = [
            d.input,
            d.cached_input,
            d.output,
            d.reasoning_output,
            d.total,
        ]
        .iter()
        .any(|v| *v < 0);
        (!backwards).then_some(d)
    }

    pub fn aggregate(&self) -> Option<CodexTokens> {
        self.latest
            .keys()
            .filter_map(|t| self.delta(t))
            .fold(None, |acc, d| {
                let a = acc.unwrap_or_default();
                Some(CodexTokens {
                    input: a.input + d.input,
                    cached_input: a.cached_input + d.cached_input,
                    output: a.output + d.output,
                    reasoning_output: a.reasoning_output + d.reasoning_output,
                    total: a.total + d.total,
                })
            })
    }

    pub fn record_reroute(&mut self, thread: &str, from: &str, to: &str) {
        self.reroutes
            .entry(thread.to_string())
            .or_default()
            .push((from.to_string(), to.to_string()));
    }

    /// A reroute means the model split of the tokens cannot be known reliably.
    pub fn model_split_uncertain(&self, thread: &str) -> bool {
        self.reroutes.get(thread).is_some_and(|r| !r.is_empty())
    }
}

// --------------------------------------------------------------- Claude

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClaudeTokens {
    pub input: i64,
    pub output: i64,
    pub cache_creation: i64,
    pub cache_read: i64,
}

#[derive(Debug, Clone, Default)]
pub struct Baseline {
    pub cost_usd: Option<f64>,
    pub tokens: Option<ClaudeTokens>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeReport {
    pub tokens: Option<ClaudeTokens>,
    pub tokens_status: Status,
    pub cost_usd: Option<f64>,
    pub cost_status: Status,
    pub per_model: BTreeMap<String, ClaudeTokens>,
    pub result_distrusted: bool,
}

struct ResultSummary {
    trusted: bool,
    /// False when the result carried no usage at all, which is unknown and not zero.
    has_usage: bool,
    tokens: ClaudeTokens,
    cost_usd: Option<f64>,
    per_model: BTreeMap<String, ClaudeTokens>,
}

pub struct ClaudeLedger {
    seen: HashSet<String>,
    streamed: ClaudeTokens,
    /// Usage from `stream_event`s, per message ID, for messages that never completed.
    partials: BTreeMap<String, ClaudeTokens>,
    current: Option<String>,
    baseline: Baseline,
    result: Option<ResultSummary>,
}

fn int(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_i64).unwrap_or(0)
}

fn is_zero(t: &ClaudeTokens) -> bool {
    *t == ClaudeTokens::default()
}

impl ClaudeLedger {
    pub fn new(baseline: Baseline) -> Self {
        ClaudeLedger {
            seen: HashSet::new(),
            streamed: ClaudeTokens::default(),
            partials: BTreeMap::new(),
            current: None,
            baseline,
            result: None,
        }
    }

    /// An `assistant` message. Parallel tool calls repeat one message ID with identical usage.
    pub fn on_assistant(&mut self, message_id: &str, usage: &Value) {
        if !self.seen.insert(message_id.to_string()) {
            return;
        }
        // The per-message output in a complete message is a placeholder, so keep the larger streamed count.
        let streamed_out = self.partials.get(message_id).map_or(0, |p| p.output);
        self.streamed.input += int(usage, "input_tokens");
        self.streamed.output += int(usage, "output_tokens").max(streamed_out);
        self.streamed.cache_creation += int(usage, "cache_creation_input_tokens");
        self.streamed.cache_read += int(usage, "cache_read_input_tokens");
    }

    /// A `stream_event` payload. Usage in `message_start` and `message_delta` stands in for a complete
    /// `assistant` message when a turn is cut off before one arrives.
    pub fn on_stream_event(&mut self, event: &Value) {
        match event.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                let Some(id) = event["message"]["id"].as_str() else {
                    return;
                };
                let u = &event["message"]["usage"];
                self.partials.insert(
                    id.to_string(),
                    ClaudeTokens {
                        input: int(u, "input_tokens"),
                        output: int(u, "output_tokens"),
                        cache_creation: int(u, "cache_creation_input_tokens"),
                        cache_read: int(u, "cache_read_input_tokens"),
                    },
                );
                self.current = Some(id.to_string());
            }
            Some("message_delta") => {
                let Some(p) = self
                    .current
                    .as_ref()
                    .and_then(|id| self.partials.get_mut(id))
                else {
                    return;
                };
                let u = &event["usage"];
                p.output = p.output.max(int(u, "output_tokens"));
                p.input = p.input.max(int(u, "input_tokens"));
            }
            _ => {}
        }
    }

    /// Complete messages plus partial ones that never completed.
    fn streamed_total(&self) -> ClaudeTokens {
        self.partials
            .iter()
            .filter(|(id, _)| !self.seen.contains(*id))
            .fold(self.streamed, |a, (_, p)| ClaudeTokens {
                input: a.input + p.input,
                output: a.output + p.output,
                cache_creation: a.cache_creation + p.cache_creation,
                cache_read: a.cache_read + p.cache_read,
            })
    }

    pub fn on_result(&mut self, result: &Value) {
        let mut per_model = BTreeMap::new();
        if let Some(models) = result.get("modelUsage").and_then(Value::as_object) {
            for (name, m) in models {
                per_model.insert(
                    name.clone(),
                    ClaudeTokens {
                        input: int(m, "inputTokens"),
                        output: int(m, "outputTokens"),
                        cache_creation: int(m, "cacheCreationInputTokens"),
                        cache_read: int(m, "cacheReadInputTokens"),
                    },
                );
            }
        }
        let tokens = if per_model.is_empty() {
            let u = result.get("usage").cloned().unwrap_or(Value::Null);
            ClaudeTokens {
                input: int(&u, "input_tokens"),
                output: int(&u, "output_tokens"),
                cache_creation: int(&u, "cache_creation_input_tokens"),
                cache_read: int(&u, "cache_read_input_tokens"),
            }
        } else {
            per_model
                .values()
                .fold(ClaudeTokens::default(), |a, m| ClaudeTokens {
                    input: a.input + m.input,
                    output: a.output + m.output,
                    cache_creation: a.cache_creation + m.cache_creation,
                    cache_read: a.cache_read + m.cache_read,
                })
        };
        let cost_usd = result.get("total_cost_usd").and_then(Value::as_f64);
        let is_error = result
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let zeroed = is_zero(&tokens) && cost_usd.unwrap_or(0.0) == 0.0;
        let trusted = !(zeroed && (is_error || !is_zero(&self.streamed_total())));
        let has_usage = !per_model.is_empty() || result.get("usage").is_some_and(Value::is_object);
        self.result = Some(ResultSummary {
            trusted,
            has_usage,
            tokens,
            cost_usd,
            per_model,
        });
    }

    pub fn report(&self) -> ClaudeReport {
        let unknown = ClaudeReport {
            tokens: None,
            tokens_status: Status::Unknown,
            cost_usd: None,
            cost_status: Status::Unknown,
            per_model: BTreeMap::new(),
            result_distrusted: false,
        };
        match &self.result {
            Some(r) if r.trusted => {
                let tokens = match self.baseline.tokens {
                    None => Some(r.tokens),
                    Some(b) => {
                        let d = ClaudeTokens {
                            input: r.tokens.input - b.input,
                            output: r.tokens.output - b.output,
                            cache_creation: r.tokens.cache_creation - b.cache_creation,
                            cache_read: r.tokens.cache_read - b.cache_read,
                        };
                        [d.input, d.output, d.cache_creation, d.cache_read]
                            .iter()
                            .all(|v| *v >= 0)
                            .then_some(d)
                    }
                };
                let cost_usd = r
                    .cost_usd
                    .map(|c| c - self.baseline.cost_usd.unwrap_or(0.0))
                    .filter(|c| *c >= 0.0);
                let (tokens, tokens_status) = if !r.has_usage {
                    // A result with no usage is unknown, never zero. Streamed usage is the fallback.
                    if is_zero(&self.streamed_total()) {
                        (None, Status::Unknown)
                    } else {
                        (Some(self.streamed_total()), Status::Estimated)
                    }
                } else if tokens.is_some() {
                    (tokens, Status::Reported)
                } else {
                    (None, Status::Unknown)
                };
                ClaudeReport {
                    tokens_status,
                    tokens,
                    cost_status: if cost_usd.is_some() {
                        Status::Estimated
                    } else {
                        Status::Unknown
                    },
                    cost_usd,
                    per_model: r.per_model.clone(),
                    result_distrusted: false,
                }
            }
            other => {
                let distrusted = other.is_some();
                if is_zero(&self.streamed_total()) {
                    return ClaudeReport {
                        result_distrusted: distrusted,
                        ..unknown
                    };
                }
                ClaudeReport {
                    tokens: Some(self.streamed_total()),
                    tokens_status: Status::Estimated,
                    result_distrusted: distrusted,
                    ..unknown
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bd(input: i64, cached: i64, output: i64, reasoning: i64, total: i64) -> TokenUsageBreakdown {
        serde_json::from_value(json!({
            "inputTokens": input, "cachedInputTokens": cached, "outputTokens": output,
            "reasoningOutputTokens": reasoning, "totalTokens": total
        }))
        .unwrap()
    }

    // ---- Codex

    #[test]
    fn successive_totals_are_replaced_not_summed() {
        let mut l = CodexTokenLedger::default();
        l.on_update("t1", &bd(100, 0, 10, 0, 110));
        l.on_update("t1", &bd(250, 0, 30, 0, 280));
        assert_eq!(l.delta("t1").unwrap().total, 280);
    }

    #[test]
    fn duplicate_snapshot_changes_nothing() {
        let mut l = CodexTokenLedger::default();
        l.on_update("t1", &bd(100, 0, 10, 0, 110));
        l.on_update("t1", &bd(100, 0, 10, 0, 110));
        assert_eq!(l.delta("t1").unwrap().total, 110);
    }

    #[test]
    fn resume_replay_baseline_is_not_counted_again() {
        let mut l = CodexTokenLedger::default();
        l.set_baseline("t1", &bd(1000, 0, 100, 0, 1100));
        l.on_update("t1", &bd(1000, 0, 100, 0, 1100)); // replayed on resume
        assert_eq!(l.delta("t1").unwrap().total, 0);
        l.on_update("t1", &bd(1150, 0, 160, 0, 1310));
        let d = l.delta("t1").unwrap();
        assert_eq!((d.input, d.output, d.total), (150, 60, 210));
    }

    #[test]
    fn reasoning_is_not_added_to_output_and_cached_is_part_of_input() {
        let mut l = CodexTokenLedger::default();
        l.on_update("t1", &bd(100, 40, 50, 20, 150));
        let d = l.delta("t1").unwrap();
        assert_eq!(d.output, 50);
        assert_eq!(d.reasoning_output, 20);
        assert_eq!(d.uncached_input(), 60);
        assert_eq!(d.total, 150);
    }

    #[test]
    fn no_usage_is_unknown_not_zero() {
        let l = CodexTokenLedger::default();
        assert_eq!(l.delta("t1"), None);
        assert_eq!(l.aggregate(), None);
    }

    #[test]
    fn totals_going_backwards_are_unknown() {
        let mut l = CodexTokenLedger::default();
        l.set_baseline("t1", &bd(1000, 0, 100, 0, 1100));
        l.on_update("t1", &bd(10, 0, 1, 0, 11));
        assert_eq!(l.delta("t1"), None);
    }

    #[test]
    fn aggregate_sums_threads_that_have_usage() {
        let mut l = CodexTokenLedger::default();
        l.on_update("t1", &bd(100, 0, 10, 0, 110));
        l.on_update("t2", &bd(200, 0, 20, 0, 220));
        assert_eq!(l.aggregate().unwrap().total, 330);
    }

    #[test]
    fn reroute_marks_the_model_split_uncertain() {
        let mut l = CodexTokenLedger::default();
        assert!(!l.model_split_uncertain("t1"));
        l.record_reroute("t1", "gpt-6-luna", "gpt-6-fallback");
        assert!(l.model_split_uncertain("t1"));
        assert!(!l.model_split_uncertain("t2"));
    }

    // ---- Claude

    fn usage(i: i64, o: i64, cc: i64, cr: i64) -> Value {
        json!({"input_tokens": i, "output_tokens": o,
               "cache_creation_input_tokens": cc, "cache_read_input_tokens": cr})
    }

    fn result(cost: f64, u: Value, model_usage: Value) -> Value {
        json!({"type": "result", "subtype": "success", "is_error": false,
               "total_cost_usd": cost, "usage": u, "modelUsage": model_usage})
    }

    fn model_usage(model: &str, i: i64, o: i64, cc: i64, cr: i64) -> Value {
        json!({ model: {"inputTokens": i, "outputTokens": o,
                        "cacheCreationInputTokens": cc, "cacheReadInputTokens": cr, "costUSD": 0.01}})
    }

    fn start(id: &str, i: i64, cc: i64, cr: i64, o: i64) -> Value {
        json!({"type": "message_start", "message": {"id": id, "usage":
            {"input_tokens": i, "cache_creation_input_tokens": cc, "cache_read_input_tokens": cr, "output_tokens": o}}})
    }

    fn delta(o: i64) -> Value {
        json!({"type": "message_delta", "delta": {}, "usage": {"output_tokens": o}})
    }

    #[test]
    fn an_interrupted_turn_recovers_its_usage_from_the_partial_stream() {
        let mut l = ClaudeLedger::new(Baseline::default());
        l.on_stream_event(&start("m1", 100, 20, 5, 1));
        l.on_stream_event(&delta(37));
        l.on_result(
            &json!({"type": "result", "subtype": "error_during_execution", "is_error": true,
                            "total_cost_usd": 0, "usage": usage(0, 0, 0, 0)}),
        );
        let r = l.report();
        let t = r.tokens.unwrap();
        assert_eq!(
            (t.input, t.output, t.cache_creation, t.cache_read),
            (100, 37, 20, 5)
        );
        assert_eq!(
            (r.tokens_status, r.result_distrusted),
            (Status::Estimated, true)
        );
        assert_eq!((r.cost_usd, r.cost_status), (None, Status::Unknown));
    }

    #[test]
    fn a_complete_message_supersedes_its_partial_but_keeps_the_larger_output() {
        let mut l = ClaudeLedger::new(Baseline::default());
        l.on_stream_event(&start("m1", 100, 0, 0, 1));
        l.on_stream_event(&delta(40));
        l.on_assistant("m1", &usage(100, 1, 0, 0));
        let t = l.report().tokens.unwrap();
        assert_eq!((t.input, t.output), (100, 40));
    }

    #[test]
    fn complete_and_partial_messages_add_up_and_a_stray_delta_is_ignored() {
        let mut l = ClaudeLedger::new(Baseline::default());
        l.on_stream_event(&delta(5)); // no message_start yet
        l.on_assistant("m1", &usage(10, 3, 0, 0));
        l.on_stream_event(&start("m2", 20, 0, 0, 1));
        l.on_stream_event(&delta(9));
        let t = l.report().tokens.unwrap();
        assert_eq!((t.input, t.output), (30, 12));
    }

    #[test]
    fn repeated_message_ids_count_once() {
        let mut l = ClaudeLedger::new(Baseline::default());
        l.on_assistant("msg_1", &usage(10, 1, 5, 0));
        l.on_assistant("msg_1", &usage(10, 1, 5, 0));
        l.on_assistant("msg_2", &usage(20, 1, 0, 5));
        let r = l.report();
        let t = r.tokens.unwrap();
        assert_eq!((t.input, t.cache_creation, t.cache_read), (30, 5, 5));
        assert_eq!(r.tokens_status, Status::Estimated);
    }

    #[test]
    fn result_totals_win_over_streamed_and_use_model_usage() {
        let mut l = ClaudeLedger::new(Baseline::default());
        l.on_assistant("msg_1", &usage(10, 1, 0, 0));
        l.on_result(&result(
            0.0123,
            usage(10, 40, 0, 0),
            model_usage("claude-haiku-5-5", 12, 44, 0, 3),
        ));
        let r = l.report();
        let t = r.tokens.unwrap();
        assert_eq!((t.input, t.output, t.cache_read), (12, 44, 3));
        assert_eq!(r.tokens_status, Status::Reported);
        assert_eq!(r.per_model["claude-haiku-5-5"].output, 44);
        assert_eq!(r.cost_usd, Some(0.0123));
        assert_eq!(r.cost_status, Status::Estimated);
    }

    #[test]
    fn result_falls_back_to_usage_without_model_usage() {
        let mut l = ClaudeLedger::new(Baseline::default());
        l.on_result(
            &json!({"type": "result", "subtype": "success", "is_error": false,
                            "total_cost_usd": 0.01, "usage": usage(7, 8, 0, 0)}),
        );
        assert_eq!(l.report().tokens.unwrap().output, 8);
    }

    #[test]
    fn killed_run_has_streamed_tokens_and_unknown_cost() {
        let mut l = ClaudeLedger::new(Baseline::default());
        l.on_assistant("msg_1", &usage(10, 1, 0, 0));
        let r = l.report();
        assert_eq!(r.tokens_status, Status::Estimated);
        assert_eq!(r.cost_usd, None);
        assert_eq!(r.cost_status, Status::Unknown);
    }

    #[test]
    fn zeroed_crash_result_is_distrusted() {
        let mut l = ClaudeLedger::new(Baseline::default());
        l.on_assistant("msg_1", &usage(10, 1, 0, 0));
        l.on_result(
            &json!({"type": "result", "subtype": "error_during_execution", "is_error": true,
                            "total_cost_usd": 0.0, "usage": usage(0, 0, 0, 0), "modelUsage": {}}),
        );
        let r = l.report();
        assert!(r.result_distrusted);
        assert_eq!(r.tokens.unwrap().input, 10);
        assert_eq!(r.tokens_status, Status::Estimated);
        assert_eq!(r.cost_usd, None);
        assert_eq!(r.cost_status, Status::Unknown);
    }

    #[test]
    fn no_events_at_all_is_unknown() {
        let r = ClaudeLedger::new(Baseline::default()).report();
        assert_eq!(
            (r.tokens, r.tokens_status, r.cost_usd),
            (None, Status::Unknown, None)
        );
    }

    #[test]
    fn resume_cost_is_a_delta_from_the_prior_total() {
        let mut l = ClaudeLedger::new(Baseline {
            cost_usd: Some(0.02),
            tokens: None,
        });
        l.on_result(&result(0.05, usage(1, 1, 0, 0), json!({})));
        let c = l.report().cost_usd.unwrap();
        assert!((c - 0.03).abs() < 1e-9, "{c}");
    }

    #[test]
    fn resume_tokens_subtract_a_cumulative_baseline() {
        let base = ClaudeTokens {
            input: 100,
            output: 50,
            cache_creation: 0,
            cache_read: 10,
        };
        let mut l = ClaudeLedger::new(Baseline {
            cost_usd: None,
            tokens: Some(base),
        });
        l.on_result(&result(
            0.05,
            usage(0, 0, 0, 0),
            model_usage("m", 130, 80, 0, 15),
        ));
        let t = l.report().tokens.unwrap();
        assert_eq!((t.input, t.output, t.cache_read), (30, 30, 5));
    }

    #[test]
    fn cost_below_the_baseline_is_unknown() {
        let mut l = ClaudeLedger::new(Baseline {
            cost_usd: Some(0.5),
            tokens: None,
        });
        l.on_result(&result(0.1, usage(1, 1, 0, 0), json!({})));
        let r = l.report();
        assert_eq!((r.cost_usd, r.cost_status), (None, Status::Unknown));
    }
}
