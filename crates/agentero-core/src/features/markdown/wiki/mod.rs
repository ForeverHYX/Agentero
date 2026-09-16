mod cache;
pub mod doctor;
pub mod embed;
pub mod extract;
pub mod frontmatter;
pub mod index;
pub mod models;
mod notes;
pub mod rename;
pub mod resolve;

pub use index::WikiIndexState;
pub use notes::{append_title_alias_best_effort, sync_notes_title_and_alias};
