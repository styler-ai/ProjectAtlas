//! Price calculation policy.

use crate::LineItem;

pub fn calculate_total(items: &[LineItem]) -> u64 {
    items
        .iter()
        .map(|item| item.unit_cents * u64::from(item.quantity))
        .sum()
}
