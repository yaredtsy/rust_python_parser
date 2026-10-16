use ruff_source_file::{LineIndex, OneIndexed, SourceCode};

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("usage: lines <file>");
    let source = std::fs::read_to_string(&path)?;

    let index = LineIndex::from_source_text(&source);
    let code = SourceCode::new(&source, &index);
    println!(
        "{}  —  {} bytes, {} chars, {} lines",
        path,
        source.len(),
        source.chars().count(),
        code.line_count()
    );
    println!();
    println!(
        "{:>4}  {:>6} {:>6} {:>6}   {}",
        "line", "start", "end", "end_ex", "text"
    );
    for n in 1..=code.line_count() {
        let line = OneIndexed::new(n).expect("line numbers start at 1");
        println!(
            "{:>4}  {:>6} {:>6} {:>6}   {:?}",
            n,
            code.line_start(line).to_u32(),
            code.line_end(line).to_u32(),
            code.line_end_exclusive(line).to_u32(),
            code.line_text(line),
        );
    }
    Ok(())
}
