use bff_core::{
    connector::{Credential, Credentials, Request, Response, Transport, TransportError},
    openrouter_limits,
};
use serde_json::json;
struct Keys(bool);
impl Credentials for Keys {
    fn get(&self, name: &str) -> Option<Credential> {
        assert_eq!(name, "openrouter-api");
        self.0.then(|| Credential::new("test-only-credential", &[]))
    }
}
struct Get {
    status: u16,
    body: String,
    calls: usize,
}
impl Transport for Get {
    fn get(&mut self, request: &Request) -> Result<Response, TransportError> {
        self.calls += 1;
        assert_eq!(request.url, "https://openrouter.ai/api/v1/key");
        assert_eq!(
            request.headers,
            vec![("Authorization".into(), "Bearer test-only-credential".into())]
        );
        Ok(Response {
            status: self.status,
            body: self.body.clone(),
        })
    }
}
#[test]
fn quota_check_reads_once_and_returns_only_counter_fields() {
    let mut transport=Get{status:200,body:json!({"data":{"label":"private-key-label","limit_remaining":17,"is_free_tier":false,"free_model_daily_requests":{"used":7,"limit":50,"remaining":43}},"private":"test-only-credential"}).to_string(),calls:0};
    let status = openrouter_limits::fetch(&Keys(true), &mut transport, 99).unwrap();
    assert_eq!(transport.calls, 1);
    let view = serde_json::to_value(status).unwrap();
    assert_eq!(
        view,
        json!({"checked_at":99,"daily":{"used":7,"limit":50,"remaining":43}})
    );
    assert!(!view.to_string().contains("private"));
    assert!(!view.to_string().contains("credential"));
}
#[test]
fn missing_malformed_or_inconsistent_counters_stay_unknown() {
    for counters in [
        json!(null),
        json!({}),
        json!({"used":0,"limit":50}),
        json!({"used":0,"limit":50,"remaining":51}),
        json!({"used":2,"limit":50,"remaining":50}),
        json!({"used":-1,"limit":50,"remaining":50}),
        json!({"used":"0","limit":50,"remaining":50}),
    ] {
        let mut transport = Get {
            status: 200,
            body: json!({"data":{"free_model_daily_requests":counters}}).to_string(),
            calls: 0,
        };
        assert!(openrouter_limits::fetch(&Keys(true), &mut transport, 1)
            .unwrap()
            .daily
            .is_none());
    }
    let mut exhausted = Get {
        status: 200,
        body: json!({"data":{"free_model_daily_requests":{"used":51,"limit":50,"remaining":0}}})
            .to_string(),
        calls: 0,
    };
    assert_eq!(
        openrouter_limits::fetch(&Keys(true), &mut exhausted, 1)
            .unwrap()
            .daily
            .unwrap()
            .remaining,
        0
    );
}
#[test]
fn missing_key_and_failures_do_not_echo_response_or_make_generation_calls() {
    let mut transport = Get {
        status: 429,
        body: "test-only-credential private error body".into(),
        calls: 0,
    };
    assert!(openrouter_limits::fetch(&Keys(false), &mut transport, 1).is_err());
    assert_eq!(transport.calls, 0);
    for status in [401, 403, 429, 500] {
        transport.status = status;
        let error = openrouter_limits::fetch(&Keys(true), &mut transport, 1).unwrap_err();
        assert!(!error.contains("credential"));
        assert!(!error.contains("private error"));
    }
    transport.status = 200;
    assert!(openrouter_limits::fetch(&Keys(true), &mut transport, 1)
        .unwrap_err()
        .contains("unreadable"));
}
