//! Crate ide-workspace : arbre de fichiers, watcher, git, icônes, config (J2).

mod editorconfig;
mod fs_tree;
mod watcher;

pub use editorconfig::{IndentStyle, indent_style};
pub use fs_tree::{DirEntry, create_dir, create_file, list_dir, rename, trash};
pub use watcher::{Watcher, watch};
