//! Bounded same-length projection of accepted inline ECMAScript regions.

use projectatlas_core::language::{
    EmbeddedLanguageCapability, SemanticProviderOwner, language_capability,
};

/// Maximum script regions admitted from one host file.
pub(crate) const MAX_EMBEDDED_SCRIPT_REGIONS: usize = 64;

/// JavaScript grammar family selected for one projected buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EmbeddedScriptLanguage {
    /// JavaScript or JSX-compatible source.
    JavaScript,
    /// TypeScript-compatible source.
    TypeScript,
}

impl EmbeddedScriptLanguage {
    /// Return the built-in grammar language identifier.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
        }
    }
}

/// One same-length source buffer containing only admitted inline scripts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EmbeddedSourceProjection {
    /// Grammar family selected for the projected regions.
    language: EmbeddedScriptLanguage,
    /// Same-length host buffer containing only admitted region bytes and newlines.
    source: String,
}

impl EmbeddedSourceProjection {
    /// Return the grammar family selected for this projection.
    pub(crate) const fn language(&self) -> EmbeddedScriptLanguage {
        self.language
    }

    /// Borrow the same-length projected host buffer.
    pub(crate) fn source(&self) -> &str {
        &self.source
    }
}

/// Fail-closed reason for an unsafe or unbounded embedded projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EmbeddedProjectionError {
    /// Host tag structure or attribute syntax was not safe to project.
    Malformed,
    /// The host exceeded the per-file script-region limit.
    RegionLimit,
}

/// Bounded embedded projections plus any incomplete host-reconciliation outcome.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EmbeddedProjectionBatch {
    /// Safely admitted same-length source projections.
    projections: Vec<EmbeddedSourceProjection>,
    /// Reason host reconciliation stopped before a complete scan, when any.
    incomplete: Option<EmbeddedProjectionError>,
}

impl EmbeddedProjectionBatch {
    /// Consume the admitted projections and any incomplete reconciliation reason.
    pub(crate) fn into_parts(
        self,
    ) -> (
        Vec<EmbeddedSourceProjection>,
        Option<EmbeddedProjectionError>,
    ) {
        (self.projections, self.incomplete)
    }
}

/// Resolve the accepted host capability, retaining legacy path inference without a language.
pub(crate) fn host_capability(
    path: &str,
    language: Option<&str>,
) -> Option<EmbeddedLanguageCapability> {
    let language = language.or_else(|| {
        let extension = path.rsplit_once('.')?.1;
        if extension.eq_ignore_ascii_case("html") || extension.eq_ignore_ascii_case("htm") {
            Some("html")
        } else if extension.eq_ignore_ascii_case("vue") {
            Some("vue")
        } else if extension.eq_ignore_ascii_case("svelte") {
            Some("svelte")
        } else {
            None
        }
    })?;
    let capability = language_capability(language)?.embedded_language?;
    (capability.semantic_provider == SemanticProviderOwner::EcmaScript).then_some(capability)
}

/// Project admitted inline JavaScript/TypeScript while preserving host byte positions.
pub(crate) fn project(source: &str) -> EmbeddedProjectionBatch {
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut region_count = 0;
    let mut javascript = None;
    let mut typescript = None;

    loop {
        let open = match find_script_open(bytes, cursor) {
            Ok(Some(open)) => open,
            Ok(None) => return finish_projections(javascript, typescript, None),
            Err(error) => return finish_projections(javascript, typescript, Some(error)),
        };
        region_count += 1;
        if region_count > MAX_EMBEDDED_SCRIPT_REGIONS {
            return finish_projections(
                javascript,
                typescript,
                Some(EmbeddedProjectionError::RegionLimit),
            );
        }
        let name_end = open + b"<script".len();
        let Some(tag_end) = find_tag_end(bytes, name_end) else {
            return finish_projections(
                javascript,
                typescript,
                Some(EmbeddedProjectionError::Malformed),
            );
        };
        let attributes = match parse_attributes(&source[name_end..tag_end]) {
            Ok(attributes) => attributes,
            Err(error) => return finish_projections(javascript, typescript, Some(error)),
        };
        let self_closing = source[name_end..tag_end].trim_end().ends_with('/');
        if self_closing {
            cursor = tag_end + 1;
            continue;
        }
        let content_start = tag_end + 1;
        let Some((content_end, close_end)) = find_script_close(bytes, content_start) else {
            return finish_projections(
                javascript,
                typescript,
                Some(EmbeddedProjectionError::Malformed),
            );
        };
        cursor = close_end;

        if attributes.iter().any(|attribute| attribute.name == "src") {
            continue;
        }
        let Some(language) = selected_language(&attributes) else {
            continue;
        };
        let target = match language {
            EmbeddedScriptLanguage::JavaScript => {
                javascript.get_or_insert_with(|| blank_host_bytes(bytes))
            }
            EmbeddedScriptLanguage::TypeScript => {
                typescript.get_or_insert_with(|| blank_host_bytes(bytes))
            }
        };
        target[content_start..content_end].copy_from_slice(&bytes[content_start..content_end]);
    }
}

/// Convert admitted same-length buffers into typed projections.
fn finish_projections(
    javascript: Option<Vec<u8>>,
    typescript: Option<Vec<u8>>,
    mut incomplete: Option<EmbeddedProjectionError>,
) -> EmbeddedProjectionBatch {
    let mut projections = Vec::with_capacity(2);
    if let Some(source) = javascript {
        match String::from_utf8(source) {
            Ok(source) => projections.push(EmbeddedSourceProjection {
                language: EmbeddedScriptLanguage::JavaScript,
                source,
            }),
            Err(_) => incomplete = Some(EmbeddedProjectionError::Malformed),
        }
    }
    if let Some(source) = typescript {
        match String::from_utf8(source) {
            Ok(source) => projections.push(EmbeddedSourceProjection {
                language: EmbeddedScriptLanguage::TypeScript,
                source,
            }),
            Err(_) => incomplete = Some(EmbeddedProjectionError::Malformed),
        }
    }
    EmbeddedProjectionBatch {
        projections,
        incomplete,
    }
}

/// One parsed opening-tag attribute.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Attribute<'a> {
    /// ASCII-lowercased attribute name.
    name: String,
    /// Borrowed attribute value when explicitly supplied.
    value: Option<&'a str>,
}

/// Parse a conservative attribute subset without interpreting code or entities.
fn parse_attributes(attributes: &str) -> Result<Vec<Attribute<'_>>, EmbeddedProjectionError> {
    let bytes = attributes.as_bytes();
    let mut parsed = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if cursor == bytes.len() || bytes[cursor] == b'/' {
            break;
        }
        let name_start = cursor;
        while bytes.get(cursor).is_some_and(|byte| {
            byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b':' | b'@')
        }) {
            cursor += 1;
        }
        if cursor == name_start {
            return Err(EmbeddedProjectionError::Malformed);
        }
        let name = attributes[name_start..cursor].to_ascii_lowercase();
        while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        let value = if bytes.get(cursor) == Some(&b'=') {
            cursor += 1;
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor += 1;
            }
            let Some(first) = bytes.get(cursor).copied() else {
                return Err(EmbeddedProjectionError::Malformed);
            };
            if matches!(first, b'\'' | b'"') {
                cursor += 1;
                let value_start = cursor;
                while bytes.get(cursor).is_some_and(|byte| *byte != first) {
                    cursor += 1;
                }
                if bytes.get(cursor) != Some(&first) {
                    return Err(EmbeddedProjectionError::Malformed);
                }
                let value = &attributes[value_start..cursor];
                cursor += 1;
                Some(value)
            } else {
                let value_start = cursor;
                while bytes
                    .get(cursor)
                    .is_some_and(|byte| !byte.is_ascii_whitespace())
                {
                    cursor += 1;
                }
                (cursor > value_start).then_some(&attributes[value_start..cursor])
            }
        } else {
            None
        };
        parsed.push(Attribute { name, value });
    }
    Ok(parsed)
}

/// Select one compatible grammar from optional `lang` and `type` attributes.
fn selected_language(attributes: &[Attribute<'_>]) -> Option<EmbeddedScriptLanguage> {
    let lang = attribute_language(attributes, "lang", language_from_lang);
    let script_type = attribute_language(attributes, "type", language_from_type);
    match (lang, script_type) {
        (AttributeLanguage::Missing, AttributeLanguage::Missing) => {
            Some(EmbeddedScriptLanguage::JavaScript)
        }
        (AttributeLanguage::Accepted(language), AttributeLanguage::Missing)
        | (AttributeLanguage::Missing, AttributeLanguage::Accepted(language)) => Some(language),
        (AttributeLanguage::Accepted(left), AttributeLanguage::Accepted(right))
            if left == right =>
        {
            Some(left)
        }
        (AttributeLanguage::Unsupported, _)
        | (_, AttributeLanguage::Unsupported)
        | (AttributeLanguage::Accepted(_), AttributeLanguage::Accepted(_)) => None,
    }
}

/// Classification of one optional script-language attribute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AttributeLanguage {
    /// The attribute was absent.
    Missing,
    /// The attribute selected an accepted grammar.
    Accepted(EmbeddedScriptLanguage),
    /// The attribute was duplicated, valueless, conflicting, or unsupported.
    Unsupported,
}

/// Classify one unique optional attribute through its owning value parser.
fn attribute_language(
    attributes: &[Attribute<'_>],
    name: &str,
    classify: fn(&str) -> Option<EmbeddedScriptLanguage>,
) -> AttributeLanguage {
    let mut matches = attributes.iter().filter(|attribute| attribute.name == name);
    let Some(attribute) = matches.next() else {
        return AttributeLanguage::Missing;
    };
    if matches.next().is_some() {
        return AttributeLanguage::Unsupported;
    }
    attribute
        .value
        .and_then(classify)
        .map_or(AttributeLanguage::Unsupported, AttributeLanguage::Accepted)
}

/// Parse accepted `lang` spellings.
fn language_from_lang(value: &str) -> Option<EmbeddedScriptLanguage> {
    match value.trim().to_ascii_lowercase().as_str() {
        "js" | "javascript" | "jsx" => Some(EmbeddedScriptLanguage::JavaScript),
        "ts" | "typescript" => Some(EmbeddedScriptLanguage::TypeScript),
        _ => None,
    }
}

/// Parse accepted ECMAScript MIME and module `type` spellings.
fn language_from_type(value: &str) -> Option<EmbeddedScriptLanguage> {
    let value = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    match value.as_str() {
        "module"
        | "text/javascript"
        | "application/javascript"
        | "text/ecmascript"
        | "application/ecmascript" => Some(EmbeddedScriptLanguage::JavaScript),
        "text/typescript" | "application/typescript" => Some(EmbeddedScriptLanguage::TypeScript),
        _ => None,
    }
}

/// Replace host bytes with spaces while retaining exact newline bytes.
fn blank_host_bytes(source: &[u8]) -> Vec<u8> {
    source
        .iter()
        .map(|byte| match byte {
            b'\r' | b'\n' => *byte,
            _ => b' ',
        })
        .collect()
}

/// Find the next real opening script tag outside comments and other tag attributes.
fn find_script_open(source: &[u8], start: usize) -> Result<Option<usize>, EmbeddedProjectionError> {
    let mut cursor = start;
    let mut tag_quote = None;
    let mut inside_tag = false;
    while cursor < source.len() {
        if !inside_tag && source[cursor..].starts_with(b"<!--") {
            let Some(comment_end) = find_exact(source, cursor + b"<!--".len(), b"-->") else {
                return Err(EmbeddedProjectionError::Malformed);
            };
            cursor = comment_end + b"-->".len();
            continue;
        }
        if !inside_tag && source[cursor] == b'<' {
            if tag_start_is(source, cursor, b"script") {
                return Ok(Some(cursor));
            }
            if let Some(name) = RAW_TEXT_HOST_ELEMENTS
                .iter()
                .copied()
                .find(|name| tag_start_is(source, cursor, name))
            {
                let Some(open_end) = find_tag_end(source, cursor + name.len() + 1) else {
                    return Err(EmbeddedProjectionError::Malformed);
                };
                if source[cursor + name.len() + 1..open_end]
                    .trim_ascii_end()
                    .ends_with(b"/")
                {
                    cursor = open_end + 1;
                    continue;
                }
                let Some((_close_start, close_end)) = find_host_close(source, open_end + 1, name)
                else {
                    return Err(EmbeddedProjectionError::Malformed);
                };
                cursor = close_end;
                continue;
            }
            inside_tag = true;
        } else if inside_tag {
            match (tag_quote, source[cursor]) {
                (Some(expected), current) if current == expected => tag_quote = None,
                (None, b'\'' | b'"') => tag_quote = Some(source[cursor]),
                (None, b'>') => inside_tag = false,
                _ => {}
            }
        }
        cursor += 1;
    }
    Ok(None)
}

/// Raw-text and RCDATA hosts whose contents cannot introduce executable scripts.
const RAW_TEXT_HOST_ELEMENTS: &[&[u8]] = &[
    b"style",
    b"textarea",
    b"title",
    b"xmp",
    b"iframe",
    b"noembed",
    b"noframes",
];

/// Return whether an opening tag with an exact ASCII-insensitive name starts here.
fn tag_start_is(source: &[u8], start: usize, name: &[u8]) -> bool {
    source
        .get(start + 1..start + 1 + name.len())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(name))
        && source
            .get(start + 1 + name.len())
            .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(*byte, b'/' | b'>'))
}

/// Find and validate one matching raw-text or RCDATA closing tag.
fn find_host_close(source: &[u8], start: usize, name: &[u8]) -> Option<(usize, usize)> {
    let mut marker = Vec::with_capacity(name.len() + 2);
    marker.extend_from_slice(b"</");
    marker.extend_from_slice(name);
    let mut cursor = start;
    loop {
        let close = find_ascii_case_insensitive(source, cursor, &marker)?;
        let mut end = close + marker.len();
        if !source
            .get(end)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'>')
        {
            cursor = end;
            continue;
        }
        while source.get(end).is_some_and(u8::is_ascii_whitespace) {
            end += 1;
        }
        if source.get(end) == Some(&b'>') {
            return Some((close, end + 1));
        }
        cursor = end;
    }
}

/// Find and validate the matching HTML-style closing script tag.
fn find_script_close(source: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut cursor = start;
    loop {
        let close = find_ascii_case_insensitive(source, cursor, b"</script")?;
        let mut end = close + b"</script".len();
        let boundary = source.get(end)?;
        if !boundary.is_ascii_whitespace() && *boundary != b'>' {
            cursor = end;
            continue;
        }
        while source.get(end).is_some_and(u8::is_ascii_whitespace) {
            end += 1;
        }
        if source.get(end) == Some(&b'>') {
            return Some((close, end + 1));
        }
        cursor = end;
    }
}

/// Find an opening tag end without accepting `>` inside quoted attributes.
fn find_tag_end(source: &[u8], start: usize) -> Option<usize> {
    let mut quote = None;
    for (offset, byte) in source.get(start..)?.iter().copied().enumerate() {
        match (quote, byte) {
            (Some(expected), current) if current == expected => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Some(start + offset),
            _ => {}
        }
    }
    None
}

/// Find an ASCII tag marker without allocating a lowercased host copy.
fn find_ascii_case_insensitive(source: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    source
        .get(start..)?
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
        .map(|offset| start + offset)
}

/// Find an exact byte marker after one byte offset.
fn find_exact(source: &[u8], start: usize, needle: &[u8]) -> Option<usize> {
    source
        .get(start..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| start + offset)
}

#[cfg(test)]
mod tests {
    use super::{
        EmbeddedProjectionError, EmbeddedScriptLanguage, MAX_EMBEDDED_SCRIPT_REGIONS, project,
    };

    #[test]
    fn projection_preserves_host_offsets_lines_columns_and_newlines() {
        let source =
            "<p>Grüße</p>\r\n<script lang='ts'>\r\n  export const answer = 42;\r\n</script>\r\n";
        let (projections, incomplete) = project(source).into_parts();
        assert_eq!(incomplete, None);
        assert!(!projections.is_empty());
        let Some(projection) = projections.first() else {
            return;
        };
        assert_eq!(projection.language(), EmbeddedScriptLanguage::TypeScript);
        assert_eq!(projection.source().len(), source.len());
        assert_eq!(projection.source().matches('\n').count(), 4);
        let marker = "export const answer";
        let source_offset = source.find(marker);
        assert!(source_offset.is_some());
        let Some(source_offset) = source_offset else {
            return;
        };
        let projected_offset = projection.source().find(marker);
        assert!(projected_offset.is_some());
        let Some(projected_offset) = projected_offset else {
            return;
        };
        assert_eq!(projected_offset, source_offset);
        assert_eq!(
            line_column(source, source_offset),
            line_column(projection.source(), projected_offset)
        );
        assert!(
            projection.source()[..source_offset]
                .bytes()
                .all(|byte| matches!(byte, b' ' | b'\r' | b'\n'))
        );
    }

    #[test]
    fn projection_accepts_conservative_script_forms_and_ignores_external_or_unknown_scripts() {
        let source = r#"<script>export const js = 1;</script>
<script type="module">export const moduleValue = 2;</script>
<script lang=ts>export const tsValue: number = 3;</script>
<script type="text/typescript; charset=utf-8">export const typed = 4;</script>
<script src="remote.js">export const external = 5;</script>
<script lang="coffee">export const unknown = 6;</script>
<!-- <script>export const commented = 7;</script> -->
<div data-code="<script>export const attributed = 8;</script>"></div>"#;
        let (projections, incomplete) = project(source).into_parts();
        assert_eq!(incomplete, None);
        assert_eq!(projections.len(), 2);
        let Some(javascript) = projections
            .iter()
            .find(|projection| projection.language() == EmbeddedScriptLanguage::JavaScript)
        else {
            return;
        };
        let Some(typescript) = projections
            .iter()
            .find(|projection| projection.language() == EmbeddedScriptLanguage::TypeScript)
        else {
            return;
        };
        assert!(javascript.source().contains("export const js"));
        assert!(javascript.source().contains("export const moduleValue"));
        assert!(typescript.source().contains("export const tsValue"));
        assert!(typescript.source().contains("export const typed"));
        assert!(!javascript.source().contains("external"));
        assert!(!typescript.source().contains("unknown"));
        assert!(!javascript.source().contains("commented"));
        assert!(!javascript.source().contains("attributed"));
    }

    #[test]
    fn malformed_and_excessive_regions_preserve_only_safely_admitted_prefixes() {
        let (projections, incomplete) =
            project("<script lang=\"ts\">export const missing = 1;").into_parts();
        assert!(projections.is_empty());
        assert_eq!(incomplete, Some(EmbeddedProjectionError::Malformed));

        let (projections, incomplete) =
            project("<script lang=\"ts>export const missing = 1;</script>").into_parts();
        assert!(projections.is_empty());
        assert_eq!(incomplete, Some(EmbeddedProjectionError::Malformed));

        let source = concat!(
            "<script>export const admitted = 1;</script>",
            "<script lang=\"ts\">export const missing = 2;"
        );
        let (projections, incomplete) = project(source).into_parts();
        assert_eq!(incomplete, Some(EmbeddedProjectionError::Malformed));
        assert_eq!(projections.len(), 1);
        assert!(projections[0].source().contains("export const admitted"));

        let source = "<script></script>".repeat(MAX_EMBEDDED_SCRIPT_REGIONS + 1);
        let (projections, incomplete) = project(&source).into_parts();
        assert_eq!(projections.len(), 1);
        assert_eq!(incomplete, Some(EmbeddedProjectionError::RegionLimit));

        let mut source = "<script>export const admitted = 1;</script>".to_string();
        source
            .push_str(&"<script src=\"external.js\"></script>".repeat(MAX_EMBEDDED_SCRIPT_REGIONS));
        let (projections, incomplete) = project(&source).into_parts();
        assert_eq!(incomplete, Some(EmbeddedProjectionError::RegionLimit));
        assert_eq!(projections.len(), 1);
        assert!(projections[0].source().contains("export const admitted"));
    }

    #[test]
    fn projection_ignores_script_text_inside_raw_text_hosts_and_reports_unclosed_hosts() {
        let source = concat!(
            "<style>.example { content: '<script>forged()</script>'; }</style>",
            "<textarea><script>forged()</script></textarea>",
            "<script>export const admitted = 1;</script>"
        );
        let (projections, incomplete) = project(source).into_parts();
        assert_eq!(incomplete, None);
        assert_eq!(projections.len(), 1);
        assert!(projections[0].source().contains("export const admitted"));
        assert!(!projections[0].source().contains("forged"));

        let (projections, incomplete) = project("<style><script>forged()</script>").into_parts();
        assert!(projections.is_empty());
        assert_eq!(incomplete, Some(EmbeddedProjectionError::Malformed));

        let (projections, incomplete) = project("<!-- <script>forged()</script>").into_parts();
        assert!(projections.is_empty());
        assert_eq!(incomplete, Some(EmbeddedProjectionError::Malformed));
    }

    fn line_column(source: &str, offset: usize) -> (usize, usize) {
        let prefix = &source[..offset];
        let line = prefix.matches('\n').count() + 1;
        let column = prefix
            .rsplit_once('\n')
            .map_or(prefix.len(), |(_, tail)| tail.len());
        (line, column)
    }
}
