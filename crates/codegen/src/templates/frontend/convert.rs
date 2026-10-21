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

/// True when the string already carries a UTC offset or zone.
fn has_offset(s: &str) -> bool {
    if s.ends_with('Z') || s.ends_with('z') {
        return true;
    }
    match s.split_once('T') {
        // Only the time part can hold the offset; the date part has
        // dashes of its own.
        Some((_, time)) => time.contains('+') || time.contains('-'),
        None => false,
    }
}

/// Normalise a datetime to RFC 3339 with an offset.
///
/// `<input type="datetime-local">` produces "YYYY-MM-DDTHH:MM" — no zone —
/// but the backend field is a `DateTime<Utc>`, and chrono's deserializer
/// rejects a timestamp without an offset. Supply UTC when the browser did
/// not.
pub fn parse_datetime(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if has_offset(trimmed) {
        return trimmed.to_string();
    }
    let (date, time) = match trimmed.split_once('T') {
        Some((date, time)) => (date, time),
        None => (trimmed, ""),
    };
    if time.is_empty() {
        return format!("{date}T00:00:00Z");
    }
    // "HH:MM" needs the seconds before it can be suffixed with a zone.
    let time = if time.split(':').count() == 2 {
        format!("{time}:00")
    } else {
        time.to_string()
    };
    format!("{date}T{time}Z")
}

pub fn parse_datetime_opt(s: &str) -> Option<String> {
    if s.trim().is_empty() {
        None
    } else {
        Some(parse_datetime(s))
    }
}
