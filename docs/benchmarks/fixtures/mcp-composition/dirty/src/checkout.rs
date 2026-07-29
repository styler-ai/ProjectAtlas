//! Checkout total entrypoint.

use crate::{pricing, LineItem};

pub fn checkout_total(items: &[LineItem]) -> u64 {
    pricing::calculate_total(items)
}
