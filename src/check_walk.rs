//! Directory walk for path-mode workspace diagnosis.
//! Recurse `*.mod`, skip directories whose names start with `+`.

use std::io;
use std::path::{Path, PathBuf};

/// Absolute path for one file: join a relative path to the current directory.
/// Does not canonicalize.
pub fn absolute_path(path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

pub fn starts_with_plus(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('+'))
}

pub fn collect_mod_files(dir: &Path) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    if starts_with_plus(dir) {
        return Ok(out);
    }
    collect_into(dir, &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_into(dir: &Path, out: &mut Vec<String>) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if starts_with_plus(&path) {
                continue;
            }
            collect_into(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "mod") {
            out.push(path.to_string_lossy().into_owned());
        }
    }
    Ok(())
}
