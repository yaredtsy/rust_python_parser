use anyhow::{Context, Result, bail};
use ruff_db::files::{File, system_path_to_file};
use ruff_db::system::{OsSystem, SystemPath};
use ruff_source_file::LineIndex;

use ruff_text_size::{TextRange, TextSize};
use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_core::ProgramFile;
use ty_python_semantic::Db as _;

/// Your wire format's position: 1-based line, 0-based column.
#[derive(Debug, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

pub fn open_project(dir: &SystemPath) -> Result<ProjectDatabase> {
    let system = OsSystem::new(dir);

    let metadata = ProjectMetadata::discover(dir, &system)
        .with_context(|| format!("Failed to discover project metadata at '{dir}'"))?;

    Ok(ProjectDatabase::use_defaults(metadata, system))
}

pub fn open<'db>(
    db: &'db ProjectDatabase,
    file_arg: &SystemPath,
) -> Result<(File, ProgramFile<'db>)> {
    let file = system_path_to_file(db, file_arg)
        .with_context(|| format!("File not found in database: '{file_arg}'"))?;

    let program_file = db.program_file(file);

    Ok((file, program_file))
}

pub fn to_position(index: &LineIndex, source: &str, range: TextRange) -> Position {
    let start = index.line_column(range.start(), source);
    let end = index.line_column(range.end(), source);

    Position {
        line: start.line.get(),                 // 1-based
        column: start.column.to_zero_indexed(), // 0-based
        end_line: end.line.get(),
        end_column: end.column.to_zero_indexed(),
    }
}
