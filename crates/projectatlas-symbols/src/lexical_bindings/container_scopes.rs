//! Same-file identities for reopened namespaces and explicitly partial types.

use super::{BindingTarget, EcmascriptVarEnvironment, LexicalCollector, RustItemOwner, Scope};
use std::sync::Arc;
use tree_sitter::Node;

/// Type families that may explicitly contribute parts to one C# declaration.
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
enum PartialTypeKind {
    /// Partial reference type.
    Class,
    /// Partial value type.
    Struct,
    /// Partial interface member owner.
    Interface,
    /// Partial record reference type.
    RecordClass,
    /// Partial record value type.
    RecordStruct,
}

/// An exact child identity under one already admitted logical container.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
enum ContainerName {
    /// Named namespace, or its containing namespace's anonymous child.
    Namespace(Option<String>),
    /// Only explicit partial parts share a type environment.
    PartialType {
        /// Keep unrelated type families distinct.
        kind: PartialTypeKind,
        /// Grammar-owned declaration identifier.
        name: String,
        /// Parameter order/spelling must not combine different generic owners.
        parameters: Vec<String>,
    },
}

/// Parent ordinals avoid copying a complete qualified prefix into every key.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
pub(super) struct ContainerScopeKey {
    /// Existing logical namespace or containing type.
    parent: usize,
    /// Exact AST-proven child identity.
    name: ContainerName,
}

impl LexicalCollector<'_> {
    /// Borrow candidates without copying anonymous members into every ancestor.
    pub(super) fn cpp_namespace_target<E>(
        &self,
        scope: usize,
        name: &str,
        offset: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<Option<&BindingTarget>, E> {
        let mut current = Some(scope);
        let mut winner = None;
        while let Some(scope) = current {
            check()?;
            if self
                .cpp_inline_names
                .get(&scope)
                .and_then(|names| names.get(name))
                .is_some_and(|from| *from <= offset)
            {
                return Ok(Some(&BindingTarget::Unresolved));
            }
            if self.scopes[scope].cpp_container == super::cpp_owners::CppContainer::Namespace
                && self.scopes[scope].cpp_lookup.is_opaque_at(offset)
            {
                return Ok(Some(&BindingTarget::Unresolved));
            }
            // Values hide constructor names inside one environment. Distinct
            // anonymous/enclosing environments must instead retain competition.
            for bindings in self.scopes[scope]
                .bindings
                .get(name)
                .into_iter()
                .chain(self.scopes[scope].namespaces.get(name))
            {
                let end = bindings
                    .transitions
                    .partition_point(|binding| binding.from <= offset);
                let target = if bindings.runtime_mutated {
                    &BindingTarget::Unresolved
                } else if end == 0 {
                    if !bindings.reserves_scope {
                        continue;
                    }
                    &BindingTarget::Shadow
                } else {
                    let target = &bindings.transitions[end - 1].target;
                    if let Some((first, last)) = self.cpp_value_visibility(target) {
                        if offset < first {
                            continue;
                        }
                        if offset < last {
                            return Ok(Some(&BindingTarget::Unresolved));
                        }
                    }
                    target
                };
                if winner.is_some() {
                    return Ok(Some(&BindingTarget::Unresolved));
                }
                winner = Some(target);
                break;
            }
            if winner.is_none()
                && self.scopes[scope].cpp_container == super::cpp_owners::CppContainer::Type
                && self.scopes[scope].cpp_lookup.is_opaque_at(offset)
            {
                return Ok(Some(&BindingTarget::Unresolved));
            }
            current = self.scopes[scope].anonymous_namespace;
        }
        Ok(winner)
    }

    /// Redirect only the current namespace block's explicit exported bindings.
    pub(super) fn binding_scope(&self, scope: usize) -> usize {
        self.namespace_export_binding
            .filter(|(private, _)| *private == scope)
            .map_or(scope, |(_, shared)| shared)
    }

    /// Reuse logical owners while retaining a separate private TS block scope.
    pub(super) fn container_scope<E>(
        &mut self,
        node: Node<'_>,
        parent: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<Option<usize>, E> {
        let body = node.child_by_field_name("body");
        if body.is_none()
            && !(matches!(self.language, "csharp" | "c_sharp")
                && node.kind() == "record_declaration")
        {
            return Ok(None);
        }
        if node.has_error() {
            return Ok(None);
        }
        let namespace = (self.language == "cpp" && node.kind() == "namespace_definition")
            || (matches!(self.language, "csharp" | "c_sharp")
                && node.kind() == "namespace_declaration")
            || (self.ecmascript() && matches!(node.kind(), "internal_module" | "module"));
        if namespace {
            let Some(body) = body else { return Ok(None) };
            let mut names = Vec::new();
            if let Some(name) = node.child_by_field_name("name") {
                if !self.container_name_parts(name, 0, &mut names, check)? {
                    return Ok(None);
                }
            } else if self.language == "cpp" {
                names.push(None);
            } else {
                return Ok(None);
            }
            if names.is_empty() {
                return Ok(None);
            }
            let inline_parts = if self.language == "cpp" {
                Self::cpp_inline_namespace_parts(node, names.len(), check)?
            } else {
                Vec::new()
            };
            let mut shared = self.binding_scope(parent);
            for (index, name) in names.into_iter().enumerate() {
                check()?;
                let cpp_name = if self.language == "cpp" {
                    name.clone()
                } else {
                    None
                };
                let parent = shared;
                let Some(scope) =
                    self.intern_container(shared, ContainerName::Namespace(name), false)
                else {
                    return Ok(None);
                };
                if let Some(name) = cpp_name {
                    self.register_cpp_owner(parent, name, scope, node.start_byte());
                }
                if inline_parts.get(index) == Some(&true) {
                    self.scopes[scope].cpp_inline_from = Some(node.start_byte());
                }
                shared = scope;
            }
            let scope = if self.ecmascript() {
                let Some(private) = self.allocate_container(parent, false) else {
                    return Ok(None);
                };
                self.scopes[private].namespace_exports = Some(shared);
                private
            } else {
                shared
            };
            self.scopes[scope].container_body = Some(body.id());
            return Ok(Some(scope));
        }
        if !matches!(self.language, "csharp" | "c_sharp") {
            return Ok(None);
        }
        let mut cursor = node.walk();
        let partial = node.named_children(&mut cursor).any(|child| {
            child.kind() == "modifier"
                && child.utf8_text(self.content.as_bytes()).ok() == Some("partial")
        });
        if !partial {
            return Ok(None);
        }
        let kind = match node.kind() {
            "class_declaration" => PartialTypeKind::Class,
            "struct_declaration" => PartialTypeKind::Struct,
            "interface_declaration" => PartialTypeKind::Interface,
            "record_declaration" => {
                let mut cursor = node.walk();
                if node
                    .children(&mut cursor)
                    .any(|child| !child.is_named() && child.kind() == "struct")
                {
                    PartialTypeKind::RecordStruct
                } else {
                    PartialTypeKind::RecordClass
                }
            }
            _ => return Ok(None),
        };
        let Some(name) = node
            .child_by_field_name("name")
            .and_then(|name| self.identifier(name))
        else {
            return Ok(None);
        };
        let mut parameters = Vec::new();
        let mut cursor = node.walk();
        for list in node
            .named_children(&mut cursor)
            .filter(|child| child.kind() == "type_parameter_list")
        {
            let mut cursor = list.walk();
            for parameter in list.named_children(&mut cursor) {
                check()?;
                if !self.admit_binding() {
                    return Ok(None);
                }
                let Some(name) = parameter
                    .child_by_field_name("name")
                    .and_then(|name| self.identifier(name))
                else {
                    return Ok(None);
                };
                parameters.push(name);
            }
        }
        let scope = self.intern_container(
            parent,
            ContainerName::PartialType {
                kind,
                name,
                parameters,
            },
            true,
        );
        if let Some(scope) = scope {
            self.scopes[scope].container_body = body.map(|body| body.id());
        }
        Ok(scope)
    }

    /// Split only grammar-owned namespace components, never source punctuation.
    fn container_name_parts<E>(
        &mut self,
        node: Node<'_>,
        depth: usize,
        names: &mut Vec<Option<String>>,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<bool, E> {
        check()?;
        if depth >= crate::MAX_TREE_SITTER_AST_DEPTH || node.has_error() {
            return Ok(false);
        }
        if matches!(
            node.kind(),
            "identifier" | "namespace_identifier" | "property_identifier"
        ) {
            if names.len() >= super::MAX_LEXICAL_SCOPES {
                self.incomplete = true;
                return Ok(false);
            }
            let name = if self.ecmascript() {
                self.binding_name(node)
            } else if matches!(self.language, "csharp" | "c_sharp") {
                self.identifier(node)
            } else if self.language == "cpp" {
                node.utf8_text(self.content.as_bytes())
                    .ok()
                    .and_then(super::native_identifiers::cpp)
            } else {
                node.utf8_text(self.content.as_bytes())
                    .ok()
                    .filter(|name| !name.is_empty())
                    .map(ToString::to_string)
            };
            let Some(name) = name else {
                return Ok(false);
            };
            names.push(Some(name));
            return Ok(true);
        }
        if !matches!(
            node.kind(),
            "nested_namespace_specifier"
                | "qualified_name"
                | "nested_identifier"
                | "member_expression"
        ) {
            return Ok(false);
        }
        let before = names.len();
        let mut cursor = node.walk();
        for child in node
            .named_children(&mut cursor)
            .filter(|child| child.kind() != "comment")
        {
            if !self.container_name_parts(child, depth + 1, names, check)? {
                return Ok(false);
            }
        }
        Ok(names.len() > before)
    }

    /// Admit one cache entry only with its bounded owning scope.
    fn intern_container(
        &mut self,
        parent: usize,
        name: ContainerName,
        class: bool,
    ) -> Option<usize> {
        let anonymous = self.language == "cpp" && name == ContainerName::Namespace(None);
        let key = ContainerScopeKey { parent, name };
        if let Some(scope) = self.container_scopes.get(&key) {
            return Some(*scope);
        }
        let scope = self.allocate_container(parent, class)?;
        if anonymous {
            self.scopes[parent].anonymous_namespace = Some(scope);
        }
        self.container_scopes.insert(key, scope);
        Some(scope)
    }

    /// Allocate a concrete lexical owner under the existing shared scope budget.
    fn allocate_container(&mut self, parent: usize, class: bool) -> Option<usize> {
        if !self.admit_scope() {
            return None;
        }
        let scope = self.scopes.len();
        self.scopes.push(Scope {
            cpp_container: if self.language == "cpp" {
                super::cpp_owners::CppContainer::Namespace
            } else {
                super::cpp_owners::CppContainer::Lexical
            },
            parent: Some(parent),
            ecma_context: self.scopes[parent].ecma_context,
            ecma_root: self.scopes[parent].ecma_root,
            ecma_var_environment: if self.ecmascript() {
                EcmascriptVarEnvironment::Local
            } else {
                EcmascriptVarEnvironment::Enclosing
            },
            owner: self.scopes[parent].owner,
            class,
            rust_module: Arc::clone(&self.scopes[parent].rust_module),
            rust_item_owner: RustItemOwner::Local,
            ..Scope::default()
        });
        Some(scope)
    }
}
