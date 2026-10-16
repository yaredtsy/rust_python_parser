use ruff_source_file::{LineIndex, SourceCode};
use ruff_text_size::{TextRange, TextSize};

fn show(code: &SourceCode<'_, '_>, range: TextRange, label: &str) {
    let start = code.line_column(range.start());
    let end = code.line_column(range.end());
    let line_no = start.line;
    let col = start.column.to_zero_indexed();

    // Width in CHARACTERS on this line, not bytes — and clamp to the line
    // if the range spans several lines.
    let width = if end.line == start.line {
        end.column.to_zero_indexed().saturating_sub(col).max(1)
    } else {
        code.line_text(line_no)
            .trim_end()
            .chars()
            .count()
            .saturating_sub(col)
            .max(1)
    };

    println!("{label} at {}:{}", line_no.get(), col);
    println!("     |");
    println!(
        "{:>4} | {}",
        line_no.get(),
        code.line_text(line_no).trim_end()
    );
    println!("     | {}{}", " ".repeat(col), "^".repeat(width));
    if end.line != start.line {
        println!(
            "     | … continues to line {}, column {}",
            end.line.get(),
            end.column.to_zero_indexed()
        );
    }
}

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 {
        eprintln!("usage: {} <file> <start-byte> <end-byte>", args[0]);
        std::process::exit(2);
    }

    let source = std::fs::read_to_string(&args[1])?;
    let index = LineIndex::from_source_text(&source);
    let code = SourceCode::new(&source, &index);

    let range = TextRange::new(
        TextSize::new(args[2].parse()?),
        TextSize::new(args[3].parse()?),
    );

    // `slice` takes anything Ranged — including a bare TextRange. [verified]
    println!("slice: {:?}", code.slice(range));
    println!();
    show(&code, range, "range");

    Ok(())
}
