//! Time: an injectable clock and the duration spelling used by node configs.
//!
//! The clock is a trait so a test can advance time without sleeping — the
//! approval timeout in §7.4 is otherwise untestable in a unit test.

use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};

/// Where "now" comes from.
pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

/// The wall clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

/// A clock a test moves by hand.
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<DateTime<Utc>>,
}

impl ManualClock {
    pub fn at(now: DateTime<Utc>) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    pub fn advance(&self, by: Duration) {
        let mut now = self.now.lock().expect("clock lock");
        *now += chrono::Duration::from_std(by).expect("a representable duration");
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.now.lock().expect("clock lock")
    }
}

/// Parse a human duration: `500ms`, `30s`, `5m`, `2h`, `7d`, `1w`, and compounds
/// like `1h30m`.
///
/// Deliberately not ISO 8601: node configs are written by hand and by a model,
/// and `"30m"` is less error-prone than `"PT30M"`. ISO input is accepted too
/// (see [`parse_duration`]'s handling of a leading `PT`), because BPMN's
/// `timeDuration` uses it and a value copied from a BPMN file should not be
/// rejected.
pub fn parse_duration(input: &str) -> Option<Duration> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }

    // ISO 8601 duration: only the day/hour/minute/second forms BPMN uses.
    if let Some(rest) = trimmed.strip_prefix("PT").or_else(|| trimmed.strip_prefix("pt")) {
        return parse_iso_time(rest);
    }
    if let Some(rest) = trimmed.strip_prefix('P').or_else(|| trimmed.strip_prefix('p')) {
        return parse_iso_date(rest);
    }

    let mut total = Duration::ZERO;
    let mut rest = trimmed;
    let mut saw_any = false;
    while !rest.is_empty() {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return None;
        }
        rest = &rest[digits.len()..];
        let unit_len = rest.chars().take_while(|c| c.is_ascii_alphabetic()).count();
        if unit_len == 0 {
            return None;
        }
        let unit = &rest[..unit_len];
        rest = &rest[unit_len..];

        let value: u64 = digits.parse().ok()?;
        let millis = match unit {
            "ms" => value,
            "s" => value.checked_mul(1_000)?,
            "m" => value.checked_mul(60_000)?,
            "h" => value.checked_mul(3_600_000)?,
            "d" => value.checked_mul(86_400_000)?,
            "w" => value.checked_mul(604_800_000)?,
            _ => return None,
        };
        total = total.checked_add(Duration::from_millis(millis))?;
        saw_any = true;
    }
    saw_any.then_some(total)
}

fn parse_iso_time(rest: &str) -> Option<Duration> {
    let mut total = Duration::ZERO;
    let mut rest = rest;
    let mut saw_any = false;
    while !rest.is_empty() {
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return None;
        }
        rest = &rest[digits.len()..];
        let mut chars = rest.chars();
        let unit = chars.next()?;
        rest = chars.as_str();
        let value: u64 = digits.parse().ok()?;
        let millis = match unit {
            'H' => value.checked_mul(3_600_000)?,
            'M' => value.checked_mul(60_000)?,
            'S' => value.checked_mul(1_000)?,
            _ => return None,
        };
        total = total.checked_add(Duration::from_millis(millis))?;
        saw_any = true;
    }
    saw_any.then_some(total)
}

fn parse_iso_date(rest: &str) -> Option<Duration> {
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || !rest[digits.len()..].starts_with('D') {
        return None;
    }
    let days: u64 = digits.parse().ok()?;
    Some(Duration::from_millis(days.checked_mul(86_400_000)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_units_parse() {
        assert_eq!(parse_duration("500ms"), Some(Duration::from_millis(500)));
        assert_eq!(parse_duration("30s"), Some(Duration::from_secs(30)));
        assert_eq!(parse_duration("5m"), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("2h"), Some(Duration::from_secs(7_200)));
        assert_eq!(parse_duration("7d"), Some(Duration::from_secs(604_800)));
    }

    #[test]
    fn compounds_add_up() {
        assert_eq!(
            parse_duration("1h30m"),
            Some(Duration::from_secs(5_400))
        );
    }

    #[test]
    fn iso_forms_from_bpmn_are_accepted() {
        assert_eq!(parse_duration("PT1H"), Some(Duration::from_secs(3_600)));
        assert_eq!(parse_duration("PT30M"), Some(Duration::from_secs(1_800)));
        assert_eq!(parse_duration("P2D"), Some(Duration::from_secs(172_800)));
    }

    #[test]
    fn junk_is_rejected_rather_than_defaulted() {
        // A typo must not silently become "no timeout".
        for bad in ["", "  ", "30", "h", "1x", "PT", "-5m", "1.5h"] {
            assert_eq!(parse_duration(bad), None, "{bad:?} should not parse");
        }
    }

    #[test]
    fn a_manual_clock_moves_without_sleeping() {
        let clock = ManualClock::at(Utc::now());
        let before = clock.now();
        clock.advance(Duration::from_secs(60));
        assert_eq!((clock.now() - before).num_seconds(), 60);
    }
}
