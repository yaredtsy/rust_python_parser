//! `pylspt-dev parse <dir> <file> [--pretty]` — the real entry point: resolves
//! the file through the ty project database, then scans it.
//! `pylspt-dev calls <dir> <file> --entry <name> [--graph | --wire] [--merged] [--ids] [--pretty]` — the call
//! tree as text, or with `--graph` the context graph as JSON: the same functions
//! the server calls.
//!
//! See `experience/02-parse-and-ast/05-object-visitor/04-wiring-it-up.md`.

use pylspt::db::{open, open_project};
use pylspt::interp::{Mode, Options, call_graph, call_wire, call_wire_at, merge_for_output, to_text};
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

const USAGE: &str = "usage: pylspt-dev parse <dir> <file> [--pretty] [-o <path>]\n       pylspt-dev calls <dir> <file> --entry <name> [--nth <n>] [--graph | --wire] [--merged] [--ids] [--pretty] [-o <path>]";

fn main() -> anyhow::Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        anyhow::bail!(USAGE);
    }
    let command = args.remove(0);
    let (out, args) = take_out(args)?;
    match command.as_str() {
        "parse" => parse(args, out),
        "calls" => calls(args, out),
        _ => anyhow::bail!(USAGE),
    }
}

/// `-o <path>` / `--out <path>`: the flag and its value, taken out of `args`.
fn take_out(mut args: Vec<String>) -> anyhow::Result<(Option<String>, Vec<String>)> {
    let Some(i) = args.iter().position(|a| a == "-o" || a == "--out") else {
        return Ok((None, args));
    };
    anyhow::ensure!(i + 1 < args.len(), "-o/--out needs a path\n{USAGE}");
    let path = args.remove(i + 1);
    args.remove(i);
    Ok((Some(path), args))
}

/// To the file when `-o` was given, else to stdout.
fn emit(text: String, out: Option<String>) -> anyhow::Result<()> {
    match out {
        Some(path) => std::fs::write(path, text)?,
        None => println!("{}", text.trim_end_matches('\n')),
    }
    Ok(())
}

fn json<T: serde::Serialize>(out: &T, pretty: bool) -> anyhow::Result<String> {
    Ok(if pretty {
        serde_json::to_string_pretty(out)?
    } else {
        serde_json::to_string(out)?
    })
}

fn calls(args: Vec<String>, out: Option<String>) -> anyhow::Result<()> {
    let pretty = args.iter().any(|a| a == "--pretty");
    let graph = args.iter().any(|a| a == "--graph");
    let entry = args
        .iter()
        .position(|a| a == "--entry")
        .and_then(|i| args.get(i + 1))
        .ok_or_else(|| anyhow::anyhow!("--entry <name> is required\n{USAGE}"))?;
    // `--nth <n>`: `--entry` is then a `__qualname__` (`Class.method`, `outer.<locals>.inner`), and this is which of
    // the defs with that qualname in the file (1-based). Without it, `--entry` is a dotted name.
    let nth: Option<u32> = args
        .iter()
        .position(|a| a == "--nth")
        .map(|i| args.get(i + 1).and_then(|n| n.parse().ok()).ok_or_else(|| anyhow::anyhow!("--nth <n> takes a number\n{USAGE}")))
        .transpose()?;
    let positional: Vec<&String> = args
        .iter()
        .enumerate()
        .filter(|(i, a)| !a.starts_with("--") && (*i == 0 || (args[i - 1] != "--entry" && args[i - 1] != "--nth")))
        .map(|(_, a)| a)
        .collect();
    let [dir, file_arg] = positional[..] else {
        anyhow::bail!(USAGE);
    };

    let dir = std::fs::canonicalize(dir)?;
    let dir = SystemPath::from_std_path(&dir)
        .ok_or_else(|| anyhow::anyhow!("project root is not valid UTF-8: {dir:?}"))?;
    let db = open_project(dir)?;
    let file_path = std::fs::canonicalize(file_arg)?;
    let file_path = SystemPath::from_std_path(&file_path)
        .ok_or_else(|| anyhow::anyhow!("file path is not valid UTF-8: {file_path:?}"))?;
    let (file, _) = open(&db, file_path)?;

    let has = |flag: &str| args.iter().any(|a| a == flag);
    // `--ids`: the request path's filter (only functions with a docstring `ID:`)
    let opts = Options { mode: Mode::Summary, require_ids: has("--ids"), ..Options::default() };
    let no_entry = || anyhow::anyhow!("no function `{entry}` in {file_arg}");
    if graph {
        let g = call_graph(&db, file, entry, &opts).ok_or_else(no_entry)?;
        emit(json(&g, pretty)?, out)?;
    } else {
        let found = match nth {
            Some(n) => call_wire_at(&db, file, entry, n, &opts),
            None => call_wire(&db, file, entry, &opts),
        };
        let (mut wire, _) = found.ok_or_else(no_entry)?;
        if has("--merged") {
            wire = merge_for_output(&wire); // the spec of v-noc's merge: never on the response path
        }
        if has("--wire") {
            emit(json(&wire, pretty)?, out)?;
        } else {
            let module = file_path.file_stem().unwrap_or_default();
            emit(to_text(&wire.to_node(), module), out)?;
        }
    }
    Ok(())
}

fn parse(args: Vec<String>, out: Option<String>) -> anyhow::Result<()> {
    let mut args = args.into_iter();

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
    let output = Output { nodes: &nodes };
    emit(json(&output, pretty)?, out)
}
