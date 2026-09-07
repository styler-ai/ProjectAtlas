//! Admission limits shared by the native document owner and its fixed PDF guest.

/// Maximum source bytes admitted to a document parser.
pub const INPUT_LIMIT: usize = 8 * 1024 * 1024;
/// Maximum expanded document bytes.
pub const EXPANDED_LIMIT: usize = 32 * 1024 * 1024;
/// Maximum retained UTF-8 text bytes.
pub const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
/// Maximum retained facts or PDF pages.
pub const FACT_LIMIT: usize = 4096;

/// Closed negative status codes returned by the fixed guest ABI.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum Failure {
    /// Document structure or content cannot be read completely.
    Malformed = -1,
    /// Credentials would be required to read the PDF.
    Encrypted = -2,
    /// The page count exceeds the fact ceiling.
    Pages = -3,
    /// The retained text exceeds its byte ceiling.
    Output = -4,
    /// Decoded streams exceed their expansion ceiling.
    Expanded = -5,
}

impl TryFrom<i32> for Failure {
    type Error = i32;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            -1 => Ok(Self::Malformed),
            -2 => Ok(Self::Encrypted),
            -3 => Ok(Self::Pages),
            -4 => Ok(Self::Output),
            -5 => Ok(Self::Expanded),
            other => Err(other),
        }
    }
}
