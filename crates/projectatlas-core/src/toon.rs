//! Purpose: Render `ProjectAtlas` responses with the TOON standard encoder.

use crate::health::{HealthFinding, Severity};
use crate::outline::FileOutline;
use crate::symbols::{CodeSymbol, SymbolRelation};
use crate::telemetry::TokenOverview;
use crate::{IndexedNode, Overview};
use serde::Serialize;
use serde_json::json;

/// Render a repository overview as standard TOON.
#[must_use]
pub fn render_overview(overview: &Overview) -> String {
    encode_agent_payload(&json!({ "overview": overview }))
}

/// Build the agent-facing folder/file row projection used by TOON and JSON.
#[must_use]
pub fn render_node_rows(label: &str, nodes: &[IndexedNode]) -> Vec<serde_json::Value> {
    nodes
        .iter()
        .map(|node| {
            let purpose = node.purpose.purpose.as_deref().unwrap_or("");
            let content_summary = node.summary.as_deref().unwrap_or("");
            match label {
                "folders" => json!({
                    "path": node.node.path,
                    "kind": node.node.kind.to_string(),
                    "folder_purpose": purpose,
                    "content_summary": content_summary,
                    "status": node.purpose.status.to_string(),
                }),
                "files" => json!({
                    "path": node.node.path,
                    "kind": node.node.kind.to_string(),
                    "language": node.node.language.as_deref().unwrap_or(""),
                    "file_purpose": purpose,
                    "content_summary": content_summary,
                    "status": node.purpose.status.to_string(),
                }),
                _ => json!({
                    "path": node.node.path,
                    "kind": node.node.kind.to_string(),
                    "purpose": purpose,
                    "content_summary": content_summary,
                    "status": node.purpose.status.to_string(),
                }),
            }
        })
        .collect()
}

/// Render indexed nodes as standard TOON.
#[must_use]
pub fn render_nodes(label: &str, nodes: &[IndexedNode]) -> String {
    let rows = render_node_rows(label, nodes);
    encode_agent_payload(&json!({ label: rows }))
}

/// Render an outline as standard TOON.
#[must_use]
pub fn render_outline(outline: &FileOutline) -> String {
    encode_agent_payload(&json!({ "outline": outline }))
}

/// Render health findings as standard TOON.
#[must_use]
pub fn render_health(findings: &[HealthFinding]) -> String {
    let rows = findings
        .iter()
        .map(|finding| {
            json!({
                "severity": render_severity(finding.severity),
                "id": finding.id,
                "category": finding.category,
                "path": finding.path,
                "related_path": finding.related_path.as_deref().unwrap_or(""),
                "message": finding.message,
                "recommendation": finding.recommendation,
            })
        })
        .collect::<Vec<_>>();
    encode_agent_payload(&json!({ "health_findings": rows }))
}

/// Render token savings overview as standard TOON.
#[must_use]
pub fn render_token_overview(overview: &TokenOverview) -> String {
    let savings_rate = percentage_label(overview.savings_rate);
    let buckets = overview
        .buckets
        .iter()
        .map(|bucket| {
            json!({
                "token_savings_bucket": bucket.token_savings_bucket,
                "provider": bucket.provider,
                "model": bucket.model,
                "tokenizer_backend": bucket.tokenizer_backend,
                "accuracy": bucket.accuracy,
                "baseline_kind": bucket.baseline_kind,
                "confidence": bucket.confidence,
                "calls": bucket.calls,
                "baseline_tokens": bucket.estimated_without_projectatlas,
                "emitted_tokens": bucket.estimated_with_projectatlas,
                "saved_tokens": bucket.estimated_saved,
                "savings_rate": percentage_label(bucket.savings_rate),
            })
        })
        .collect::<Vec<_>>();
    encode_agent_payload(&json!({
        "token_savings": {
            "estimate_kind": overview.estimate_kind,
            "estimator": overview.estimator,
            "estimate_scope": overview.estimate_scope,
            "calls": overview.calls,
            "estimated_without_projectatlas": overview.estimated_without_projectatlas,
            "estimated_with_projectatlas": overview.estimated_with_projectatlas,
            "estimated_saved": overview.estimated_saved,
            "savings_rate": savings_rate,
            "totals": {
                "baseline_tokens": overview.estimated_without_projectatlas,
                "emitted_tokens": overview.estimated_with_projectatlas,
                "saved_tokens": overview.estimated_saved,
                "savings_rate": savings_rate,
            },
            "buckets": buckets,
        }
    }))
}

/// Render symbols as standard TOON.
#[must_use]
pub fn render_symbols(symbols: &[CodeSymbol]) -> String {
    let rows = symbols
        .iter()
        .map(|symbol| {
            json!({
                "path": symbol.path,
                "kind": symbol.kind.to_string(),
                "name": symbol.name,
                "start": symbol.line_start,
                "end": symbol.line_end,
                "parent": symbol.parent.as_deref().unwrap_or(""),
                "parser": symbol.parser.to_string(),
                "signature": symbol.signature,
                "exported": symbol.exported,
                "documentation": symbol.documentation.as_deref().unwrap_or(""),
            })
        })
        .collect::<Vec<_>>();
    encode_agent_payload(&json!({ "symbols": rows }))
}

/// Render symbol relations as standard TOON.
#[must_use]
pub fn render_symbol_relations(relations: &[SymbolRelation]) -> String {
    let rows = relations
        .iter()
        .map(|relation| {
            json!({
                "path": relation.path,
                "kind": relation.kind.to_string(),
                "source": relation.source_name,
                "target": relation.target_name,
                "line": relation.line,
                "parser": relation.parser.to_string(),
                "context": relation.context,
            })
        })
        .collect::<Vec<_>>();
    encode_agent_payload(&json!({ "symbol_relations": rows }))
}

/// Encode a serializable payload through the standard TOON Rust implementation.
#[must_use]
pub fn encode_agent_payload<T>(payload: &T) -> String
where
    T: Serialize,
{
    match toon_format::encode_default(payload) {
        Ok(mut encoded) => {
            encoded.push('\n');
            encoded
        }
        Err(error) => format!("toon_error: {}\n", encode_error_text(&error.to_string())),
    }
}

/// Encode one string using TOON by wrapping it in an object and extracting text.
#[must_use]
pub fn encode_error_text(value: &str) -> String {
    match toon_format::encode_default(&json!({ "value": value })) {
        Ok(encoded) => encoded
            .strip_prefix("value: ")
            .map_or_else(|| quoted_fallback(value), ToString::to_string),
        Err(_) => quoted_fallback(value),
    }
}

/// Render a severity enum as a stable TOON value.
fn render_severity(severity: Severity) -> &'static str {
    match severity {
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
    }
}

/// Format an optional savings rate as a stable display label.
fn percentage_label(rate: Option<f64>) -> String {
    rate.map_or_else(
        || "unknown".to_string(),
        |value| format!("{:.1}%", value * 100.0),
    )
}

/// Return a conservative quoted fallback for rare encoder failures.
fn quoted_fallback(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::{encode_agent_payload, render_symbols};
    use crate::symbols::{CodeSymbol, ParserKind, SymbolKind};
    use serde_json::Value;

    #[test]
    fn renders_round_trippable_toon_with_standard_decoder() -> Result<(), Box<dyn std::error::Error>>
    {
        let toon = encode_agent_payload(&serde_json::json!({
            "items": [
                {"path": "src/lib.rs", "text": "alpha,beta"},
                {"path": "src/main.rs", "text": "line\nbreak"}
            ]
        }));
        let decoded: Value = toon_format::decode_default(&toon)?;
        if decoded["items"][0]["text"] != "alpha,beta" {
            return Err("first decoded item did not round-trip".into());
        }
        if decoded["items"][1]["text"] != "line\nbreak" {
            return Err("second decoded item did not round-trip".into());
        }
        Ok(())
    }

    #[test]
    fn renders_symbols_as_tabular_toon() {
        let toon = render_symbols(&[CodeSymbol {
            path: "src/lib.rs".to_string(),
            language: Some("rust".to_string()),
            name: "scan".to_string(),
            kind: SymbolKind::Function,
            signature: "fn scan()".to_string(),
            exported: false,
            documentation: None,
            line_start: 1,
            line_end: 3,
            parent: None,
            parser: ParserKind::TreeSitter,
            detail: Some("function_item".to_string()),
        }]);
        assert!(toon.contains(
            "symbols[1]{path,kind,name,start,end,parent,parser,signature,exported,documentation}:"
        ));
    }
}
