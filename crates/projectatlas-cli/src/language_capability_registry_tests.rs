use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::io;

const CAPABILITY_REGISTRY: &str =
    include_str!("../../../docs/benchmarks/projectatlas-v0.4-capability-registry.json");
const EXPECTED_ACCEPTED_SET_DIGEST: &str =
    "ed9d1012653830f9efabf2b678c8849e5372a1a8a8b390da4f764a53553e7512";
const DEFAULT_CORE_PACK_ID: &str = "default-core";
const DEFAULT_CORE_OWNER: &str = "projectatlas-symbols";
const BROAD_LANGUAGE_PACK_ID: &str = "broad-language-pack";
const BROAD_LANGUAGE_PACK_OWNER: &str = "broad-language-pack-worker";
const OBJECTSCRIPT_EXPORT_XML_MODE_ID: &str = "mode.objectscript-export-xml";
const OBJECTSCRIPT_UDL_PARSER_ID: &str = "parse.objectscript-udl";
const REQUIRED_PLATFORMS: [&str; 4] = [
    "windows-x86_64",
    "linux-x86_64",
    "macos-aarch64",
    "macos-x86_64",
];

fn require(condition: bool, message: impl Into<String>) -> Result<(), Box<dyn Error>> {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

fn required_string(value: &Value, field: &str) -> Result<String, Box<dyn Error>> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| io::Error::other(format!("{field} is missing or empty")).into())
}

fn required_array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value], Box<dyn Error>> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| io::Error::other(format!("{field} is not an array")).into())
}

fn string_list(value: &Value, field: &str) -> Result<Vec<String>, Box<dyn Error>> {
    required_array(value, field)?
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .filter(|text| !text.trim().is_empty())
                .map(ToOwned::to_owned)
                .ok_or_else(|| {
                    io::Error::other(format!("{field} contains a non-string or empty value"))
                })
        })
        .collect::<Result<_, _>>()
        .map_err(Into::into)
}

fn validate_object_fields(
    value: &Value,
    required: &[&str],
    optional: &[&str],
    context: &str,
) -> Result<(), Box<dyn Error>> {
    let object = value
        .as_object()
        .ok_or_else(|| io::Error::other(format!("{context} is not an object")))?;
    let actual = object.keys().map(String::as_str).collect::<BTreeSet<_>>();
    let required = required.iter().copied().collect::<BTreeSet<_>>();
    let allowed = required
        .iter()
        .copied()
        .chain(optional.iter().copied())
        .collect::<BTreeSet<_>>();
    require(
        required.is_subset(&actual) && actual.is_subset(&allowed),
        format!("{context} has missing or unsupported fields"),
    )
}

fn expected_mode_defaults() -> Value {
    json!({
        "accepted_delivery_target": true,
        "alias_of": null,
        "detection_rule_id_template": "detect.{public_mode}",
        "fixture_id_templates": [
            "lang.{public_mode}.valid",
            "lang.{public_mode}.malformed"
        ],
        "required_claims": ["detected", "parsed"],
        "achieved_claims": [],
        "evidence_state": "pending",
        "advertisement": "blocked-until-achieved-manifest",
        "owner_source": "pack.language_owner",
        "required_platforms_source": "registry.required_platforms",
        "allowed_override_fields": [
            "accepted_delivery_target",
            "alias_of",
            "detection_rule_id",
            "fixture_ids",
            "required_claims",
            "achieved_claims",
            "evidence_state",
            "advertisement",
            "owner",
            "required_platforms"
        ]
    })
}

fn expected_parser_defaults() -> Value {
    json!({
        "kind": "tree-sitter-or-vetted-parser",
        "grammar_symbol": null,
        "tree_sitter_abi": null,
        "asset_id_template": "asset.{parser_suffix}",
        "query_pack_id_template": "queries.{parser_suffix}",
        "evidence_state": "pending-asset-fixture-and-platform-verification",
        "advertised": false,
        "owner_source": "pack.language_owner",
        "required_platforms_source": "registry.required_platforms",
        "allowed_override_fields": [
            "kind",
            "grammar_symbol",
            "tree_sitter_abi",
            "asset_id",
            "query_pack_id",
            "evidence_state",
            "advertised",
            "owner",
            "required_platforms"
        ]
    })
}

fn expected_objectscript_export_transform() -> Value {
    json!({
        "transform_id": "transform.objectscript-export-xml-to-udl",
        "version": 1,
        "behavior": "export-container-to-udl-records",
        "deterministic": true,
        "target_mode_id": OBJECTSCRIPT_EXPORT_XML_MODE_ID,
        "target_parser_id": OBJECTSCRIPT_UDL_PARSER_ID,
        "detection_ownership": "mode-detection-rule-before-transform",
        "detection_rule_id": "detect.objectscript-export-xml",
        "limits": {
            "max_input_bytes": 2_000_000,
            "max_derived_output_bytes": 2_000_000,
            "max_records": 1_024,
            "max_nesting_depth": 256,
            "max_diagnostics": 256,
            "deadline_ms": 300_000
        },
        "cancellation": {
            "enabled": true,
            "poll_interval_ms": 25,
            "grace_period_ms": 1_000
        },
        "source_mapping": {
            "original_file_identity": true,
            "per_record_provenance": true,
            "every_derived_fact": true,
            "every_diagnostic": true
        },
        "security": {
            "dtd": "denied",
            "entity_expansion": "denied",
            "external_resources": "denied",
            "schema_fetch": "denied",
            "execution": "denied"
        },
        "failure_policy": {
            "empty_input": "unavailable",
            "malformed_input": "partial-or-unavailable",
            "oversized_input": "unavailable",
            "deeply_nested_input": "unavailable",
            "multi_record_input": "parse-each-record-in-source-order",
            "unrelated_parser_fallback": false,
            "guessed_symbols_after_failure": false,
            "coverage_after_failure": "partial-or-unavailable"
        }
    })
}

fn pack_language_owner(registry: &Value, pack_id: &str) -> Result<String, Box<dyn Error>> {
    let mut matching_packs = required_array(registry, "packs")?
        .iter()
        .filter(|pack| pack["pack_id"] == pack_id);
    let pack = matching_packs
        .next()
        .ok_or_else(|| io::Error::other(format!("missing language pack {pack_id}")))?;
    require(
        matching_packs.next().is_none(),
        format!("duplicate language pack {pack_id}"),
    )?;
    required_string(pack, "language_owner")
}

fn validate_compact_schema(registry: &Value) -> Result<(), Box<dyn Error>> {
    require(
        registry["schema_version"] == 2 && registry["format"] == "projectatlas.capability-registry",
        "capability registry schema identity drifted",
    )?;
    require(
        registry["mode_defaults"] == expected_mode_defaults(),
        "mode defaults or derivation conventions drifted",
    )?;
    require(
        registry["parser_defaults"] == expected_parser_defaults(),
        "parser defaults or derivation conventions drifted",
    )?;
    require(
        string_list(registry, "required_platforms")? == REQUIRED_PLATFORMS.map(ToOwned::to_owned),
        "registry required-platform matrix drifted",
    )?;
    require(
        pack_language_owner(registry, DEFAULT_CORE_PACK_ID)? == DEFAULT_CORE_OWNER
            && pack_language_owner(registry, BROAD_LANGUAGE_PACK_ID)? == BROAD_LANGUAGE_PACK_OWNER,
        "language-pack ownership drifted",
    )
}

fn apply_overrides(
    effective: &mut Value,
    compact: &Value,
    defaults: &Value,
    context: &str,
) -> Result<(), Box<dyn Error>> {
    let Some(overrides) = compact.get("overrides") else {
        return Ok(());
    };
    let overrides = overrides
        .as_object()
        .ok_or_else(|| io::Error::other(format!("{context} overrides is not an object")))?;
    require(
        !overrides.is_empty(),
        format!("{context} has an empty overrides object"),
    )?;
    let allowed = string_list(defaults, "allowed_override_fields")?
        .into_iter()
        .collect::<BTreeSet<_>>();
    let effective = effective
        .as_object_mut()
        .ok_or_else(|| io::Error::other(format!("{context} materialization is not an object")))?;
    for (field, value) in overrides {
        require(
            allowed.contains(field),
            format!("{context} override {field} is not allowed"),
        )?;
        let previous = effective
            .get(field)
            .ok_or_else(|| io::Error::other(format!("{context} override {field} has no target")))?;
        require(
            previous != value,
            format!("{context} override {field} redundantly repeats its default"),
        )?;
        effective.insert(field.clone(), value.clone());
    }
    Ok(())
}

fn materialize_mode(registry: &Value, compact: &Value) -> Result<Value, Box<dyn Error>> {
    validate_object_fields(
        compact,
        &["mode_id", "public_mode", "origin", "parser_id", "pack_id"],
        &["pre_parse_transform", "overrides"],
        "compact mode",
    )?;
    let mode_id = required_string(compact, "mode_id")?;
    let public_mode = required_string(compact, "public_mode")?;
    let origin = required_string(compact, "origin")?;
    let parser_id = required_string(compact, "parser_id")?;
    let pack_id = required_string(compact, "pack_id")?;
    let defaults = &registry["mode_defaults"];
    let mut effective = json!({
        "mode_id": mode_id,
        "public_mode": public_mode,
        "origin": origin,
        "parser_id": parser_id,
        "pack_id": pack_id,
        "pre_parse_transform": compact
            .get("pre_parse_transform")
            .cloned()
            .unwrap_or(Value::Null),
        "accepted_delivery_target": defaults["accepted_delivery_target"].clone(),
        "alias_of": defaults["alias_of"].clone(),
        "detection_rule_id": format!("detect.{public_mode}"),
        "fixture_ids": [
            format!("lang.{public_mode}.valid"),
            format!("lang.{public_mode}.malformed")
        ],
        "required_claims": defaults["required_claims"].clone(),
        "achieved_claims": defaults["achieved_claims"].clone(),
        "evidence_state": defaults["evidence_state"].clone(),
        "advertisement": defaults["advertisement"].clone(),
        "owner": pack_language_owner(registry, &pack_id)?,
        "required_platforms": registry["required_platforms"].clone()
    });
    apply_overrides(&mut effective, compact, defaults, &mode_id)?;
    Ok(effective)
}

fn materialize_parser(registry: &Value, compact: &Value) -> Result<Value, Box<dyn Error>> {
    validate_object_fields(
        compact,
        &["parser_id", "pack_id", "normalized_modes"],
        &["overrides"],
        "compact parser",
    )?;
    let parser_id = required_string(compact, "parser_id")?;
    let parser_suffix = parser_id
        .strip_prefix("parse.")
        .filter(|suffix| !suffix.is_empty())
        .ok_or_else(|| io::Error::other(format!("invalid parser ID {parser_id}")))?;
    let pack_id = required_string(compact, "pack_id")?;
    let normalized_modes = string_list(compact, "normalized_modes")?;
    let defaults = &registry["parser_defaults"];
    let mut effective = json!({
        "parser_id": parser_id,
        "kind": defaults["kind"].clone(),
        "pack_id": pack_id,
        "owner": pack_language_owner(registry, &pack_id)?,
        "normalized_modes": normalized_modes,
        "grammar_symbol": defaults["grammar_symbol"].clone(),
        "tree_sitter_abi": defaults["tree_sitter_abi"].clone(),
        "asset_id": format!("asset.{parser_suffix}"),
        "query_pack_id": format!("queries.{parser_suffix}"),
        "evidence_state": defaults["evidence_state"].clone(),
        "advertised": defaults["advertised"].clone(),
        "required_platforms": registry["required_platforms"].clone()
    });
    apply_overrides(&mut effective, compact, defaults, &parser_id)?;
    Ok(effective)
}

fn materialize_registry(registry: &Value) -> Result<Value, Box<dyn Error>> {
    let modes = required_array(registry, "modes")?
        .iter()
        .map(|mode| materialize_mode(registry, mode))
        .collect::<Result<Vec<_>, _>>()?;
    let parsers = required_array(registry, "parsers")?
        .iter()
        .map(|parser| materialize_parser(registry, parser))
        .collect::<Result<Vec<_>, _>>()?;
    let mut effective = registry.clone();
    effective["modes"] = Value::Array(modes);
    effective["parsers"] = Value::Array(parsers);
    Ok(effective)
}

fn accepted_set_digest(registry: &Value) -> Result<String, Box<dyn Error>> {
    accepted_set_digest_from_effective(&materialize_registry(registry)?)
}

fn accepted_set_digest_from_effective(registry: &Value) -> Result<String, Box<dyn Error>> {
    let policy = &registry["accepted_set_policy"];
    let platforms = string_list(registry, "required_platforms")?;
    let mut parts = vec![format!(
        "policy|{}|{}|{}",
        policy["target_runnable_modes"],
        policy["target_normalized_parser_capabilities"],
        platforms.join(",")
    )];

    let mut modes = required_array(registry, "modes")?
        .iter()
        .collect::<Vec<_>>();
    modes.sort_by_key(|mode| mode["mode_id"].as_str().unwrap_or_default());
    for mode in modes {
        let pre_parse_transform = serde_json::to_string(&mode["pre_parse_transform"])?;
        parts.push(format!(
            "mode|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            required_string(mode, "mode_id")?,
            required_string(mode, "public_mode")?,
            required_string(mode, "parser_id")?,
            required_string(mode, "pack_id")?,
            required_string(mode, "owner")?,
            mode["alias_of"].as_str().unwrap_or_default(),
            string_list(mode, "fixture_ids")?.join(","),
            string_list(mode, "required_platforms")?.join(","),
            string_list(mode, "required_claims")?.join(","),
            pre_parse_transform
        ));
    }

    let mut parsers = required_array(registry, "parsers")?
        .iter()
        .collect::<Vec<_>>();
    parsers.sort_by_key(|parser| parser["parser_id"].as_str().unwrap_or_default());
    for parser in parsers {
        parts.push(format!(
            "parser|{}|{}|{}|{}|{}|{}",
            required_string(parser, "parser_id")?,
            required_string(parser, "kind")?,
            required_string(parser, "pack_id")?,
            required_string(parser, "owner")?,
            string_list(parser, "normalized_modes")?.join(","),
            string_list(parser, "required_platforms")?.join(",")
        ));
    }

    let mut crosswalk = required_array(&registry["accepted_language_crosswalk"], "entries")?
        .iter()
        .collect::<Vec<_>>();
    crosswalk.sort_by_key(|row| row["accepted_name_id"].as_str().unwrap_or_default());
    for row in crosswalk {
        parts.push(format!(
            "crosswalk|{}|{}|{}|{}|{}",
            required_string(row, "accepted_name_id")?,
            required_string(row, "standard_name")?,
            row["dialect"].as_str().unwrap_or_default(),
            required_string(row, "mode_id")?,
            required_string(row, "mapping")?
        ));
    }

    Ok(format!("{:x}", Sha256::digest(parts.join("\n").as_bytes())))
}

fn validate_crosswalk(registry: &Value, mode_ids: &BTreeSet<String>) -> Result<(), Box<dyn Error>> {
    let crosswalk_rows = required_array(&registry["accepted_language_crosswalk"], "entries")?;
    require(
        registry["accepted_language_crosswalk"]["identity"]
            == "projectatlas-accepted-language-set-215"
            && crosswalk_rows.len() == 163,
        "accepted language crosswalk identity or row count drifted",
    )?;
    let mut accepted_name_ids = BTreeSet::new();
    let mut mapping_counts = BTreeMap::<String, usize>::new();
    let mut dialect_rows = BTreeSet::new();
    for row in crosswalk_rows {
        validate_object_fields(
            row,
            &[
                "accepted_name_id",
                "standard_name",
                "dialect",
                "mode_id",
                "mapping",
            ],
            &[],
            "accepted language crosswalk row",
        )?;
        let accepted_name_id = required_string(row, "accepted_name_id")?;
        let standard_name = required_string(row, "standard_name")?;
        let mode_id = required_string(row, "mode_id")?;
        let mapping = required_string(row, "mapping")?;
        require(
            accepted_name_ids.insert(accepted_name_id.clone()),
            "duplicate accepted language crosswalk ID",
        )?;
        require(
            mode_ids.contains(&mode_id),
            "crosswalk points to a missing mode",
        )?;
        require(
            matches!(
                mapping.as_str(),
                "canonical-mode" | "standard-name-alias" | "dialect-mode"
            ),
            "crosswalk contains an unsupported mapping kind",
        )?;
        *mapping_counts.entry(mapping.clone()).or_default() += 1;
        if mapping == "dialect-mode" {
            let dialect = required_string(row, "dialect")?;
            dialect_rows.insert((accepted_name_id, standard_name, dialect, mode_id));
        } else {
            require(
                row["dialect"].is_null(),
                "non-dialect crosswalk row declares a dialect",
            )?;
        }
    }
    require(
        mapping_counts.get("standard-name-alias") == Some(&11)
            && mapping_counts.get("dialect-mode") == Some(&5)
            && dialect_rows
                == BTreeSet::from([
                    (
                        "accepted.cfml-script".to_string(),
                        "CFML".to_string(),
                        "script".to_string(),
                        "mode.cfml-script".to_string(),
                    ),
                    (
                        "accepted.cfml-tag".to_string(),
                        "CFML".to_string(),
                        "tag".to_string(),
                        "mode.cfml-tag".to_string(),
                    ),
                    (
                        "accepted.objectscript-export-xml".to_string(),
                        "ObjectScript".to_string(),
                        "export-xml".to_string(),
                        OBJECTSCRIPT_EXPORT_XML_MODE_ID.to_string(),
                    ),
                    (
                        "accepted.objectscript-routine".to_string(),
                        "ObjectScript".to_string(),
                        "routine".to_string(),
                        "mode.objectscript-routine".to_string(),
                    ),
                    (
                        "accepted.objectscript-udl".to_string(),
                        "ObjectScript".to_string(),
                        "udl".to_string(),
                        "mode.objectscript-udl".to_string(),
                    ),
                ]),
        "standard aliases or distinct CFML/ObjectScript dialect modes drifted",
    )
}

fn validate_effective_registry(
    source_registry: &Value,
    registry: &Value,
) -> Result<(), Box<dyn Error>> {
    let modes = required_array(registry, "modes")?;
    let parsers = required_array(registry, "parsers")?;
    let expected_platforms = REQUIRED_PLATFORMS.map(ToOwned::to_owned);

    require(modes.len() == 215, "accepted mode union is not exactly 215")?;
    require(
        parsers.len() == 209,
        "normalized parser target is not exactly 209",
    )?;
    require(
        registry["registry_id"] == "projectatlas-v0.4-candidate-215x209"
            && registry["status"] == "candidate-pending-evidence"
            && registry["counts"]["modes"] == 215
            && registry["counts"]["normalized_parser_capabilities"] == 209
            && registry["counts"]["accepted_language_crosswalk_entries"] == 163
            && registry["counts"]["current_public_modes"] == 63
            && registry["accepted_set_policy"]["minimum_runnable_modes"] == 215
            && registry["accepted_set_policy"]["target_runnable_modes"] == 215
            && registry["accepted_set_policy"]["minimum_normalized_parser_capabilities"] == 209
            && registry["accepted_set_policy"]["target_normalized_parser_capabilities"] == 209
            && registry["accepted_set_policy"]["minimum_current_public_modes"] == 63
            && registry["accepted_set_policy"]["aliases_count_toward_modes"] == false
            && registry["accepted_set_policy"]["shared_fallback_counts_as_parser"] == false
            && registry["accepted_set_policy"]["advertisement_requires_achieved_manifest"] == true,
        "accepted-set identity, counts, or fail-closed policy drifted",
    )?;
    require(
        registry["achieved_manifest"].is_null(),
        "pending target has an achieved manifest",
    )?;

    let mut mode_ids = BTreeSet::new();
    let mut public_modes = BTreeSet::new();
    let mut modes_by_parser = BTreeMap::<String, BTreeSet<String>>::new();
    let mut mode_contracts = BTreeMap::<String, (String, String)>::new();
    let mut objectscript_modes = BTreeSet::new();
    let mut pre_parse_transform_count = 0;
    for mode in modes {
        let mode_id = required_string(mode, "mode_id")?;
        let public_mode = required_string(mode, "public_mode")?;
        let parser_id = required_string(mode, "parser_id")?;
        let pack_id = required_string(mode, "pack_id")?;
        let owner = required_string(mode, "owner")?;
        let expected_owner = match pack_id.as_str() {
            DEFAULT_CORE_PACK_ID => DEFAULT_CORE_OWNER,
            BROAD_LANGUAGE_PACK_ID => BROAD_LANGUAGE_PACK_OWNER,
            _ => return Err(io::Error::other("mode has an unknown pack owner").into()),
        };
        require(mode_ids.insert(mode_id.clone()), "duplicate mode ID")?;
        require(
            public_modes.insert(public_mode.clone()),
            "duplicate public mode",
        )?;
        if public_mode.starts_with("objectscript-") {
            objectscript_modes.insert((mode_id.clone(), public_mode.clone(), parser_id.clone()));
        }
        require(
            mode["accepted_delivery_target"] == true && mode["alias_of"].is_null(),
            "accepted modes cannot be hidden behind aliases",
        )?;
        require(
            required_string(mode, "detection_rule_id")? == format!("detect.{public_mode}"),
            "mode detection-rule convention drifted",
        )?;
        require(
            string_list(mode, "required_platforms")? == expected_platforms,
            "mode required-platform matrix drifted",
        )?;
        require(
            owner == expected_owner,
            "mode owner does not match its pack",
        )?;
        require(
            string_list(mode, "fixture_ids")?
                == [
                    format!("lang.{public_mode}.valid"),
                    format!("lang.{public_mode}.malformed"),
                ],
            "mode fixture IDs are missing or unstable",
        )?;
        let expected_claims = if public_mode.starts_with("objectscript-") {
            ["detected", "parsed", "symbols"]
                .map(ToOwned::to_owned)
                .to_vec()
        } else {
            ["detected", "parsed"].map(ToOwned::to_owned).to_vec()
        };
        require(
            string_list(mode, "required_claims")? == expected_claims,
            "accepted mode tier drifted",
        )?;
        if mode_id == OBJECTSCRIPT_EXPORT_XML_MODE_ID {
            pre_parse_transform_count += 1;
            require(
                mode["pre_parse_transform"] == expected_objectscript_export_transform(),
                "ObjectScript export-XML transform contract drifted",
            )?;
        } else {
            require(
                mode["pre_parse_transform"].is_null(),
                "a non-export mode owns a pre-parse transform",
            )?;
        }
        require(
            string_list(mode, "achieved_claims")?.is_empty()
                && mode["evidence_state"] == "pending"
                && mode["advertisement"] == "blocked-until-achieved-manifest",
            "mode claims achievement or advertisement without evidence",
        )?;
        modes_by_parser
            .entry(parser_id.clone())
            .or_default()
            .insert(public_mode);
        if let Some((existing_pack, existing_owner)) = mode_contracts.get(&parser_id) {
            require(
                existing_pack == &pack_id && existing_owner == &owner,
                "modes sharing a parser disagree on pack or owner",
            )?;
        } else {
            mode_contracts.insert(parser_id, (pack_id, owner));
        }
    }
    require(
        pre_parse_transform_count == 1
            && objectscript_modes
                == BTreeSet::from([
                    (
                        OBJECTSCRIPT_EXPORT_XML_MODE_ID.to_string(),
                        "objectscript-export-xml".to_string(),
                        OBJECTSCRIPT_UDL_PARSER_ID.to_string(),
                    ),
                    (
                        "mode.objectscript-routine".to_string(),
                        "objectscript-routine".to_string(),
                        "parse.objectscript-routine".to_string(),
                    ),
                    (
                        "mode.objectscript-udl".to_string(),
                        "objectscript-udl".to_string(),
                        OBJECTSCRIPT_UDL_PARSER_ID.to_string(),
                    ),
                ]),
        "ObjectScript runnable modes, parser reuse, or transform ownership drifted",
    )?;

    let frozen_current_modes = projectatlas_core::language::LANGUAGE_SPECS
        .iter()
        .map(|spec| spec.language.to_string())
        .collect::<BTreeSet<_>>();
    require(
        frozen_current_modes.len() == 63 && frozen_current_modes.is_subset(&public_modes),
        "the current 63 public modes were shrunk",
    )?;
    validate_crosswalk(registry, &mode_ids)?;

    let mut parser_ids = BTreeSet::new();
    let mut objectscript_parsers = BTreeMap::new();
    for parser in parsers {
        let parser_id = required_string(parser, "parser_id")?;
        require(parser_ids.insert(parser_id.clone()), "duplicate parser ID")?;
        let owner = required_string(parser, "owner")?;
        let pack_id = required_string(parser, "pack_id")?;
        let expected_owner = match pack_id.as_str() {
            DEFAULT_CORE_PACK_ID => DEFAULT_CORE_OWNER,
            BROAD_LANGUAGE_PACK_ID => BROAD_LANGUAGE_PACK_OWNER,
            _ => return Err(io::Error::other("parser has an unknown pack owner").into()),
        };
        let expected_kind = match parser_id.as_str() {
            "parse.cargo-lock" | "parse.cargo-manifest" => "builtin-manifest",
            _ => "tree-sitter-or-vetted-parser",
        };
        let parser_suffix = parser_id
            .strip_prefix("parse.")
            .filter(|suffix| !suffix.is_empty())
            .ok_or_else(|| io::Error::other(format!("invalid parser ID {parser_id}")))?;
        require(
            required_string(parser, "kind")? == expected_kind
                && parser["grammar_symbol"].is_null()
                && parser["tree_sitter_abi"].is_null()
                && required_string(parser, "asset_id")? == format!("asset.{parser_suffix}")
                && required_string(parser, "query_pack_id")? == format!("queries.{parser_suffix}"),
            "parser kind, ABI, or asset conventions drifted",
        )?;
        require(
            owner == expected_owner,
            "parser owner does not match its pack",
        )?;
        require(
            string_list(parser, "required_platforms")? == expected_platforms,
            "parser required-platform matrix drifted",
        )?;
        require(
            parser["advertised"] == false
                && parser["evidence_state"] == "pending-asset-fixture-and-platform-verification",
            "parser is advertised without platform and fixture evidence",
        )?;
        let normalized_modes = string_list(parser, "normalized_modes")?
            .into_iter()
            .collect::<BTreeSet<_>>();
        if parser_id.starts_with("parse.objectscript-") {
            objectscript_parsers.insert(parser_id.clone(), normalized_modes.clone());
        }
        require(
            modes_by_parser.get(&parser_id) == Some(&normalized_modes),
            "parser reuse does not reconcile with its mode set",
        )?;
        let (mode_pack, mode_owner) = mode_contracts
            .get(&parser_id)
            .ok_or_else(|| io::Error::other("parser has no accepted mode"))?;
        require(
            mode_pack == &pack_id && mode_owner == &owner,
            "mode and parser ownership drifted",
        )?;
    }
    require(
        modes_by_parser.keys().all(|id| parser_ids.contains(id)),
        "mode references a missing parser capability",
    )?;
    require(
        objectscript_parsers
            == BTreeMap::from([
                (
                    "parse.objectscript-routine".to_string(),
                    BTreeSet::from(["objectscript-routine".to_string()]),
                ),
                (
                    OBJECTSCRIPT_UDL_PARSER_ID.to_string(),
                    BTreeSet::from([
                        "objectscript-export-xml".to_string(),
                        "objectscript-udl".to_string(),
                    ]),
                ),
            ]),
        "ObjectScript must own exactly two parser grammars",
    )?;
    let multi_mode_parsers = modes_by_parser
        .iter()
        .filter(|(_, modes)| modes.len() > 1)
        .map(|(parser, modes)| (parser.clone(), modes.clone()))
        .collect::<BTreeMap<_, _>>();
    require(
        multi_mode_parsers
            == BTreeMap::from([
                (
                    "parse.c".to_string(),
                    BTreeSet::from(["c".to_string(), "h".to_string()]),
                ),
                (
                    "parse.cpp".to_string(),
                    BTreeSet::from(["cpp".to_string(), "hpp".to_string()]),
                ),
                (
                    "parse.rust".to_string(),
                    BTreeSet::from(["rust".to_string(), "rust-build-script".to_string()]),
                ),
                (
                    OBJECTSCRIPT_UDL_PARSER_ID.to_string(),
                    BTreeSet::from([
                        "objectscript-export-xml".to_string(),
                        "objectscript-udl".to_string(),
                    ]),
                ),
                (
                    "parse.yaml".to_string(),
                    BTreeSet::from([
                        "kubernetes".to_string(),
                        "kustomize".to_string(),
                        "yaml".to_string(),
                    ]),
                ),
            ]),
        "multi-mode parser reuse does not match the complete accepted map",
    )?;

    let computed_digest = accepted_set_digest_from_effective(registry)?;
    require(
        source_registry["accepted_set_digest_algorithm"] == "sha256"
            && required_string(source_registry, "accepted_set_digest")? == computed_digest,
        "accepted-set digest does not bind the registry contract",
    )?;
    require(
        computed_digest == EXPECTED_ACCEPTED_SET_DIGEST,
        "accepted set differs from the independently pinned digest",
    )
}

fn validate_accepted_language_registry(registry: &Value) -> Result<(), Box<dyn Error>> {
    validate_compact_schema(registry)?;
    let effective = materialize_registry(registry)?;
    validate_effective_registry(registry, &effective)
}

fn registry() -> Result<Value, Box<dyn Error>> {
    serde_json::from_str(CAPABILITY_REGISTRY).map_err(Into::into)
}

fn mode_mut<'a>(registry: &'a mut Value, mode_id: &str) -> Result<&'a mut Value, Box<dyn Error>> {
    registry["modes"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("modes is not an array"))?
        .iter_mut()
        .find(|mode| mode["mode_id"] == mode_id)
        .ok_or_else(|| io::Error::other(format!("missing test mode {mode_id}")).into())
}

fn parser_mut<'a>(
    registry: &'a mut Value,
    parser_id: &str,
) -> Result<&'a mut Value, Box<dyn Error>> {
    registry["parsers"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("parsers is not an array"))?
        .iter_mut()
        .find(|parser| parser["parser_id"] == parser_id)
        .ok_or_else(|| io::Error::other(format!("missing test parser {parser_id}")).into())
}

fn transform_mut(registry: &mut Value) -> Result<&mut Value, Box<dyn Error>> {
    mode_mut(registry, OBJECTSCRIPT_EXPORT_XML_MODE_ID)?
        .get_mut("pre_parse_transform")
        .ok_or_else(|| io::Error::other("missing ObjectScript export transform").into())
}

fn crosswalk_mut<'a>(
    registry: &'a mut Value,
    accepted_name_id: &str,
) -> Result<&'a mut Value, Box<dyn Error>> {
    registry["accepted_language_crosswalk"]["entries"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("accepted language crosswalk is not an array"))?
        .iter_mut()
        .find(|row| row["accepted_name_id"] == accepted_name_id)
        .ok_or_else(|| {
            io::Error::other(format!("missing test crosswalk row {accepted_name_id}")).into()
        })
}

fn set_override(row: &mut Value, field: &str, value: Value) -> Result<(), Box<dyn Error>> {
    let row = row
        .as_object_mut()
        .ok_or_else(|| io::Error::other("test registry row is not an object"))?;
    let overrides = row.entry("overrides").or_insert_with(|| json!({}));
    overrides
        .as_object_mut()
        .ok_or_else(|| io::Error::other("test overrides is not an object"))?
        .insert(field.to_string(), value);
    Ok(())
}

fn require_rejected(registry: &Value, mutation: &str) -> Result<(), Box<dyn Error>> {
    require(
        validate_accepted_language_registry(registry).is_err(),
        format!("{mutation} mutation was accepted"),
    )
}

#[test]
fn accepted_language_union_is_complete_and_pending() -> Result<(), Box<dyn Error>> {
    validate_accepted_language_registry(&registry()?)
}

#[test]
fn accepted_language_union_rejects_contract_mutations() -> Result<(), Box<dyn Error>> {
    let valid = registry()?;
    validate_accepted_language_registry(&valid)?;

    let mut count_only = valid.clone();
    let modes = count_only["modes"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("modes is not an array"))?;
    let bash = modes
        .iter()
        .position(|mode| mode["mode_id"] == "mode.bash")
        .ok_or_else(|| io::Error::other("missing Bash mode"))?;
    let mut replacement = modes.remove(bash);
    replacement["mode_id"] = json!("mode.count-only-extra");
    replacement["public_mode"] = json!("count-only-extra");
    modes.push(replacement);
    require_rejected(&count_only, "count-only parity")?;

    let mut missing = valid.clone();
    missing["modes"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("modes is not an array"))?
        .retain(|mode| mode["mode_id"] != "mode.bash");
    require_rejected(&missing, "missing required entry")?;

    let mut duplicate = valid.clone();
    let modes = duplicate["modes"]
        .as_array_mut()
        .ok_or_else(|| io::Error::other("modes is not an array"))?;
    let first_id = modes
        .first()
        .and_then(|mode| mode["mode_id"].as_str())
        .ok_or_else(|| io::Error::other("missing first mode ID"))?
        .to_string();
    let second = modes
        .get_mut(1)
        .ok_or_else(|| io::Error::other("missing second mode"))?;
    second["mode_id"] = json!(first_id);
    require_rejected(&duplicate, "duplicate mode ID")?;

    let mut fake_alias = valid.clone();
    set_override(
        mode_mut(&mut fake_alias, "mode.bash")?,
        "alias_of",
        json!("mode.shell"),
    )?;
    require_rejected(&fake_alias, "fake alias override")?;

    let mut missing_fixture = valid.clone();
    set_override(
        mode_mut(&mut missing_fixture, "mode.bash")?,
        "fixture_ids",
        json!([]),
    )?;
    require_rejected(&missing_fixture, "missing fixture override")?;

    let mut missing_owner = valid.clone();
    set_override(
        mode_mut(&mut missing_owner, "mode.bash")?,
        "owner",
        json!(""),
    )?;
    require_rejected(&missing_owner, "missing owner override")?;

    let mut missing_platforms = valid.clone();
    set_override(
        mode_mut(&mut missing_platforms, "mode.bash")?,
        "required_platforms",
        json!([]),
    )?;
    require_rejected(&missing_platforms, "missing required-platform override")?;

    let mut inconsistent_reuse_owner = valid.clone();
    set_override(
        mode_mut(&mut inconsistent_reuse_owner, "mode.kustomize")?,
        "owner",
        json!(BROAD_LANGUAGE_PACK_OWNER),
    )?;
    require_rejected(
        &inconsistent_reuse_owner,
        "inconsistent shared-parser ownership override",
    )?;

    let mut unknown_override = valid.clone();
    set_override(
        mode_mut(&mut unknown_override, "mode.bash")?,
        "unsupported_contract_field",
        json!(true),
    )?;
    require_rejected(&unknown_override, "unknown override field")?;

    let mut legacy_expanded_field = valid.clone();
    mode_mut(&mut legacy_expanded_field, "mode.bash")?["owner"] = json!("projectatlas-symbols");
    require_rejected(&legacy_expanded_field, "legacy expanded mode field")?;

    let mut missing_parser_override = valid.clone();
    parser_mut(&mut missing_parser_override, "parse.cargo-manifest")?
        .as_object_mut()
        .ok_or_else(|| io::Error::other("test parser is not an object"))?
        .remove("overrides");
    require_rejected(&missing_parser_override, "missing parser-kind override")?;

    let mut fake_reuse = valid.clone();
    mode_mut(&mut fake_reuse, "mode.kubernetes")?["parser_id"] = json!("parse.c");
    mode_mut(&mut fake_reuse, "mode.h")?["parser_id"] = json!("parse.yaml");
    parser_mut(&mut fake_reuse, "parse.c")?["normalized_modes"] = json!(["c", "kubernetes"]);
    parser_mut(&mut fake_reuse, "parse.yaml")?["normalized_modes"] =
        json!(["h", "kustomize", "yaml"]);
    require_rejected(&fake_reuse, "count-preserving fake parser reuse")?;

    let mut missing_transform = valid.clone();
    mode_mut(&mut missing_transform, OBJECTSCRIPT_EXPORT_XML_MODE_ID)?
        .as_object_mut()
        .ok_or_else(|| io::Error::other("ObjectScript export mode is not an object"))?
        .remove("pre_parse_transform");
    require_rejected(&missing_transform, "missing required pre-parse transform")?;

    let mut extra_transform = valid.clone();
    let transform = transform_mut(&mut extra_transform)?.clone();
    mode_mut(&mut extra_transform, "mode.objectscript-routine")?["pre_parse_transform"] = transform;
    require_rejected(&extra_transform, "pre-parse transform on another mode")?;

    let mut changed_transform_id = valid.clone();
    transform_mut(&mut changed_transform_id)?["transform_id"] =
        json!("transform.objectscript-export-xml-to-udl-v2");
    require_rejected(&changed_transform_id, "changed transform identity")?;

    let mut changed_transform_version = valid.clone();
    transform_mut(&mut changed_transform_version)?["version"] = json!(2);
    require_rejected(&changed_transform_version, "changed transform version")?;

    let mut wrong_transform_mode = valid.clone();
    transform_mut(&mut wrong_transform_mode)?["target_mode_id"] = json!("mode.objectscript-udl");
    require_rejected(&wrong_transform_mode, "wrong transform target mode")?;

    let mut wrong_transform_parser = valid.clone();
    transform_mut(&mut wrong_transform_parser)?["target_parser_id"] =
        json!("parse.objectscript-routine");
    require_rejected(&wrong_transform_parser, "wrong transform target parser")?;

    let mut nondeterministic_transform = valid.clone();
    transform_mut(&mut nondeterministic_transform)?["deterministic"] = json!(false);
    require_rejected(&nondeterministic_transform, "nondeterministic transform")?;

    let mut zero_transform_limit = valid.clone();
    transform_mut(&mut zero_transform_limit)?["limits"]["max_records"] = json!(0);
    require_rejected(&zero_transform_limit, "zero transform limit")?;

    let mut excessive_transform_limit = valid.clone();
    transform_mut(&mut excessive_transform_limit)?["limits"]["max_input_bytes"] = json!(2_000_001);
    require_rejected(&excessive_transform_limit, "excessive transform limit")?;

    let mut cancellation_disabled = valid.clone();
    transform_mut(&mut cancellation_disabled)?["cancellation"]["enabled"] = json!(false);
    require_rejected(&cancellation_disabled, "disabled transform cancellation")?;

    for field in [
        "original_file_identity",
        "per_record_provenance",
        "every_derived_fact",
        "every_diagnostic",
    ] {
        let mut missing_source_mapping = valid.clone();
        transform_mut(&mut missing_source_mapping)?["source_mapping"][field] = json!(false);
        require_rejected(
            &missing_source_mapping,
            &format!("missing transform source mapping {field}"),
        )?;
    }

    for field in [
        "dtd",
        "entity_expansion",
        "external_resources",
        "schema_fetch",
        "execution",
    ] {
        let mut relaxed_security = valid.clone();
        transform_mut(&mut relaxed_security)?["security"][field] = json!("allowed");
        require_rejected(
            &relaxed_security,
            &format!("relaxed transform security policy {field}"),
        )?;
    }

    let mut unrelated_fallback = valid.clone();
    transform_mut(&mut unrelated_fallback)?["failure_policy"]["unrelated_parser_fallback"] =
        json!(true);
    require_rejected(&unrelated_fallback, "unrelated parser fallback")?;

    let mut guessed_symbols = valid.clone();
    transform_mut(&mut guessed_symbols)?["failure_policy"]["guessed_symbols_after_failure"] =
        json!(true);
    require_rejected(&guessed_symbols, "guessed symbols after transform failure")?;

    let mut invalid_multi_record = valid.clone();
    transform_mut(&mut invalid_multi_record)?["failure_policy"]["multi_record_input"] =
        json!("parse-first-record-only");
    require_rejected(
        &invalid_multi_record,
        "invalid multi-record transform behavior",
    )?;

    let mut recomputed_transform_drift = valid.clone();
    transform_mut(&mut recomputed_transform_drift)?["version"] = json!(2);
    let recomputed_transform_digest = accepted_set_digest(&recomputed_transform_drift)?;
    require(
        recomputed_transform_digest != EXPECTED_ACCEPTED_SET_DIGEST,
        "transform drift did not change the accepted semantic digest",
    )?;
    recomputed_transform_drift["accepted_set_digest"] = json!(recomputed_transform_digest);
    require_rejected(
        &recomputed_transform_drift,
        "transform drift with recomputed stored digest",
    )?;

    let mut unsupported_claim = valid.clone();
    let bash = mode_mut(&mut unsupported_claim, "mode.bash")?;
    set_override(bash, "achieved_claims", json!(["detected"]))?;
    set_override(bash, "advertisement", json!("advertised"))?;
    unsupported_claim["achieved_manifest"] = json!({"state": "achieved"});
    require_rejected(
        &unsupported_claim,
        "unsupported achieved or advertised claim",
    )?;

    let mut recomputed_crosswalk = valid.clone();
    crosswalk_mut(&mut recomputed_crosswalk, "accepted.ada")?["standard_name"] = json!("Ada drift");
    let recomputed_digest = accepted_set_digest(&recomputed_crosswalk)?;
    recomputed_crosswalk["accepted_set_digest"] = json!(recomputed_digest);
    require_rejected(
        &recomputed_crosswalk,
        "crosswalk drift with recomputed stored digest",
    )?;

    let mut current_mode_shrinkage = valid;
    mode_mut(&mut current_mode_shrinkage, "mode.rust-build-script")?["public_mode"] =
        json!("removed-current-mode");
    require_rejected(&current_mode_shrinkage, "current 63-mode shrinkage")
}
