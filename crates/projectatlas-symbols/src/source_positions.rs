//! Bounded conversion of parser byte boundaries to original scalar coordinates.

/// Map requested byte boundaries in one source traversal, with cooperative checks.
///
/// # Errors
///
/// Returns the caller's work-control failure before returning partial coordinates.
pub(crate) fn source_positions<E>(
    content: &str,
    mut offsets: Vec<usize>,
    check: &mut impl FnMut() -> Result<(), E>,
) -> Result<Vec<(usize, usize, usize)>, E> {
    check()?;
    offsets.retain(|offset| content.is_char_boundary(*offset));
    offsets.sort_unstable();
    offsets.dedup();
    if offsets.is_empty() {
        return Ok(Vec::new());
    }
    let mut positions = Vec::with_capacity(offsets.len());
    let mut next_offset = 0;
    let mut byte = 0;
    let mut line = 1;
    let mut column = 0;
    for (iteration, character) in content.chars().enumerate() {
        crate::check_parser_iteration(iteration, check)?;
        while offsets
            .get(next_offset)
            .is_some_and(|offset| *offset == byte)
        {
            positions.push((byte, line, column));
            next_offset += 1;
        }
        if next_offset == offsets.len() {
            return Ok(positions);
        }
        byte += character.len_utf8();
        if character == '\n' {
            line += 1;
            column = 0;
        } else {
            column += 1;
        }
    }
    while offsets
        .get(next_offset)
        .is_some_and(|offset| *offset == byte)
    {
        positions.push((byte, line, column));
        next_offset += 1;
    }
    check()?;
    Ok(positions)
}

#[cfg(test)]
mod tests {
    use super::source_positions;

    #[test]
    fn scalar_positions_preserve_crlf_and_ignore_invalid_utf8_boundaries() {
        let result = source_positions(
            "α💡\r\nβx",
            vec![11, 2, 6, 8, 10, 7, 0, 3, 2],
            &mut || Ok::<(), ()>(()),
        );
        assert_eq!(
            result,
            Ok(vec![
                (0, 1, 0),
                (2, 1, 1),
                (6, 1, 2),
                (7, 1, 3),
                (8, 2, 0),
                (10, 2, 1),
                (11, 2, 2),
            ])
        );
    }

    #[test]
    fn coordinate_mapping_observes_cancellation_during_source_traversal() {
        let source = "α".repeat(5_000);
        let mut checkpoints = 0;
        let result = source_positions(&source, vec![source.len()], &mut || {
            checkpoints += 1;
            if checkpoints > 2 {
                Err("cancelled")
            } else {
                Ok(())
            }
        });
        assert_eq!(result, Err("cancelled"));
    }
}
