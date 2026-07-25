//! Route-to-handler adapter.

use crate::handler;

pub fn dispatch(path: &str) -> Option<String> {
    (path == "/health").then(handler::health_response)
}
