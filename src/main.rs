mod db;
use db::{open, open_project};
use ruff_python_ast::{ModModule, PythonVersion, Stmt};
use ruff_python_parser::{Mode, ParseOptions, Parsed, parse_unchecked};
use ruff_source_file::{LineIndex, SourceCode};
use ruff_text_size::{Ranged, TextSize};

fn parse_path(path: &str) -> anyhow::Result<(String, Parsed<ModModule>)> {
    let source = std::fs::read_to_string(path)?;
    let options = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY313);
    let parsed = parse_unchecked(&source, options)
        .try_into_module()
        .expect("Mode::Module always produces a module");
    Ok((source, parsed))
}

/// line:col for a byte offset, 1-based line / 0-based column.
fn at(code: &SourceCode<'_, '_>, offset: TextSize) -> String {
    let lc = code.line_column(offset);
    format!("{}:{}", lc.line.get(), lc.column.to_zero_indexed())
}

fn report(code: &SourceCode<'_, '_>, stmt: &Stmt, depth: usize) {
    let pad = " ".repeat(depth);

    match stmt {
        Stmt::FunctionDef(def) => {
            println!(
                "{pad}fn {:<18} range={:>4}..{:<4} ({:6}) name@{:<7} async={:<5} decorators={}",
                def.name.as_str(),
                def.range.start().to_u32(),
                def.range.end().to_u32(),
                at(code, def.range.start()),
                at(code, def.name.range().start()),
                def.is_async,
                def.decorator_list.len(),
            );
            for inner in &def.body {
                report(code, inner, depth + 1)
            }
        }
        Stmt::ClassDef(def) => {
            let bases = def.arguments.as_ref().map_or(0, |a| a.args.len());
            println!(
                "{pad}cls {:<18} range={:>4}..{:<4} ({:>6})  bases={}",
                def.name.as_str(),
                def.range.start().to_u32(),
                def.range.end().to_u32(),
                at(code, def.range.start()),
                bases,
            );
            for inner in &def.body {
                report(code, inner, depth + 1);
            }
        }
        _ => {}
    }
}

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).expect("usage: defs <file>");
    let (source, parsed) = parse_path(&path)?;

    let index = LineIndex::from_source_text(&source);
    let code = SourceCode::new(&source, &index);

    println!(
        "{path}  —  {} bytes, {} syntax errors, {} unsupported",
        source.len(),
        parsed.errors().len(),
        parsed.unsupported_syntax_errors().len()
    );
    println!();

    for stmt in &parsed.syntax().body {
        report(&code, stmt, 0);
    }

    Ok(())
}
