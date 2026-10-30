//! Crate ide-workspace : arbre de fichiers, watcher, git, icônes, config (J2).

mod editorconfig;
mod fs_tree;

pub use editorconfig::{IndentStyle, indent_style};
pub use fs_tree::{DirEntry, create_dir, create_file, list_dir, rename, trash};
