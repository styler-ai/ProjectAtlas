//! Public order submission entrypoint.

use crate::{service, storage, Order, OrderRequest};

pub fn submit_order(request: OrderRequest) -> Result<Order, &'static str> {
    let order = service::validate_order(request)?;
    storage::save_order(&order)?;
    Ok(order)
}
