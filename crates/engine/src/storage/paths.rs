//! Filesystem work stays outside the database mutex.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::{Error, Result};

// Follow existing symlinks before processing parent components. Missing final components are
// allowed: model downloads and audio writers may register a path before creating the file.
fn resolve(path: &Path) -> Result<PathBuf> {
    let mut resolved = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => resolved.push(component),
            Component::CurDir => (),
            Component::ParentDir => {
                resolved.pop();
            }
            Component::Normal(name) => {
                resolved.push(name);
                match std::fs::canonicalize(&resolved) {
                    Ok(canonical) => resolved = canonical,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                    Err(error) => return Err(error.into()),
                }
            }
        }
    }
    Ok(resolved)
}

pub(super) fn relative(root: &Path, path: &Path) -> Result<String> {
    if !path.is_absolute() {
        return Err(Error::Other("stored file path must be absolute".into()));
    }
    let canonical_root = std::fs::canonicalize(root)?;
    let resolved = resolve(path)?;
    let relative = resolved
        .strip_prefix(canonical_root)
        .map_err(|_| Error::Other("stored file path must be inside data_dir".into()))?;
    if relative.as_os_str().is_empty() {
        return Err(Error::Other(
            "stored file path must name a file inside data_dir".into(),
        ));
    }
    let components = relative
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .ok_or_else(|| Error::Other("stored file path is not valid UTF-8".into()))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(components.join("/"))
}

pub(super) fn orphans(root: &Path, live: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    let live: HashSet<PathBuf> = live.into_iter().collect();
    let audio = root.join("audio");
    // Do not descend through directory symlinks: audio storage must not sweep other trees.
    match std::fs::symlink_metadata(&audio) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => (),
        Ok(_) => return Ok(Vec::new()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    }
    let mut pending = vec![audio];
    let mut orphans = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() && !live.contains(&path) {
                orphans.push(path);
            }
        }
    }
    orphans.sort();
    Ok(orphans)
}
