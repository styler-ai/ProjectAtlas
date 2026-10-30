//! Purpose: Attach C++ declaration bodies to exact source container environments.

use super::{BindingNamespace, BindingTarget, LexicalCollector};
use tree_sitter::Node;

/// Namespace/type containers are distinct from lexical statement environments.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(super) enum CppContainer {
    /// A parameter, function or statement environment.
    #[default]
    Lexical,
    /// The file or one actual namespace body.
    Namespace,
    /// One actual class, struct or union body.
    Type,
}

/// Unsupported lookup contributors cannot authorize a guessed outer target.
#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(super) enum CppLookup {
    /// Direct bindings and their lexical parent are available.
    #[default]
    Complete,
    /// Using directives or bases leave additional names unknown.
    Opaque {
        /// Earliest source position at which the contributor is visible.
        from: usize,
    },
}

impl CppLookup {
    /// Preserve preceding source lookup when a later directive is unsupported.
    pub(super) fn is_opaque_at(self, offset: usize) -> bool {
        matches!(self, Self::Opaque { from } if from <= offset)
    }
}

/// A parsed declaration qualifier never falls back to its physical file scope.
#[derive(Clone, Copy)]
pub(super) enum CppDeclarationOwner {
    /// Ordinary declaration in its physical lexical environment.
    Unqualified,
    /// An explicit leading global qualifier with no named owner.
    Global,
    /// A unique same-file namespace or type body.
    Known(usize),
    /// No unique supported source owner is available.
    Unresolved,
}

/// Distinguish a missing child from an ambiguous or opaque lookup.
enum ChildOwner {
    /// Continue searching the enclosing declaration environment.
    Missing,
    /// One exact source body.
    Known(usize),
    /// Further fallback could select a false outer homonym.
    Unresolved,
}

impl LexicalCollector<'_> {
    /// Retain name-specific inline contributors without inferring promoted targets.
    ///
    /// Each new name is charged before copying; lexical depth and the shared name
    /// budget bound propagation. Consumers use one borrowed-name lookup per scope.
    pub(super) fn cpp_inline_blockers(&mut self, mut scope: usize, name: &str, mut from: usize) {
        while let Some(parent) = self.scopes[scope].parent {
            if let Some(inline) = self.scopes[scope].cpp_inline_from {
                from = from.max(inline);
                if let Some(existing) = self
                    .cpp_inline_names
                    .get_mut(&parent)
                    .and_then(|names| names.get_mut(name))
                {
                    *existing = (*existing).min(from);
                } else if self.admit_binding() {
                    self.cpp_inline_names
                        .entry(parent)
                        .or_default()
                        .insert(name.to_owned(), from);
                } else {
                    break;
                }
            } else if self.scopes[parent].anonymous_namespace != Some(scope) {
                break;
            }
            scope = parent;
        }
    }

    /// Record the first source position that introduces unknown lookup contributors.
    pub(super) fn cpp_opaque_from(&mut self, scope: usize, from: usize) {
        let mut current = Some(scope);
        while let Some(scope) = current {
            let earliest = match self.scopes[scope].cpp_lookup {
                CppLookup::Complete => from,
                CppLookup::Opaque { from: existing } => existing.min(from),
            };
            self.scopes[scope].cpp_lookup = CppLookup::Opaque { from: earliest };
            current = self.scopes[scope].parent.filter(|parent| {
                self.scopes[scope].cpp_inline_from.is_some()
                    || self.scopes[*parent].anonymous_namespace == Some(scope)
            });
        }
    }

    /// Associate grammar-owned inline tokens with their following namespace component.
    pub(super) fn cpp_inline_namespace_parts<E>(
        node: Node<'_>,
        count: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<Vec<bool>, E> {
        let mut parts = vec![false; count];
        let mut inline = false;
        for child in node.children(&mut node.walk()) {
            check()?;
            inline |= child.kind() == "inline";
        }
        let Some(name) = node.child_by_field_name("name") else {
            if let Some(first) = parts.first_mut() {
                *first = inline;
            }
            return Ok(parts);
        };
        let mut cursor = name.walk();
        let mut index = 0;
        loop {
            check()?;
            let current = cursor.node();
            if current.kind() == "inline" {
                inline = true;
            }
            if matches!(current.kind(), "namespace_identifier" | "identifier") {
                if let Some(part) = parts.get_mut(index) {
                    *part = inline;
                }
                inline = false;
                index += 1;
            }
            if cursor.goto_first_child() {
                continue;
            }
            while !cursor.goto_next_sibling() {
                if !cursor.goto_parent() {
                    return Ok(parts);
                }
            }
        }
    }

    /// Retain one cooked child name under an existing scope, charging before growth.
    pub(super) fn register_cpp_owner(
        &mut self,
        parent: usize,
        name: String,
        scope: usize,
        from: usize,
    ) {
        self.cpp_inline_blockers(parent, &name, from);
        if let Some(existing) = self
            .cpp_owners
            .get_mut(&parent)
            .and_then(|names| names.get_mut(&name))
        {
            if *existing != Some(scope) {
                *existing = None;
            }
        } else if self.admit_binding() {
            self.cpp_owners
                .entry(parent)
                .or_default()
                .insert(name, Some(scope));
        }
    }

    /// Register only an actual named type body, never a forward declaration.
    pub(super) fn register_cpp_type<E>(
        &mut self,
        node: Node<'_>,
        parent: usize,
        scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        if node.child_by_field_name("body").is_none() {
            return Ok(());
        }
        self.scopes[scope].cpp_container = CppContainer::Type;
        for child in node.named_children(&mut node.walk()) {
            check()?;
            if child.kind() == "base_class_clause" {
                self.scopes[scope].cpp_lookup = CppLookup::Opaque { from: 0 };
            }
        }
        if !node.has_error()
            && self.declaration(node).is_some()
            && let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name| self.identifier(name))
        {
            self.register_cpp_owner(parent, name, scope, node.start_byte());
        }
        Ok(())
    }

    /// Wildcard contributors are opaque; explicit using names block only that name.
    pub(super) fn cpp_using<E>(
        &mut self,
        node: Node<'_>,
        scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        if node.kind() == "type_definition" {
            for declarator in node.children_by_field_name("declarator", &mut node.walk()) {
                check()?;
                if let Some(name) = crate::declarator_name(declarator, self.content)
                    .and_then(|name| super::native_identifiers::cpp(&name))
                {
                    self.bind_in(
                        scope,
                        name.clone(),
                        node.end_byte(),
                        false,
                        BindingTarget::Shadow,
                        BindingNamespace::Namespace,
                    );
                    self.bind(scope, name, node.end_byte(), false, BindingTarget::Shadow);
                }
            }
            return Ok(());
        }
        if matches!(
            node.kind(),
            "namespace_alias_definition" | "alias_declaration"
        ) {
            if let Some(name) = node
                .child_by_field_name("name")
                .and_then(|name| self.identifier(name))
            {
                self.bind_in(
                    scope,
                    name.clone(),
                    node.end_byte(),
                    false,
                    BindingTarget::Shadow,
                    BindingNamespace::Namespace,
                );
                self.bind(scope, name, node.end_byte(), false, BindingTarget::Shadow);
            }
            return Ok(());
        }
        if !matches!(node.kind(), "using_declaration" | "using_directive") {
            return Ok(());
        }
        for child in node.children(&mut node.walk()) {
            check()?;
            if matches!(child.kind(), "namespace" | "enum") {
                self.cpp_opaque_from(scope, node.end_byte());
                return Ok(());
            }
        }
        let mut target = node
            .named_children(&mut node.walk())
            .filter(|node| !node.kind().contains("comment"))
            .last();
        for _ in 0..crate::MAX_TREE_SITTER_AST_DEPTH {
            check()?;
            let Some(node) = target else { return Ok(()) };
            if node.kind() == "qualified_identifier" {
                target = node.child_by_field_name("name");
                continue;
            }
            if let Some(name) = self.identifier(node) {
                let from = if self.scopes[scope].class {
                    0
                } else {
                    node.end_byte()
                };
                self.bind_in(
                    scope,
                    name.clone(),
                    from,
                    false,
                    BindingTarget::Shadow,
                    BindingNamespace::Value,
                );
                self.bind_in(
                    scope,
                    name,
                    from,
                    false,
                    BindingTarget::Shadow,
                    BindingNamespace::Namespace,
                );
            }
            return Ok(());
        }
        Ok(())
    }

    /// Read qualifiers only along the declarator spine, not parameter/type text.
    pub(super) fn cpp_declaration_owner<E>(
        &self,
        node: Node<'_>,
        scope: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<CppDeclarationOwner, E> {
        if !matches!(node.kind(), "function_definition" | "function_declarator") {
            return Ok(CppDeclarationOwner::Unqualified);
        }
        let mut declarator = Some(node);
        for _ in 0..crate::MAX_TREE_SITTER_AST_DEPTH {
            check()?;
            let Some(node) = declarator else {
                return Ok(CppDeclarationOwner::Unqualified);
            };
            if node.kind() == "qualified_identifier" {
                let mut parts = Vec::new();
                let mut absolute = false;
                if !self.cpp_declarator_prefix(node, &mut parts, &mut absolute, 0, check)? {
                    return Ok(CppDeclarationOwner::Unresolved);
                }
                if parts.is_empty() {
                    return Ok(if absolute {
                        CppDeclarationOwner::Global
                    } else {
                        CppDeclarationOwner::Unresolved
                    });
                }
                let mut context = Some(scope);
                while let Some(owner) = context {
                    check()?;
                    if self.scopes[owner].function || self.scopes[owner].class {
                        return Ok(CppDeclarationOwner::Unresolved);
                    }
                    context = self.scopes[owner].parent;
                }
                return Ok(self
                    .cpp_named_owner(
                        if absolute { 0 } else { scope },
                        &parts,
                        absolute,
                        node.start_byte(),
                        check,
                    )?
                    .map_or(CppDeclarationOwner::Unresolved, CppDeclarationOwner::Known));
            }
            if node.kind() == "parenthesized_declarator" {
                let mut inner = None;
                for child in node.named_children(&mut node.walk()) {
                    check()?;
                    if child.kind() == "ms_call_modifier" || child.kind().contains("comment") {
                        continue;
                    }
                    if inner.replace(child).is_some() {
                        return Ok(CppDeclarationOwner::Unresolved);
                    }
                }
                if inner.is_none() {
                    return Ok(CppDeclarationOwner::Unresolved);
                }
                declarator = inner;
            } else {
                declarator = node.child_by_field_name("declarator");
            }
        }
        Ok(CppDeclarationOwner::Unresolved)
    }

    /// Collect every scope component while leaving the terminal function name out.
    fn cpp_declarator_prefix<E>(
        &self,
        node: Node<'_>,
        parts: &mut Vec<String>,
        absolute: &mut bool,
        depth: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<bool, E> {
        check()?;
        if depth >= crate::MAX_TREE_SITTER_AST_DEPTH || node.has_error() {
            return Ok(false);
        }
        *absolute |= node.child(0).is_some_and(|child| child.kind() == "::");
        if let Some(scope) = node.child_by_field_name("scope")
            && !self.cpp_owner_parts(scope, parts, absolute, depth + 1, check)?
        {
            return Ok(false);
        }
        if let Some(name) = node.child_by_field_name("name")
            && name.kind() == "qualified_identifier"
        {
            return self.cpp_declarator_prefix(name, parts, absolute, depth + 1, check);
        }
        Ok(true)
    }

    /// Follow exact parsed namespace/type components, rejecting dependent owners.
    fn cpp_owner_parts<E>(
        &self,
        node: Node<'_>,
        parts: &mut Vec<String>,
        absolute: &mut bool,
        depth: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<bool, E> {
        check()?;
        if depth >= crate::MAX_TREE_SITTER_AST_DEPTH || node.has_error() {
            return Ok(false);
        }
        *absolute |= node.child(0).is_some_and(|child| child.kind() == "::");
        match node.kind() {
            "identifier" | "namespace_identifier" | "type_identifier" => {
                let Some(name) = self.identifier(node) else {
                    return Ok(false);
                };
                parts.push(name);
                Ok(true)
            }
            "qualified_identifier" | "nested_namespace_specifier" => {
                for child in node
                    .named_children(&mut node.walk())
                    .filter(|node| !node.kind().contains("comment"))
                {
                    if !self.cpp_owner_parts(child, parts, absolute, depth + 1, check)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Search lexical ancestors once, then exact children without prefix copies.
    fn cpp_named_owner<E>(
        &self,
        scope: usize,
        parts: &[String],
        absolute: bool,
        offset: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<Option<usize>, E> {
        let mut context = Some(scope);
        let mut owner = None;
        while let Some(scope) = context {
            check()?;
            match self.cpp_child_owner(scope, &parts[0], offset, check)? {
                ChildOwner::Known(found) => {
                    owner = Some(found);
                    break;
                }
                ChildOwner::Unresolved => return Ok(None),
                ChildOwner::Missing => {}
            }
            context = if absolute {
                None
            } else {
                self.scopes[scope].parent
            };
        }
        let Some(mut owner) = owner else {
            return Ok(None);
        };
        for part in &parts[1..] {
            check()?;
            match self.cpp_child_owner(owner, part, offset, check)? {
                ChildOwner::Known(found) => owner = found,
                ChildOwner::Missing | ChildOwner::Unresolved => return Ok(None),
            }
        }
        Ok(Some(owner))
    }

    /// Anonymous namespace injection retains competition rather than first-match bias.
    fn cpp_child_owner<E>(
        &self,
        scope: usize,
        name: &str,
        offset: usize,
        check: &mut impl FnMut() -> Result<(), E>,
    ) -> Result<ChildOwner, E> {
        let mut context = Some(scope);
        let mut winner = None;
        while let Some(scope) = context {
            check()?;
            if self
                .cpp_inline_names
                .get(&scope)
                .and_then(|names| names.get(name))
                .is_some_and(|from| *from <= offset)
            {
                return Ok(ChildOwner::Unresolved);
            }
            if self.scopes[scope].cpp_container == CppContainer::Namespace
                && self.scopes[scope].cpp_lookup.is_opaque_at(offset)
            {
                return Ok(ChildOwner::Unresolved);
            }
            if let Some(owner) = self
                .cpp_owners
                .get(&scope)
                .and_then(|names| names.get(name))
            {
                let Some(owner) = *owner else {
                    return Ok(ChildOwner::Unresolved);
                };
                if winner.is_some_and(|winner| winner != owner) {
                    return Ok(ChildOwner::Unresolved);
                }
                winner = Some(owner);
            } else if self.scopes[scope].namespaces.contains_key(name) {
                // A forward type, alias or explicit using hides an outer owner.
                return Ok(ChildOwner::Unresolved);
            }
            context = self.scopes[scope].anonymous_namespace;
        }
        Ok(winner.map_or(ChildOwner::Missing, ChildOwner::Known))
    }
}
