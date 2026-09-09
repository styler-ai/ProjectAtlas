//! Fixed PDF parser guest. The host owns execution fuel and linear-memory limits.
//!
//! Output is little-endian page count followed by (page, byte length, UTF-8) records.
//! No output is published until all pages have been accepted.

use std::cell::RefCell;
use std::collections::HashSet;

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
    refuse_structure_replacements(&document)?;
    let discovered = validate_page_tree(&mut document)?;
    let pages = document.get_pages();
    if pages.len() != discovered {
        return Err(Failure::Malformed);
    }
    if pages.len() > FACT_LIMIT {
        return Err(Failure::Pages);
    }
    // Decode admitted streams once with an aggregate ceiling. The canonical
    // formatter therefore never reaches its historical decompression fallback.
    // Images remain opaque because their pixels are outside text extraction.
    let images: HashSet<_> = document
        .objects
        .iter()
        .filter_map(|(id, object)| {
            let lopdf::Object::Stream(stream) = object else {
                return None;
            };
            (stream
                .dict
                .get_deref(b"Subtype", &document)
                .and_then(lopdf::Object::as_name)
                .ok()
                == Some(b"Image".as_slice()))
            .then_some(*id)
        })
        .collect();
    let mut expanded = 0usize;
    for (id, object) in &mut document.objects {
        let lopdf::Object::Stream(stream) = object else {
            continue;
        };
        if images.contains(id) {
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
    drop(images);
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

/// Inspect only structure children, without interpreting logical order or replacements.
fn refuse_structure_replacements(document: &lopdf::Document) -> Result<(), Failure> {
    let catalog = document.catalog().map_err(parser_failure)?;
    let root = match catalog.get(b"StructTreeRoot") {
        Ok(root) => root,
        Err(lopdf::Error::DictKey(_)) => return Ok(()),
        Err(error) => return Err(parser_failure(error)),
    };
    document
        .dereference(root)
        .map_err(parser_failure)?
        .1
        .as_dict()
        .map_err(parser_failure)?;
    let mut pending = vec![root];
    let mut visited = HashSet::new();
    while let Some(object) = pending.pop() {
        let (id, object) = document.dereference(object).map_err(parser_failure)?;
        if matches!(
            object,
            lopdf::Object::Dictionary(_) | lopdf::Object::Array(_)
        ) && id.is_some_and(|id| !visited.insert(id))
        {
            return Err(Failure::Malformed);
        }
        match object {
            lopdf::Object::Dictionary(node) => {
                if node.has(b"ActualText") {
                    node.get_deref(b"ActualText", document)
                        .map_err(parser_failure)?
                        .as_str()
                        .map_err(parser_failure)?;
                    return Err(Failure::Unsupported);
                }
                if let Ok(children) = node.get(b"K") {
                    pending.push(children);
                }
            }
            lopdf::Object::Array(children) => pending.extend(children),
            lopdf::Object::Integer(value) if *value >= 0 => {}
            lopdf::Object::Null => {}
            _ => return Err(Failure::Malformed),
        }
    }
    Ok(())
}

/// Check every declared page-tree edge; the library iterator may silently skip it.
fn validate_page_tree(document: &mut lopdf::Document) -> Result<usize, Failure> {
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
                .get_deref(b"Count", document)
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
        let indirect_type = matches!(node.get(b"Type"), Ok(lopdf::Object::Reference(_)));
        let node_type = match node
            .get_deref(b"Type", document)
            .map_err(parser_failure)?
            .as_name()
            .map_err(parser_failure)?
        {
            b"Pages" => b"Pages".as_slice(),
            b"Page" => b"Page".as_slice(),
            _ => return Err(Failure::Malformed),
        };
        match node_type {
            b"Pages" => {
                let children = node
                    .get_deref(b"Kids", document)
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
            }
            _ => return Err(Failure::Malformed),
        }
        if indirect_type {
            // The upstream page iterator requires direct type names. Only
            // canonicalize validated nodes in this private parsed document.
            document
                .get_dictionary_mut(id)
                .map_err(parser_failure)?
                .set("Type", lopdf::Object::Name(node_type.to_vec()));
        }
    }
    Ok(discovered)
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
    fn empty_page_tree_publishes_empty_output_and_checks_declared_count() {
        let mut document = lopdf::Document::new();
        let pages = document.add_object(dictionary! {
            "Type" => "Pages", "Kids" => Vec::<lopdf::Object>::new(), "Count" => 0
        });
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        document.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        assert_eq!(parse(&bytes), Ok((0u32.to_le_bytes().to_vec(), 0)));
        OUTPUT.with(|output| *output.borrow_mut() = b"previous text".to_vec());
        INPUT.with(|input| *input.borrow_mut() = bytes);
        assert_eq!(extract(), 0);
        OUTPUT.with(|output| assert_eq!(*output.borrow(), 0u32.to_le_bytes()));

        let children = document.add_object(Vec::<lopdf::Object>::new());
        let count = document.add_object(0);
        let node = document.get_dictionary_mut(pages).unwrap();
        node.set("Kids", children);
        node.set("Count", count);
        let mut indirect = Vec::new();
        document.save_to(&mut indirect).unwrap();
        assert_eq!(parse(&indirect), Ok((0u32.to_le_bytes().to_vec(), 0)));

        let cycle = document.new_object_id();
        document
            .objects
            .insert(cycle, lopdf::Object::Reference(cycle));
        for invalid_type in [
            None,
            Some(lopdf::Object::Integer(7)),
            Some(lopdf::Object::Reference((999, 0))),
            Some(lopdf::Object::Reference(cycle)),
        ] {
            let node = document.get_dictionary_mut(pages).unwrap();
            if let Some(value) = invalid_type {
                node.set("Type", value);
            } else {
                node.remove(b"Type");
            }
            let mut invalid = Vec::new();
            document.save_to(&mut invalid).unwrap();
            assert_eq!(parse(&invalid), Err(Failure::Malformed));
        }
        document
            .get_dictionary_mut(pages)
            .unwrap()
            .set("Type", "Pages");

        document.get_dictionary_mut(pages).unwrap().set("Count", 1);
        let mut invalid = Vec::new();
        document.save_to(&mut invalid).unwrap();
        assert_eq!(parse(&invalid), Err(Failure::Malformed));
    }

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

        let parse_document = |document: &mut lopdf::Document| {
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).unwrap();
            parse(&bytes)
        };
        let direct = parse_document(&mut document).unwrap();
        let children = document.add_object(vec![lopdf::Object::Reference(page)]);
        let count = document.add_object(1);
        for (kids, declared) in [
            (children.into(), lopdf::Object::Integer(1)),
            (lopdf::Object::Array(vec![page.into()]), count.into()),
            (children.into(), count.into()),
        ] {
            let node = document.get_dictionary_mut(pages).unwrap();
            node.set("Kids", kids);
            node.set("Count", declared);
            assert_eq!(parse_document(&mut document), Ok(direct.clone()));
        }

        let nested = document.add_object(dictionary! {
            "Type" => "Pages", "Parent" => pages, "Kids" => children, "Count" => count
        });
        document
            .get_dictionary_mut(page)
            .unwrap()
            .set("Parent", nested);
        let nested_children = document.add_object(vec![lopdf::Object::Reference(nested)]);
        let nested_alias = document.add_object(lopdf::Object::Reference(nested_children));
        document
            .get_dictionary_mut(pages)
            .unwrap()
            .set("Kids", nested_alias);
        assert_eq!(parse_document(&mut document), Ok(direct));

        let cycle = document.new_object_id();
        document
            .objects
            .insert(cycle, lopdf::Object::Reference(cycle));
        let missing = lopdf::Object::Reference(document.new_object_id());
        for invalid in [
            lopdf::Object::Null,
            0.into(),
            cycle.into(),
            missing.clone(),
            lopdf::Object::Array(vec![missing.clone()]),
            lopdf::Object::Array(vec![pages.into()]),
        ] {
            document
                .get_dictionary_mut(pages)
                .unwrap()
                .set("Kids", invalid);
            assert_eq!(parse_document(&mut document), Err(Failure::Malformed));
        }
        document
            .get_dictionary_mut(pages)
            .unwrap()
            .set("Kids", nested_alias);
        for invalid in [
            lopdf::Object::Null,
            cycle.into(),
            missing,
            0.into(),
            2.into(),
        ] {
            document
                .get_dictionary_mut(pages)
                .unwrap()
                .set("Count", invalid);
            assert_eq!(parse_document(&mut document), Err(Failure::Malformed));
        }
    }

    #[test]
    fn closed_subpaths_preserve_visible_text_and_missing_points_refuse() {
        let mut document = lopdf::Document::new();
        let pages = document.new_object_id();
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"
        });
        let content = document.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), Vec::new()));
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
        for (path, valid) in [
            ("10 20 m 30 40 l h 50 60 70 80 v S", true),
            ("10 20 30 40 re 50 60 70 80 v S", true),
            (
                "10 20 m 30 40 l h 90 100 m 110 120 l h 50 60 70 80 v S",
                true,
            ),
            ("10 20 m 30 40 50 60 70 80 c 90 100 110 120 v S", true),
            ("50 60 70 80 v", false),
            ("h 50 60 70 80 v", false),
            ("10 20 m n 50 60 70 80 v", false),
            ("10 20 m s 50 60 70 80 v", false),
            ("10 20 m f* 50 60 70 80 v", false),
            ("10 20 m B 50 60 70 80 v", false),
            ("10 20 m B* 50 60 70 80 v", false),
            ("10 20 m b 50 60 70 80 v", false),
            ("10 20 m b* 50 60 70 80 v", false),
            ("10 20 m S 50 60 70 80 v", false),
            ("10 20 m f 50 60 70 80 v", false),
            ("10 20 m F 50 60 70 80 v", false),
            (
                "10 20 30 40 re s 10 20 30 40 re f* 10 20 30 40 re B 10 20 30 40 re B* 10 20 30 40 re b 10 20 30 40 re b*",
                true,
            ),
        ] {
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(format!("{path} BT /F1 12 Tf 72 700 Td (Visible) Tj ET").into_bytes());
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).unwrap();
            let result = parse(&bytes);
            if valid {
                let (wire, _) = result.unwrap();
                assert_eq!(std::str::from_utf8(&wire[12..]).unwrap().trim(), "Visible");
            } else {
                assert_eq!(result, Err(Failure::Malformed));
            }
        }
    }

    #[test]
    fn postscript_xobjects_do_not_change_displayed_text() {
        let mut document = lopdf::Document::new();
        let pages = document.new_object_id();
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"
        });
        let postscript = document.add_object(lopdf::Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "PS" },
            b"/Helvetica findfont 12 scalefont setfont (Print only) show".to_vec(),
        ));
        let form = document.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 612.into(), 792.into()]
            },
            b"/Print Do BT /F1 12 Tf 72 700 Td (Visible) Tj ET".to_vec(),
        ));
        let image = document.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Filter" => "DCTDecode",
                "Width" => 1, "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8
            },
            vec![0],
        ));
        let content = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            b"/Image Do /Print Do /Form Do".to_vec(),
        ));
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font },
                "XObject" => dictionary! { "Print" => postscript, "Form" => form, "Image" => image }
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
        for (indirect, subtype) in [
            (false, "PS"),
            (false, "Form"),
            (false, "Unknown"),
            (true, "PS"),
            (true, "Form"),
            (true, "Unknown"),
        ] {
            for (id, key, name) in [
                (pages, "Type", "Pages"),
                (page, "Type", "Page"),
                (form, "Subtype", "Form"),
                (image, "Subtype", "Image"),
                (postscript, "Subtype", subtype),
                (postscript, "Subtype2", "PS"),
            ] {
                let value = lopdf::Object::Name(name.as_bytes().to_vec());
                let value = if indirect {
                    document.add_object(value).into()
                } else {
                    value
                };
                let dictionary = match document.get_object_mut(id).unwrap() {
                    lopdf::Object::Stream(stream) => &mut stream.dict,
                    lopdf::Object::Dictionary(dictionary) => dictionary,
                    _ => unreachable!(),
                };
                dictionary.set(key, value);
            }
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).unwrap();
            let result = parse(&bytes);
            if subtype == "Unknown" {
                assert_eq!(result, Err(Failure::Malformed));
            } else {
                let (wire, count) = result.unwrap();
                assert_eq!(std::str::from_utf8(&wire[12..]).unwrap().trim(), "Visible");
                assert_eq!(count, wire.len() - 12);
            }
        }
    }

    fn glyph_transforms(document: &lopdf::Document) -> Vec<[f64; 6]> {
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
        let mut output = GlyphTransforms::default();
        pdf_extract::output_doc_page(document, &mut output, 1).unwrap();
        output.0
    }

    #[test]
    fn nested_forms_preserve_graphics_state_and_composed_transforms() {
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
        let expected_transforms = glyph_transforms(&document);
        let assert_transforms = |document: &lopdf::Document| {
            let actual = glyph_transforms(document);
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
        let text_scope = b"BT /F1 12 Tf 20 TL 72 500 Td q 100 -100 Td (A) Tj Q T* (B) Tj ET";
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(text_scope.to_vec());
        let positions = glyph_transforms(&document);
        assert_eq!(
            positions.iter().map(|m| (m[4], m[5])).collect::<Vec<_>>(),
            [(172., 400.), (72., 480.)]
        );
        let form = document
            .get_object_mut(inner)
            .unwrap()
            .as_stream_mut()
            .unwrap();
        form.dict.remove(b"Matrix");
        form.set_content(text_scope.to_vec());
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(
                b"BT /F1 12 Tf 72 600 Td (C) Tj ET /Inner Do BT /F1 12 Tf 72 300 Td (D) Tj ET"
                    .to_vec(),
            );
        let positions = glyph_transforms(&document);
        assert_eq!(
            positions.iter().map(|m| (m[4], m[5])).collect::<Vec<_>>(),
            [(72., 600.), (172., 400.), (72., 480.), (72., 300.)]
        );
        let form = document
            .get_object_mut(inner)
            .unwrap()
            .as_stream_mut()
            .unwrap();
        form.dict
            .set("BBox", vec![0.into(), 0.into(), 612.into(), 792.into()]);
        form.dict.set(
            "Matrix",
            vec![
                0.into(),
                1.into(),
                (-1).into(),
                0.into(),
                800.into(),
                0.into(),
            ],
        );
        form.set_content(
            b"BT /F1 12 Tf 72 500 Td (First) Tj 108 0 Td (Second) Tj -108 -100 Td (Next) Tj ET"
                .to_vec(),
        );
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"/Inner Do".to_vec());
        assert_eq!(text::page(&document, 1, 128).unwrap(), "First Second\nNext");
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
    fn cid_range_widths_match_explicit_widths_including_endpoints() {
        let mut document = lopdf::Document::new();
        let pages = document.new_object_id();
        let cmap = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            br"/CIDInit /ProcSet findresource begin
12 dict begin begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Fixture def /CMapType 2 def
1 begincodespacerange <0000> <FFFF> endcodespacerange
4 beginbfchar <0001> <0041> <0002> <0042> <0003> <0043> <0004> <0044> endbfchar
endcmap CMapName currentdict /CMap defineresource pop end end"
                .to_vec(),
        ));
        let descriptor = document.add_object(dictionary! {
            "Type" => "FontDescriptor", "FontName" => "Fixture", "Flags" => 4,
            "FontBBox" => vec![0.into(), (-200).into(), 1000.into(), 1000.into()],
            "ItalicAngle" => 0, "Ascent" => 800, "Descent" => -200,
            "CapHeight" => 700, "StemV" => 80
        });
        let descendant = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "CIDFontType2", "BaseFont" => "Fixture",
            "FontDescriptor" => descriptor, "DW" => 1000,
            "CIDSystemInfo" => dictionary! { "Registry" => lopdf::Object::string_literal("Adobe"),
                "Ordering" => lopdf::Object::string_literal("Identity"), "Supplement" => 0 }
        });
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "Fixture",
            "Encoding" => "Identity-H", "DescendantFonts" => vec![descendant.into()], "ToUnicode" => cmap
        });
        let content = document.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), Vec::new()));
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }
        });
        document.objects.insert(
            pages,
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
        );
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        document.trailer.set("Root", catalog);
        document.version = "2.0".to_owned();
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"BT /F1 10 Tf 72 500 Td <00010002> Tj ET".to_vec());
        for (width, advance) in [
            (lopdf::Object::Integer(500), 5.0),
            (lopdf::Object::Real(500.5), 5.005),
        ] {
            document
                .get_dictionary_mut(descendant)
                .unwrap()
                .set("DW", width);
            let positions = glyph_transforms(&document);
            assert_eq!(positions.len(), 2);
            assert_eq!((positions[0][4], positions[0][5]), (72.0, 500.0));
            assert!((positions[1][4] - 72.0 - advance).abs() < 1e-9);
            assert_eq!(positions[1][5], 500.0);
        }
        document
            .get_dictionary_mut(descendant)
            .unwrap()
            .remove(b"DW");
        assert_eq!(glyph_transforms(&document)[1][4], 82.0);
        for (widths, text, expected) in [
            (
                vec![1.into(), 2.into(), 200.into()],
                "<0001> Tj 3 0 Td <0002> Tj 8 0 Td <0003> Tj 8 0 Td <0004> Tj",
                "AB CD",
            ),
            (
                vec![1.into(), lopdf::Object::Array(vec![200.into(), 200.into()])],
                "<0001> Tj 3 0 Td <0002> Tj 8 0 Td <0003> Tj 8 0 Td <0004> Tj",
                "AB CD",
            ),
            (
                vec![1.into(), 1.into(), 200.into()],
                "<0001> Tj 8 0 Td <0002> Tj",
                "A B",
            ),
        ] {
            document
                .get_object_mut(descendant)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("W", widths);
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(format!("BT /F1 12 Tf 72 500 Td {text} ET").into_bytes());
            assert_eq!(text::page(&document, 1, 128).unwrap().trim(), expected);
        }
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"BT /F1 12 Tf 72 500 Td <00010005> Tj ET".to_vec());
        assert!(matches!(
            text::page(&document, 1, 128),
            Err(Failure::Unsupported)
        ));
        let resources = document
            .get_object_mut(page)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .get_mut(b"Resources")
            .unwrap()
            .as_dict_mut()
            .unwrap();
        resources.set(
            "ExtGState",
            dictionary! { "GS" => dictionary! { "Font" => vec![font.into(), 12.into()] } },
        );
        let vertical_cmap = document.add_object(lopdf::Stream::new(
            dictionary! { "Type" => "CMap" },
            br"begincmap /WMode 1 def
1 begincodespacerange <0000> <FFFF> endcodespacerange
1 begincidrange <0001> <0001> 1 endcidrange endcmap"
                .to_vec(),
        ));
        for encoding in [
            lopdf::Object::Name(b"Identity-V".to_vec()),
            vertical_cmap.into(),
        ] {
            document
                .get_object_mut(font)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Encoding", encoding);
            for selection in ["/F1 12 Tf", "/GS gs"] {
                document
                    .get_object_mut(content)
                    .unwrap()
                    .as_stream_mut()
                    .unwrap()
                    .set_content(format!("BT {selection} 72 500 Td <0001> Tj ET").into_bytes());
                assert!(matches!(
                    text::page(&document, 1, 128),
                    Err(Failure::Unsupported)
                ));
            }
        }
        for declaration in ["", "/WMode 0 def", "/WMode 1 def"] {
            document.get_object_mut(vertical_cmap).unwrap().as_stream_mut().unwrap().set_content(
                format!("begincmap {declaration}\n1 begincodespacerange <0000> <FFFF> endcodespacerange\n1 begincidrange <0001> <0001> 1 endcidrange endcmap").into_bytes()
            );
            assert!(matches!(
                text::page(&document, 1, 128),
                Err(Failure::Unsupported)
            ));
        }
        document
            .get_object_mut(font)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Encoding", 12);
        assert!(matches!(
            text::page(&document, 1, 128),
            Err(Failure::Malformed)
        ));
    }

    #[test]
    fn type3_widths_follow_the_font_matrix() {
        let mut document = lopdf::Document::new();
        let pages = document.new_object_id();
        let glyph = document.add_object(lopdf::Stream::new(dictionary! {}, b"600 0 d0".to_vec()));
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type3",
            "FontBBox" => vec![0.into(), 0.into(), 600.into(), 600.into()],
            "CharProcs" => dictionary! { "A" => glyph, "B" => glyph },
            "Encoding" => dictionary! { "Differences" => vec![65.into(), "A".into(), "B".into()] },
            "FirstChar" => 65, "LastChar" => 66, "Widths" => vec![600.into(), 600.into()]
        });
        let content = document.add_object(lopdf::Stream::new(dictionary! {}, Vec::new()));
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
        for (matrix, next_x, expected) in [
            ([0.002, 0., 0., 0.002, 0., 0.], 86.4, "AB"),
            ([0.002, 0., 0., 0.002, 0., 0.], 90., "A B"),
            ([0., 0.002, -0.002, 0., 0., 0.], 75., "A B"),
            ([-0.002, 0., 0., 0.002, 0., 0.], 60., "A B"),
            ([0.001, 0., 0., 0.001, 10., 20.], 79.2, "AB"),
        ] {
            let matrix = document.add_object(lopdf::Object::Array(
                matrix.into_iter().map(Into::into).collect(),
            ));
            document
                .get_dictionary_mut(font)
                .unwrap()
                .set("FontMatrix", matrix);
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(
                    format!("BT /F1 12 Tf 72 500 Td (A) Tj 1 0 0 1 {next_x} 500 Tm (B) Tj ET")
                        .into_bytes(),
                );
            assert_eq!(text::page(&document, 1, 128).unwrap(), expected);
        }
        document.get_dictionary_mut(font).unwrap().set(
            "Encoding",
            dictionary! { "Differences" => vec![66.into(), "B".into()] },
        );
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"BT /F1 12 Tf 72 500 Td (A) Tj ET".to_vec());
        assert!(matches!(
            text::page(&document, 1, 128),
            Err(Failure::Unsupported)
        ));
        {
            let font = document.get_dictionary_mut(font).unwrap();
            font.set("Encoding", "WinAnsiEncoding");
            font.set("FirstChar", 0);
            font.set("LastChar", 255);
            font.set("Widths", vec![lopdf::Object::Integer(600); 256]);
        }
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"BT /F1 12 Tf 72 500 Td <4181> Tj ET".to_vec());
        assert_eq!(text::page(&document, 1, 128).unwrap(), "A\u{2022}");
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"BT /F1 12 Tf 72 500 Td <4101> Tj ET".to_vec());
        assert!(matches!(
            text::page(&document, 1, 128),
            Err(Failure::Unsupported)
        ));
        for matrix in [
            None,
            Some(lopdf::Object::Array(vec![1.into()])),
            Some(lopdf::Object::Array(vec![
                "bad".into(),
                0.into(),
                0.into(),
                1.into(),
                0.into(),
                0.into(),
            ])),
            Some(lopdf::Object::Array(vec![
                f32::INFINITY.into(),
                0.into(),
                0.into(),
                1.into(),
                0.into(),
                0.into(),
            ])),
        ] {
            let font = document.get_dictionary_mut(font).unwrap();
            font.remove(b"FontMatrix");
            if let Some(matrix) = matrix {
                font.set("FontMatrix", matrix);
            }
            assert!(matches!(
                text::page(&document, 1, 128),
                Err(Failure::Malformed)
            ));
        }
    }

    #[test]
    fn partial_unicode_maps_use_known_font_encodings_or_refuse() {
        let mut document = lopdf::Document::new();
        let pages = document.new_object_id();
        let cmap = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            br"/CIDInit /ProcSet findresource begin
12 dict begin begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /Fixture def /CMapType 2 def
1 begincodespacerange <00> <FF> endcodespacerange
1 beginbfchar <41> <005A> endbfchar
endcmap CMapName currentdict /CMap defineresource pop end end"
                .to_vec(),
        ));
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
            "FirstChar" => 65, "LastChar" => 66, "Widths" => vec![600.into(), 600.into()],
            "ToUnicode" => cmap
        });
        let content = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            b"BT /F1 12 Tf 72 500 Td (AB) Tj ET".to_vec(),
        ));
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }
        });
        document.objects.insert(
            pages,
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
        );
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        document.trailer.set("Root", catalog);
        for (base, code, encoding, expected) in [
            ("Helvetica", "27", dictionary! {}, Some("\u{2019}")),
            ("Symbol", "42", dictionary! {}, Some("\u{0392}")),
            ("Fixture", "42", dictionary! {}, None),
            (
                "Helvetica",
                "27",
                dictionary! { "BaseEncoding" => "WinAnsiEncoding" },
                Some("'"),
            ),
            (
                "Helvetica",
                "27",
                dictionary! { "Differences" => vec![39.into(), "quotesingle".into()] },
                Some("'"),
            ),
            (
                "Helvetica",
                "27",
                dictionary! { "Differences" => vec![39.into(), ".notdef".into()] },
                None,
            ),
            (
                "Helvetica",
                "41",
                dictionary! { "Differences" => vec![65.into(), ".notdef".into()] },
                Some("Z"),
            ),
        ] {
            let selected = document.get_dictionary_mut(font).unwrap();
            selected.set("BaseFont", base);
            selected.set("Encoding", encoding);
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(format!("BT /F1 12 Tf 72 500 Td <{code}> Tj ET").into_bytes());
            let result = text::page(&document, 1, 128);
            match expected {
                Some(expected) => assert_eq!(result.unwrap(), expected),
                None => assert!(matches!(result, Err(Failure::Unsupported)), "{result:?}"),
            }
        }
        let program = document.add_object(lopdf::Stream::new(
            dictionary! {},
            b"/Encoding 256 array dup 39 /quotesingle put readonly def".to_vec(),
        ));
        for (descriptor, subtype, code, encoding, expected) in [
            (
                dictionary! { "Flags" => 32, "FontFile" => program },
                "Type1",
                "27",
                Some(dictionary! {}),
                Some("'"),
            ),
            (
                dictionary! { "Flags" => 32, "FontFile" => program },
                "Type1",
                "27",
                None,
                Some("'"),
            ),
            (
                dictionary! { "Flags" => 32, "FontFile" => program },
                "Type1",
                "27",
                Some(dictionary! { "Differences" => vec![39.into(), "quoteright".into()] }),
                Some("\u{2019}"),
            ),
            (
                dictionary! { "Flags" => 32 },
                "Type1",
                "27",
                Some(dictionary! {}),
                Some("\u{2019}"),
            ),
            (
                dictionary! { "Flags" => 4 },
                "Type1",
                "27",
                Some(dictionary! {}),
                None,
            ),
            (dictionary! { "Flags" => 4 }, "TrueType", "27", None, None),
            (
                dictionary! { "Flags" => 4 },
                "TrueType",
                "41",
                None,
                Some("Z"),
            ),
            (
                dictionary! { "Flags" => 4 },
                "TrueType",
                "27",
                Some(dictionary! { "BaseEncoding" => "WinAnsiEncoding" }),
                Some("'"),
            ),
        ] {
            let selected = document.get_dictionary_mut(font).unwrap();
            selected.set("BaseFont", "Fixture");
            selected.set("Subtype", subtype);
            selected.set("FontDescriptor", descriptor);
            selected.remove(b"Encoding");
            if let Some(encoding) = encoding {
                selected.set("Encoding", encoding);
            }
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(format!("BT /F1 12 Tf 72 500 Td <{code}> Tj ET").into_bytes());
            let result = text::page(&document, 1, 128);
            match expected {
                Some(expected) => assert_eq!(result.unwrap(), expected),
                None => assert!(matches!(result, Err(Failure::Unsupported)), "{result:?}"),
            }
        }
        document
            .get_dictionary_mut(font)
            .unwrap()
            .remove(b"FontDescriptor");
        document
            .get_dictionary_mut(font)
            .unwrap()
            .set("Subtype", "Type1");
        document
            .get_dictionary_mut(font)
            .unwrap()
            .remove(b"Encoding");
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"BT /F1 12 Tf 72 500 Td (AB) Tj ET".to_vec());
        for (base, expected) in [
            ("Helvetica", "ZB"),
            ("Symbol", "Z\u{0392}"),
            ("ZapfDingbats", "Z\u{2722}"),
        ] {
            document
                .get_dictionary_mut(font)
                .unwrap()
                .set("BaseFont", base);
            assert_eq!(text::page(&document, 1, 128).unwrap().trim(), expected);
        }
        document
            .get_dictionary_mut(font)
            .unwrap()
            .set("BaseFont", "Fixture");
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        assert_eq!(parse(&bytes), Err(Failure::Unsupported));
        OUTPUT.with(|output| *output.borrow_mut() = b"previous text".to_vec());
        INPUT.with(|input| *input.borrow_mut() = bytes);
        assert_eq!(extract(), Failure::Unsupported as i32);
        assert_eq!(output_len(), 0);
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"BT /F1 12 Tf 72 500 Td (A) Tj ET".to_vec());
        assert_eq!(text::page(&document, 1, 128).unwrap().trim(), "Z");
        let map = document
            .get_object_mut(cmap)
            .unwrap()
            .as_stream_mut()
            .unwrap();
        let complete = String::from_utf8(map.content.clone()).unwrap().replace(
            "1 beginbfchar <41> <005A>",
            "2 beginbfchar <41> <005A> <42> <0042>",
        );
        map.set_content(complete.into_bytes());
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"BT /F1 12 Tf 72 500 Td (AB) Tj ET".to_vec());
        assert_eq!(text::page(&document, 1, 128).unwrap().trim(), "ZB");
        for (encoding, code, expected) in [
            ("WinAnsiEncoding", "4181", Some("Z\u{2022}")),
            ("WinAnsiEncoding", "4101", None),
            ("MacRomanEncoding", "4101", None),
            ("MacExpertEncoding", "4100", None),
        ] {
            document
                .get_dictionary_mut(font)
                .unwrap()
                .set("Encoding", encoding);
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(format!("BT /F1 12 Tf 72 500 Td <{code}> Tj ET").into_bytes());
            match expected {
                Some(expected) => assert_eq!(text::page(&document, 1, 128).unwrap(), expected),
                None => {
                    assert!(matches!(
                        text::page(&document, 1, 128),
                        Err(Failure::Unsupported)
                    ));
                    let mut bytes = Vec::new();
                    document.save_to(&mut bytes).unwrap();
                    INPUT.with(|input| *input.borrow_mut() = bytes);
                    OUTPUT.with(|output| *output.borrow_mut() = b"previous text".to_vec());
                    assert_eq!(extract(), Failure::Unsupported as i32);
                    assert_eq!(output_len(), 0);
                }
            }
        }
        let map = document
            .get_object_mut(cmap)
            .unwrap()
            .as_stream_mut()
            .unwrap();
        let complete = String::from_utf8(map.content.clone())
            .unwrap()
            .replace("2 beginbfchar", "3 beginbfchar <01> <0043>");
        map.set_content(complete.into_bytes());
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"BT /F1 12 Tf 72 500 Td <4101> Tj ET".to_vec());
        assert_eq!(text::page(&document, 1, 128).unwrap(), "ZC");
    }

    #[test]
    fn simple_font_missing_width_comes_from_its_descriptor() {
        let mut document = lopdf::Document::new();
        let pages = document.new_object_id();
        let descriptor = document.add_object(dictionary! {
            "Type" => "FontDescriptor", "FontName" => "Fixture", "Flags" => 32,
            "FontBBox" => vec![0.into(), (-200).into(), 1000.into(), 1000.into()],
            "ItalicAngle" => 0, "Ascent" => 800, "Descent" => -200, "CapHeight" => 700,
            "StemV" => 80, "MissingWidth" => 600
        });
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Fixture",
            "Encoding" => "WinAnsiEncoding", "FontDescriptor" => descriptor,
            "FirstChar" => 65, "LastChar" => 65, "Widths" => vec![600.into()]
        });
        let content = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            b"BT /F1 12 Tf 72 500 Td (B) Tj 6 0 Td (C) Tj ET".to_vec(),
        ));
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }
        });
        document.objects.insert(
            pages,
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
        );
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        document.trailer.set("Root", catalog);
        for width in [lopdf::Object::Integer(600), lopdf::Object::Real(600.)] {
            document
                .get_dictionary_mut(descriptor)
                .unwrap()
                .set("MissingWidth", width);
            assert_eq!(text::page(&document, 1, 128).unwrap().trim(), "BC");
        }
        for width in [
            lopdf::Object::string_literal("600"),
            lopdf::Object::Real(f32::INFINITY),
        ] {
            document
                .get_dictionary_mut(descriptor)
                .unwrap()
                .set("MissingWidth", width);
            assert!(matches!(
                text::page(&document, 1, 128),
                Err(Failure::Malformed)
            ));
        }
        for invalid in [
            lopdf::Object::Integer(1),
            lopdf::Object::Reference((999, 0)),
        ] {
            document
                .get_dictionary_mut(font)
                .unwrap()
                .set("FontDescriptor", invalid);
            assert!(matches!(
                text::page(&document, 1, 128),
                Err(Failure::Malformed)
            ));
        }
        document
            .get_dictionary_mut(font)
            .unwrap()
            .remove(b"FontDescriptor");
        assert_eq!(text::page(&document, 1, 128).unwrap().trim(), "B C");
    }

    #[test]
    fn extended_graphics_state_fonts_preserve_selection_and_scope() {
        let mut document = lopdf::Document::new();
        let pages = document.new_object_id();
        let plain = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"
        });
        let mapped = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
            "Encoding" => dictionary! { "Type" => "Encoding", "BaseEncoding" => "WinAnsiEncoding",
                "Differences" => vec![65.into(), lopdf::Object::Name(b"Z".to_vec())] }
        });
        let state_type = document.add_object(lopdf::Object::Name(b"ExtGState".to_vec()));
        let state = document.add_object(dictionary! {
            "Type" => state_type, "Font" => vec![mapped.into(), 12.into()]
        });
        let content = document.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), Vec::new()));
        let form = document.add_object(lopdf::Stream::new(dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! { "ExtGState" => dictionary! {
                "GS" => dictionary! { "Font" => vec![plain.into(), 12.into()] }
            } }
        }, b"/GS gs BT 72 400 Td (A) Tj ET".to_vec()));
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "Plain" => plain, "Mapped" => mapped },
                "ColorSpace" => dictionary! { "CMYK" => "DeviceCMYK", "PatternAlias" => "Pattern",
                    "IndexedAlias" => vec!["Indexed".into(), "DeviceRGB".into(), 255.into(),
                        lopdf::Object::String(vec![0; 768], lopdf::StringFormat::Hexadecimal)] },
                "ExtGState" => dictionary! { "GS" => state },
                "XObject" => dictionary! { "Form" => form }
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
        for (space, components) in [("CalGray", 1), ("CalRGB", 3), ("Lab", 3)] {
            let parameters = dictionary! { "WhitePoint" => vec![1.into(), 1.into(), 1.into()] };
            let indirect = document.add_object(parameters.clone());
            let tint = document.add_object(dictionary! {
                "FunctionType" => 2, "Domain" => vec![0.into(), 1.into()], "N" => 1,
                "C0" => vec![lopdf::Object::Integer(0); components],
                "C1" => vec![lopdf::Object::Integer(1); components]
            });
            for parameters in [lopdf::Object::Dictionary(parameters), indirect.into()] {
                let calibrated = lopdf::Object::Array(vec![
                    lopdf::Object::Name(space.as_bytes().to_vec()),
                    parameters,
                ]);
                for selected in [
                    calibrated.clone(),
                    lopdf::Object::Array(vec![
                        "Separation".into(),
                        "Spot".into(),
                        calibrated,
                        tint.into(),
                    ]),
                ] {
                    document
                        .get_dictionary_mut(page)
                        .unwrap()
                        .get_mut(b"Resources")
                        .unwrap()
                        .as_dict_mut()
                        .unwrap()
                        .get_mut(b"ColorSpace")
                        .unwrap()
                        .as_dict_mut()
                        .unwrap()
                        .set("Calibrated", selected);
                    document
                        .get_object_mut(content)
                        .unwrap()
                        .as_stream_mut()
                        .unwrap()
                        .set_content(
                            b"/Calibrated cs /Calibrated CS /GS gs BT 72 500 Td (A) Tj ET".to_vec(),
                        );
                    assert_eq!(
                        text::page(&document, 1, 128).unwrap().trim(),
                        "Z",
                        "{space}"
                    );
                }
            }
        }
        for (stream, expected) in [
            (
                b"/CMYK cs 0 0 0 1 sc /CMYK CS 0 0 0 1 SC /GS gs BT 72 500 Td (A) Tj ET".as_slice(),
                vec!["Z"],
            ),
            (
                b"/IndexedAlias cs 0 sc /IndexedAlias CS 0 SC /GS gs BT 72 500 Td (A) Tj ET",
                vec!["Z"],
            ),
            (
                b"/PatternAlias cs /PatternAlias CS /GS gs BT 72 500 Td (A) Tj ET",
                vec!["Z"],
            ),
            (
                b"/GS gs BT 72 500 Td (A) Tj 20 0 Td (A) Tj ET".as_slice(),
                vec!["Z", "Z"],
            ),
            (
                b"BT /Plain 48 Tf ET /GS gs BT 72 500 Td (A) Tj 20 0 Td (A) Tj ET",
                vec!["Z", "Z"],
            ),
            (
                b"BT /Plain 12 Tf ET q /GS gs BT 72 500 Td (A) Tj ET Q BT 72 400 Td (A) Tj ET",
                vec!["Z", "A"],
            ),
            (b"/GS gs BT 72 500 Td (A) Tj ET /Form Do", vec!["Z", "A"]),
        ] {
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(stream.to_vec());
            assert_eq!(
                text::page(&document, 1, 128)
                    .unwrap()
                    .split_whitespace()
                    .collect::<Vec<_>>(),
                expected
            );
        }
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"/GS gs BT 72 500 Td (A) Tj ET".to_vec());
        for invalid in [
            lopdf::Object::Array(vec![mapped.into()]),
            lopdf::Object::Array(vec![mapped.into(), 12.into(), 1.into()]),
            lopdf::Object::Array(vec![lopdf::Object::Reference((999, 0)), 12.into()]),
            lopdf::Object::Array(vec![12.into(), 12.into()]),
            lopdf::Object::Array(vec![mapped.into(), lopdf::Object::Name(b"large".to_vec())]),
            lopdf::Object::Array(vec![mapped.into(), lopdf::Object::Real(f32::NAN)]),
        ] {
            document
                .get_object_mut(state)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Font", invalid);
            assert!(matches!(
                text::page(&document, 1, 128),
                Err(Failure::Malformed)
            ));
        }
    }

    #[test]
    fn actual_text_requires_supported_semantics_before_text_publication() {
        let mut document = lopdf::Document::new();
        let pages = document.new_object_id();
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"
        });
        let replacement = document.add_object(dictionary! {
            "ActualText" => lopdf::Object::string_literal("replacement")
        });
        let content = document.add_object(lopdf::Stream::new(lopdf::Dictionary::new(), Vec::new()));
        let form = document.add_object(lopdf::Stream::new(dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font },
                "Properties" => dictionary! { "Local" => replacement }
            }
        }, b"BT /F1 12 Tf /Span /Local BDC (glyph) Tj EMC ET".to_vec()));
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => dictionary! {
                "Font" => dictionary! { "F1" => font },
                "Properties" => dictionary! { "Replacement" => replacement, "Local" => dictionary! { "MCID" => 0 } },
                "XObject" => dictionary! { "Form" => form }
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
        for stream in [
            b"BT /F1 12 Tf /Span << /ActualText (replacement) >> BDC (glyph) Tj EMC ET".as_slice(),
            b"BT /F1 12 Tf /Artifact BMC /Span /Replacement BDC (glyph) Tj EMC EMC ET",
            b"/Form Do",
            b"BT /F1 12 Tf /ReversedChars BMC (desrever) Tj EMC ET",
            b"BT /F1 12 Tf /ReversedChars << /MCID 0 >> BDC (desrever) Tj EMC ET",
            b"BT /F1 12 Tf /ReversedChars /Local BDC (desrever) Tj EMC ET",
        ] {
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(stream.to_vec());
            assert!(matches!(
                text::page(&document, 1, 256),
                Err(Failure::Unsupported)
            ));
        }
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        INPUT.with(|input| *input.borrow_mut() = bytes);
        OUTPUT.with(|output| *output.borrow_mut() = b"previous text".to_vec());
        assert_eq!(extract(), Failure::Unsupported as i32);
        assert_eq!(output_len(), 0);
        for (stream, valid) in [
            (
                b"BT /F1 12 Tf /Span << /MCID 0 >> BDC (Visible) Tj EMC ET".as_slice(),
                true,
            ),
            (b"BT /F1 12 Tf /Span /Local BDC (Visible) Tj EMC ET", true),
            (b"BT /F1 12 Tf /Span BMC (Visible) Tj EMC ET", true),
            (b"BT /F1 12 Tf BMC (Visible) Tj EMC ET", false),
            (b"BT /F1 12 Tf 1 BMC (Visible) Tj EMC ET", false),
            (b"BT /F1 12 Tf /Span /Extra BMC (Visible) Tj EMC ET", false),
            (
                b"BT /F1 12 Tf /Span << /ActualText 12 >> BDC (Visible) Tj EMC ET",
                false,
            ),
            (
                b"BT /F1 12 Tf /Span /Missing BDC (Visible) Tj EMC ET",
                false,
            ),
        ] {
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(stream.to_vec());
            let result = text::page(&document, 1, 256);
            if valid {
                assert!(result.unwrap().contains("Visible"));
            } else {
                assert!(matches!(result, Err(Failure::Malformed)));
            }
        }
        for (target, stream) in [
            (
                form,
                b"BT /F1 12 Tf /ReversedChars BMC (desrever) Tj EMC ET".as_slice(),
            ),
            (content, b"/Form Do".as_slice()),
        ] {
            document
                .get_object_mut(target)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(stream.to_vec());
        }
        assert!(matches!(
            text::page(&document, 1, 256),
            Err(Failure::Unsupported)
        ));
    }

    #[test]
    fn structure_replacements_refuse_without_rejecting_ordinary_tags() {
        let mut document = lopdf::Document::new();
        let pages = document.new_object_id();
        let root = document.new_object_id();
        let ancestor = document.new_object_id();
        let element = document.new_object_id();
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"
        });
        let content = document.add_object(lopdf::Stream::new(
            dictionary! {},
            b"BT /F1 12 Tf 72 500 Td (Prefix) Tj /Span << /MCID 0 >> BDC (glyph) Tj EMC ET"
                .to_vec(),
        ));
        let page = document.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages, "Contents" => content, "StructParents" => 0,
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
        document.objects.insert(
            element,
            dictionary! {
                "Type" => "StructElem", "S" => "Span", "P" => ancestor, "Pg" => page, "K" => 0
            }
            .into(),
        );
        document.objects.insert(
            ancestor,
            dictionary! {
                "Type" => "StructElem", "S" => "P", "P" => root, "K" => vec![element.into()]
            }
            .into(),
        );
        let parent_tree = document.add_object(dictionary! {
            "Nums" => vec![0.into(), lopdf::Object::Array(vec![element.into()])]
        });
        document.objects.insert(
            root,
            dictionary! {
                "Type" => "StructTreeRoot", "K" => ancestor, "ParentTree" => parent_tree
            }
            .into(),
        );
        let catalog = document.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => pages, "StructTreeRoot" => root,
            "MarkInfo" => dictionary! { "Marked" => true }
        });
        document.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        let (wire, _) = parse(&bytes).unwrap();
        assert_eq!(&wire[12..], b"Prefixglyph");
        for target in [element, ancestor] {
            document
                .get_dictionary_mut(target)
                .unwrap()
                .set("ActualText", lopdf::Object::string_literal("replacement"));
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).unwrap();
            assert_eq!(parse(&bytes), Err(Failure::Unsupported));
            INPUT.with(|input| *input.borrow_mut() = bytes);
            OUTPUT.with(|output| *output.borrow_mut() = b"previous text".to_vec());
            assert_eq!(extract(), Failure::Unsupported as i32);
            assert_eq!(output_len(), 0);
            document
                .get_dictionary_mut(target)
                .unwrap()
                .remove(b"ActualText");
        }
        let form = document.add_object(lopdf::Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form", "StructParents" => 1,
                "BBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }
            },
            b"BT /F1 12 Tf /Span << /MCID 0 >> BDC (glyph) Tj EMC ET".to_vec(),
        ));
        document.get_dictionary_mut(page).unwrap().set(
            "Resources",
            dictionary! { "XObject" => dictionary! { "Form" => form } },
        );
        document
            .get_object_mut(content)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_content(b"/Form Do".to_vec());
        document.get_dictionary_mut(element).unwrap().set(
            "K",
            dictionary! { "Type" => "MCR", "Pg" => page, "Stm" => form, "MCID" => 0 },
        );
        document.get_dictionary_mut(parent_tree).unwrap().set(
            "Nums",
            vec![1.into(), lopdf::Object::Array(vec![element.into()])],
        );
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        assert_eq!(&parse(&bytes).unwrap().0[12..], b"glyph");
        document
            .get_dictionary_mut(element)
            .unwrap()
            .set("ActualText", lopdf::Object::string_literal("replacement"));
        for child in [
            lopdf::Object::Reference(element),
            document.get_dictionary(element).unwrap().clone().into(),
        ] {
            document
                .get_dictionary_mut(ancestor)
                .unwrap()
                .set("K", child);
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).unwrap();
            assert_eq!(parse(&bytes), Err(Failure::Unsupported));
            INPUT.with(|input| *input.borrow_mut() = bytes);
            OUTPUT.with(|output| *output.borrow_mut() = b"previous text".to_vec());
            assert_eq!(extract(), Failure::Unsupported as i32);
            assert_eq!(output_len(), 0);
        }
        for child in [
            lopdf::Object::Reference(root),
            lopdf::Object::Reference((999, 0)),
            lopdf::Object::string_literal("invalid child"),
        ] {
            document
                .get_dictionary_mut(ancestor)
                .unwrap()
                .set("K", child);
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).unwrap();
            assert_eq!(parse(&bytes), Err(Failure::Malformed));
            INPUT.with(|input| *input.borrow_mut() = bytes);
            OUTPUT.with(|output| *output.borrow_mut() = b"previous text".to_vec());
            assert_eq!(extract(), Failure::Malformed as i32);
            assert_eq!(output_len(), 0);
        }
    }

    #[test]
    fn quote_operators_match_explicit_text_state_operations() {
        let mut document = lopdf::Document::new();
        let pages = document.new_object_id();
        let font = document.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"
        });
        let explicit = b"BT /F1 12 Tf 20 TL 72 700 Td (First) Tj T* (Second) Tj 3 Tw 2 Tc T* (Third word) Tj (tail) Tj ET";
        let content = document.add_object(lopdf::Stream::new(
            lopdf::Dictionary::new(),
            explicit.to_vec(),
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
        let expected = text::page(&document, 1, 256).unwrap();
        document.get_object_mut(content).unwrap().as_stream_mut().unwrap().set_content(
            b"BT /F1 12 Tf 20 TL 72 700 Td (First) Tj (Second) ' 3 2 (Third word) \" (tail) Tj ET".to_vec()
        );
        assert_eq!(text::page(&document, 1, 256).unwrap(), expected);
        for (transform, scale, text, expected) in [
            ("", 100, "2 Tc (A) Tj (B) Tj", "AB"),
            ("0 1 -1 0 0 0 cm", 100, "2 Tc (A) Tj (B) Tj", "AB"),
            ("0 -1 1 0 0 0 cm", 100, "10 Tw (A ) Tj (B) Tj", "A B"),
            ("4 0 2 1 0 0 cm", 100, "2 Tc (A) Tj 16 0 Td (B) Tj", "A B"),
            ("", 0, "(A) Tj (B) Tj", "AB"),
            ("", 100, "/F1 0 Tf (A) Tj 16 0 Td (B) Tj", "AB"),
            ("", 100, "/F1 -12 Tf 2 Tc (A) Tj (B) Tj", "AB"),
            ("", 100, "2 Tc [(A) (B)] TJ", "AB"),
            ("", 100, "10 Tw (A ) Tj (B) Tj", "A B"),
            ("", 100, "-2 Tc (A) Tj (B) Tj", "AB"),
            ("", 25, "8 Tc (A) Tj (B) Tj", "AB"),
            ("4 0 0 1 0 0 cm", 100, "2 Tc (A) Tj (B) Tj", "AB"),
            ("", 100, "2 Tc (A) Tj 16 0 Td (B) Tj", "A B"),
            ("", 400, "(First) Tj (Second) Tj", "FirstSecond"),
            ("", 25, "(A) Tj 4 0 Td (B) Tj", "A B"),
            (
                "4 0 0 1 0 0 cm",
                100,
                "(First) Tj (Second) Tj",
                "FirstSecond",
            ),
            ("1 0 0 4 0 0 cm", 100, "(A) Tj 12 0 Td (B) Tj", "A B"),
        ] {
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(
                    format!("{transform} BT /F1 12 Tf {scale} Tz 72 500 Td {text} ET").into_bytes(),
                );
            assert_eq!(text::page(&document, 1, 256).unwrap().trim(), expected);
        }
        for malformed in [
            b"BT /F1 12 Tf 1 ' ET".as_slice(),
            b"BT /F1 12 Tf (text) 1 ' ET",
            b"BT /F1 12 Tf 3 (text) \" ET",
            b"BT /F1 12 Tf 3 2 1 \" ET",
        ] {
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(malformed.to_vec());
            assert!(matches!(
                text::page(&document, 1, 256),
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
        assert_eq!(expected, "First Second\nNext");
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
        for rotation in [90, 270] {
            document
                .get_object_mut(pages)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Rotate", rotation);
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(direct.to_vec());
            let text = text::page(&document, 1, 128).unwrap();
            assert_eq!(text, expected, "ordinary rotated text: rotation={rotation}");
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
        for matrix in ["0.6 0.8 -0.8 0.6", "4 0 2 1", "-1 0 0 1"] {
            document
                .get_object_mut(content)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_content(
                    format!("{matrix} 0 0 cm {}", std::str::from_utf8(direct).unwrap())
                        .into_bytes(),
                );
            assert_eq!(
                text::page(&document, 1, 128).unwrap(),
                expected,
                "matrix={matrix}"
            );
        }
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
