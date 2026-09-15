use anyhow::{Context, Result, bail};
use ruff_db::files::{File, system_path_to_file};
use ruff_db::system::{OsSystem, SystemPath};
use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_core::ProgramFile;
use ty_python_semantic::Db as _;

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
