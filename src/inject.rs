//! Stage 2's impure half: the four-step order, and the guards. Nothing here
//! decides which defs exist or where their slot is — `crate::defs` and
//! `crate::edit` already did. See
//! `experience/10-lossless-edits/04-object-purity.md` and
//! `05-putting-it-together.md`.
//!
//! Not a salsa query: it writes files and mints random UUIDs, either of
//! which breaks query purity on its own.

use ruff_db::files::File;
use ruff_db::parsed::parsed_module;
use ruff_db::source::source_text;
use ruff_db::system::SystemPath;
use ruff_python_ast::AnyNodeRef;
use ruff_source_file::find_newline;
use ty_project::ProjectDatabase;
use ty_project::glob::IncludeResult;
use ty_project::Db as _;
use ty_python_semantic::Db as _;
use uuid::Uuid;

use crate::defs::{DefNode, definitions};
use crate::edit::{apply, plan_ids};

#[derive(Debug)]
pub struct InjectReport {
    /// Whether the file was (or, with `write: false`, would be) changed.
    pub modified: bool,
    /// Always the outline computed from the text that is now on disk — the
    /// pre-write outline is never handed back (object 4, step 4).
    pub defs: Vec<DefNode>,
}

#[derive(Debug)]
pub enum InjectError {
    /// Quirk 2's boundary: never write third-party or out-of-project files.
    OutsideProject,
    /// A syntax error earlier in the file can still yield plausible slots
    /// later; the correct amount of rewriting for a file that does not parse
    /// is none.
    DoesNotParse,
    Io(std::io::Error),
}

impl std::fmt::Display for InjectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutsideProject => write!(f, "refusing to write outside the project"),
            Self::DoesNotParse => write!(f, "refusing to write a file with syntax errors"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for InjectError {}

impl From<std::io::Error> for InjectError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Inject an `ID:` into every def in `file` that does not have one yet.
///
/// `write` gates the actual write — dry-run is the only safe default (a
/// parity run over a corpus must not silently rewrite the corpus, goldens
/// included).
///
/// 1. parse (cached) → 2. `plan_ids` (pure, given a real UUID generator) →
/// 3. if there are edits: `apply` → `fs::write` → `File::sync_path` →
/// 4. `definitions(db, file)`, which re-walks the text now on disk.
///
/// Step 4 is the point: the outline this returns is always computed from the
/// *post-write* text, and that is a compile-time property
/// (`definitions` borrows `db` immutably; `File::sync_path` needs `&mut`), not
/// a convention one more careful call site could get away with skipping.
pub fn inject_ids(
    db: &mut ProjectDatabase,
    file: File,
    path: &SystemPath,
    write: bool,
) -> Result<InjectReport, InjectError> {
    // Guard: only first-party project files. Same boundary exercise 02's
    // quirk 2 uses for descent, now gating the write too.
    let project = db.project();
    if !matches!(project.is_file_included(db, path), IncludeResult::Included { .. }) {
        return Err(InjectError::OutsideProject);
    }

    // Guard: write only `.py`. `source_text` on a notebook is the JOINED
    // CODE CELLS, not the file's bytes (`ruff_db::source`), so writing it
    // back would overwrite the notebook's JSON with bare Python source.
    if path.extension() != Some("py") {
        return Ok(InjectReport { modified: false, defs: definitions(db, file).clone() });
    }

    // 1. parse — cached, and the only read of the pre-write text.
    let python_file = db.program_file(file).python_file(db);
    let parsed = parsed_module(db, python_file);

    // 2. plan. This borrow of `db` (via `parsed`/`defs`) ends with the block,
    // before the write in step 3 needs `&mut`.
    let edits = {
        let module = parsed.load(db);

        // Guard: refuse to rewrite a file that does not parse. A slot further
        // down can still look plausible even when an earlier statement is
        // broken, and the user is very possibly mid-keystroke.
        if !module.errors().is_empty() {
            return Err(InjectError::DoesNotParse);
        }

        let source = source_text(db, file);
        let nl = find_newline(&source).map_or("\n", |(_, ending)| ending.as_str());
        let root = AnyNodeRef::from(module.syntax());
        let defs = definitions(db, file);

        plan_ids(defs, root, &source, nl, &mut || Uuid::new_v4().to_string())
    };

    if edits.is_empty() {
        return Ok(InjectReport { modified: false, defs: definitions(db, file).clone() });
    }
    if !write {
        return Ok(InjectReport { modified: true, defs: definitions(db, file).clone() });
    }

    // 3. write, then tell the db.
    let source = source_text(db, file);
    let new_source = apply(&source, edits);
    drop(source); // the borrow must be dead before `&mut db` below
    std::fs::write(path.as_std_path(), &new_source)?;
    File::sync_path(db, path);

    // 4. collect from the NEW text. `definitions` is salsa-cached per file,
    // so this re-walks exactly the one file that changed.
    Ok(InjectReport { modified: true, defs: definitions(db, file).clone() })
}
