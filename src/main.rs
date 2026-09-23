mod db;
use db::{open, open_project, to_position};
use ruff_python_ast::{Expr, ExprCall, ModModule, PythonVersion, Stmt};
use ruff_python_parser::{Mode, ParseOptions, Parsed, parse_unchecked};
use ruff_source_file::{LineIndex, SourceCode};
use ruff_text_size::{Ranged, TextRange, TextSize};
use ty_python_core::expression::Expression;

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

fn report(index: &LineIndex, code: &SourceCode<'_, '_>, stmt: &Stmt, depth: usize) {
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
                report(index, code, inner, depth + 1)
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
                report(index, code, inner, depth + 1);
            }
        }
        Stmt::Expr(expr_stmt) => {
            if let Expr::Call(call) = expr_stmt.value.as_ref() {
                report_chain(index, code, call);
            }
        }
        _ => {}
    }
}

/// Walk a call chain inside-out, mirroring parso's trailer order.
/// Returns calls with call_index 0,1,2… from innermost to outermost.
fn flatten_call_chain<'a>(outer: &'a ExprCall) -> Vec<&'a ExprCall> {
    let mut chain = Vec::new();
    let mut cur = Some(outer);
    while let Some(call) = cur {
        chain.push(call);
        cur = match call.func.as_ref() {
            Expr::Call(inner) => Some(inner),                   // f()()
            Expr::Attribute(attr) => attr.value.as_call_expr(), // f().g()
            Expr::Subscript(sub) => sub.value.as_call_expr(),   // f()[k].g()
            _ => None,
        };
    }
    chain.reverse(); // innermost first == call_index 0
    chain
}
fn report_chain(index: &LineIndex, code: &SourceCode<'_, '_>, outer: &ExprCall) {
    let chain = flatten_call_chain(outer);
    let chain_start = chain[0].range().start();

    for (call_index, call) in chain.iter().enumerate() {
        let name = code.slice(call.func.as_ref()).trim();
        let position = to_position(
            index,
            code.text(),
            TextRange::new(chain_start, call.range().end()),
        );
        let col = code.line_column(call.arguments.range.start());

        println!(
            "call_index={call_index}  name={name:?}  \
             pos={}:{}..{}:{}  call_col_pos={}  args={}",
            position.line,
            position.column,
            position.end_line,
            position.end_column,
            col.column.to_zero_indexed(),
            call.arguments.len(),
        );
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
        report(&index, &code, stmt, 0);
    }

    Ok(())
}
