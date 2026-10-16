//! `scan <file> [--pretty]` — parse one file and print the node tree as JSON.
//!
//! See `experience/02-parse-and-ast/05-object-visitor/04-wiring-it-up.md`.

use pylspt::nodes::Node;
use pylspt::scan::{Ctx, scan_module};
use ruff_python_ast::PythonVersion;
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
use ruff_source_file::LineIndex;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or_else(|| anyhow::anyhow!("usage: scan <file> [--pretty]"))?;
    let pretty = args.any(|a| a == "--pretty");

    // 1. source → AST. One parse, kept alive for the whole scan.
    let source = std::fs::read_to_string(&path)?;
    let options = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY313);
    let parsed = parse_unchecked(&source, options)
        .try_into_module()
        .expect("Mode::Module always produces a module");

    // 2. the two borrows every helper needs, built once.
    let index = LineIndex::from_source_text(&source);
    let ctx = Ctx::new(&source, &index);

    // 3. scan the module body — not a node, so scan_module, not scan_scope.
    let nodes = scan_module(&ctx, &parsed.syntax().body);

    // 4. serialise. The JSON is the contract — including KEY ORDER, so this
    //    goes through a struct, never through serde_json::json!/Value, whose
    //    Map is a BTreeMap and would sort the keys alphabetically.
    #[derive(serde::Serialize)]
    struct Output<'a> {
        nodes: &'a [Node],
    }
    let out = Output { nodes: &nodes };
    println!(
        "{}",
        if pretty {
            serde_json::to_string_pretty(&out)?
        } else {
            serde_json::to_string(&out)?
        }
    );
    Ok(())
}
