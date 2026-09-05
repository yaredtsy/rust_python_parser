use pylspt::db::{open, open_project}; // your helpers from exercise 00
use ruff_db::source::source_text;
use ruff_db::system::SystemPath;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: prog <dir> <file>");
    let file_arg = std::env::args().nth(2).expect("usage: prog <dir> <file>");

    let db = open_project(SystemPath::new(&dir))?;
    let (file, _program_file) = open(&db, SystemPath::new(&file_arg))?;

    let text = source_text(&db, file);

    if let Some(err) = text.read_error() {
        eprintln!("could not read: {err}");
        return Ok(());
    }

    let s = text.as_str();
    println!("bytes      = {}", s.len());
    println!("chars      = {}", s.chars().count());
    println!("lines      = {}", s.lines().count());
    println!("first line = {:?}", s.lines().next());

    Ok(())
}
