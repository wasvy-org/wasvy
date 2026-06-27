use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::Result;
use serde::Deserialize;

use crate::{
    languages::{Metadata, Rust, cargo_metadata},
    named::Named,
    runtime::Runtime,
    source::Source,
};

/// Create a custom search query for Sources on the current filesystem
pub struct SearchBuilder<'a> {
    runtime: &'a Runtime,
    dir_path: Option<&'a Path>,
    native_path: Option<&'a Path>,
    wasm_path: Option<&'a Path>,
    ignore: Vec<PathBuf>,
}

impl<'a> SearchBuilder<'a> {
    pub fn new(runtime: &'a Runtime) -> Self {
        Self {
            runtime,
            dir_path: None,
            native_path: None,
            wasm_path: None,
            ignore: Vec::new(),
        }
    }

    /// Search for unbuilt mod directories in the location defined by the path.
    pub fn dir(mut self, path: &'a Path) -> Self {
        self.dir_path = Some(path);
        self
    }

    /// Search for native mods in the workspace location defined by the path.
    ///
    /// Note: the path can be in the workspace, it does not need to be the root.
    pub fn native(mut self, path: &'a Path) -> Self {
        self.native_path = Some(path);
        self
    }

    /// Search for pre-built wasm mods in the location defined by the path.
    pub fn wasm(mut self, path: &'a Path) -> Self {
        self.wasm_path = Some(path);
        self
    }

    /// Adds a file system path who's decendants will be ignored
    pub fn ignore(mut self, path: &'a Path) -> Self {
        if let Ok(path) = fs::canonicalize(path) {
            self.ignore.push(path);
        }
        self
    }

    /// Searches for compatible [Source]s (build files) for Mods
    ///
    /// This will locate, depending on what builder options were used:
    /// - Native mods in the host app workspace (Rust)
    /// - External mods located somewhere within the path (Rust, Python, Go, etc)
    /// - Pre-compiled binaries located somewhere within the path (Wasm)
    pub fn search(self) -> Result<Vec<Source>> {
        // Locate native mods
        let native: Vec<Source> = if let Some(path) = self.native_path {
            let Native {
                crate_names,
                workspace_root,
            } = find_native(path);

            crate_names
                .into_iter()
                .filter_map(|crate_name| {
                    Source::new_native(&workspace_root, crate_name, self.runtime).ok()
                })
                .collect()
        } else {
            Vec::new()
        };

        // Locate dir mods
        let mut mods: Vec<Source> = if let Some(path) = self.dir_path {
            let path = normalize_glob_root(path);
            search_glob(path.join("**/wit/*.wit"))
                .filter_map(|p| p.parent().and_then(Path::parent).map(Path::to_path_buf))
                .collect::<HashSet<_>>() // Dedupe
                .into_iter()
                .filter_map(|path| Source::new_dir(&path, self.runtime).ok())
                .collect()
        } else {
            Vec::new()
        };

        // Avoid duplicates with native sources
        if let Some(workspace_root) = native.first().map(|source| source.path()) {
            let rust = Rust::id();
            mods.retain(|source| {
                !(source.path().starts_with(workspace_root)
                    && source.is_language(&rust)
                    && native.iter().any(|native| native.name() == source.name()))
            });
        }

        // Locate wasm mods
        let mut wasm: Vec<Source> = if let Some(path) = self.dir_path {
            let path = normalize_glob_root(path);
            search_glob(path.join("**/*.wasm"))
                // Ignore wasm build artifacts located in source directories (such as dest directory for python)
                .filter(|path| !mods.iter().any(|source| path.starts_with(source.path())))
                // Ignore wasm files in rust build directories: **/target/wasm32-*/**/*.wasm
                .filter(|path| {
                    let mut parts = path.components();
                    while let Some(target) = parts.next() {
                        if target.as_os_str() == "target"
                            && let Some(next) = parts.next().map(|part| part.as_os_str())
                        {
                            if let Some(target) = next.to_str()
                                && target.starts_with("wasm32-")
                            {
                                return false;
                            }

                            // Ideally this wouldn't be here, but we also want to avoid match binaries created during tests
                            if next == "tests" {
                                return false;
                            }
                        }
                    }
                    true
                })
                .filter_map(|path| Source::new_wasm(path, None, self.runtime).ok())
                .collect()
        } else {
            Vec::new()
        };

        let mut sources = native;
        sources.append(&mut mods);
        sources.append(&mut wasm);

        sources.retain(|source| {
            let path = fs::canonicalize(source.path()).unwrap_or_default();
            !self.ignore.iter().any(|ignore| path.starts_with(ignore))
        });

        Ok(sources)
    }
}

fn search_glob(pattern: PathBuf) -> impl Iterator<Item = PathBuf> {
    let pattern = pattern.to_str().expect("unicode path");
    glob::glob(pattern)
        .expect("valid glob")
        .filter_map(Result::ok)
        .filter(|path| {
            // Omit hidden directories
            !path
                .components()
                .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
        })
}

fn normalize_glob_root(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[derive(Deserialize, Default)]
struct Native {
    crate_names: Vec<String>,
    workspace_root: PathBuf,
}

fn find_native(path: impl AsRef<Path>) -> Native {
    let Metadata {
        packages,
        workspace_root,
        ..
    } = cargo_metadata(path).unwrap_or_default();

    let crate_names = packages
        .into_iter()
        .filter(|package| {
            package.targets.iter().any(|target| {
                // All native mods have these two crate types.
                // TODO: there's probably more we can check here to avoid false positives
                target.crate_types.contains("rlib") && target.crate_types.contains("cdylib")
            })
        })
        .map(|package| package.name)
        .collect();

    Native {
        crate_names,
        workspace_root,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glob_root_resolves_relative_parent_segments() {
        let root = normalize_glob_root(Path::new("../../examples/mods"));
        let matches: Vec<_> = search_glob(root.join("**/wit/*.wit")).collect();

        assert!(
            matches
                .iter()
                .any(|path| path.ends_with("examples/mods/rust/basic/wit/guest.wit")),
            "relative search roots with parent segments should find example WIT files"
        );
    }
}
