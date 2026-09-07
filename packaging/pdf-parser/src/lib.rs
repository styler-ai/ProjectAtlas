//! Fixed PDF parser guest. The host owns execution fuel and linear-memory limits.
//!
//! Output is little-endian page count followed by (page, byte length, UTF-8) records.
//! No output is published until all pages have been accepted.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

#[path = "../limits.rs"]
mod limits;
mod text;
use limits::{EXPANDED_LIMIT, FACT_LIMIT, Failure, INPUT_LIMIT, OUTPUT_LIMIT};

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Reserve bounded input; zero is a refusal and never a writable input pointer.
#[unsafe(no_mangle)]
pub extern "C" fn input(size: u32) -> u32 {
    if size == 0 || size as usize > INPUT_LIMIT {
        return 0;
    }
    INPUT.with(|input| {
        let mut input = input.borrow_mut();
        input.resize(size as usize, 0);
        input.as_mut_ptr() as u32
    })
}

fn parser_failure(error: lopdf::Error) -> Failure {
    match error {
        lopdf::Error::Decompress(lopdf::DecompressError::MemoryLimitExceeded { .. }) => {
            Failure::Expanded
        }
        _ => Failure::Malformed,
    }
}

/// Validate every declared stream instead of accepting the library's error fallback.
fn page_content(
    document: &lopdf::Document,
    page: lopdf::ObjectId,
    maximum: usize,
) -> Result<Vec<u8>, Failure> {
    let page = document.get_dictionary(page).map_err(parser_failure)?;
    let contents = match page.get(b"Contents") {
        Ok(contents) => contents,
        Err(lopdf::Error::DictKey(_)) => return Ok(Vec::new()),
        Err(error) => return Err(parser_failure(error)),
    };
    let (_, contents) = document.dereference(contents).map_err(parser_failure)?;
    let streams = match contents {
        lopdf::Object::Array(streams) => streams.as_slice(),
        lopdf::Object::Null => return Ok(Vec::new()),
        stream => std::slice::from_ref(stream),
    };
    let mut content = Vec::new();
    for object in streams {
        let (_, object) = document.dereference(object).map_err(parser_failure)?;
        let stream = object.as_stream().map_err(parser_failure)?;
        if stream.dict.has(b"Filter") {
            stream.filters().map_err(parser_failure)?;
        }
        let data = stream
            .get_plain_content_with_limit(maximum.saturating_sub(content.len()))
            .map_err(parser_failure)?;
        if content.len().saturating_add(data.len()).saturating_add(1) > maximum {
            return Err(Failure::Expanded);
        }
        content.extend_from_slice(&data);
        content.push(b'\n');
    }
    Ok(content)
}

fn parse(bytes: &[u8]) -> Result<(Vec<u8>, usize), Failure> {
    if !bytes.starts_with(b"%PDF-") {
        return Err(Failure::Malformed);
    }
    let mut document = lopdf::Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions::with_max_decompressed_size(EXPANDED_LIMIT),
    )
    .map_err(parser_failure)?;
    if document.is_encrypted() {
        return Err(Failure::Encrypted);
    }
    let pages = document.get_pages();
    validate_page_tree(&document, &pages)?;
    if pages.is_empty() {
        return Err(Failure::Malformed);
    }
    if pages.len() > FACT_LIMIT {
        return Err(Failure::Pages);
    }
    // Decode admitted streams once with an aggregate ceiling. The canonical
    // formatter therefore never reaches its historical decompression fallback.
    // Images remain opaque because their pixels are outside text extraction.
    let mut expanded = 0usize;
    for object in document.objects.values_mut() {
        let lopdf::Object::Stream(stream) = object else {
            continue;
        };
        if stream
            .dict
            .get(b"Subtype")
            .and_then(lopdf::Object::as_name)
            .ok()
            == Some(b"Image".as_slice())
        {
            continue;
        }
        if stream.dict.has(b"Filter") {
            stream.filters().map_err(parser_failure)?;
        }
        let decoded = stream
            .get_plain_content_with_limit(EXPANDED_LIMIT.saturating_sub(expanded))
            .map_err(parser_failure)?;
        expanded = expanded.saturating_add(decoded.len());
        if expanded > EXPANDED_LIMIT {
            return Err(Failure::Expanded);
        }
        stream.set_content(decoded);
        stream.dict.remove(b"Filter");
        stream.dict.remove(b"DecodeParms");
    }
    let mut wire = Vec::new();
    wire.extend_from_slice(&(pages.len() as u32).to_le_bytes());
    let mut total = 0usize;
    for (page, id) in pages {
        let content = page_content(&document, id, EXPANDED_LIMIT)?;
        lopdf::content::Content::decode_strict(&content).map_err(parser_failure)?;
        drop(content);
        let text = text::page(&document, page, OUTPUT_LIMIT.saturating_sub(total))?;
        total = total.saturating_add(text.len());
        if total > OUTPUT_LIMIT {
            return Err(Failure::Output);
        }
        wire.extend_from_slice(&page.to_le_bytes());
        wire.extend_from_slice(&(text.len() as u32).to_le_bytes());
        wire.extend_from_slice(text.as_bytes());
    }
    Ok((wire, total))
}

/// Check every declared page-tree edge; the library iterator may silently skip it.
fn validate_page_tree(
    document: &lopdf::Document,
    pages: &BTreeMap<u32, lopdf::ObjectId>,
) -> Result<(), Failure> {
    let root = document
        .catalog()
        .map_err(parser_failure)?
        .get(b"Pages")
        .map_err(parser_failure)?
        .as_reference()
        .map_err(parser_failure)?;
    let mut pending = vec![(root, None)];
    let mut visited = HashSet::new();
    let mut discovered = 0usize;
    while let Some((id, entered_at)) = pending.pop() {
        let node = document.get_dictionary(id).map_err(parser_failure)?;
        if let Some(before) = entered_at {
            let declared = node
                .get(b"Count")
                .map_err(parser_failure)?
                .as_i64()
                .map_err(parser_failure)?;
            if usize::try_from(declared).ok() != Some(discovered - before) {
                return Err(Failure::Malformed);
            }
            continue;
        }
        if !visited.insert(id) {
            return Err(Failure::Malformed);
        }
        match node
            .get(b"Type")
            .map_err(parser_failure)?
            .as_name()
            .map_err(parser_failure)?
        {
            b"Pages" => {
                let children = node
                    .get(b"Kids")
                    .map_err(parser_failure)?
                    .as_array()
                    .map_err(parser_failure)?;
                pending.push((id, Some(discovered)));
                for child in children.iter().rev() {
                    pending.push((child.as_reference().map_err(parser_failure)?, None));
                }
            }
            b"Page" => {
                discovered += 1;
                if discovered > FACT_LIMIT {
                    return Err(Failure::Pages);
                }
                if pages.get(&(discovered as u32)) != Some(&id) {
                    return Err(Failure::Malformed);
                }
            }
            _ => return Err(Failure::Malformed),
        }
    }
    if discovered == 0 || discovered != pages.len() {
        return Err(Failure::Malformed);
    }
    Ok(())
}

/// Parse the admitted input atomically; failed calls expose no previous output.
#[unsafe(no_mangle)]
pub extern "C" fn extract() -> i32 {
    OUTPUT.with(|output| output.borrow_mut().clear());
    INPUT.with(|input| match parse(&input.borrow()) {
        Ok((wire, total)) => {
            OUTPUT.with(|output| *output.borrow_mut() = wire);
            total as i32
        }
        Err(error) => error as i32,
    })
}

/// Return the output's linear-memory address; the host validates its range.
#[unsafe(no_mangle)]
pub extern "C" fn output_ptr() -> u32 {
    OUTPUT.with(|output| output.borrow().as_ptr() as u32)
}

/// Return the complete output wire length.
#[unsafe(no_mangle)]
pub extern "C" fn output_len() -> u32 {
    OUTPUT.with(|output| output.borrow().len() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn invalid_input_cannot_expose_previous_output() {
        OUTPUT.with(|output| *output.borrow_mut() = b"previous text".to_vec());
        INPUT.with(|input| *input.borrow_mut() = b"not a PDF".to_vec());
        assert_eq!(extract(), Failure::Malformed as i32);
        assert_eq!(output_len(), 0);
        assert_eq!(input(0), 0);
        assert_eq!(input(INPUT_LIMIT as u32 + 1), 0);
    }

    #[test]
    fn canonical_page_formatter_propagates_the_output_limit() {
        let mut document = lopdf::Document::new();
        let pages = document.new_object_id();
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"
        });
        let content = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            b"BT /F1 12 Tf 72 720 Td (Output Marker) Tj ET".to_vec(),
        ));
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }
        });
        document.objects.insert(
            pages,
            dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1
            }
            .into(),
        );
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        document.trailer.set("Root", catalog);
        assert!(
            text::page(&document, 1, 64)
                .unwrap()
                .contains("Output Marker")
        );
        assert!(matches!(text::page(&document, 1, 4), Err(Failure::Output)));
    }

    #[test]
    fn nested_forms_preserve_graphics_state_and_composed_transforms() {
        #[derive(Default)]
        struct GlyphTransforms(Vec<[f64; 6]>);
        impl pdf_extract::OutputDev for GlyphTransforms {
            fn begin_page(
                &mut self,
                _: u32,
                _: &pdf_extract::MediaBox,
                _: Option<(f64, f64, f64, f64)>,
            ) -> Result<(), pdf_extract::OutputError> {
                Ok(())
            }
            fn end_page(&mut self) -> Result<(), pdf_extract::OutputError> {
                Ok(())
            }
            fn begin_word(&mut self) -> Result<(), pdf_extract::OutputError> {
                Ok(())
            }
            fn end_word(&mut self) -> Result<(), pdf_extract::OutputError> {
                Ok(())
            }
            fn end_line(&mut self) -> Result<(), pdf_extract::OutputError> {
                Ok(())
            }
            fn output_character(
                &mut self,
                transform: &pdf_extract::Transform,
                _: f64,
                _: f64,
                _: f64,
                _: &str,
            ) -> Result<(), pdf_extract::OutputError> {
                self.0.push([
                    transform.m11,
                    transform.m12,
                    transform.m21,
                    transform.m22,
                    transform.m31,
                    transform.m32,
                ]);
                Ok(())
            }
        }
        let transforms = |document: &lopdf::Document| {
            let mut output = GlyphTransforms::default();
            pdf_extract::output_doc_page(document, &mut output, 1).unwrap();
            output.0
        };
        let mut document = lopdf::Document::new();
        let pages = document.new_object_id();
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"
        });
        let inner = document.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 200.into(), 100.into()],
                "Matrix" => vec![2.into(), 0.into(), 0.into(), 3.into(), 5.into(), 7.into()]
            },
            b"BT /F1 12 Tf (Form Marker) Tj ET".to_vec(),
        ));
        let outer = document.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 500.into(), 400.into()],
                "Matrix" => vec![1.into(), 0.into(), 0.into(), 1.into(), 10.into(), 20.into()]
            },
            b"q /Inner Do Q".to_vec(),
        ));
        let direct = b"BT /F1 12 Tf 2 0 0 3 40 520 Tm (Form Marker) Tj 2 0 0 3 40 220 Tm (Form Marker) Tj ET";
        let content = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            direct.to_vec(),
        ));
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font },
                "XObject" => dictionary! { "Inner" => inner, "Outer" => outer }
            }
        });
        document.objects.insert(
            pages,
            dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1
            }
            .into(),
        );
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        document.trailer.set("Root", catalog);
        let expected = text::page(&document, 1, 128).unwrap();
        let expected_transforms = transforms(&document);
        let assert_transforms = |document: &lopdf::Document| {
            let actual = transforms(document);
            assert_eq!(actual.len(), expected_transforms.len());
            for (actual, expected) in actual
                .iter()
                .flatten()
                .zip(expected_transforms.iter().flatten())
            {
                assert!((actual - expected).abs() < 1e-9, "{actual} != {expected}");
            }
        };
        assert_eq!(expected.matches("Form Marker").count(), 2);
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(
                b"BT /F1 12 Tf ET q 1 0 0 1 25 493 cm /Outer Do Q q 1 0 0 1 25 193 cm /Outer Do Q"
                    .to_vec(),
            );
        assert_eq!(text::page(&document, 1, 128).unwrap(), expected);
        assert_transforms(&document);
        document
            .get_object_mut(inner)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"BT (Form Marker) Tj ET".to_vec());
        assert_eq!(text::page(&document, 1, 128).unwrap(), expected);
        assert_transforms(&document);
        for invalid in [
            vec![1.into()],
            vec![
                1.into(),
                0.into(),
                0.into(),
                1.into(),
                0.into(),
                lopdf::Object::Null,
            ],
        ] {
            document
                .get_object_mut(inner)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .dict
                .set("Matrix", invalid);
            assert!(matches!(
                text::page(&document, 1, 128),
                Err(Failure::Malformed)
            ));
        }
    }

    #[test]
    fn inherited_page_rotation_preserves_displayed_text_lines() {
        let mut document = lopdf::Document::new();
        let pages = document.new_object_id();
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"
        });
        let direct = b"BT /F1 12 Tf 1 0 0 1 72 500 Tm (First) Tj 1 0 0 1 180 500 Tm (Second) Tj 1 0 0 1 72 400 Tm (Next) Tj ET";
        let content = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            direct.to_vec(),
        ));
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }
        });
        document.objects.insert(
            pages,
            dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1
            }
            .into(),
        );
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        document.trailer.set("Root", catalog);
        let expected = text::page(&document, 1, 128).unwrap();
        assert!(expected.contains("First Second"));
        for (rotation, matrix, positions) in [
            (0, [1, 0, 0, 1], [[72, 500], [180, 500], [72, 400]]),
            (90, [0, 1, -1, 0], [[112, 72], [112, 180], [212, 72]]),
            (180, [-1, 0, 0, -1], [[540, 292], [432, 292], [540, 392]]),
            (270, [0, -1, 1, 0], [[500, 720], [500, 612], [400, 720]]),
            (450, [0, 1, -1, 0], [[112, 72], [112, 180], [212, 72]]),
            (-90, [0, -1, 1, 0], [[500, 720], [500, 612], [400, 720]]),
        ] {
            document
                .get_object_mut(pages)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Rotate", rotation);
            let mut stream = String::from("BT /F1 12 Tf ");
            for ([x, y], word) in positions.into_iter().zip(["First", "Second", "Next"]) {
                stream.push_str(&format!(
                    "{} {} {} {} {x} {y} Tm ({word}) Tj ",
                    matrix[0], matrix[1], matrix[2], matrix[3]
                ));
            }
            stream.push_str("ET");
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(stream.into_bytes());
            assert_eq!(
                text::page(&document, 1, 128).unwrap(),
                expected,
                "rotation={rotation}"
            );
        }
        document
            .get_object_mut(page)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Rotate", 0);
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(direct.to_vec());
        assert_eq!(
            text::page(&document, 1, 128).unwrap(),
            expected,
            "leaf overrides inherited rotation"
        );
        for invalid in [
            lopdf::Object::Integer(45),
            lopdf::Object::Real(90.0),
            lopdf::Object::Name(b"90".to_vec()),
        ] {
            document
                .get_object_mut(page)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Rotate", invalid);
            assert!(matches!(
                text::page(&document, 1, 128),
                Err(Failure::Malformed)
            ));
        }
    }

    #[test]
    fn declared_missing_stream_is_not_a_blank_page() {
        let mut document = lopdf::Document::new();
        let page = document.add_object(lopdf::dictionary! {
            "Type" => "Page",
            "Contents" => lopdf::Object::Reference((999, 0)),
        });
        assert!(matches!(
            page_content(&document, page, EXPANDED_LIMIT),
            Err(Failure::Malformed)
        ));
    }

    #[test]
    fn compressed_stream_refuses_expansion_before_returning_content() {
        let mut document = lopdf::Document::new();
        let mut stream = lopdf::Stream::new(lopdf::Dictionary::new(), vec![b' '; 8192]);
        stream.compress().unwrap();
        assert!(stream.content.len() < 1024);
        let stream = document.add_object(stream);
        let page = document.add_object(dictionary! { "Contents" => stream });
        assert!(matches!(
            page_content(&document, page, 1024),
            Err(Failure::Expanded)
        ));
        assert_eq!(page_content(&document, page, 8193).unwrap().len(), 8193);
    }

    #[test]
    fn stream_separator_counts_toward_expansion_limit() {
        let mut document = lopdf::Document::new();
        let stream = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            b"BT ET".to_vec(),
        ));
        let page = document.add_object(dictionary! { "Contents" => stream });
        assert!(matches!(
            page_content(&document, page, 5),
            Err(Failure::Expanded)
        ));
        assert_eq!(
            page_content(&document, page, 6).ok(),
            Some(b"BT ET\n".to_vec())
        );
    }
}
