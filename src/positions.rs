use ruff_source_file::{LineIndex, SourceCode};
use ruff_text_size::TextRange;

/// Your wire format's position. 1-based line, 0-based column — parso's
/// convention, which v-noc depends on (plan/00-orientation/01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

/// Convert a byte range to a wire-format position.
///
/// Columns are CHARACTER counts, matching parso: `LineIndex::line_column`
/// uses `PositionEncoding::Utf32` internally [verified, line_index.rs:117].
pub fn to_position(index: &LineIndex, source: &str, range: TextRange) -> Position {
    let start = index.line_column(range.start(), source);
    let end = index.line_column(range.end(), source);

    Position {
        line: start.line.get(),
        column: start.column.to_zero_indexed(),
        end_line: end.line.get(),
        end_column: end.column.to_zero_indexed(),
    }
}

/// The same thing, when you already have a `SourceCode`.
pub fn to_position_with(code: &SourceCode<'_, '_>, range: TextRange) -> Position {
    let start = code.line_column(range.start());
    let end = code.line_column(range.end());

    Position {
        line: start.line.get(),
        column: start.column.to_zero_indexed(),
        end_line: end.line.get(),
        end_column: end.column.to_zero_indexed(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruff_source_file::LineIndex;
    use ruff_text_size::TextSize;

    fn pos(source: &str, start: u32, end: u32) -> Position {
        let index = LineIndex::from_source_text(source);
        to_position(
            &index,
            source,
            TextRange::new(TextSize::new(start), TextSize::new(end)),
        )
    }

    #[test]
    fn non_zero_column() {
        // `greet` in `def greet(name):` — bytes 4..9, line 1
        let p = pos("def greet(name):\n    return 1\n", 4, 9);
        assert_eq!(
            p,
            Position {
                line: 1,
                column: 4,
                end_line: 1,
                end_column: 9
            }
        );
    }

    #[test]
    fn characters_not_bytes() {
        // `(` in `def café(x):` is byte 9, character 8
        let src = "def café(x):\n";
        let p = pos(src, 9, 9);
        assert_eq!(p.column, 8, "columns must count characters, not bytes");
    }

    #[test]
    fn end_before_newline() {
        let src = "a = 1\nb = 2\n";
        // 0..5 is `a = 1`, not including the newline at byte 5
        let p = pos(src, 0, 5);
        assert_eq!((p.end_line, p.end_column), (1, 5));
        // 0..6 INCLUDES the newline, so the end lands on line 2
        let p = pos(src, 0, 6);
        assert_eq!((p.end_line, p.end_column), (2, 0));
    }
}
