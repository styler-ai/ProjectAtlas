//! Price calculation policy.

use crate::LineItem;

pub fn calculate_total(items: &[LineItem]) -> u64 {
    let subtotal = items
        .iter()
        .map(|item| item.unit_cents * u64::from(item.quantity))
        .sum();
    apply_discount(subtotal)
}

pub fn apply_discount(subtotal_cents: u64) -> u64 {
    if subtotal_cents >= 10_000 {
        subtotal_cents - 1_000
    } else {
        subtotal_cents
    }
}
