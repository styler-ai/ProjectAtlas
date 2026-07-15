//! Bounded typed repository-graph query services shared by adapters and evaluations.

use crate::{ServiceError, ServiceResult};
use projectatlas_core::graph::{GraphRelationKind, PublicationState};
use projectatlas_db::{
    AtlasStore, GraphRelationDirection, PersistedGraphRelation, PersistedGraphTarget,
};

/// One fully materialized adjacency step in a bounded graph path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedGraphHop {
    /// One-based traversal depth.
    pub depth: u8,
    /// Stable entity digest whose adjacency was read.
    pub source_entity_digest: [u8; 32],
    /// Deterministically ordered typed relations returned for this step.
    pub relations: Vec<PersistedGraphRelation>,
}

/// Fully materialized three-hop graph result bound to one structural publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BoundedThreeHopResult {
    /// Publication observed before and after result materialization.
    pub publication: PublicationState,
    /// Stable entity digest where traversal began.
    pub seed_entity_digest: [u8; 32],
    /// Exactly three deterministically ordered adjacency steps.
    pub hops: Vec<BoundedGraphHop>,
}

/// Require two observations to describe the same structural publication.
fn require_unchanged_publication(
    before: PublicationState,
    after: PublicationState,
) -> ServiceResult<PublicationState> {
    if before == after {
        Ok(before)
    } else {
        Err(ServiceError::PublicationDrift { before, after })
    }
}

/// Materialize exactly three bounded adjacency steps from one stable entity.
///
/// Each step follows the first internal target in the store's stable relation order while
/// retaining every decoded relation for that step. The publication is checked before the first
/// read and after materialization so the result cannot silently combine two structural epochs.
///
/// # Errors
///
/// Returns an error when the publication cannot be read, a step has no internal target, or the
/// active publication changes before the complete result is materialized.
pub fn bounded_three_hop(
    store: &AtlasStore,
    seed_entity_digest: [u8; 32],
    relation_kind: GraphRelationKind,
    limit_per_hop: u32,
) -> ServiceResult<BoundedThreeHopResult> {
    let before = store.publication_state()?;
    let mut source = seed_entity_digest;
    let mut hops = Vec::with_capacity(3);
    for depth in 1_u8..=3 {
        let relations = store.load_graph_adjacency(
            &source,
            GraphRelationDirection::Outbound,
            Some(relation_kind),
            limit_per_hop,
        )?;
        let next = relations.iter().find_map(|relation| match relation.target {
            PersistedGraphTarget::Internal(target) => Some(target),
            PersistedGraphTarget::External { .. } => None,
        });
        let Some(next) = next else {
            require_unchanged_publication(before, store.publication_state()?)?;
            return Err(ServiceError::IncompleteGraphTraversal {
                depth,
                source_digest: source,
            });
        };
        hops.push(BoundedGraphHop {
            depth,
            source_entity_digest: source,
            relations,
        });
        source = next;
    }
    let publication = require_unchanged_publication(before, store.publication_state()?)?;
    Ok(BoundedThreeHopResult {
        publication,
        seed_entity_digest,
        hops,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use projectatlas_core::graph::{GraphEntityKind, IndexEpoch, StructuralSlot};
    use projectatlas_core::symbols::{
        CodeSymbol, ParserKind, RelationKind, SymbolGraph, SymbolKind, SymbolRelation,
    };
    use std::error::Error;

    #[test]
    fn task_arri_ut_arri_3_5_relations_use_shared_service_boundary() -> Result<(), Box<dyn Error>> {
        let mut store = AtlasStore::in_memory()?;
        let graph = SymbolGraph {
            path: "src/service-boundary.rs".to_owned(),
            language: Some("rust".to_owned()),
            parser: ParserKind::TreeSitter,
            symbols: vec![
                symbol("source", 1),
                symbol("second", 2),
                symbol("third", 3),
                symbol("fourth", 4),
            ],
            relations: vec![
                relation("source", "second", 1),
                relation("second", "third", 2),
                relation("third", "fourth", 3),
            ],
        };
        store.replace_symbol_graph(&graph)?;

        let source = store
            .load_graph_entities_by_qualified_name(GraphEntityKind::Declaration, "source", 1)?
            .into_iter()
            .next()
            .ok_or("service boundary source entity missing")?;
        let loaded = store
            .load_graph_entity(&source.stable_key_digest)?
            .ok_or("service boundary stable-key lookup missing")?;
        let calls = store.load_graph_adjacency(
            &source.stable_key_digest,
            GraphRelationDirection::Outbound,
            Some(GraphRelationKind::Calls),
            4,
        )?;
        let family = store.load_graph_relations_by_kind(GraphRelationKind::Calls, 4)?;
        let traversal = bounded_three_hop(
            &store,
            source.stable_key_digest,
            GraphRelationKind::Calls,
            4,
        )?;
        let second = store
            .load_graph_entities_by_qualified_name(GraphEntityKind::Declaration, "second", 1)?
            .into_iter()
            .next()
            .ok_or("service boundary second entity missing")?;
        let fourth = store
            .load_graph_entities_by_qualified_name(GraphEntityKind::Declaration, "fourth", 1)?
            .into_iter()
            .next()
            .ok_or("service boundary fourth entity missing")?;
        let Err(incomplete) = bounded_three_hop(
            &store,
            second.stable_key_digest,
            GraphRelationKind::Calls,
            4,
        ) else {
            return Err("two-edge suffix unexpectedly completed three hops".into());
        };

        require(
            loaded.stable_key_digest == source.stable_key_digest,
            "service boundary stable-key lookup returned another entity",
        )?;
        require(calls.len() == 1, "service boundary adjacency count drifted")?;
        require(
            family.len() == 3 && family.contains(&calls[0]),
            "relation-family and adjacency reads diverged",
        )?;
        require(
            traversal.hops.len() == 3 && traversal.hops.iter().all(|hop| hop.relations.len() == 1),
            "bounded service traversal did not materialize three typed hops",
        )?;
        require(
            traversal.publication == store.publication_state()?,
            "bounded service traversal publication drifted",
        )?;
        require(
            matches!(
                incomplete,
                ServiceError::IncompleteGraphTraversal {
                    depth: 3,
                    source_digest
                } if source_digest == fourth.stable_key_digest
            ),
            "bounded service traversal did not return its typed incomplete-path error",
        )?;
        Ok(())
    }

    #[test]
    fn task_arri_ut_arri_3_5_publication_consistency_rejects_epoch_drift()
    -> Result<(), Box<dyn Error>> {
        let before = PublicationState {
            active_slot: StructuralSlot::A,
            active_epoch: IndexEpoch::new(7),
        };
        let after = PublicationState {
            active_slot: StructuralSlot::A,
            active_epoch: IndexEpoch::new(8),
        };

        require(
            require_unchanged_publication(before, before)? == before,
            "unchanged publication was not preserved",
        )?;
        let Err(ServiceError::PublicationDrift {
            before: observed_before,
            after: observed_after,
        }) = require_unchanged_publication(before, after)
        else {
            return Err("incremental publication drift was not rejected".into());
        };
        require(
            observed_before == before && observed_after == after,
            "publication drift did not preserve both observations",
        )
    }

    fn relation(source: &str, target: &str, line: usize) -> SymbolRelation {
        SymbolRelation {
            path: "src/service-boundary.rs".to_owned(),
            source_name: source.to_owned(),
            target_name: target.to_owned(),
            kind: RelationKind::Calls,
            line,
            context: format!("{target}();"),
            parser: ParserKind::TreeSitter,
        }
    }

    fn require(condition: bool, message: &'static str) -> Result<(), Box<dyn Error>> {
        if condition {
            Ok(())
        } else {
            Err(message.into())
        }
    }

    fn symbol(name: &str, line: usize) -> CodeSymbol {
        CodeSymbol {
            path: "src/service-boundary.rs".to_owned(),
            language: Some("rust".to_owned()),
            name: name.to_owned(),
            kind: SymbolKind::Function,
            signature: format!("fn {name}()"),
            exported: false,
            documentation: None,
            line_start: line,
            line_end: line,
            parent: None,
            parser: ParserKind::TreeSitter,
            detail: Some("function_item".to_owned()),
        }
    }
}
