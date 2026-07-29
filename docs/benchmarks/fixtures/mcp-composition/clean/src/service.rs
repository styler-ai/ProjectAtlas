//! Order validation policy.

use crate::{Order, OrderRequest};

pub fn validate_order(request: OrderRequest) -> Result<Order, &'static str> {
    if request.quantity == 0 {
        return Err("quantity must be positive");
    }
    Ok(Order {
        id: request.id,
        quantity: request.quantity,
    })
}
