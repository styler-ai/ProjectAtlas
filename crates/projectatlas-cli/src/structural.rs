//! Deterministic structural summaries for files without declaration symbols.

use cssparser::{Parser as CssParser, ParserInput as CssParserInput, Token as CssToken};
use jsonc_parser::{ParseOptions as JsoncParseOptions, parse_to_serde_value};
use pulldown_cmark::{
    Event as MarkdownEvent, HeadingLevel, Options as MarkdownOptions, Parser as MarkdownParser,
    Tag as MarkdownTag, TagEnd as MarkdownTagEnd,
};
use scraper::{Html, Selector};
use serde_json::Value as JsonValue;
use std::collections::BTreeSet;
use toml::Value as TomlValue;
use yaml_rust2::{Yaml, YamlLoader};

/// Maximum named items rendered into one structural summary list.
const LIST_LIMIT: usize = 4;

/// Maximum characters retained for a single extracted label.
const LABEL_LIMIT: usize = 80;

/// Build a deterministic one-line structural summary for an indexed file.
pub(crate) fn structural_summary_for_path(
    path: &str,
    language: Option<&str>,
    content: &str,
) -> Option<String> {
    let language = language.unwrap_or_default();
    match language {
        "markdown" => markdown_summary(content),
        "json" => json_summary(path, content),
        "yaml" => yaml_summary(path, content),
        "toml" | "cargo-manifest" => toml_summary(path, content),
        "css" => css_summary(content),
        "html" => html_summary(content),
        "config" | "text" => config_text_summary(path, content),
        _ if has_extension(path, "toon") => toon_summary(content),
        _ => None,
    }
}

/// Return whether a file family has a lightweight structural adapter.
pub(crate) fn is_structural_summary_candidate(path: &str, language: Option<&str>) -> bool {
    matches!(
        language.unwrap_or_default(),
        "markdown"
            | "json"
            | "yaml"
            | "toml"
            | "cargo-manifest"
            | "css"
            | "html"
            | "config"
            | "text"
    ) || has_extension(path, "toon")
}

/// Return whether a repository path has a case-insensitive extension.
fn has_extension(path: &str, extension: &str) -> bool {
    path.rsplit(['/', '\\'])
        .next()
        .and_then(|file_name| file_name.rsplit_once('.').map(|(_stem, ext)| ext))
        .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
}

/// Return whether a stored summary is still only the scanner byte fallback.
pub(crate) fn is_scanner_fallback_summary(summary: &str) -> bool {
    let trimmed = summary.trim_end_matches('.');
    let Some((_, tail)) = trimmed.rsplit_once(", ") else {
        return false;
    };
    let Some(number) = tail.strip_suffix(" bytes") else {
        return false;
    };
    !number.is_empty() && number.chars().all(|character| character.is_ascii_digit())
}

/// Summarize a Markdown or MDX document from parsed `CommonMark` headings.
fn markdown_summary(content: &str) -> Option<String> {
    let headings = markdown_headings(content);
    if headings.is_empty() {
        let lines = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .count();
        return (lines > 0).then(|| format!("markdown document with {lines} non-empty lines."));
    }
    let title = headings
        .iter()
        .find(|heading| heading.level == 1)
        .unwrap_or(&headings[0]);
    let section_names = headings
        .iter()
        .filter(|heading| heading.text != title.text)
        .map(|heading| heading.text.as_str())
        .collect::<Vec<_>>();
    if section_names.is_empty() {
        Some(format!("markdown document titled {}.", title.text))
    } else {
        Some(format!(
            "markdown document titled {} with sections {}.",
            title.text,
            join_limited(section_names)
        ))
    }
}

/// One parsed Markdown heading.
struct MarkdownHeading {
    /// Heading level from one to six.
    level: usize,
    /// Compact heading text.
    text: String,
}

/// Extract Markdown headings through `pulldown-cmark`.
fn markdown_headings(content: &str) -> Vec<MarkdownHeading> {
    let parser = MarkdownParser::new_ext(content, MarkdownOptions::all());
    let mut current_heading: Option<MarkdownHeading> = None;
    let mut headings = Vec::new();
    for event in parser {
        match event {
            MarkdownEvent::Start(MarkdownTag::Heading { level, .. }) => {
                current_heading = Some(MarkdownHeading {
                    level: markdown_heading_level(level),
                    text: String::new(),
                });
            }
            MarkdownEvent::Text(text) | MarkdownEvent::Code(text) => {
                if let Some(heading) = current_heading.as_mut() {
                    if !heading.text.is_empty() {
                        heading.text.push(' ');
                    }
                    heading.text.push_str(text.as_ref());
                }
            }
            MarkdownEvent::End(MarkdownTagEnd::Heading(_level)) => {
                if let Some(mut heading) = current_heading.take() {
                    heading.text = compact_label(&heading.text);
                    if !heading.text.is_empty() {
                        headings.push(heading);
                    }
                }
            }
            _ => {}
        }
    }
    headings
}

/// Convert a Markdown heading level into a display depth.
fn markdown_heading_level(level: HeadingLevel) -> usize {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Summarize JSON and JSONC files from parsed object structure.
fn json_summary(path: &str, content: &str) -> Option<String> {
    let value: JsonValue = parse_to_serde_value(content, &JsoncParseOptions::default()).ok()?;
    let object = value.as_object()?;
    if path.ends_with("package.json") {
        return Some(package_json_summary(object));
    }
    let keys = object.keys().map(String::as_str).collect::<Vec<_>>();
    if object.contains_key("datasets") || path.ends_with("datasets.manifest.json") {
        let (dataset_count, dataset_ids) = dataset_manifest_facts(object.get("datasets"));
        let key_list = join_limited(keys);
        if dataset_ids.is_empty() {
            return Some(format!(
                "json dataset manifest with {dataset_count} datasets and keys {key_list}."
            ));
        }
        return Some(format!(
            "json dataset manifest with {dataset_count} datasets including {} and keys {key_list}.",
            join_limited(dataset_ids.iter().map(String::as_str).collect())
        ));
    }
    if keys.is_empty() {
        None
    } else {
        Some(format!(
            "json document with top-level keys {}.",
            join_limited(keys)
        ))
    }
}

/// Extract dataset count and bounded identifiers from common manifest shapes.
fn dataset_manifest_facts(value: Option<&JsonValue>) -> (usize, Vec<String>) {
    let Some(value) = value else {
        return (0, Vec::new());
    };
    if let Some(object) = value.as_object() {
        let ids = object.keys().cloned().collect::<Vec<_>>();
        return (ids.len(), ids);
    }
    let Some(array) = value.as_array() else {
        return (0, Vec::new());
    };
    let ids = array
        .iter()
        .filter_map(|item| {
            item.as_object().and_then(|object| {
                object
                    .get("id")
                    .or_else(|| object.get("name"))
                    .and_then(JsonValue::as_str)
                    .map(compact_label)
            })
        })
        .collect::<Vec<_>>();
    (array.len(), ids)
}

/// Build a package.json summary from common manifest keys.
fn package_json_summary(object: &serde_json::Map<String, JsonValue>) -> String {
    let name = object
        .get("name")
        .and_then(JsonValue::as_str)
        .map_or_else(|| "unnamed package".to_string(), compact_label);
    let script_names = object_keys(object.get("scripts"));
    let dependency_names = object_keys(object.get("dependencies"));
    let dev_dependency_names = object_keys(object.get("devDependencies"));
    let dependencies = dependency_names
        .len()
        .saturating_add(dev_dependency_names.len());
    if script_names.is_empty() && dependencies == 0 {
        format!("package manifest for {name}.")
    } else if script_names.is_empty() {
        format!("package manifest for {name} with {dependencies} dependencies.")
    } else {
        format!(
            "package manifest for {name} with scripts {} and {dependencies} dependencies.",
            join_limited(script_names.iter().map(String::as_str).collect())
        )
    }
}

/// Return sorted object keys for a JSON object value.
fn object_keys(value: Option<&JsonValue>) -> Vec<String> {
    let mut keys = value
        .and_then(JsonValue::as_object)
        .map(|object| object.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort();
    keys
}

/// Summarize YAML files, including GitHub Actions workflow structure.
fn yaml_summary(path: &str, content: &str) -> Option<String> {
    let document = YamlLoader::load_from_str(content)
        .ok()?
        .into_iter()
        .next()?;
    let keys = yaml_mapping_keys(&document);
    let jobs = yaml_child_mapping_keys(&document, "jobs");
    let triggers = yaml_triggers(&document);
    let workflow_name = yaml_scalar_value(&document, "name");
    if path.contains(".github/workflows/") || (!jobs.is_empty() && !triggers.is_empty()) {
        let name = workflow_name
            .as_deref()
            .map_or_else(|| "unnamed workflow".to_string(), compact_label);
        return Some(format!(
            "yaml workflow {name} triggered by {} with jobs {}.",
            join_limited(triggers.iter().map(String::as_str).collect()),
            join_limited(jobs.iter().map(String::as_str).collect())
        ));
    }
    if keys.is_empty() {
        None
    } else {
        Some(format!(
            "yaml document with top-level keys {}.",
            join_limited(keys.iter().map(String::as_str).collect())
        ))
    }
}

/// Extract mapping keys from a YAML node.
fn yaml_mapping_keys(value: &Yaml) -> Vec<String> {
    let mut keys = value
        .as_hash()
        .map(|hash| {
            hash.iter()
                .filter_map(|(key, _value)| key.as_str().map(compact_label))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    keys.sort();
    keys.dedup();
    keys
}

/// Extract child mapping keys from a top-level YAML key.
fn yaml_child_mapping_keys(value: &Yaml, key: &str) -> Vec<String> {
    yaml_mapping_keys(&value[key])
}

/// Extract GitHub Actions trigger names from a YAML document.
fn yaml_triggers(value: &Yaml) -> Vec<String> {
    let trigger = &value["on"];
    if let Some(trigger) = trigger.as_str() {
        return trigger
            .trim_matches(['[', ']'])
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(compact_label)
            .collect();
    }
    if let Some(triggers) = trigger.as_vec() {
        return triggers
            .iter()
            .filter_map(Yaml::as_str)
            .map(compact_label)
            .collect();
    }
    yaml_mapping_keys(trigger)
}

/// Extract a scalar string for a top-level YAML key.
fn yaml_scalar_value(value: &Yaml, key: &str) -> Option<String> {
    value[key].as_str().map(ToString::to_string)
}

/// Summarize TOML configuration and manifest files.
fn toml_summary(path: &str, content: &str) -> Option<String> {
    let value = toml::from_str::<TomlValue>(content).ok()?;
    let table = value.as_table()?;
    let keys = table.keys().map(String::as_str).collect::<Vec<_>>();
    if path.ends_with("Cargo.toml") {
        let package = table
            .get("package")
            .and_then(TomlValue::as_table)
            .and_then(|package| package.get("name"))
            .and_then(TomlValue::as_str)
            .map_or_else(|| "workspace".to_string(), compact_label);
        return Some(format!(
            "cargo manifest for {package} with tables {}.",
            join_limited(keys)
        ));
    }
    if path.ends_with(".projectatlas/config.toml") || path.ends_with("projectatlas.toml") {
        let excludes = table
            .get("scan")
            .and_then(TomlValue::as_table)
            .map_or(0, toml_scan_exclude_count);
        return Some(format!(
            "ProjectAtlas config with tables {} and {excludes} scan excludes.",
            join_limited(keys)
        ));
    }
    if keys.is_empty() {
        None
    } else {
        Some(format!("toml document with tables {}.", join_limited(keys)))
    }
}

/// Count configured scan exclude entries in a TOML scan table.
fn toml_scan_exclude_count(scan: &toml::map::Map<String, TomlValue>) -> usize {
    ["exclude_dir_names", "exclude_path_prefixes"]
        .iter()
        .filter_map(|key| scan.get(*key))
        .filter_map(TomlValue::as_array)
        .map(Vec::len)
        .sum()
}

/// Summarize CSS-like stylesheets from parser tokens.
fn css_summary(content: &str) -> Option<String> {
    let mut input = CssParserInput::new(content);
    let mut parser = CssParser::new(&mut input);
    let mut facts = CssFacts::default();
    scan_css_tokens(&mut parser, CssMode::StyleSheet, &mut facts);
    if facts.selectors.is_empty() && facts.custom_properties.is_empty() {
        return None;
    }
    let selector_list = facts
        .selectors
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let property_list = facts
        .custom_properties
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    Some(format!(
        "css stylesheet with selectors {}, custom properties {}, {} media queries, and {} supports queries.",
        join_or_none(selector_list),
        join_or_none(property_list),
        facts.media_queries,
        facts.supports_queries
    ))
}

/// CSS parser traversal mode.
#[derive(Clone, Copy, Eq, PartialEq)]
enum CssMode {
    /// Parse rule preludes as stylesheet selectors.
    StyleSheet,
    /// Parse rule bodies as declarations.
    DeclarationBlock,
}

/// Kind of CSS rule prelude most recently seen.
#[derive(Clone, Copy, Eq, PartialEq)]
enum CssRuleKind {
    /// No rule prelude has been classified yet.
    None,
    /// The prelude is an `@media` rule.
    Media,
    /// The prelude is an `@supports` rule.
    Supports,
    /// The prelude is a qualified selector rule.
    Qualified,
}

/// Structural facts extracted from CSS tokens.
#[derive(Default)]
struct CssFacts {
    /// Selector labels from qualified rules.
    selectors: BTreeSet<String>,
    /// Custom property names declared in rule bodies.
    custom_properties: BTreeSet<String>,
    /// Number of media query at-rules.
    media_queries: usize,
    /// Number of supports query at-rules.
    supports_queries: usize,
}

/// Walk CSS parser tokens and collect stable structural facts.
fn scan_css_tokens(parser: &mut CssParser<'_, '_>, mode: CssMode, facts: &mut CssFacts) {
    let mut pending_delimiter: Option<char> = None;
    let mut after_colon = false;
    let mut rule_kind = CssRuleKind::None;
    while let Ok(token) = parser.next_including_whitespace_and_comments().cloned() {
        match token {
            CssToken::AtKeyword(name) => {
                if name.eq_ignore_ascii_case("media") {
                    facts.media_queries = facts.media_queries.saturating_add(1);
                    rule_kind = CssRuleKind::Media;
                } else if name.eq_ignore_ascii_case("supports") {
                    facts.supports_queries = facts.supports_queries.saturating_add(1);
                    rule_kind = CssRuleKind::Supports;
                }
            }
            CssToken::IDHash(name) | CssToken::Hash(name) if mode == CssMode::StyleSheet => {
                facts.selectors.insert(format!("#{}", compact_label(&name)));
                rule_kind = CssRuleKind::Qualified;
            }
            CssToken::Delim('.') if mode == CssMode::StyleSheet => {
                pending_delimiter = Some('.');
            }
            CssToken::Colon if mode == CssMode::StyleSheet => {
                after_colon = true;
            }
            CssToken::Ident(name) => {
                let name = name.as_ref();
                if name.starts_with("--") {
                    facts.custom_properties.insert(compact_label(name));
                } else if mode == CssMode::StyleSheet {
                    if let Some(delimiter) = pending_delimiter.take() {
                        facts
                            .selectors
                            .insert(format!("{delimiter}{}", compact_label(name)));
                        rule_kind = CssRuleKind::Qualified;
                    } else if after_colon {
                        facts.selectors.insert(format!(":{}", compact_label(name)));
                        rule_kind = CssRuleKind::Qualified;
                    } else if is_css_type_selector(name) {
                        facts.selectors.insert(compact_label(name));
                        rule_kind = CssRuleKind::Qualified;
                    }
                    after_colon = false;
                }
            }
            CssToken::Comma if mode == CssMode::StyleSheet => {
                after_colon = false;
                pending_delimiter = None;
            }
            CssToken::CurlyBracketBlock => {
                let nested_mode = if matches!(rule_kind, CssRuleKind::Media | CssRuleKind::Supports)
                {
                    CssMode::StyleSheet
                } else {
                    CssMode::DeclarationBlock
                };
                let nested_result: Result<(), cssparser::ParseError<'_, ()>> = parser
                    .parse_nested_block(|nested| {
                        scan_css_tokens(nested, nested_mode, facts);
                        Ok(())
                    });
                drop(nested_result);
                after_colon = false;
                pending_delimiter = None;
                rule_kind = CssRuleKind::None;
            }
            _ => {}
        }
    }
}

/// Return whether an identifier is a useful type selector label.
fn is_css_type_selector(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "body" | "html" | "main" | "section" | "article" | "header" | "footer"
    )
}

/// Summarize HTML documents from parsed title, metadata, headings, and structured data.
fn html_summary(content: &str) -> Option<String> {
    let document = Html::parse_document(content);
    let title = first_html_text(&document, "title");
    let description = first_html_attribute(
        &document,
        "meta[name=\"description\"], meta[property=\"description\"], meta[property=\"og:description\"]",
        "content",
    );
    let headings = html_texts(&document, "h1, h2", LIST_LIMIT);
    let link_rels = html_link_rel_values(&document, LIST_LIMIT);
    let has_structured_data = html_select(&document, "script[type=\"application/ld+json\"]")
        .is_some_and(|selector| document.select(&selector).next().is_some());
    if title.is_none()
        && description.is_none()
        && headings.is_empty()
        && link_rels.is_empty()
        && !has_structured_data
    {
        return None;
    }
    let mut parts = Vec::new();
    if let Some(title) = title {
        parts.push(format!("title {title}"));
    }
    if let Some(description) = description {
        parts.push(format!("meta description {description}"));
    }
    if !headings.is_empty() {
        parts.push(format!(
            "headings {}",
            join_limited(headings.iter().map(String::as_str).collect())
        ));
    }
    if !link_rels.is_empty() {
        parts.push(format!(
            "link rels {}",
            join_limited(link_rels.iter().map(String::as_str).collect())
        ));
    }
    if has_structured_data {
        parts.push("structured data".to_string());
    }
    Some(format!("html document with {}.", parts.join(", ")))
}

/// Extract bounded link relation markers from an HTML document.
fn html_link_rel_values(document: &Html, limit: usize) -> Vec<String> {
    let Some(selector) = html_select(document, "link[rel]") else {
        return Vec::new();
    };
    let mut rels = document
        .select(&selector)
        .filter_map(|element| element.attr("rel"))
        .flat_map(|value| {
            value
                .split_ascii_whitespace()
                .map(compact_label)
                .collect::<Vec<_>>()
        })
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    rels.truncate(limit);
    rels
}

/// Compile an HTML selector.
fn html_select(_document: &Html, selector: &str) -> Option<Selector> {
    Selector::parse(selector).ok()
}

/// Extract first text value for an HTML selector.
fn first_html_text(document: &Html, selector: &str) -> Option<String> {
    html_texts(document, selector, 1).into_iter().next()
}

/// Extract text values for an HTML selector.
fn html_texts(document: &Html, selector: &str, limit: usize) -> Vec<String> {
    let Some(selector) = html_select(document, selector) else {
        return Vec::new();
    };
    document
        .select(&selector)
        .filter_map(|element| {
            let text = element.text().collect::<Vec<_>>().join(" ");
            let text = compact_label(&text);
            (!text.is_empty()).then_some(text)
        })
        .take(limit)
        .collect()
}

/// Extract one attribute from the first matching HTML element.
fn first_html_attribute(document: &Html, selector: &str, attribute: &str) -> Option<String> {
    let selector = html_select(document, selector)?;
    document
        .select(&selector)
        .find_map(|element| element.attr(attribute))
        .map(compact_label)
}

/// Summarize TOON files from the decoded top-level shape.
fn toon_summary(content: &str) -> Option<String> {
    let mut sections = if let Ok(value) = toon_format::decode_default::<JsonValue>(content) {
        value
            .as_object()
            .map(|object| object.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if sections.is_empty() {
        sections = toon_section_names(content)
            .into_iter()
            .map(str::to_string)
            .collect();
    }
    if sections.is_empty() {
        None
    } else {
        Some(format!(
            "TOON document with sections {}.",
            join_limited(sections.iter().map(String::as_str).collect())
        ))
    }
}

/// Extract declared top-level TOON section names from compact table/list headers.
fn toon_section_names(content: &str) -> Vec<&str> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let (head, _tail) = trimmed.split_once(':')?;
            let name = head
                .split_once('[')
                .map_or(head, |(name, _columns)| name)
                .split_once('{')
                .map_or_else(
                    || head.split_once('[').map_or(head, |(name, _columns)| name),
                    |(name, _columns)| name,
                )
                .trim();
            (!name.is_empty() && !name.starts_with('#')).then_some(name)
        })
        .collect()
}

/// Summarize simple config or text files from key-like lines.
fn config_text_summary(path: &str, content: &str) -> Option<String> {
    let keys = content
        .lines()
        .filter_map(config_key)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if keys.is_empty() {
        if has_extension(path, "txt") {
            let excerpt = content
                .lines()
                .map(compact_label)
                .find(|line| !line.is_empty())?;
            let file_name = path.rsplit('/').next().unwrap_or(path);
            return Some(format!("text file {file_name} beginning with {excerpt}."));
        }
        return None;
    }
    let file_name = path.rsplit('/').next().unwrap_or(path);
    Some(format!(
        "config file {file_name} with keys {}.",
        join_limited(keys.iter().map(String::as_str).collect())
    ))
}

/// Extract a key from simple `key=value`, `key: value`, or `export key=value` lines.
fn config_key(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
        return None;
    }
    let trimmed = trimmed.strip_prefix("export ").unwrap_or(trimmed);
    for separator in ['=', ':'] {
        if let Some((key, _value)) = trimmed.split_once(separator) {
            let key = key.trim();
            if !key.is_empty()
                && key.chars().all(|character| {
                    character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
                })
            {
                return Some(key.to_string());
            }
        }
    }
    None
}

/// Join a list with a fixed display limit.
fn join_limited(values: Vec<&str>) -> String {
    let mut values = values
        .into_iter()
        .map(compact_label)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    let extra = values.len().saturating_sub(LIST_LIMIT);
    values.truncate(LIST_LIMIT);
    let joined = values.join(", ");
    if extra == 0 {
        joined
    } else {
        format!("{joined}, and {extra} more")
    }
}

/// Join a list or return `none` when it is empty.
fn join_or_none(values: Vec<&str>) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        join_limited(values)
    }
}

/// Compact a label into one line.
fn compact_label(text: &str) -> String {
    let compact = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(['"', '\''])
        .trim_end_matches(['#'])
        .trim()
        .to_string();
    if compact.chars().count() <= LABEL_LIMIT {
        compact
    } else {
        truncate_chars(&compact, LABEL_LIMIT)
    }
}

/// Truncate a string to a character boundary and add an ellipsis marker.
fn truncate_chars(text: &str, limit: usize) -> String {
    let mut output = text
        .chars()
        .take(limit.saturating_sub(3))
        .collect::<String>();
    output.push_str("...");
    output
}

#[cfg(test)]
mod tests {
    use super::{is_scanner_fallback_summary, structural_summary_for_path};

    #[test]
    fn summarizes_markdown_headings() {
        let summary = structural_summary_for_path(
            "README.md",
            Some("markdown"),
            "# ProjectAtlas\n\n## Install\nUsage\n-----\n",
        );
        assert_eq!(
            summary.as_deref(),
            Some("markdown document titled ProjectAtlas with sections Install, Usage.")
        );
    }

    #[test]
    fn summarizes_package_jsonc() {
        let summary = structural_summary_for_path(
            "package.json",
            Some("json"),
            "{\n  // project name\n  \"name\":\"demo\",\n  \"scripts\":{\"test\":\"vitest\"},\n  \"dependencies\":{\"react\":\"1.0.0\"}\n}",
        );
        assert_eq!(
            summary.as_deref(),
            Some("package manifest for demo with scripts test and 1 dependencies.")
        );
    }

    #[test]
    fn summarizes_object_keyed_dataset_manifest() {
        let summary = structural_summary_for_path(
            "app/public/data/datasets.manifest.json",
            Some("json"),
            r#"{
  "generated_at": "2026-06-28T00:00:00Z",
  "version": "2026.06.28",
  "datasets": {
    "catalog.primary": {"path": "primary.json"},
    "catalog.secondary": {"path": "secondary.json"},
    "catalog.archive": {"path": "archive.json"}
  }
}"#,
        );
        assert_eq!(
            summary.as_deref(),
            Some(
                "json dataset manifest with 3 datasets including catalog.archive, catalog.primary, catalog.secondary and keys datasets, generated_at, version."
            )
        );
    }

    #[test]
    fn summarizes_workflow_yaml() {
        let summary = structural_summary_for_path(
            ".github/workflows/ci.yml",
            Some("yaml"),
            "name: CI\non:\n  push:\n  pull_request:\njobs:\n  test:\n    runs-on: ubuntu-latest\n",
        );
        assert_eq!(
            summary.as_deref(),
            Some("yaml workflow CI triggered by pull_request, push with jobs test.")
        );
    }

    #[test]
    fn summarizes_projectatlas_config_toml() {
        let summary = structural_summary_for_path(
            ".projectatlas/config.toml",
            Some("toml"),
            "[project]\nroot = \".\"\n[scan]\nexclude_dir_names = [\"target\"]\nexclude_path_prefixes = [\"docs/api\"]\n",
        );
        assert_eq!(
            summary.as_deref(),
            Some("ProjectAtlas config with tables project, scan and 2 scan excludes.")
        );
    }

    #[test]
    fn summarizes_css_structure() {
        let summary = structural_summary_for_path(
            "app/styles.css",
            Some("css"),
            ":root { --brand: #fff; }\n.card, .panel { color: red; }\n@media (min-width: 40rem) { .card { display: grid; } }\n",
        );
        assert_eq!(
            summary.as_deref(),
            Some(
                "css stylesheet with selectors .card, .panel, :root, custom properties --brand, 1 media queries, and 0 supports queries."
            )
        );
    }

    #[test]
    fn summarizes_html_metadata() {
        let summary = structural_summary_for_path(
            "index.html",
            Some("html"),
            "<html><head><title>Home</title><meta name=\"description\" content=\"Welcome page\"><link rel=\"canonical\" href=\"https://example.test/\"><link rel=\"manifest\" href=\"/site.webmanifest\"><link rel=\"alternate\" href=\"/de/\"></head><body><h1>Hello</h1><script type=\"application/ld+json\">{}</script></body></html>",
        );
        assert_eq!(
            summary.as_deref(),
            Some(
                "html document with title Home, meta description Welcome page, headings Hello, link rels alternate, canonical, manifest, structured data."
            )
        );
    }

    #[test]
    fn summarizes_plain_text_excerpt() {
        let summary =
            structural_summary_for_path("notes.txt", Some("text"), "\n\nProjectAtlas notes\n");
        assert_eq!(
            summary.as_deref(),
            Some("text file notes.txt beginning with ProjectAtlas notes.")
        );
    }

    #[test]
    fn classifies_summary_quality() {
        assert!(is_scanner_fallback_summary("rust file, 120 bytes."));
    }
}
