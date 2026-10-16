use pylspt::db::{open, open_project};
use ruff_db::parsed::parsed_module;
use ruff_db::system::SystemPath;
use ty_python_semantic::Db as _;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args().nth(1).expect("usage: prog <dir> <file>");
    let file_arg = std::env::args().nth(2).expect("usage: prog <dir> <file>");

    println!("hello");
    let db = open_project(SystemPath::new(&dir))?;
    let (_file, program_file) = open(&db, SystemPath::new(&file_arg))?;

    let parsed = parsed_module(&db, program_file.python_file(&db)).load(&db);
    let ast = parsed.syntax();

    println!("top-level statements: {}", ast.body.len());
    println!("range:                {:?}", ast.range);
    println!("tokens:               {}", parsed.tokens().len());
    println!("syntax errors:        {}", parsed.errors().len());
    println!(
        "unsupported syntax:   {}",
        parsed.unsupported_syntax_errors().len()
    );

    for err in parsed.unsupported_syntax_errors() {
        println!("  ⚠ {err:?}");
    }

    Ok(())
}
