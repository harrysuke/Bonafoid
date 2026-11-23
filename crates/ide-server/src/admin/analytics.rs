//! Pure revenue formulas for the admin analytics endpoint.
//!
//! These functions take plain tuples so they can be tested with hand-computed
//! numbers. The only source of truth for the data is the `subscriptions` and
//! `invoices` tables; nothing here duplicates a billing-state machine.
//!
//! A subscription/invoice tuple is `(plan_id, amount_cents, status)`. For MRR
//! the status may also encode the interval (`active_annual`) because that is
//! how the store projection marks a subscription whose latest paid invoice
//! covers a year. Unknown statuses are ignored (fail closed).

use std::collections::BTreeMap;

/// Monthly recurring revenue in cents.
///
/// Active monthly subscriptions contribute their amount in full; annual ones
/// contribute `amount / 12`; anything not active contributes nothing.
pub fn mrr_cents(subs: &[(String, i64, String)]) -> i64 {
    subs.iter()
        .map(|(_, amount, status)| match status.as_str() {
            "active" | "trialing" | "past_due" | "month" | "monthly" | "active_monthly" => *amount,
            "annual" | "year" | "yearly" | "active_annual" | "annual_active" => amount / 12,
            _ => 0,
        })
        .sum()
}

/// Annual recurring revenue in cents from monthly recurring revenue.
pub fn arr_cents(mrr: i64) -> i64 {
    mrr.saturating_mul(12)
}

/// Churned fraction of the population active at the start of the period.
///
/// A period that started with no active subscriptions has no meaningful churn,
/// so it reports `0.0` instead of dividing by zero.
pub fn churn_rate(active_start: i64, churned: i64) -> f64 {
    if active_start == 0 {
        return 0.0;
    }
    churned as f64 / active_start as f64
}

/// Paid revenue per plan. Invoice tuples are `(plan_id, amount_cents, status)`.
pub fn revenue_by_plan(invoices: &[(String, i64, String)]) -> BTreeMap<String, i64> {
    let mut totals = BTreeMap::new();
    for (plan, amount, status) in invoices {
        if status == "paid" {
            *totals.entry(plan.clone()).or_insert(0_i64) += *amount;
        }
    }
    totals
}

/// Paid revenue per calendar month. Invoice tuples are
/// `(issued_at, amount_cents, status)`; both SQLite `YYYY-MM-DD HH:MM:SS` and
/// RFC 3339 `YYYY-MM-DDTHH:MM:SSZ` timestamps reduce to their first 7 chars.
pub fn monthly_series(invoices: &[(String, i64, String)]) -> BTreeMap<String, i64> {
    let mut totals = BTreeMap::new();
    for (issued_at, amount, status) in invoices {
        if status != "paid" {
            continue;
        }
        let month: String = issued_at.chars().take(7).collect();
        if month.len() == 7 && month.as_bytes()[4] == b'-' {
            *totals.entry(month).or_insert(0_i64) += *amount;
        }
    }
    totals
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mrr_sums_active_monthly_and_divides_annual() {
        let subs = vec![
            ("starter".to_string(), 1_000, "active".to_string()),
            ("pro".to_string(), 2_500, "active".to_string()),
            ("yearly".to_string(), 12_000, "active_annual".to_string()),
            ("gone".to_string(), 9_999, "canceled".to_string()),
        ];
        // 1000 + 2500 + (12000 / 12) + 0 = 4500
        assert_eq!(mrr_cents(&subs), 4_500);
    }

    #[test]
    fn mrr_is_zero_for_empty_or_all_inactive() {
        assert_eq!(mrr_cents(&[]), 0);
        let subs = vec![("a".to_string(), 500, "canceled".to_string())];
        assert_eq!(mrr_cents(&subs), 0);
    }

    #[test]
    fn arr_is_twelve_times_mrr() {
        assert_eq!(arr_cents(4_500), 54_000);
        assert_eq!(arr_cents(0), 0);
    }

    #[test]
    fn churn_rate_handles_zero_start_and_fractions() {
        assert_eq!(churn_rate(0, 5), 0.0);
        assert!((churn_rate(10, 2) - 0.2).abs() < 1e-9);
        assert!((churn_rate(8, 2) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn revenue_by_plan_counts_paid_only() {
        let invoices = vec![
            ("starter".to_string(), 1_000, "paid".to_string()),
            ("starter".to_string(), 500, "open".to_string()),
            ("pro".to_string(), 2_000, "paid".to_string()),
        ];
        let totals = revenue_by_plan(&invoices);
        assert_eq!(totals.len(), 2);
        assert_eq!(totals.get("starter"), Some(&1_000));
        assert_eq!(totals.get("pro"), Some(&2_000));
    }

    #[test]
    fn monthly_series_groups_paid_revenue() {
        let invoices = vec![
            (
                "2026-01-15T10:00:00Z".to_string(),
                1_000,
                "paid".to_string(),
            ),
            ("2026-01-31 10:00:00".to_string(), 500, "paid".to_string()),
            ("2026-02-01T00:00:00Z".to_string(), 700, "open".to_string()),
            ("2026-02-05T00:00:00Z".to_string(), 300, "paid".to_string()),
        ];
        let series = monthly_series(&invoices);
        assert_eq!(series.len(), 2);
        assert_eq!(series.get("2026-01"), Some(&1_500));
        assert_eq!(series.get("2026-02"), Some(&300));
    }
}
