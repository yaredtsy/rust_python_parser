//! `pylspt-dev parse <dir> <file> [--pretty]` — the real entry point: resolves
//! the file through the ty project database, then scans it.
//!
//! See `experience/02-parse-and-ast/05-object-visitor/04-wiring-it-up.md`.

use pylspt::db::{open, open_project};
use pylspt::nodes::Node;
use pylspt::scan::{Ctx, scan_module};
use ruff_db::parsed::parsed_module;
use ruff_db::source::source_text;
use ruff_db::system::SystemPath;
use ruff_source_file::LineIndex;

#[derive(serde::Serialize)]
struct Output<'a> {
    nodes: &'a [Node],
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: pylspt-dev <dir> <file> [--pretty]"))?;
    let file_arg = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("usage: pylspt-dev <dir> <file> [--pretty]"))?;
    let pretty = args.any(|a| a == "--pretty");

    // 1. the database resolves the file and owns the parse. Never parse by hand
    //    here — salsa caches this, and object 1 explains why that matters.
    //    ⚠ OsSystem::new asserts the project root is ABSOLUTE (os.rs:41), so a
    //    relative argument must be canonicalised or it panics, not errors.
    let dir = std::fs::canonicalize(&dir)?;
    let dir = SystemPath::from_std_path(&dir)
        .ok_or_else(|| anyhow::anyhow!("project root is not valid UTF-8: {dir:?}"))?;
    let db = open_project(dir)?;
    // the file path must be absolute too, and for the same reason: a relative
    // one silently resolves against the system root and yields an empty file.
    let file_path = std::fs::canonicalize(&file_arg)?;
    let file_path = SystemPath::from_std_path(&file_path)
        .ok_or_else(|| anyhow::anyhow!("file path is not valid UTF-8: {file_path:?}"))?;
    let (file, program_file) = open(&db, file_path)?;
    let parsed = parsed_module(&db, program_file.python_file(&db)).load(&db);

    // 2. the source text must outlive the Ctx, so bind it before building one.
    let source = source_text(&db, file);
    let index = LineIndex::from_source_text(&source);
    let ctx = Ctx::new(&source, &index);

    // 3. scan, serialise. Same two calls as the standalone `scan` bin.
    let nodes = scan_module(&ctx, &parsed.syntax().body);
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
