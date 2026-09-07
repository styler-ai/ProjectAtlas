//! Bound the canonical text formatter and reject undecodable character callbacks.

use crate::limits::Failure;
use pdf_extract::{MediaBox, OutputDev, OutputError, PlainTextOutput, Transform};

struct Text {
    value: String,
    maximum: usize,
    exceeded: bool,
}

impl std::fmt::Write for Text {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        if self.value.len().saturating_add(value.len()) > self.maximum {
            self.exceeded = true;
            return Err(std::fmt::Error);
        }
        self.value.push_str(value);
        Ok(())
    }
}

impl<'a> pdf_extract::ConvertToFmt for &'a mut Text {
    type Writer = &'a mut Text;

    fn convert(self) -> Self::Writer {
        self
    }
}

struct CheckedText<'a>(PlainTextOutput<&'a mut Text>);

impl OutputDev for CheckedText<'_> {
    fn begin_page(
        &mut self,
        page: u32,
        media: &MediaBox,
        art: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.0.begin_page(page, media, art)
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        self.0.end_page()
    }

    fn output_character(
        &mut self,
        transform: &Transform,
        width: f64,
        spacing: f64,
        size: f64,
        character: &str,
    ) -> Result<(), OutputError> {
        if character.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "PDF character has no Unicode mapping",
            )
            .into());
        }
        self.0
            .output_character(transform, width, spacing, size, character)
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        self.0.begin_word()
    }
    fn end_word(&mut self) -> Result<(), OutputError> {
        self.0.end_word()
    }
    fn end_line(&mut self) -> Result<(), OutputError> {
        self.0.end_line()
    }
}

pub(crate) fn page(
    document: &lopdf::Document,
    page: u32,
    maximum: usize,
) -> Result<String, Failure> {
    let mut text = Text {
        value: String::new(),
        maximum,
        exceeded: false,
    };
    let result = pdf_extract::output_doc_page(
        document,
        &mut CheckedText(PlainTextOutput::new(&mut text)),
        page,
    );
    if text.exceeded {
        return Err(Failure::Output);
    }
    result.map_err(|_| Failure::Malformed)?;
    Ok(text.value)
}
