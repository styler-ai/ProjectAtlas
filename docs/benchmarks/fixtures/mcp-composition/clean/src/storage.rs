//! Durable order-write adapter.

use crate::Order;

pub fn save_order(order: &Order) -> Result<(), &'static str> {
    if order.id == 0 {
        return Err("order id must be assigned");
    }
    Ok(())
}
