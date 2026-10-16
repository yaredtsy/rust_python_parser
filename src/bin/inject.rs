//! `inject <dir> <file> [--write]` — ID injection (object 5's CLI, scoped to
//! one file). Dry-run by default; `--write` is the only way to touch disk.
//!
//! See `experience/10-lossless-edits/README.md`.

use pylspt::db::{open, open_project};
use pylspt::inject::inject_ids;
use ruff_db::system::SystemPath;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let dir = args.next().ok_or_else(|| anyhow::anyhow!("usage: inject <dir> <file> [--write]"))?;
    let file_arg =
        args.next().ok_or_else(|| anyhow::anyhow!("usage: inject <dir> <file> [--write]"))?;
    let write = args.any(|a| a == "--write"); // dry-run is the default (object 4's guard)

    let dir = std::fs::canonicalize(&dir)?;
    let dir = SystemPath::from_std_path(&dir)
        .ok_or_else(|| anyhow::anyhow!("project root is not valid UTF-8: {dir:?}"))?;
    let mut db = open_project(dir)?;

    let file_path = std::fs::canonicalize(&file_arg)?;
    let file_path = SystemPath::from_std_path(&file_path)
        .ok_or_else(|| anyhow::anyhow!("file path is not valid UTF-8: {file_path:?}"))?;
    let (file, _program_file) = open(&db, file_path)?;

    let report = inject_ids(&mut db, file, file_path, write)?;

    println!("modified: {}{}", report.modified, if write { "" } else { " (dry-run)" });
    for def in &report.defs {
        print_def(def, 0);
    }
    Ok(())
}

fn print_def(def: &pylspt::defs::DefNode, depth: usize) {
    println!(
        "{}{:?} {} — {}",
        "  ".repeat(depth),
        def.kind,
        def.name,
        def.id.as_deref().unwrap_or("<no id>"),
    );
    for child in &def.children {
        print_def(child, depth + 1);
    }
}
