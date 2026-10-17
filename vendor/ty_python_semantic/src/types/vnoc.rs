//! `pub` wrappers around otherwise-private `ty_python_semantic` accessors,
//! for `pylspt` (`crates/pylspt`) — the entire reason this crate is forked
//! rather than pulled from crates.io/git.
//!
//! Lives under `types` (not a sibling of it) because Rust privacy is
//! module-scoped: `crate::types::{ClassBase, ClassLiteral, ClassType}` are
//! only re-exported `pub(crate)` from `types.rs` (`types.rs:118`), so a
//! wrapper declared outside `types` could not even *name* them to call
//! `ClassLiteral::iter_mro`/`Type::member`, let alone call them.
//!
//! Rules for anything added here, because a fork without them is
//! unmaintainable:
//!
//! 1. One `pub fn` per accessor `pylspt` actually needs — add on demand, not
//!    speculatively.
//! 2. One internal call each. No logic, no reimplemented semantics — that is
//!    the entire point of forking instead of hand-rolling MRO/descriptors/
//!    `__getattr__` a second time, where a second copy would disagree with
//!    ty wherever ty is right.
//! 3. Only genuinely PUBLIC types in a wrapper's signature — `Type<'db>`,
//!    `&dyn Db`, `&ProgramEnvironment<'db>`, plain Rust types. Never
//!    `ClassLiteral`/`ClassBase`/`ClassType` themselves: `pylspt` could not
//!    write that type name even if the wrapper were `pub`, for the reason
//!    above.
//! 4. A rebase only ever conflicts on this file plus the one `pub mod vnoc;`
//!    line in `types.rs` — keep it that way.
//!
//! `pylspt` owns one test per wrapper (not here): the compiler catches a
//! signature change on rebase, only a test catches a *behaviour* change.

use crate::types::{ClassLiteral, Type};
use crate::{Db, ProgramEnvironment};

/// The real C3-linearised MRO, `object` last — `ClassLiteral::iter_mro`
/// (`class.rs:644`), `pub(crate)`. Not a class literal → empty, matching
/// `type_hierarchy_supertypes`'s own quirk-13 posture (never panic on input
/// that is not what you expected).
///
/// Supersedes `pylspt::mro::base_classes`'s hand-rolled C3 over the public,
/// one-level-only `type_hierarchy_supertypes` IDE API — that function
/// existed only because this one was unreachable without the fork.
pub fn class_mro<'db>(db: &'db dyn Db, ty: Type<'db>) -> Vec<Type<'db>> {
    let Type::ClassLiteral(literal) = ty else {
        return Vec::new();
    };
    literal.iter_mro(db).map(Type::from).collect()
}

/// Runtime attribute access on `ty` — `Type::member` (`types.rs:4929`),
/// private. Goes through the descriptor protocol, properties and
/// `__getattr__`/`__getattribute__`, unlike the public
/// `static_member`/`static_member_type_for_attribute` (`ide_support.rs`),
/// which is IDE-oriented (hover/completion) and does not fully simulate
/// what `getattr(<instance of ty>, name)` does at runtime. The interpreter
/// (`pylspt`'s call tree, stage 3) needs the runtime answer: given *this*
/// object's actual type on *this* path, what does `.save` resolve to.
///
/// `None` for "no such attribute" or "could not be determined" — callers
/// should treat both the same way object 5's quirk 13 treats a bad parse:
/// skip, do not panic.
pub fn member<'db>(db: &'db dyn Db, env: &ProgramEnvironment<'db>, ty: Type<'db>, name: &str) -> Option<Type<'db>> {
    ty.member(db, env, name).ignore_possibly_undefined()
}
