//! Purpose: Preserve eager iterable lookup and conservative comprehension writes.

use super::{BindingNamespace, BindingTarget, LexicalCollector, Scope};
use std::sync::Arc;
use tree_sitter::Node;

/// Compiler-owned iteration binders are distinct from conservative walrus barriers.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(super) enum BindingRole {
    /// An ordinary declaration or conservative value transition.
    #[default]
    Ordinary,
    /// A comprehension target cannot be rebound by a walrus in that loop chain.
    IterationTarget,
}

/// A loop environment keeps source-level write ownership separate from lookup.
#[derive(Clone, Copy)]
pub(super) struct ComprehensionScope {
    /// Containing writes become visible outside the complete expression.
    end: usize,
    /// The physical containing class is excluded from closure lookup.
    class_owner: bool,
    /// An error-free literal singleton preserves the existing single-body timeline.
    single_pass: bool,
}

/// Syntax validity follows recursive visits without a retained per-call stack.
#[derive(Clone, Copy)]
pub(super) struct VisitState {
    /// Any walrus in an iterable expression is forbidden, including nested lambdas.
    iterable: bool,
    /// Invalid expressions cannot grant calls or mutate independent outer bindings.
    invalid: bool,
}

/// An already admitted write moves into its owner only after syntax validation.
pub(super) struct PendingWrite {
    /// Existing local/global/nonlocal directive environment.
    scope: usize,
    /// Grammar-owned target identifier.
    name: String,
    /// Completion of the outermost containing comprehension.
    from: usize,
    /// Function locals retain compiler-wide reservation before their first write.
    reserves_scope: bool,
}

impl LexicalCollector<'_> {
    /// Keep first-iterable state while refusing loop-carried value guesses.
    pub(super) fn python_comprehension_write<E>(
        &mut self,
        node: Node<'_>,
        pattern: Node<'_>,
        mut scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<bool, E> {
        if self
            .python_comprehension
            .is_some_and(|state| state.iterable)
        {
            if let Some(state) = &mut self.python_comprehension {
                state.invalid = true;
            }
            return Ok(true);
        }
        if self.scopes[scope].comprehension.is_none() {
            // A lambda-owned walrus has its own ordinary function reservation.
            return Ok(false);
        }
        let Some(name) = self.shadow_name(pattern) else {
            if let Some(state) = &mut self.python_comprehension {
                state.invalid = true;
            }
            return Ok(true);
        };
        let mut from = 0;
        let mut body_end = None;
        let mut write_at = node.end_byte();
        let mut invalid = false;
        let mut shared_name = None;
        while let Some(comprehension) = self.scopes[scope].comprehension {
            check()?;
            from = from.max(comprehension.end);
            if body_end.is_some_and(|end| end != comprehension.end) {
                // An enclosing singleton sees a nested write only after that
                // nested expression completes, including its eager iterable.
                write_at = body_end.unwrap_or(write_at);
            }
            body_end = Some(comprehension.end);
            let barrier_at = if comprehension.single_pass {
                write_at
            } else {
                0
            };
            invalid |= comprehension.class_owner;
            let existing = self.scopes[scope].bindings.get(&name);
            invalid |=
                existing.is_some_and(|binding| binding.python_role == BindingRole::IterationTarget);
            if !self
                .python_loop_names
                .update_existing(scope, &name, barrier_at)
            {
                if !self.admit_binding() {
                    return Ok(true);
                }
                let shared = shared_name.get_or_insert_with(|| Arc::<str>::from(name.as_str()));
                self.python_loop_names
                    .insert_admitted(scope, Arc::clone(shared), barrier_at);
            }
            let Some(parent) = self.scopes[scope].parent else {
                break;
            };
            scope = parent;
        }
        if let Some(state) = &mut self.python_comprehension {
            state.invalid |= invalid;
            invalid = state.invalid;
        }
        if invalid || !self.admit_binding() {
            return Ok(true);
        }
        scope = self.value_declaration_scope(scope);
        self.python_walrus_writes.push(PendingWrite {
            scope,
            name,
            from,
            reserves_scope: self.scopes[scope].function,
        });
        Ok(true)
    }

    /// Restore validity context on cancellation before publishing any outer writes.
    pub(super) fn python_comprehension_scope<E>(
        &mut self,
        node: Node<'_>,
        parent: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        let previous = self.python_comprehension;
        let first_site = self.sites.len();
        let first_write = self.python_walrus_writes.len();
        self.python_comprehension = Some(VisitState {
            iterable: previous.is_some_and(|state| state.iterable),
            invalid: false,
        });
        let result = self.python_comprehension_contents(node, parent, check);
        let invalid = self.python_comprehension.is_some_and(|state| state.invalid);
        self.python_comprehension = previous;
        if result.is_err() || invalid || self.incomplete {
            self.python_walrus_writes.truncate(first_write);
        }
        result?;
        if invalid {
            if let Some(state) = &mut self.python_comprehension {
                state.invalid = true;
            }
            for site in &mut self.sites[first_site..] {
                check()?;
                site.name = None;
            }
        } else if previous.is_none() && !self.incomplete {
            // The pending slot was charged before growth. Move its name rather
            // than charging/copying it again when the whole expression is valid.
            for write in std::mem::take(&mut self.python_walrus_writes) {
                check()?;
                self.insert_admitted_binding(
                    write.scope,
                    write.name,
                    write.from,
                    write.reserves_scope,
                    BindingTarget::Shadow,
                    BindingNamespace::Value,
                );
            }
        }
        Ok(())
    }

    /// Recognize only an eager, unfiltered loop over an explicit singleton list.
    fn literal_singleton_comprehension<E>(
        node: Node<'_>,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<bool, E> {
        if !matches!(
            node.kind(),
            "list_comprehension" | "set_comprehension" | "dictionary_comprehension"
        ) || node.has_error()
        {
            return Ok(false);
        }
        let body = node.child_by_field_name("body");
        let mut clause = None;
        for child in node.named_children(&mut node.walk()) {
            check()?;
            if child.kind().contains("comment") || body.is_some_and(|body| body.id() == child.id())
            {
                continue;
            }
            if child.kind() != "for_in_clause" || clause.replace(child).is_some() {
                return Ok(false);
            }
        }
        let Some(clause) = clause else {
            return Ok(false);
        };
        for child in clause.children(&mut clause.walk()) {
            check()?;
            if child.kind() == "async" {
                return Ok(false);
            }
        }
        let Some(iterable) = clause.child_by_field_name("right") else {
            return Ok(false);
        };
        if iterable.kind() != "list" || iterable.has_error() {
            return Ok(false);
        }
        let mut count = 0;
        for child in iterable.named_children(&mut iterable.walk()) {
            check()?;
            if child.kind().contains("comment") {
                continue;
            }
            if child.kind() == "list_splat" {
                return Ok(false);
            }
            count += 1;
            if count > 1 {
                return Ok(false);
            }
        }
        Ok(count == 1)
    }

    /// Evaluate clauses outside their own binder, then evaluate the body in all loops.
    fn python_comprehension_contents<E>(
        &mut self,
        node: Node<'_>,
        parent: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        let body = node.child_by_field_name("body");
        let single_pass = Self::literal_singleton_comprehension(node, check)?;
        let mut scope = parent;
        for child in node.named_children(&mut node.walk()) {
            check()?;
            if body.is_some_and(|body| body.id() == child.id()) {
                continue;
            }
            if child.kind() != "for_in_clause" {
                self.visit(child, scope, check)?;
                continue;
            }
            let previous = scope;
            if !self.admit_scope() {
                return Ok(());
            }
            scope = self.scopes.len();
            self.scopes.push(Scope {
                parent: Some(self.python_closure_parent(previous)),
                function: true,
                comprehension: Some(ComprehensionScope {
                    end: node.end_byte(),
                    class_owner: self.scopes[parent].class,
                    single_pass,
                }),
                enclosing_visibility_floor: if node.kind() == "generator_expression" {
                    node.end_byte()
                } else {
                    0
                },
                owner: self.scopes[previous].owner,
                ..Scope::default()
            });
            if let Some(pattern) = child.child_by_field_name("left") {
                self.pattern_names(pattern, scope, 0, false, check)?;
                for binding in self.scopes[scope].bindings.values_mut() {
                    check()?;
                    binding.python_role = BindingRole::IterationTarget;
                }
            }
            for expression in child.named_children(&mut child.walk()) {
                check()?;
                let binding = child
                    .child_by_field_name("left")
                    .is_some_and(|left| left.id() == expression.id());
                let iterable = self
                    .python_comprehension
                    .is_some_and(|state| state.iterable);
                if !binding && let Some(state) = &mut self.python_comprehension {
                    state.iterable = true;
                }
                let result = self.visit(expression, if binding { scope } else { previous }, check);
                if let Some(state) = &mut self.python_comprehension {
                    state.iterable = iterable;
                }
                result?;
            }
        }
        if let Some(body) = body {
            self.visit(body, scope, check)?;
        }
        Ok(())
    }
}
