//! Preserve the public `dygnosis mcp` command with an MCP-only companion.

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

fn companion(current: &Path) -> io::Result<PathBuf> {
    let directory = current
        .parent()
        .ok_or_else(|| io::Error::other("The Dygnosis executable has no containing directory"))?;
    Ok(directory.join(if cfg!(windows) {
        "dygnosis-mcp.exe"
    } else {
        "dygnosis-mcp"
    }))
}

pub(crate) fn run() -> io::Result<ExitStatus> {
    let current = std::env::current_exe()?;
    run_companion(&current)
}

fn run_companion(current: &Path) -> io::Result<ExitStatus> {
    let selected = companion(current)?;
    let resolved = selected.canonicalize().map_err(|error| {
        io::Error::new(error.kind(), format!(
            "Cannot read MCP companion {}: {error}. No MCP server started. Install a complete Dygnosis distribution with both executables in the same directory.", selected.display()))
    })?;
    if resolved == current.canonicalize()? {
        return Err(io::Error::other(format!(
            "MCP companion {} resolves to the LSP executable. No MCP server started. Install a complete Dygnosis distribution.",
            selected.display()
        )));
    }
    validate_companion(&resolved)?;
    Command::new(&resolved)
        .arg("mcp")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "Cannot start MCP companion {}: {error}. No MCP server started.",
                    selected.display()
                ),
            )
        })
}

fn validate_companion(selected: &Path) -> io::Result<()> {
    let expected = format!("dygnosis-mcp {}", dygnosis::VERSION);
    let failure = |cause: String| {
        io::Error::other(format!(
            "MCP companion identity check failed: {cause}. Expected {expected} at {}. No MCP server started. Install matching executables from one Dygnosis distribution.",
            selected.display()
        ))
    };
    let mut child = Command::new(selected)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| failure(error.to_string()))?;
    let output = child.stdout.take().expect("piped version output");
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = output.take(512).read_to_end(&mut bytes).map(|_| bytes);
        let _ = sender.send(result);
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(failure(match result {
                    Err(error) => error.to_string(),
                    _ => "no version response within five seconds".into(),
                }));
            }
        }
    };
    let bytes = receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| failure("version output did not close within five seconds".into()))?
        .map_err(|error| failure(error.to_string()))?;
    let actual = String::from_utf8_lossy(&bytes);
    if !status.success() || actual.trim() != expected {
        return Err(failure(format!(
            "reported {:?} and {status}",
            actual.trim()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Directory(PathBuf);
    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "dygnosis launcher {} {}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(companion(&self.0.join("dygnosis")).unwrap());
            let _ = std::fs::remove_file(self.0.join("dygnosis"));
            std::fs::remove_dir(&self.0).unwrap();
        }
    }

    #[test]
    fn missing_companion_explains_that_no_server_started() {
        // Guards a missing package member becoming an unexplained startup failure.
        let directory = Directory::new();
        let current = directory.0.join("dygnosis");
        let error = run_companion(&current).unwrap_err().to_string();
        assert!(error.contains("MCP companion"));
        assert!(error.contains("No MCP server started"));
        assert!(error.contains("both executables"));
    }

    #[test]
    fn self_companion_is_rejected_instead_of_spawning_recursively() {
        // Guards an incorrect executable/symlink selection causing a process loop.
        let directory = Directory::new();
        let selected = companion(&directory.0.join("dygnosis")).unwrap();
        std::fs::copy(std::env::current_exe().unwrap(), &selected).unwrap();
        let error = run_companion(&selected).unwrap_err().to_string();
        assert!(error.contains("resolves to the LSP executable"));
        assert!(error.contains("No MCP server started"));
    }

    #[test]
    fn wrong_companion_identity_is_rejected_before_protocol_start() {
        // Guards accepting a different executable merely because its filename matches.
        let directory = Directory::new();
        let current = directory.0.join("dygnosis");
        std::fs::write(&current, b"launcher path").unwrap();
        std::fs::copy(
            std::env::current_exe().unwrap(),
            companion(&current).unwrap(),
        )
        .unwrap();
        let error = run_companion(&current).unwrap_err().to_string();
        assert!(error.contains("identity check failed"));
        assert!(error.contains("Expected dygnosis-mcp"));
        assert!(error.contains("No MCP server started"));
    }
}
