//! Locating game files across the base game, update and DLC folders.
//!
//! A Wii U dump has `content/` folders (the DLC keeps its files one level
//! deeper, in `content/0010/`); a Switch dump has `romfs/`. Users may point at
//! either the dump folder or the content folder itself, so both are accepted. Later roots override earlier ones (base → update → DLC), matching
//! how the game layers its own files.

use std::path::{Path, PathBuf};

/// Ordered list of content roots. Lookups return the last root that has the file.
#[derive(Clone, Debug, Default)]
pub struct ContentRoots {
    roots: Vec<PathBuf>,
}

impl ContentRoots {
    /// Resolves each path to its content directory, skipping paths that do
    /// not look like game data. Returns the roots together with the paths that
    /// were rejected, so callers can report them.
    pub fn resolve<I, P>(paths: I) -> (Self, Vec<PathBuf>)
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut roots = Vec::new();
        let mut rejected = Vec::new();
        for path in paths {
            let path = path.as_ref();
            match resolve_content_dir(path) {
                Some(root) => roots.push(root),
                None => rejected.push(path.to_path_buf()),
            }
        }
        (Self { roots }, rejected)
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Finds a file by its path relative to the content root, e.g.
    /// `Terrain/A/MainField.tscb`.
    pub fn find(&self, relative: impl AsRef<Path>) -> Option<PathBuf> {
        let relative = relative.as_ref();
        self.roots
            .iter()
            .rev()
            .map(|root| root.join(relative))
            .find(|candidate| candidate.is_file())
    }

    /// Lists every file in `relative_dir` across all roots, later roots
    /// replacing earlier files with the same name.
    pub fn list_dir(&self, relative_dir: impl AsRef<Path>) -> Vec<(String, PathBuf)> {
        let mut files = std::collections::BTreeMap::new();
        for root in &self.roots {
            let Ok(entries) = std::fs::read_dir(root.join(relative_dir.as_ref())) else {
                continue;
            };
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str()
                    && entry.path().is_file()
                {
                    files.insert(name.to_owned(), entry.path());
                }
            }
        }
        files.into_iter().collect()
    }
}

/// Accepts a dump folder (`…/content`, `…/content/0010` for Wii U DLC,
/// `…/romfs` inside) or a content folder directly. Recognised by the presence
/// of the `Terrain`, `Model` or `Pack` folders every BotW content root has.
pub fn resolve_content_dir(path: &Path) -> Option<PathBuf> {
    let looks_like_content = |dir: &Path| {
        ["Terrain", "Model", "Pack"]
            .iter()
            .any(|marker| dir.join(marker).is_dir())
    };
    [
        path.to_path_buf(),
        path.join("content"),
        path.join("content/0010"),
        path.join("0010"),
        path.join("romfs"),
    ]
    .into_iter()
    .find(|dir| looks_like_content(dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("botw-content-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn later_roots_override_earlier_ones() {
        let base = temp_dir("base");
        let update = temp_dir("update");
        for (root, body) in [(&base, "base"), (&update, "update")] {
            let dir = root.join("content/Terrain/A");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("MainField.tscb"), body).unwrap();
        }
        std::fs::write(base.join("content/Terrain/A/OnlyInBase.txt"), "x").unwrap();

        // Wii U DLC keeps its content one folder deeper.
        let dlc = temp_dir("dlc");
        std::fs::create_dir_all(dlc.join("content/0010/Pack")).unwrap();

        let (roots, rejected) = ContentRoots::resolve([&base, &update, &temp_dir("missing"), &dlc]);
        assert_eq!(roots.roots().len(), 3);
        assert_eq!(roots.roots()[2], dlc.join("content/0010"));
        assert_eq!(rejected.len(), 1);

        let tscb = roots.find("Terrain/A/MainField.tscb").unwrap();
        assert_eq!(std::fs::read_to_string(tscb).unwrap(), "update");
        let listed = roots.list_dir("Terrain/A");
        assert_eq!(listed.len(), 2);
        assert!(
            listed
                .iter()
                .any(|(name, path)| name == "MainField.tscb" && path.starts_with(&update))
        );

        for dir in [&base, &update, &dlc] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
