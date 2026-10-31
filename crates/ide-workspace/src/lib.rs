//! Crate ide-workspace : arbre de fichiers, watcher, git, icônes, config (J2).

mod config;
mod editorconfig;
mod fs_tree;

pub use config::{load_config, load_config_from};
pub use editorconfig::{IndentStyle, indent_style, indent_style_or};
pub use fs_tree::{DirEntry, create_dir, create_file, list_dir, rename, trash};
