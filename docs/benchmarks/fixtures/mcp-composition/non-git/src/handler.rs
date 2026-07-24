//! Health request handler.

use crate::config;

pub fn health_response() -> String {
    let timeout = config::load_timeout_millis();
    format!("healthy:{timeout}")
}
