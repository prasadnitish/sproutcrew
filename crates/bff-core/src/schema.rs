//! A small JSON Schema validator for the fixed schemas M0 uses to check structured output
//! (`--json-schema`, `outputSchema`). Supports `type`, `enum`, `const`, `properties`, `required`,
//! `additionalProperties: false`, `items`, `minimum`, `maximum`, `minLength`, and `minItems`.

use serde_json::Value;

/// Every violation found, as `path: message`. Empty means valid.
pub fn validate(schema: &Value, value: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    check(schema, value, "$", &mut errors);
    errors
}

fn type_matches(ty: &str, v: &Value) -> bool {
    match ty {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        "number" => v.is_number(),
        "integer" => v.is_i64() || v.is_u64(),
        _ => false,
    }
}

fn check(schema: &Value, v: &Value, path: &str, errors: &mut Vec<String>) {
    if let Some(ty) = schema.get("type") {
        let allowed: Vec<&str> = match ty {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        if !allowed.iter().any(|t| type_matches(t, v)) {
            errors.push(format!("{path}: expected type {allowed:?}"));
            return;
        }
    }
    if let Some(c) = schema.get("const") {
        if c != v {
            errors.push(format!("{path}: must equal the constant"));
        }
    }
    if let Some(e) = schema.get("enum").and_then(Value::as_array) {
        if !e.contains(v) {
            errors.push(format!("{path}: not one of the allowed values"));
        }
    }
    if let Some(n) = v.as_f64() {
        if schema
            .get("minimum")
            .and_then(Value::as_f64)
            .is_some_and(|m| n < m)
        {
            errors.push(format!("{path}: below minimum"));
        }
        if schema
            .get("maximum")
            .and_then(Value::as_f64)
            .is_some_and(|m| n > m)
        {
            errors.push(format!("{path}: above maximum"));
        }
    }
    if let Some(s) = v.as_str() {
        if schema
            .get("minLength")
            .and_then(Value::as_u64)
            .is_some_and(|m| (s.chars().count() as u64) < m)
        {
            errors.push(format!("{path}: shorter than minLength"));
        }
    }
    if let Some(obj) = v.as_object() {
        let props = schema.get("properties").and_then(Value::as_object);
        for req in schema
            .get("required")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !obj.contains_key(req) {
                errors.push(format!("{path}: {req} is required"));
            }
        }
        for (k, child) in obj {
            match props.and_then(|p| p.get(k)) {
                Some(sub) => check(sub, child, &format!("{path}.{k}"), errors),
                None if schema.get("additionalProperties") == Some(&Value::Bool(false)) => {
                    errors.push(format!("{path}: additional property {k}"));
                }
                None => {}
            }
        }
    }
    if let Some(arr) = v.as_array() {
        if schema
            .get("minItems")
            .and_then(Value::as_u64)
            .is_some_and(|m| (arr.len() as u64) < m)
        {
            errors.push(format!("{path}: fewer than minItems"));
        }
        if let Some(items) = schema.get("items") {
            for (i, child) in arr.iter().enumerate() {
                check(items, child, &format!("{path}[{i}]"), errors);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn schema() -> Value {
        json!({"type": "object", "additionalProperties": false, "required": ["verdict", "count"],
               "properties": {"verdict": {"type": "string", "enum": ["pass", "fail"]},
                              "count": {"type": "integer", "minimum": 0},
                              "tags": {"type": "array", "items": {"type": "string"}, "minItems": 1}}})
    }

    #[test]
    fn valid_values_pass() {
        assert!(validate(&schema(), &json!({"verdict": "pass", "count": 7})).is_empty());
        assert!(validate(
            &schema(),
            &json!({"verdict": "fail", "count": 0, "tags": ["a"]})
        )
        .is_empty());
    }

    #[test]
    fn missing_required_and_extra_keys_are_reported() {
        let e = validate(&schema(), &json!({"verdict": "pass", "extra": 1}));
        assert!(
            e.iter()
                .any(|m| m.contains("count") && m.contains("required")),
            "{e:?}"
        );
        assert!(
            e.iter()
                .any(|m| m.contains("extra") && m.contains("additional")),
            "{e:?}"
        );
    }

    #[test]
    fn wrong_types_enums_and_bounds_are_reported() {
        assert!(!validate(&schema(), &json!({"verdict": "maybe", "count": 1})).is_empty());
        assert!(!validate(&schema(), &json!({"verdict": "pass", "count": "7"})).is_empty());
        assert!(!validate(&schema(), &json!({"verdict": "pass", "count": 7.5})).is_empty());
        assert!(!validate(&schema(), &json!({"verdict": "pass", "count": -1})).is_empty());
        assert!(!validate(
            &schema(),
            &json!({"verdict": "pass", "count": 1, "tags": []})
        )
        .is_empty());
        assert!(!validate(
            &schema(),
            &json!({"verdict": "pass", "count": 1, "tags": [3]})
        )
        .is_empty());
    }

    #[test]
    fn non_objects_fail_an_object_schema() {
        assert!(!validate(&schema(), &json!("pass")).is_empty());
        assert!(!validate(&schema(), &json!(null)).is_empty());
    }

    #[test]
    fn type_arrays_and_const_work() {
        let s = json!({"type": ["string", "null"]});
        assert!(validate(&s, &json!(null)).is_empty());
        assert!(!validate(&s, &json!(1)).is_empty());
        assert!(validate(&json!({"const": "x"}), &json!("x")).is_empty());
        assert!(!validate(&json!({"const": "x"}), &json!("y")).is_empty());
    }

    #[test]
    fn errors_name_the_path() {
        let e = validate(
            &schema(),
            &json!({"verdict": "pass", "count": 1, "tags": [1]}),
        );
        assert!(e[0].starts_with("$.tags[0]"), "{e:?}");
    }
}
