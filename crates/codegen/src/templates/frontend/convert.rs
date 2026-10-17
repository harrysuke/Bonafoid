//! Helpers for converting raw `<input>` strings into typed DTO fields.

pub fn parse_string(s: &str) -> String {
    s.to_string()
}

pub fn parse_string_opt(s: &str) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s.to_string())
    }
}

pub fn parse_i32(s: &str) -> i32 {
    s.trim().parse().unwrap_or_default()
}

pub fn parse_i32_opt(s: &str) -> Option<i32> {
    if s.trim().is_empty() {
        None
    } else {
        s.trim().parse().ok()
    }
}

pub fn parse_i64(s: &str) -> i64 {
    s.trim().parse().unwrap_or_default()
}

pub fn parse_i64_opt(s: &str) -> Option<i64> {
    if s.trim().is_empty() {
        None
    } else {
        s.trim().parse().ok()
    }
}

pub fn parse_f64(s: &str) -> f64 {
    s.trim().parse().unwrap_or_default()
}

pub fn parse_f64_opt(s: &str) -> Option<f64> {
    if s.trim().is_empty() {
        None
    } else {
        s.trim().parse().ok()
    }
}

pub fn parse_bool(s: &str) -> bool {
    matches!(
        s.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "on"
    )
}

pub fn parse_bool_opt(s: &str) -> Option<bool> {
    if s.trim().is_empty() {
        None
    } else {
        Some(parse_bool(s))
    }
}

pub fn parse_json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or(serde_json::Value::Null)
}

pub fn parse_json_opt(s: &str) -> Option<serde_json::Value> {
    if s.trim().is_empty() {
        None
    } else {
        Some(parse_json(s))
    }
}
