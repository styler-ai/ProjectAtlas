//! Validate the accepted relation-family inventory against the typed graph contract.

use projectatlas_core::graph::GraphRelationKind;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::error::Error;
use std::io;

const CAPABILITY_REGISTRY: &str =
    include_str!("../../../docs/benchmarks/projectatlas-v0.4-capability-registry.json");
const GRAPH_RELATION_TYPE: &str = "projectatlas_core::graph::GraphRelationKind";
const GRAPH_RELATION_OWNER: &str = "projectatlas_core::graph";
const LEGACY_PRODUCER_STATE: &str = "implemented-legacy-projection";
const PENDING_STATE: &str = "pending";

fn require(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

fn required_str<'a>(value: &'a Value, field: &str) -> Result<&'a str, Box<dyn Error>> {
    value[field]
        .as_str()
        .filter(|item| !item.trim().is_empty())
        .ok_or_else(|| io::Error::other(format!("missing nonempty {field}")).into())
}

fn required_object<'a>(
    value: &'a Value,
    field: &str,
) -> Result<&'a serde_json::Map<String, Value>, Box<dyn Error>> {
    value[field]
        .as_object()
        .ok_or_else(|| io::Error::other(format!("missing object {field}")).into())
}

fn required_array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], Box<dyn Error>> {
    value[field]
        .as_array()
        .filter(|rows| !rows.is_empty())
        .map(Vec::as_slice)
        .ok_or_else(|| io::Error::other(format!("missing nonempty array {field}")).into())
}

fn string_set(value: &Value, field: &str) -> Result<BTreeSet<String>, Box<dyn Error>> {
    required_array(value, field)?
        .iter()
        .map(|item| {
            item.as_str()
                .filter(|text| !text.trim().is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| io::Error::other(format!("{field} contains a blank value")).into())
        })
        .collect()
}

fn validate_relation_traceability(registry: &Value) -> Result<(), Box<dyn Error>> {
    let contract = &registry["relation_traceability_contract"];
    require(
        contract["schema_version"] == 1
            && contract["matrix_source"] == "capabilities[family=relation].traceability"
            && contract["typed_enum"] == GRAPH_RELATION_TYPE,
        "relation traceability contract identity drifted",
    )?;
    let fixture_classes = string_set(contract, "fixture_classes")?;
    require(
        fixture_classes
            == BTreeSet::from([
                "adversarial-negative".to_string(),
                "ambiguous-or-unresolved".to_string(),
                "positive".to_string(),
            ]),
        "relation fixture classes are incomplete",
    )?;
    let settings_exposure = required_str(contract, "settings_exposure")?;
    let evidence_profiles = required_object(contract, "evidence_profiles")?;
    let persistence_profiles = required_object(contract, "persistence_profiles")?;
    let invalidation_profiles = required_object(contract, "invalidation_profiles")?;
    let accuracy_gates = required_object(contract, "accuracy_gates")?;

    let capabilities = registry["capabilities"]
        .as_array()
        .ok_or_else(|| io::Error::other("capabilities is not an array"))?;
    let relation_rows = capabilities
        .iter()
        .filter(|capability| capability["family"] == "relation")
        .collect::<Vec<_>>();
    require(
        relation_rows.len() == GraphRelationKind::ALL.len(),
        "accepted relation rows do not match the typed enum inventory",
    )?;

    let mut remaining = GraphRelationKind::ALL.into_iter().collect::<BTreeSet<_>>();
    let mut capability_ids = BTreeSet::new();
    for capability in relation_rows {
        let capability_id = required_str(capability, "capability_id")?;
        require(
            capability_ids.insert(capability_id),
            format!("duplicate relation capability {capability_id}"),
        )?;
        require(
            capability["advertised"] == false && capability["evidence_state"] == PENDING_STATE,
            format!("relation {capability_id} is advertised without completed evidence"),
        )?;
        required_array(capability, "fixture_ids")?;

        let trace = &capability["traceability"];
        let serialized_kind = required_str(trace, "serialized_kind")?;
        let kind: GraphRelationKind =
            serde_json::from_value(Value::String(serialized_kind.into()))?;
        require(
            remaining.remove(&kind),
            format!("relation {capability_id} duplicates or omits a typed enum variant"),
        )?;
        require(
            required_str(trace, "typed_enum")? == format!("{GRAPH_RELATION_TYPE}::{kind:?}")
                && kind.as_str() == serialized_kind
                && required_str(trace, "owning_module")? == GRAPH_RELATION_OWNER,
            format!("relation {capability_id} has an inaccurate typed owner mapping"),
        )?;

        let producer_state = required_str(trace, "producer_state")?;
        required_str(trace, "producer")?;
        require(
            matches!(producer_state, LEGACY_PRODUCER_STATE | PENDING_STATE),
            format!("relation {capability_id} has an unknown producer state"),
        )?;
        require(
            (kind.legacy_kind().is_some()) == (producer_state == LEGACY_PRODUCER_STATE),
            format!("relation {capability_id} misstates legacy producer availability"),
        )?;

        let evidence_profile = evidence_profiles
            .get(required_str(trace, "evidence_profile")?)
            .ok_or_else(|| {
                io::Error::other(format!("{capability_id} has unknown evidence profile"))
            })?;
        required_array(evidence_profile, "evidence_classes")?;
        required_array(evidence_profile, "required_fields")?;

        let persistence_profile_id = required_str(trace, "persistence_profile")?;
        let persistence_profile = persistence_profiles
            .get(persistence_profile_id)
            .ok_or_else(|| {
                io::Error::other(format!("{capability_id} has unknown persistence profile"))
            })?;
        let persistence_mode = required_str(persistence_profile, "mode")?;
        if persistence_mode == "bounded-call-memory-only" {
            require(
                capability_id == "relation.cross-repository"
                    && persistence_profile["persistence_prohibited"] == true
                    && persistence_profile["tables"]
                        .as_array()
                        .is_some_and(Vec::is_empty)
                    && persistence_profile["indexes"]
                        .as_array()
                        .is_some_and(Vec::is_empty),
                "only call-scoped federation may omit persistent tables and indexes",
            )?;
        } else {
            require(
                persistence_mode == "persistent-derived-slot",
                format!("relation {capability_id} has an unknown persistence mode"),
            )?;
            required_array(persistence_profile, "tables")?;
            required_array(persistence_profile, "indexes")?;
        }
        require(
            persistence_profile["payload_schema"].is_null()
                && persistence_profile["stable_identity_fields"]
                    .as_array()
                    .is_some_and(Vec::is_empty),
            format!("relation {capability_id} introduced an unratified payload identity"),
        )?;

        let invalidation_profile = invalidation_profiles
            .get(required_str(trace, "invalidation_profile")?)
            .ok_or_else(|| {
                io::Error::other(format!("{capability_id} has unknown invalidation profile"))
            })?;
        require(
            invalidation_profile
                .as_array()
                .is_some_and(|keys| !keys.is_empty()),
            format!("relation {capability_id} has no invalidation keys"),
        )?;
        required_array(trace, "query_surfaces")?;
        require(
            required_str(trace, "settings_exposure")? == settings_exposure
                && trace["fixture_inventory_source"] == "capability.fixture_ids"
                && trace["availability"] == PENDING_STATE,
            format!("relation {capability_id} has incomplete settings or fixture exposure"),
        )?;

        let accuracy_gate = accuracy_gates
            .get(required_str(trace, "accuracy_gate")?)
            .ok_or_else(|| {
                io::Error::other(format!("{capability_id} has unknown accuracy gate"))
            })?;
        require(
            accuracy_gate["minimum_precision"]
                .as_f64()
                .is_some_and(|value| value > 0.0)
                && accuracy_gate["minimum_recall"]
                    .as_f64()
                    .is_some_and(|value| value > 0.0),
            format!("relation {capability_id} lacks semantic accuracy thresholds"),
        )?;
        required_array(accuracy_gate, "required_metrics")?;
        required_str(accuracy_gate, "decision")?;
    }

    require(
        remaining.is_empty(),
        "typed relation variants are missing traceability rows",
    )
}

fn relation_capability_mut<'a>(
    registry: &'a mut Value,
    capability_id: &str,
) -> Result<&'a mut Value, Box<dyn Error>> {
    registry["capabilities"]
        .as_array_mut()
        .and_then(|capabilities| {
            capabilities
                .iter_mut()
                .find(|capability| capability["capability_id"] == capability_id)
        })
        .ok_or_else(|| io::Error::other(format!("missing capability {capability_id}")).into())
}

fn reject_registry_mutation(
    label: &str,
    expected_error: &str,
    mutate: impl FnOnce(&mut Value) -> Result<(), Box<dyn Error>>,
) -> Result<(), Box<dyn Error>> {
    let mut registry: Value = serde_json::from_str(CAPABILITY_REGISTRY)?;
    mutate(&mut registry)?;
    let message = match validate_relation_traceability(&registry) {
        Ok(()) => {
            return Err(io::Error::other(format!(
                "{label} mutation passed relation traceability validation"
            ))
            .into());
        }
        Err(error) => error.to_string(),
    };
    require(
        message.contains(expected_error),
        format!("{label} mutation returned unexpected error: {message}"),
    )
}

/// ARRI-4.4: every accepted relation family maps to one complete typed trace row.
#[test]
fn task_arri_ut_arri_4_4() -> Result<(), Box<dyn Error>> {
    let registry: Value = serde_json::from_str(CAPABILITY_REGISTRY)?;
    validate_relation_traceability(&registry)
}

/// ARRI-4.5: incomplete or inaccurate relation trace rows fail closed.
#[test]
fn task_arri_ut_arri_4_5() -> Result<(), Box<dyn Error>> {
    reject_registry_mutation(
        "missing row",
        "accepted relation rows do not match",
        |registry| {
            let capabilities = registry["capabilities"]
                .as_array_mut()
                .ok_or_else(|| io::Error::other("capabilities is not an array"))?;
            let index = capabilities
                .iter()
                .position(|capability| capability["capability_id"] == "relation.calls")
                .ok_or_else(|| io::Error::other("missing relation.calls"))?;
            capabilities.remove(index);
            Ok(())
        },
    )?;
    reject_registry_mutation(
        "duplicate typed mapping",
        "duplicates or omits a typed enum variant",
        |registry| {
            relation_capability_mut(registry, "relation.channel")?["traceability"]["serialized_kind"] =
                json!("calls");
            Ok(())
        },
    )?;
    reject_registry_mutation(
        "string-only mapping",
        "missing nonempty typed_enum",
        |registry| {
            relation_capability_mut(registry, "relation.calls")?["traceability"]
                .as_object_mut()
                .ok_or_else(|| io::Error::other("traceability is not an object"))?
                .remove("typed_enum");
            Ok(())
        },
    )?;
    reject_registry_mutation(
        "unpersisted mapping",
        "only call-scoped federation may omit",
        |registry| {
            relation_capability_mut(registry, "relation.calls")?["traceability"]["persistence_profile"] =
                json!("call-scoped-federation-v1");
            Ok(())
        },
    )?;
    reject_registry_mutation(
        "unqueryable mapping",
        "missing nonempty array query_surfaces",
        |registry| {
            relation_capability_mut(registry, "relation.calls")?["traceability"]["query_surfaces"] =
                json!([]);
            Ok(())
        },
    )?;
    reject_registry_mutation(
        "untested mapping",
        "missing nonempty array fixture_ids",
        |registry| {
            relation_capability_mut(registry, "relation.calls")?["fixture_ids"] = json!([]);
            Ok(())
        },
    )?;
    reject_registry_mutation(
        "inaccurately advertised mapping",
        "advertised without completed evidence",
        |registry| {
            relation_capability_mut(registry, "relation.calls")?["advertised"] = json!(true);
            Ok(())
        },
    )
}
