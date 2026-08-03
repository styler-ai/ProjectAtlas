//! Purpose: Enforce `ProjectAtlas` source-code policy beyond built-in Clippy lints.
//! Cargo-adjacent lint gate for `ProjectAtlas`-specific Rust contracts.

use proc_macro2::{TokenStream, TokenTree};
use regex::Regex;
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::fmt::{self, Display};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::{
    Expr, ExprLit, ExprMethodCall, ImplItemFn, ItemConst, ItemFn, ItemMod, ItemStatic, Lit, LitStr,
    Macro, Meta, Token,
};

/// Subcommand that runs strict string-contract linting.
const COMMAND_STRICT_STRINGS: &str = "strict-strings";
/// Successful process exit.
const EXIT_OK: u8 = 0;
/// Lint failure process exit.
const EXIT_FAILURE: u8 = 1;
/// Incorrect command usage process exit.
const EXIT_USAGE: u8 = 2;

/// Stable identifier for machine-specific absolute paths in public source.
const PRIVATE_PATH_RULE_ID: &str = "private-absolute-path";
/// One explicit escape hatch for intentional path-semantics examples.
const PRIVATE_PATH_FIXTURE_MARKER: &str = "projectatlas: path-fixture";
/// Small set of machine-specific path shapes that must use portable placeholders.
const PRIVATE_PATH_PATTERNS: &[&str] = &[
    r"(?i)[A-Z]:[\\/]Users[\\/][^\\/\s<>{}]+(?:[\\/]|$)",
    r"(?i)[A-Z]:[\\/]media[\\/]projects(?:[\\/]|$)",
    r"(?:^|[^A-Za-z0-9_.])/(?:home|Users)/[^/\s<>{}]+(?:/|$)",
    r"(?:^|[^A-Za-z0-9_.])/root(?:/|$)",
];

/// MCP project-selection response strings owned by the MCP adapter contract.
const MCP_PROJECT_SCHEMA_LITERALS: &[&str] =
    &["project", "root", "db", "config", "status", "active"];

/// Cross-module domain contract strings that must be centralized before reuse.
const DOMAIN_CONTRACT_LITERALS: &[&str] = &[
    "missing-purpose",
    "suggested-purpose-review",
    "stale-purpose",
    "purpose-agent-review-required",
    "duplicate-purpose",
    "repeated-temporary-folder",
    "repository-intelligence",
    "approved",
    "agent",
    "pass",
    "fail",
    "info",
    "warning",
    "error",
];

/// Reused e2e fixture path segments that should stay centralized.
const E2E_FIXTURE_PATH_LITERALS: &[&str] = &[
    "repo",
    "src",
    ".projectatlas",
    ".codex",
    "fake-codex.log",
    "fake-path",
    "isolated-home",
];

/// Existing repeated e2e path joins reviewed as ordinary fixture structure.
const E2E_ALLOWED_REPEATED_PATH_JOIN_LITERALS: &[&str] = &[
    ".cargo",
    ".github",
    ".gitignore",
    ".purpose",
    "AppData",
    "Cargo.toml",
    "Local",
    "README.md",
    "Roaming",
    "a.rs",
    "api",
    "app",
    "assets",
    "atlas_core",
    "b.rs",
    "bin",
    "build.gradle.kts",
    "ci.yml",
    "codex.cmd",
    "config.toml",
    "crates",
    "customers",
    "data",
    "detail.rs",
    "docs",
    "empty.rs",
    "engine.rs",
    "feature",
    "fixtures",
    "function_alias",
    "generated",
    "install-runtime.ps1",
    "install-runtime.sh",
    "kept-state.txt",
    "languages",
    "lib.rs",
    "live.rs",
    "local-cache",
    "logo.svg",
    "main.rs",
    "metadata.egg-info",
    "module_alias",
    "named_alias",
    "nested",
    "no_alias",
    "node_modules",
    "noise.rs",
    "outside",
    "package",
    "package_entry",
    "pkg",
    "plugin.json",
    "plugins",
    "projectatlas",
    "projectatlas-nonsource-files.toon",
    "projectatlas.claude.mcp.json",
    "projectatlas.cmd",
    "projectatlas.db",
    "projectatlas.exe",
    "projectatlas.mcp.json",
    "projectatlas.opencode.json",
    "projectatlas.toml",
    "projectatlas.toon",
    "public",
    "py",
    "python",
    "release.yml",
    "rogue",
    "runtimes",
    "rust",
    "scripts",
    "service.rs",
    "service.ts",
    "settings",
    "styles",
    "target",
    "tmp",
    "ts",
    "unrelated",
    "workflows",
    "x86_64-pc-windows-msvc",
];

/// Reviewed diagnostic format templates that must stay inline for Rust format macros.
const MCP_ALLOWED_INLINE_LITERALS: &[&str] = &[
    "absolute path '{}' has no existing ancestor",
    "ProjectAtlas index '{}' is missing for selected project root '{}'; {MISSING_INDEX_GUIDANCE}",
    "project path '{}' is not a directory",
    "MCP path '{original}' resolves to '{resolved_display}', not the selected project root '{project_root_display}'; {SELECTED_ROOT_ASSERTION_GUIDANCE}",
    "MCP path '{original}' resolves to '{resolved_display}', outside the selected project root '{project_root_display}'; {OUTSIDE_SELECTED_PROJECT_GUIDANCE}",
    "path {node_key:?} is not indexed in the MCP-bound project",
    "{message}; {OUTSIDE_SELECTED_PROJECT_GUIDANCE}",
    "invalid health severity '{trimmed}'; expected {expected}",
    "unsupported token trend window {window:?}; {TOKEN_TREND_WINDOW_ERROR_SUFFIX}",
    "CARGO_PKG_VERSION",
];

/// Strict string literal rules enabled for this repository.
const STRICT_STRING_RULES: &[StringLiteralRule] = &[
    StringLiteralRule {
        id: "mcp-production-inline-strings",
        description: "MCP production strings must be represented by typed structs, enums, constants, or a reviewed inline-format allowlist.",
        paths: &["crates/projectatlas-cli/src/mcp.rs"],
        ban_unlisted: true,
        literals: MCP_PROJECT_SCHEMA_LITERALS,
        allowed_literals: MCP_ALLOWED_INLINE_LITERALS,
    },
    StringLiteralRule {
        id: "domain-contract-inline-strings",
        description: "Domain contract strings must live in the owning enum, typed schema, or constants instead of being repeated inline.",
        paths: &[
            "crates/projectatlas-core/src/lib.rs",
            "crates/projectatlas-core/src/health.rs",
            "crates/projectatlas-core/src/toon.rs",
            "crates/projectatlas-db/src/lib.rs",
            "crates/projectatlas-cli/src/main.rs",
            "crates/projectatlas-cli/src/runtime.rs",
        ],
        ban_unlisted: false,
        literals: DOMAIN_CONTRACT_LITERALS,
        allowed_literals: &[],
    },
    StringLiteralRule {
        id: "e2e-fixture-path-inline-strings",
        description: "Repeated e2e fixture path segments must live in local constants instead of being repeated inline.",
        paths: &["crates/projectatlas-cli/tests/e2e.rs"],
        ban_unlisted: false,
        literals: E2E_FIXTURE_PATH_LITERALS,
        allowed_literals: &[],
    },
];

/// Repeated path-join rules enabled for this repository.
const REPEATED_PATH_JOIN_RULES: &[PathJoinLiteralRule] = &[PathJoinLiteralRule {
    id: "repeated-path-join-inline-strings",
    description: "Repeated path join string literals must be centralized as constants or reviewed into the fixture allowlist.",
    paths: &["crates/projectatlas-cli/tests/e2e.rs"],
    allowed_repeated_literals: E2E_ALLOWED_REPEATED_PATH_JOIN_LITERALS,
}];

/// Run the cargo-adjacent `ProjectAtlas` lint command.
fn main() -> ExitCode {
    match run(env::args_os().skip(1), &current_dir()) {
        Ok(()) => ExitCode::from(EXIT_OK),
        Err(LintError::Violations(violations)) => {
            let mut stderr = io::stderr().lock();
            if write_violations(&mut stderr, &violations).is_err() {
                return ExitCode::from(EXIT_FAILURE);
            }
            ExitCode::from(EXIT_FAILURE)
        }
        Err(error) => {
            let mut stderr = io::stderr().lock();
            if writeln!(stderr, "{error}").is_err() {
                return ExitCode::from(EXIT_FAILURE);
            }
            if matches!(error, LintError::Usage(_)) {
                ExitCode::from(EXIT_USAGE)
            } else {
                ExitCode::from(EXIT_FAILURE)
            }
        }
    }
}

/// Return the current process directory or the relative default when unavailable.
fn current_dir() -> PathBuf {
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Dispatch one lint command from normalized command-line arguments.
fn run(args: impl IntoIterator<Item = OsString>, current_dir: &Path) -> Result<(), LintError> {
    let args = normalized_args(args);
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return help();
    }
    let Some(command) = args.first() else {
        return help();
    };
    match command.as_str() {
        COMMAND_STRICT_STRINGS => run_strict_strings(current_dir),
        other => Err(LintError::Usage(format!(
            "unknown projectatlas lint command {other:?}"
        ))),
    }
}

/// Normalize direct binary and Cargo external-subcommand invocation shapes.
fn normalized_args(args: impl IntoIterator<Item = OsString>) -> Vec<String> {
    let mut args = args
        .into_iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if args.first().is_some_and(|arg| arg == "projectatlas-lints") {
        args.remove(0);
    }
    args
}

/// Print command help.
fn help() -> Result<(), LintError> {
    let mut stdout = io::stdout().lock();
    writeln!(
        stdout,
        "Usage: cargo projectatlas-lints {COMMAND_STRICT_STRINGS}\n\nCommands:\n  {COMMAND_STRICT_STRINGS}  Enforce ProjectAtlas string contracts and reject machine-specific paths in the current tracked tree."
    )
    .map_err(LintError::Io)
}

/// Run all strict string-contract rules from a workspace root.
fn run_strict_strings(root: &Path) -> Result<(), LintError> {
    let mut violations = Vec::new();
    for rule in STRICT_STRING_RULES {
        for relative_path in rule.paths {
            let path = root.join(relative_path);
            let source = fs::read_to_string(&path).map_err(|source| LintError::ReadFile {
                path: (*relative_path).to_string(),
                source,
            })?;
            violations.extend(lint_source(relative_path, rule, &source)?);
        }
    }
    for rule in REPEATED_PATH_JOIN_RULES {
        for relative_path in rule.paths {
            let path = root.join(relative_path);
            let source = fs::read_to_string(&path).map_err(|source| LintError::ReadFile {
                path: (*relative_path).to_string(),
                source,
            })?;
            violations.extend(lint_repeated_path_join_literals(
                relative_path,
                rule,
                &source,
            )?);
        }
    }
    violations.extend(lint_repository_private_paths(root)?);
    if violations.is_empty() {
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "projectatlas-lints: source policy passed").map_err(LintError::Io)
    } else {
        Err(LintError::Violations(violations))
    }
}

/// Reject machine-specific paths from the current tracked UTF-8 tree.
fn lint_repository_private_paths(root: &Path) -> Result<Vec<StringLiteralViolation>, LintError> {
    let output = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .map_err(LintError::ListGitFiles)?;
    if !output.status.success() {
        return Err(LintError::GitFileListFailed(output.status.code()));
    }
    let rules = private_path_rules()?;
    let mut violations = Vec::new();
    for raw_path in output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
    {
        let relative_path = std::str::from_utf8(raw_path).map_err(LintError::NonUtf8GitPath)?;
        let path = root.join(relative_path);
        let metadata = fs::symlink_metadata(&path).map_err(|source| LintError::ReadFile {
            path: relative_path.to_string(),
            source,
        })?;
        let source = if metadata.file_type().is_symlink() {
            let target = fs::read_link(&path).map_err(|source| LintError::ReadFile {
                path: relative_path.to_string(),
                source,
            })?;
            let Ok(target) = target.into_os_string().into_string() else {
                continue;
            };
            target
        } else {
            let bytes = fs::read(&path).map_err(|source| LintError::ReadFile {
                path: relative_path.to_string(),
                source,
            })?;
            let Ok(source) = String::from_utf8(bytes) else {
                continue;
            };
            source
        };
        violations.extend(lint_private_path_source(relative_path, &source, &rules));
    }
    Ok(violations)
}

/// Compile the fixed repository-private path rules.
fn private_path_rules() -> Result<Vec<Regex>, LintError> {
    PRIVATE_PATH_PATTERNS
        .iter()
        .map(|pattern| Regex::new(pattern))
        .collect::<Result<Vec<_>, _>>()
        .map_err(LintError::InvalidPrivatePathRule)
}

/// Return redacted path findings without retaining matched source text.
fn lint_private_path_source(
    relative_path: &str,
    source: &str,
    rules: &[Regex],
) -> Vec<StringLiteralViolation> {
    source
        .lines()
        .enumerate()
        .filter(|(_, line)| {
            !line.contains(PRIVATE_PATH_FIXTURE_MARKER)
                && rules.iter().any(|rule| rule.is_match(line))
        })
        .map(|(line, _)| StringLiteralViolation {
            path: relative_path.to_string(),
            line: line + 1,
            column: 1,
            rule_id: PRIVATE_PATH_RULE_ID,
            literal: "<redacted>".to_string(),
            description: "Machine-specific paths must use a portable placeholder or derived path.",
        })
        .collect()
}

/// Parse one Rust source file and return strict string-contract violations.
fn lint_source(
    relative_path: &str,
    rule: &'static StringLiteralRule,
    source: &str,
) -> Result<Vec<StringLiteralViolation>, LintError> {
    let file = syn::parse_file(source).map_err(|source| LintError::Parse {
        path: relative_path.to_string(),
        source,
    })?;
    let mut visitor = StringLiteralVisitor {
        relative_path,
        rule,
        centralized_depth: 0,
        violations: Vec::new(),
    };
    visitor.visit_file(&file);
    Ok(visitor.violations)
}

/// Parse one Rust source file and return repeated path-join literal violations.
fn lint_repeated_path_join_literals(
    relative_path: &str,
    rule: &'static PathJoinLiteralRule,
    source: &str,
) -> Result<Vec<StringLiteralViolation>, LintError> {
    let file = syn::parse_file(source).map_err(|source| LintError::Parse {
        path: relative_path.to_string(),
        source,
    })?;
    let mut visitor = PathJoinLiteralVisitor {
        occurrences: Vec::new(),
    };
    visitor.visit_file(&file);

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for occurrence in &visitor.occurrences {
        *counts.entry(occurrence.literal.clone()).or_default() += 1;
    }

    let violations = visitor
        .occurrences
        .into_iter()
        .filter(|occurrence| {
            counts
                .get(occurrence.literal.as_str())
                .is_some_and(|count| *count > 1)
                && !rule
                    .allowed_repeated_literals
                    .iter()
                    .any(|allowed| *allowed == occurrence.literal)
        })
        .map(|occurrence| StringLiteralViolation {
            path: relative_path.to_string(),
            line: occurrence.line,
            column: occurrence.column,
            rule_id: rule.id,
            literal: occurrence.literal,
            description: rule.description,
        })
        .collect();
    Ok(violations)
}

/// Path-scoped rule for protected exact string literals.
#[derive(Clone, Copy, Debug)]
struct StringLiteralRule {
    /// Stable rule identifier printed in diagnostics.
    id: &'static str,
    /// Human-readable rule description printed in diagnostics.
    description: &'static str,
    /// Repository-relative Rust source files protected by this rule.
    paths: &'static [&'static str],
    /// Exact string literal values protected by this rule.
    literals: &'static [&'static str],
    /// Whether every unallowed production string is prohibited.
    ban_unlisted: bool,
    /// Exact string literal values explicitly allowed inline.
    allowed_literals: &'static [&'static str],
}

/// Path-scoped rule for repeated `.join("...")` string literals.
#[derive(Clone, Copy, Debug)]
struct PathJoinLiteralRule {
    /// Stable rule identifier printed in diagnostics.
    id: &'static str,
    /// Human-readable rule description printed in diagnostics.
    description: &'static str,
    /// Repository-relative Rust source files protected by this rule.
    paths: &'static [&'static str],
    /// Repeated path-join literals reviewed as acceptable inline fixtures.
    allowed_repeated_literals: &'static [&'static str],
}

/// One source-aware string-contract violation.
#[derive(Debug)]
struct StringLiteralViolation {
    /// Repository-relative source path.
    path: String,
    /// One-based source line.
    line: usize,
    /// One-based source column.
    column: usize,
    /// Stable rule identifier.
    rule_id: &'static str,
    /// Exact literal value that violated the rule.
    literal: String,
    /// Human-readable rule description.
    description: &'static str,
}

impl Display for StringLiteralViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}:{}:{}: {}: inline string literal {:?}; {}",
            self.path, self.line, self.column, self.rule_id, self.literal, self.description
        )
    }
}

/// Syntax visitor that records protected inline string literals.
struct StringLiteralVisitor<'a> {
    /// Repository-relative source path.
    relative_path: &'a str,
    /// Rule used by this visitor.
    rule: &'static StringLiteralRule,
    /// Nesting depth for allowed centralization declarations.
    centralized_depth: usize,
    /// Collected violations.
    violations: Vec<StringLiteralViolation>,
}

/// One `.join("literal")` occurrence found by the path-join rule.
#[derive(Debug)]
struct PathJoinLiteralOccurrence {
    /// Exact path segment literal.
    literal: String,
    /// One-based source line.
    line: usize,
    /// One-based source column.
    column: usize,
}

/// Syntax visitor that records direct `.join("...")` calls.
struct PathJoinLiteralVisitor {
    /// Collected path-join literal occurrences.
    occurrences: Vec<PathJoinLiteralOccurrence>,
}

impl PathJoinLiteralVisitor {
    /// Record a direct path join literal.
    fn record_join_literal(&mut self, literal: &LitStr) {
        if !is_path_join_literal(&literal.value()) {
            return;
        }
        let location = literal.span().start();
        self.occurrences.push(PathJoinLiteralOccurrence {
            literal: literal.value(),
            line: location.line,
            column: location.column + 1,
        });
    }
}

/// Return whether a method-call `join` literal looks like a path segment, not a collection separator.
fn is_path_join_literal(literal: &str) -> bool {
    !matches!(
        literal,
        "" | " " | "\n" | "\r\n" | "\t" | "," | ", " | "; " | ": "
    )
}

impl<'ast> Visit<'ast> for PathJoinLiteralVisitor {
    /// Record only direct `.join("...")` calls; constants are the desired form.
    fn visit_expr_method_call(&mut self, expr: &'ast ExprMethodCall) {
        if expr.method == "join"
            && let Some(Expr::Lit(ExprLit {
                lit: Lit::Str(literal),
                ..
            })) = expr.args.first()
        {
            self.record_join_literal(literal);
        }
        visit::visit_expr_method_call(self, expr);
    }
}

impl StringLiteralVisitor<'_> {
    /// Record one string literal unless it is allowed by the current context.
    fn record_literal(&mut self, literal: &LitStr) {
        if self.centralized_depth > 0 {
            return;
        }
        let value = literal.value();
        if self
            .rule
            .allowed_literals
            .iter()
            .any(|allowed| *allowed == value)
        {
            return;
        }
        let protected = self
            .rule
            .literals
            .iter()
            .any(|protected| *protected == value);
        if !self.rule.ban_unlisted && !protected {
            return;
        }
        let location = literal.span().start();
        self.violations.push(StringLiteralViolation {
            path: self.relative_path.to_string(),
            line: location.line,
            column: location.column + 1,
            rule_id: self.rule.id,
            literal: value,
            description: self.rule.description,
        });
    }

    /// Visit a constant/static item as an allowed centralization declaration.
    fn visit_centralized_item(&mut self, visit: impl FnOnce(&mut Self)) {
        self.centralized_depth += 1;
        visit(self);
        self.centralized_depth -= 1;
    }

    /// Return whether a function is an explicit string centralization method.
    fn is_centralized_function_name(name: &syn::Ident) -> bool {
        name == "as_str"
    }

    /// Scan macro token trees because `syn` does not parse macro bodies as expressions.
    fn scan_macro_tokens(&mut self, tokens: TokenStream) {
        for token in tokens {
            match token {
                TokenTree::Group(group) => self.scan_macro_tokens(group.stream()),
                TokenTree::Literal(literal) => {
                    if let Ok(literal) = syn::parse_str::<LitStr>(&literal.to_string()) {
                        self.record_literal(&literal);
                    }
                }
                TokenTree::Ident(_) | TokenTree::Punct(_) => {}
            }
        }
    }

    /// Return whether an attribute marks test-only code.
    fn is_test_attribute(attribute: &syn::Attribute) -> bool {
        match &attribute.meta {
            Meta::Path(path) => path.is_ident("test"),
            Meta::List(list) => {
                if !list.path.is_ident("cfg") {
                    return false;
                }
                let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
                let Ok(cfg_items) = parser.parse2(list.tokens.clone()) else {
                    return false;
                };
                cfg_items
                    .iter()
                    .any(StringLiteralVisitor::cfg_meta_is_test_only)
            }
            Meta::NameValue(_) => false,
        }
    }

    /// Return whether one cfg expression can only be active for tests.
    fn cfg_meta_is_test_only(meta: &Meta) -> bool {
        match meta {
            Meta::Path(path) => path.is_ident("test"),
            Meta::List(list) if list.path.is_ident("all") => {
                let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
                parser.parse2(list.tokens.clone()).is_ok_and(|items| {
                    items
                        .iter()
                        .any(StringLiteralVisitor::cfg_meta_is_test_only)
                })
            }
            Meta::List(list) if list.path.is_ident("any") => {
                let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
                parser.parse2(list.tokens.clone()).is_ok_and(|items| {
                    !items.is_empty()
                        && items
                            .iter()
                            .all(StringLiteralVisitor::cfg_meta_is_test_only)
                })
            }
            Meta::List(_) | Meta::NameValue(_) => false,
        }
    }
}

impl<'ast> Visit<'ast> for StringLiteralVisitor<'_> {
    /// Ignore attributes so serialization metadata on local enums stays centralized.
    fn visit_attribute(&mut self, _attribute: &'ast syn::Attribute) {}

    /// Allow constants as local centralization points.
    fn visit_item_const(&mut self, item: &'ast ItemConst) {
        self.visit_centralized_item(|visitor| visit::visit_item_const(visitor, item));
    }

    /// Allow dedicated string-conversion functions as centralization points.
    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        if StringLiteralVisitor::is_centralized_function_name(&item.sig.ident) {
            self.visit_centralized_item(|visitor| visit::visit_item_fn(visitor, item));
            return;
        }
        visit::visit_item_fn(self, item);
    }

    /// Allow dedicated string-conversion methods as centralization points.
    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        if StringLiteralVisitor::is_centralized_function_name(&item.sig.ident) {
            self.visit_centralized_item(|visitor| visit::visit_impl_item_fn(visitor, item));
            return;
        }
        visit::visit_impl_item_fn(self, item);
    }

    /// Allow statics as local centralization points.
    fn visit_item_static(&mut self, item: &'ast ItemStatic) {
        self.visit_centralized_item(|visitor| visit::visit_item_static(visitor, item));
    }

    /// Ignore test-only modules so fixtures and assertion text do not create lint noise.
    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        if item
            .attrs
            .iter()
            .any(StringLiteralVisitor::is_test_attribute)
        {
            return;
        }
        visit::visit_item_mod(self, item);
    }

    /// Record ordinary Rust string literals.
    fn visit_expr_lit(&mut self, expr: &'ast ExprLit) {
        if let Lit::Str(literal) = &expr.lit {
            self.record_literal(literal);
        }
        visit::visit_expr_lit(self, expr);
    }

    /// Record macro-contained string literals such as `json!` schema keys.
    fn visit_macro(&mut self, macro_call: &'ast Macro) {
        if self.centralized_depth == 0 {
            self.scan_macro_tokens(macro_call.tokens.clone());
        }
        visit::visit_macro(self, macro_call);
    }
}

/// Errors returned by the cargo-adjacent lint command.
#[derive(Debug)]
enum LintError {
    /// Invalid command-line usage.
    Usage(String),
    /// Generic stream IO failure.
    Io(io::Error),
    /// File read failure.
    ReadFile {
        /// Repository-relative file path that could not be read.
        path: String,
        /// Source IO error.
        source: io::Error,
    },
    /// Rust parser failure.
    Parse {
        /// Repository-relative file path that could not be parsed.
        path: String,
        /// Source parser error.
        source: syn::Error,
    },
    /// Failed to start tracked-file discovery.
    ListGitFiles(io::Error),
    /// Tracked-file discovery returned an unsuccessful status.
    GitFileListFailed(Option<i32>),
    /// A tracked repository path was not UTF-8.
    NonUtf8GitPath(std::str::Utf8Error),
    /// A built-in private-path rule did not compile.
    InvalidPrivatePathRule(regex::Error),
    /// Strict string-contract violations.
    Violations(Vec<StringLiteralViolation>),
}

impl Display for LintError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(formatter, "{message}"),
            Self::Io(source) => write!(formatter, "io error: {source}"),
            Self::ReadFile { path, source } => {
                write!(formatter, "failed to read {path:?}: {source}")
            }
            Self::Parse { path, source } => write!(formatter, "failed to parse {path}: {source}"),
            Self::ListGitFiles(_) => write!(formatter, "failed to list tracked repository files"),
            Self::GitFileListFailed(code) => {
                write!(
                    formatter,
                    "tracked repository file listing failed with status {code:?}"
                )
            }
            Self::NonUtf8GitPath(_) => write!(formatter, "tracked repository path is not UTF-8"),
            Self::InvalidPrivatePathRule(_) => {
                write!(formatter, "built-in private-path rule is invalid")
            }
            Self::Violations(violations) => {
                write!(
                    formatter,
                    "{} repository source-policy violations",
                    violations.len()
                )
            }
        }
    }
}

impl std::error::Error for LintError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(source) | Self::ListGitFiles(source) | Self::ReadFile { source, .. } => {
                Some(source)
            }
            Self::Parse { source, .. } => Some(source),
            Self::NonUtf8GitPath(source) => Some(source),
            Self::InvalidPrivatePathRule(source) => Some(source),
            Self::Usage(_) | Self::GitFileListFailed(_) | Self::Violations(_) => None,
        }
    }
}

/// Write all repository source-policy diagnostics.
fn write_violations(
    writer: &mut impl Write,
    violations: &[StringLiteralViolation],
) -> io::Result<()> {
    writeln!(
        writer,
        "projectatlas-lints: {} repository source-policy violation(s)",
        violations.len()
    )?;
    for violation in violations {
        writeln!(writer, "{violation}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        E2E_FIXTURE_PATH_LITERALS, MCP_PROJECT_SCHEMA_LITERALS, PathJoinLiteralRule,
        StringLiteralRule, lint_repeated_path_join_literals, lint_repository_private_paths,
        lint_source, run_strict_strings,
    };
    use std::fs;
    use std::io;
    use std::path::Path;
    use std::process::{self, Command};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Rule used by parser-focused unit tests.
    const TEST_RULE: StringLiteralRule = StringLiteralRule {
        id: "test-rule",
        description: "test protected string rule",
        paths: &["demo.rs"],
        ban_unlisted: false,
        literals: MCP_PROJECT_SCHEMA_LITERALS,
        allowed_literals: &[],
    };

    /// Broad rule used by production-string tests.
    const BROAD_TEST_RULE: StringLiteralRule = StringLiteralRule {
        id: "test-broad-rule",
        description: "test broad string rule",
        paths: &["demo.rs"],
        ban_unlisted: true,
        literals: MCP_PROJECT_SCHEMA_LITERALS,
        allowed_literals: &["allowed inline format {value}"],
    };

    /// Rule used by e2e fixture path centralization tests.
    const E2E_FIXTURE_TEST_RULE: StringLiteralRule = StringLiteralRule {
        id: "test-e2e-fixture-path-rule",
        description: "test e2e fixture path string rule",
        paths: &["demo.rs"],
        ban_unlisted: false,
        literals: E2E_FIXTURE_PATH_LITERALS,
        allowed_literals: &[],
    };

    /// Rule used by repeated path-join tests.
    const PATH_JOIN_TEST_RULE: PathJoinLiteralRule = PathJoinLiteralRule {
        id: "test-path-join-rule",
        description: "test repeated path join rule",
        paths: &["demo.rs"],
        allowed_repeated_literals: &["reviewed-existing"],
    };

    /// Return a test error instead of panicking on a failed condition.
    fn require(condition: bool, message: &str) -> Result<(), Box<dyn std::error::Error>> {
        if condition {
            Ok(())
        } else {
            Err(io::Error::other(message.to_string()).into())
        }
    }

    /// Ordinary workspace tests must enforce the same repository source contracts as CI.
    #[test]
    fn repository_strict_string_contracts_pass() -> Result<(), Box<dyn std::error::Error>> {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| {
                io::Error::other("lint crate must be nested below the workspace root")
            })?;
        run_strict_strings(workspace_root)?;
        Ok(())
    }

    /// The current-tree rule scans tracked symlinks, accepts portable fixtures, and redacts findings.
    #[test]
    fn private_path_lint_is_redacted_and_fixture_aware() -> Result<(), Box<dyn std::error::Error>> {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| {
                io::Error::other("lint crate must be nested below the workspace root")
            })?;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let repo = workspace_root
            .join(".tmp")
            .join(format!("private-path-lint-{}-{nonce}", process::id()));
        fs::create_dir_all(&repo)?;

        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            require(
                Command::new("git")
                    .args(["init", "--quiet"])
                    .current_dir(&repo)
                    .status()?
                    .success(),
                "temporary Git repository initialization failed",
            )?;
            fs::write(repo.join("portable.txt"), "checkout = <project-root>\n")?;
            fs::write(
                repo.join("fixture.txt"),
                "example = C:/Users/example/ProjectAtlas # projectatlas: path-fixture\n",
            )?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(
                "/home/private-owner/ProjectAtlas", // projectatlas: path-fixture
                repo.join("private-link"),
            )?;
            #[cfg(not(unix))]
            fs::write(
                repo.join("private-link"),
                "checkout = C:/Users/private-owner/ProjectAtlas\n", // projectatlas: path-fixture
            )?;
            require(
                Command::new("git")
                    .args(["add", "."])
                    .current_dir(&repo)
                    .status()?
                    .success(),
                "temporary Git repository staging failed",
            )?;

            let violations = lint_repository_private_paths(&repo)?;
            require(violations.len() == 1, "private path was not rejected")?;
            let diagnostic = violations[0].to_string();
            require(
                diagnostic.contains("private-link:1:1") && diagnostic.contains("<redacted>"),
                "diagnostic lost its repository-relative redacted location",
            )?;
            require(
                !diagnostic.contains("private-owner") && !diagnostic.contains("ProjectAtlas"),
                "diagnostic exposed matched source",
            )
        })();
        let cleanup = fs::remove_dir_all(&repo);
        result?;
        cleanup?;
        Ok(())
    }

    /// Macro literals are parsed and reported as protected schema strings.
    #[test]
    fn strict_string_lint_flags_macro_schema_literals() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &TEST_RULE,
            r#"fn payload() { let _ = serde_json::json!({ "status": "active" }); }"#,
        )?;
        require(violations.len() == 2, "expected two macro violations")?;
        require(
            violations
                .iter()
                .any(|violation| violation.literal == "status"),
            "missing status macro violation",
        )?;
        require(
            violations
                .iter()
                .any(|violation| violation.literal == "active"),
            "missing active macro violation",
        )?;
        Ok(())
    }

    /// Constants are accepted as local centralization points.
    #[test]
    fn strict_string_lint_allows_constants() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &TEST_RULE,
            r#"const STATUS_FIELD: &str = "status"; fn payload() { let _ = STATUS_FIELD; }"#,
        )?;
        require(violations.is_empty(), "constant declaration was flagged")?;
        Ok(())
    }

    /// E2E fixture path segments must be centralized just like production contracts.
    #[test]
    fn strict_string_lint_flags_inline_e2e_fixture_paths() -> Result<(), Box<dyn std::error::Error>>
    {
        let violations = lint_source(
            "demo.rs",
            &E2E_FIXTURE_TEST_RULE,
            r#"fn fixture(temp: &std::path::Path) { let _ = temp.join("fake-codex.log"); }"#,
        )?;
        require(
            violations.len() == 1,
            "expected one e2e fixture path violation",
        )?;
        let violation = violations
            .first()
            .ok_or_else(|| io::Error::other("missing e2e fixture path violation"))?;
        require(
            violation.literal == "fake-codex.log",
            "e2e fixture path violation had the wrong value",
        )?;
        Ok(())
    }

    /// Constants remain the expected centralization point for e2e fixture paths.
    #[test]
    fn strict_string_lint_allows_e2e_fixture_path_constants()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &E2E_FIXTURE_TEST_RULE,
            r#"const FAKE_CODEX_LOG_FILE: &str = "fake-codex.log"; fn fixture(temp: &std::path::Path) { let _ = temp.join(FAKE_CODEX_LOG_FILE); }"#,
        )?;
        require(
            violations.is_empty(),
            "e2e fixture path constant was flagged",
        )?;
        Ok(())
    }

    /// Repeated unreviewed `.join("...")` path literals are reported.
    #[test]
    fn strict_string_lint_flags_repeated_path_join_literals()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_repeated_path_join_literals(
            "demo.rs",
            &PATH_JOIN_TEST_RULE,
            r#"
fn fixture(root: &std::path::Path) {
    let _ = root.join("future-fixture");
    let _ = root.join("future-fixture").join("leaf");
}
"#,
        )?;
        require(
            violations.len() == 2,
            "expected both repeated path joins to be flagged",
        )?;
        require(
            violations
                .iter()
                .all(|violation| violation.literal == "future-fixture"),
            "unexpected repeated path join literal was flagged",
        )?;
        Ok(())
    }

    /// One-off path joins are allowed to avoid fixture noise.
    #[test]
    fn strict_string_lint_allows_one_off_path_join_literals()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_repeated_path_join_literals(
            "demo.rs",
            &PATH_JOIN_TEST_RULE,
            r#"fn fixture(root: &std::path::Path) { let _ = root.join("one-off"); }"#,
        )?;
        require(violations.is_empty(), "one-off path join was flagged")?;
        Ok(())
    }

    /// Collection/string join separators are not path fragments.
    #[test]
    fn strict_string_lint_ignores_repeated_join_separators()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_repeated_path_join_literals(
            "demo.rs",
            &PATH_JOIN_TEST_RULE,
            r#"
fn format_messages(messages: &[&str]) -> String {
    let first = messages.join("\n");
    let second = messages.join("\n");
    format!("{first}{second}")
}
"#,
        )?;
        require(
            violations.is_empty(),
            "collection join separator was flagged as a path literal",
        )?;
        Ok(())
    }

    /// Reviewed repeated path joins stay available for existing broad fixtures.
    #[test]
    fn strict_string_lint_allows_reviewed_repeated_path_join_literals()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_repeated_path_join_literals(
            "demo.rs",
            &PATH_JOIN_TEST_RULE,
            r#"
fn fixture(root: &std::path::Path) {
    let _ = root.join("reviewed-existing");
    let _ = root.join("reviewed-existing");
}
"#,
        )?;
        require(
            violations.is_empty(),
            "reviewed repeated path joins were flagged",
        )?;
        Ok(())
    }

    /// Enum `as_str` methods are accepted as typed string centralization points.
    #[test]
    fn strict_string_lint_allows_as_str_centralization() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"
enum Status {
    Active,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
        }
    }
}
"#,
        )?;
        require(violations.is_empty(), "as_str centralization was flagged")?;
        Ok(())
    }

    /// Direct expression literals are reported, not only macro-contained strings.
    #[test]
    fn strict_string_lint_flags_direct_expr_literals() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &TEST_RULE,
            r#"fn status() { let _ = "active"; }"#,
        )?;
        require(
            violations.len() == 1,
            "expected one direct literal violation",
        )?;
        let violation = violations
            .first()
            .ok_or_else(|| io::Error::other("missing direct literal violation"))?;
        require(
            violation.literal == "active",
            "direct literal violation had the wrong value",
        )?;
        Ok(())
    }

    /// Comments and serialization attributes are ignored to avoid false positives.
    #[test]
    fn strict_string_lint_ignores_comments_and_attributes() -> Result<(), Box<dyn std::error::Error>>
    {
        let violations = lint_source(
            "demo.rs",
            &TEST_RULE,
            r#"
// "status" and "active" appear only in a comment.
#[serde(rename_all = "snake_case")]
enum Status {
    Active,
}
"#,
        )?;
        require(
            violations.is_empty(),
            "comments or serde attributes were flagged",
        )?;
        Ok(())
    }

    /// Typed serialization is the preferred schema/status representation.
    #[test]
    fn strict_string_lint_allows_typed_project_state_serialization()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &TEST_RULE,
            r#"
#[derive(serde::Serialize)]
struct ProjectStateResponse {
    project: ProjectStatePayload,
}

#[derive(serde::Serialize)]
struct ProjectStatePayload {
    root: String,
    db: String,
    config: Option<String>,
    status: ProjectStatus,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum ProjectStatus {
    Active,
}
"#,
        )?;
        require(
            violations.is_empty(),
            "typed project-state serialization was flagged",
        )?;
        Ok(())
    }

    /// Human-readable prose with protected words inside a larger sentence is ignored.
    #[test]
    fn strict_string_lint_ignores_non_exact_prose() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &TEST_RULE,
            r#"fn message() -> &'static str { "selected project root is active" }"#,
        )?;
        require(violations.is_empty(), "non-exact prose was flagged")?;
        Ok(())
    }

    /// Broad production mode rejects arbitrary inline strings.
    #[test]
    fn strict_string_lint_flags_unlisted_production_strings()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"fn message() -> &'static str { "selected project root is active" }"#,
        )?;
        require(
            violations.len() == 1,
            "expected one unlisted production string violation",
        )?;
        Ok(())
    }

    /// Reviewed inline format templates are accepted in broad production mode.
    #[test]
    fn strict_string_lint_allows_reviewed_inline_format_templates()
    -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"fn message(value: &str) -> String { format!("allowed inline format {value}") }"#,
        )?;
        require(
            violations.is_empty(),
            "allowlisted inline format was flagged",
        )?;
        Ok(())
    }

    /// Test modules are ignored so fixture strings do not weaken production checks.
    #[test]
    fn strict_string_lint_ignores_test_modules() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"
#[cfg(test)]
mod tests {
    fn fixture() -> &'static str { "arbitrary fixture text" }
}
"#,
        )?;
        require(violations.is_empty(), "test module fixture was flagged")?;
        Ok(())
    }

    /// Test-only `all(...)` cfg modules are ignored.
    #[test]
    fn strict_string_lint_ignores_all_test_modules() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"
#[cfg(all(test, feature = "fixtures"))]
mod tests {
    fn fixture() -> &'static str { "arbitrary fixture text" }
}
"#,
        )?;
        require(violations.is_empty(), "test-only cfg module was flagged")?;
        Ok(())
    }

    /// Production-only cfg modules must still be scanned.
    #[test]
    fn strict_string_lint_scans_not_test_modules() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"
#[cfg(not(test))]
mod prod {
    fn payload() {
        let _ = serde_json::json!({ "status": "active" });
    }
}
"#,
        )?;
        require(
            violations.len() == 2,
            "production-only cfg module was not scanned",
        )?;
        Ok(())
    }

    /// Production branches inside `all(...)` cfg modules must still be scanned.
    #[test]
    fn strict_string_lint_scans_all_not_test_modules() -> Result<(), Box<dyn std::error::Error>> {
        let violations = lint_source(
            "demo.rs",
            &BROAD_TEST_RULE,
            r#"
#[cfg(all(not(test), feature = "runtime"))]
mod prod {
    fn payload() {
        let _ = serde_json::json!({ "status": "active" });
    }
}
"#,
        )?;
        require(
            violations.len() == 2,
            "production all-not-test cfg module was not scanned",
        )?;
        Ok(())
    }
}
