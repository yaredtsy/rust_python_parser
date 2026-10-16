use anyhow::Ok;
use ruff_source_file::LineIndex;
use ruff_text_size::{TextRange, TextSize};

/// Your wire format's position: 1-based line, 0-based column.
#[derive(Debug, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

pub fn to_position(index: &LineIndex, source: &str, range: TextRange) -> Position {
    let start = index.line_column(range.start(), source);
    let end = index.line_column(range.end(), source);

    Position {
        line: start.line.get(),                 // 1-based
        column: start.column.to_zero_indexed(), // 0-based
        end_line: end.line.get(),
        end_column: end.column.to_zero_indexed(),
    }
}

fn check(source: &str, start: u32, end: u32, expected: (usize, usize, usize, usize)) {
    let index = LineIndex::from_source_text(source);
    let range = TextRange::new(TextSize::new(start), TextSize::new(end));

    let p = to_position(&index, source, range);
    let got = (p.line, p.column, p.end_line, p.end_column);

    let mark = if got == expected { "ok " } else { "FAIL" };
    println!(
        "{mark} {start:>3}..{end:<3} {:?}  got {:?}  expected {:?}",
        &source[range.to_std_range()],
        got,
        expected,
    );
}

fn main() -> anyhow::Result<()> {
    let ascii = std::fs::read_to_string("experience/01-source-and-positions/python/ascii.py")?;

    println!("---- ascii.py (100 bytes, 100 chard)---");

    // the last statement, `        greet("world")`
    check(&ascii, 77, 99, (7, 0, 7, 22));
    // one byte further: offset 100 is AFTER the final newline
    check(&ascii, 77, 100, (7, 0, 8, 0));

    let unicode = std::fs::read_to_string("experience/01-source-and-positions/python/unicode.py")?;

    println!("\n--- unicode.py (88 bytes, 79 chars) ---");
    // the `(` on line 1: byte 9, character 8
    check(&unicode, 9, 9, (1, 8, 1, 8));
    // all of line 2: starts at byte 20, 37 bytes, 32 characters
    check(&unicode, 20, 57, (2, 0, 2, 32));

    Ok(())
}
