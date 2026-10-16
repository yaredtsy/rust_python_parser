use ruff_db::files::{File, system_path_to_file};

use ruff_db::system::{OsSystem, SystemPath, SystemPathBuf};

use ty_project::{ProjectDatabase, ProjectMetadata};
use ty_python_core::ProgramFile;
use ty_python_semantic::Db as _;

pub fn open_project(dir: &SystemPath) -> anyhow::Result<ProjectDatabase> {
    let system = OsSystem::new(dir);
    let metadata = match ProjectMetadata::discover(dir, &system) {
        Ok(m) => m,
        Err(err) => {
            // Broken config must never take the analyser down (quirk 13).
            // tracing::warn!("project discovery failed: {err}; using defaults");
            ProjectMetadata::new("adhoc", SystemPathBuf::from(dir.as_str()))
        }
    };
    Ok(ProjectDatabase::use_defaults(metadata, system))
}

/// Open `path`, returning both handles.
pub fn open<'db>(
    db: &'db ProjectDatabase,
    path: &SystemPath,
) -> anyhow::Result<(File, ProgramFile<'db>)> {
    let file = system_path_to_file(db, path)?;
    Ok((file, db.program_file(file)))
}
