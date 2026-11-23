//! Pure billing rules: subscription state machine, proration, seats and
//! entitlement checks.
//!
//! Nothing in this module touches the database, the network or the environment.
//! Keeping the money/authorization arithmetic here makes it cheap to test and
//! impossible for the HTTP layer to silently change its meaning.

use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Lifecycle of a paid subscription, serialized as the snake_case value stored
/// in the database and emitted by the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    Trialing,
    Active,
    PastDue,
    Cancelled,
    Expired,
}

impl SubscriptionStatus {
    /// All statuses, in declaration order.
    pub const ALL: [Self; 5] = [
        Self::Trialing,
        Self::Active,
        Self::PastDue,
        Self::Cancelled,
        Self::Expired,
    ];

    /// Stable storage/API spelling.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Trialing => "trialing",
            Self::Active => "active",
            Self::PastDue => "past_due",
            Self::Cancelled => "cancelled",
            Self::Expired => "expired",
        }
    }

    /// Parse a stored value, returning `None` for anything unrecognized.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|status| status.as_str() == value)
    }

    /// Whether this status still represents a live billing relationship.
    pub fn is_current(&self) -> bool {
        matches!(self, Self::Trialing | Self::Active | Self::PastDue)
    }
}

impl FromStr for SubscriptionStatus {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value).ok_or_else(|| format!("unknown subscription status `{value}`"))
    }
}

impl std::fmt::Display for SubscriptionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Whether a status change is allowed.
///
/// Deliberately strict: self-transitions are *not* legal, and a cancelled
/// subscription can only expire. Re-subscribing therefore creates a new
/// subscription rather than resurrecting a cancelled row.
pub fn legal_transition(from: &SubscriptionStatus, to: &SubscriptionStatus) -> bool {
    use SubscriptionStatus as S;
    matches!(
        (from, to),
        (S::Trialing, S::Active)
            | (S::Trialing, S::Cancelled)
            | (S::Active, S::PastDue)
            | (S::Active, S::Cancelled)
            | (S::PastDue, S::Active)
            | (S::PastDue, S::Cancelled)
            | (S::Cancelled, S::Expired)
    )
}

/// Credit for the unused portion of a prepaid period, in cents.
///
/// Uses i128 intermediates so a large amount or period cannot overflow, rounds
/// to the nearest cent, and saturates to the i64 range. A non-positive period
/// has no meaningful "unused portion" and yields 0.
pub fn proration_cents(amount_cents: i64, period_seconds: i64, elapsed_seconds: i64) -> i64 {
    if period_seconds <= 0 {
        return 0;
    }
    let remaining = (period_seconds - elapsed_seconds)
        .min(period_seconds)
        .max(0);
    if remaining == 0 {
        return 0;
    }
    if remaining == period_seconds {
        return amount_cents;
    }

    let numerator = amount_cents as i128 * remaining as i128;
    let denominator = period_seconds as i128;
    let rounded = if numerator >= 0 {
        (2 * numerator + denominator) / (2 * denominator)
    } else {
        (2 * numerator - denominator) / (2 * denominator)
    };
    rounded.clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

/// Total charge for `seats` at `cents_per_seat`, saturating on overflow.
/// Negative seat counts are treated as zero.
pub fn seat_amount(cents_per_seat: i64, seats: i64) -> i64 {
    cents_per_seat.saturating_mul(seats.max(0))
}

/// The smallest entitlement projection shared by storage and API checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entitlement {
    pub feature_key: String,
    pub enabled: bool,
    pub limit_value: Option<i64>,
}

/// Whether a feature is explicitly enabled. An absent key is `false`: access
/// fails closed so a missing grant cannot accidentally become "unlimited".
pub fn entitlement_for(entitlements: &[Entitlement], key: &str) -> bool {
    entitlements
        .iter()
        .any(|entry| entry.feature_key == key && entry.enabled)
}

/// Numeric limit attached to an enabled feature, if any. Disabled or absent
/// features have no limit.
pub fn entitlement_limit(entitlements: &[Entitlement], key: &str) -> Option<i64> {
    entitlements
        .iter()
        .find(|entry| entry.feature_key == key && entry.enabled)
        .and_then(|entry| entry.limit_value)
}

/// API-side enforcement helper: a feature must be enabled and, when it carries
/// a limit, current usage must still be below it.
pub fn entitlement_allows(entitlements: &[Entitlement], key: &str, current_usage: i64) -> bool {
    if !entitlement_for(entitlements, key) {
        return false;
    }
    match entitlement_limit(entitlements, key) {
        Some(limit) => current_usage >= 0 && current_usage < limit,
        None => true,
    }
}

/// Dunning rule: an unpaid invoice moves a live subscription to `past_due`.
/// Terminal states are never resurrected by a dunning pass.
pub fn dunning_status(current: &SubscriptionStatus, invoice_unpaid: bool) -> SubscriptionStatus {
    if invoice_unpaid
        && matches!(
            current,
            SubscriptionStatus::Trialing | SubscriptionStatus::Active
        )
    {
        SubscriptionStatus::PastDue
    } else {
        *current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_serializes_as_snake_case_and_parses() {
        assert_eq!(
            serde_json::to_string(&SubscriptionStatus::PastDue).unwrap(),
            "\"past_due\""
        );
        for status in SubscriptionStatus::ALL {
            assert_eq!(SubscriptionStatus::parse(status.as_str()), Some(status));
            assert_eq!(
                status.to_string().parse::<SubscriptionStatus>().unwrap(),
                status
            );
        }
        assert_eq!(SubscriptionStatus::parse("paid"), None);
        assert!("paid".parse::<SubscriptionStatus>().is_err());
    }

    #[test]
    fn all_legal_transitions_are_accepted() {
        let legal = [
            (SubscriptionStatus::Trialing, SubscriptionStatus::Active),
            (SubscriptionStatus::Trialing, SubscriptionStatus::Cancelled),
            (SubscriptionStatus::Active, SubscriptionStatus::PastDue),
            (SubscriptionStatus::Active, SubscriptionStatus::Cancelled),
            (SubscriptionStatus::PastDue, SubscriptionStatus::Active),
            (SubscriptionStatus::PastDue, SubscriptionStatus::Cancelled),
            (SubscriptionStatus::Cancelled, SubscriptionStatus::Expired),
        ];
        for (from, to) in legal {
            assert!(legal_transition(&from, &to), "{from} -> {to} must be legal");
        }
    }

    #[test]
    fn illegal_and_self_transitions_are_rejected() {
        use SubscriptionStatus as S;
        let illegal = [
            (S::Active, S::Trialing),
            (S::PastDue, S::Trialing),
            (S::Cancelled, S::Active),
            (S::Expired, S::Trialing),
            (S::Expired, S::Active),
            (S::Expired, S::Cancelled),
            (S::Active, S::Expired),
            (S::Trialing, S::PastDue),
            (S::Trialing, S::Trialing),
            (S::Active, S::Active),
            (S::PastDue, S::PastDue),
            (S::Cancelled, S::Cancelled),
            (S::Expired, S::Expired),
        ];
        for (from, to) in illegal {
            assert!(
                !legal_transition(&from, &to),
                "{from} -> {to} must be illegal"
            );
        }
        // Every legal pair has exactly the 7 edges above out of 25 pairs.
        let count = S::ALL
            .iter()
            .flat_map(|from| S::ALL.iter().map(move |to| legal_transition(from, to)))
            .filter(|allowed| *allowed)
            .count();
        assert_eq!(count, 7);
    }

    #[test]
    fn proration_uses_hand_computed_values() {
        // Exactly half of a 30-day period elapsed => half of 1000 cents.
        let month = 30 * 24 * 60 * 60;
        assert_eq!(proration_cents(1000, month, month / 2), 500);
        // 2/3 of a 3-second period remains: round(100 * 2 / 3) == 67.
        assert_eq!(proration_cents(100, 3, 1), 67);
        // 1/3 remains: round(100 / 3) == 33.
        assert_eq!(proration_cents(100, 3, 2), 33);
        // No time elapsed => the whole amount is credited.
        assert_eq!(proration_cents(999, 10, 0), 999);
    }

    #[test]
    fn proration_is_defensive_and_saturating() {
        assert_eq!(proration_cents(1000, 0, 0), 0);
        assert_eq!(proration_cents(1000, -5, 0), 0);
        // Fully elapsed, or elapsed beyond the period => zero credit.
        assert_eq!(proration_cents(1000, 10, 10), 0);
        assert_eq!(proration_cents(1000, 10, 50), 0);
        // Large values must not overflow i64 intermediates:
        // round(i64::MAX * 5 / 10) == 4611686018427387904.
        assert_eq!(proration_cents(i64::MAX, 10, 5), 4_611_686_018_427_387_904);
    }

    #[test]
    fn seat_amount_is_saturating() {
        assert_eq!(seat_amount(1500, 3), 4500);
        assert_eq!(seat_amount(0, 100), 0);
        assert_eq!(seat_amount(1500, 0), 0);
        assert_eq!(seat_amount(1500, -2), 0);
        assert_eq!(seat_amount(i64::MAX, 2), i64::MAX);
    }

    #[test]
    fn entitlement_absence_is_false_and_limits_enforced() {
        let grants = vec![
            Entitlement {
                feature_key: "deploy".into(),
                enabled: true,
                limit_value: None,
            },
            Entitlement {
                feature_key: "seats".into(),
                enabled: true,
                limit_value: Some(5),
            },
            Entitlement {
                feature_key: "beta".into(),
                enabled: false,
                limit_value: Some(10),
            },
        ];
        assert!(entitlement_for(&grants, "deploy"));
        assert!(entitlement_for(&grants, "seats"));
        assert!(!entitlement_for(&grants, "beta"));
        assert!(!entitlement_for(&grants, "missing"));
        assert_eq!(entitlement_limit(&grants, "seats"), Some(5));
        assert_eq!(entitlement_limit(&grants, "beta"), None);
        assert!(entitlement_allows(&grants, "deploy", 1_000_000));
        assert!(entitlement_allows(&grants, "seats", 4));
        assert!(!entitlement_allows(&grants, "seats", 5));
        assert!(!entitlement_allows(&grants, "beta", 0));
        assert!(!entitlement_allows(&grants, "missing", 0));
    }

    #[test]
    fn dunning_only_affects_live_subscriptions() {
        use SubscriptionStatus as S;
        assert_eq!(dunning_status(&S::Active, true), S::PastDue);
        assert_eq!(dunning_status(&S::Trialing, true), S::PastDue);
        assert_eq!(dunning_status(&S::Active, false), S::Active);
        assert_eq!(dunning_status(&S::PastDue, true), S::PastDue);
        assert_eq!(dunning_status(&S::Cancelled, true), S::Cancelled);
        assert_eq!(dunning_status(&S::Expired, true), S::Expired);
    }
}
