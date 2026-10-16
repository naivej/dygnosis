//! Saved model roots and include-file inventory; skip generated `+` directories.

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

pub fn is_model_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mod") || ext.eq_ignore_ascii_case("dyn"))
}

pub fn is_include_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("inc"))
}

#[derive(Default)]
pub struct WorkspaceFiles {
    pub models: Vec<String>,
    pub includes: Vec<String>,
}

pub fn collect_model_files(dir: &Path) -> io::Result<Vec<String>> {
    Ok(collect_workspace_files(dir)?.models)
}

pub fn collect_workspace_files(dir: &Path) -> io::Result<WorkspaceFiles> {
    let mut out = WorkspaceFiles::default();
    if starts_with_plus(dir) {
        return Ok(out);
    }
    collect_into(dir, &mut out)?;
    out.models.sort();
    out.includes.sort();
    Ok(out)
}

fn collect_into(dir: &Path, out: &mut WorkspaceFiles) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if starts_with_plus(&path) {
                continue;
            }
            collect_into(&path, out)?;
        } else if is_model_path(&path) {
            out.models.push(path.to_string_lossy().into_owned());
        } else if is_include_path(&path) {
            out.includes.push(path.to_string_lossy().into_owned());
        }
    }
    Ok(())
}
