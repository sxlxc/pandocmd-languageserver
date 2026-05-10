use std::io::Write;
use std::process::{Command, Stdio};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PandocError {
    #[error("pandoc executable was not found")]
    NotFound,
    #[error("failed to run pandoc: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PandocDiagnostic {
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct PandocValidator {
    executable: String,
}

impl PandocValidator {
    pub fn detect() -> Option<Self> {
        let executable = "pandoc".to_string();
        let output = Command::new(&executable)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .ok()?;

        output.success().then_some(Self { executable })
    }

    pub fn validate_markdown(&self, text: &str) -> Result<Vec<PandocDiagnostic>, PandocError> {
        let mut child = Command::new(&self.executable)
            .args(["--from", "markdown", "--to", "native"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(text.as_bytes())?;
        }

        let output = child.wait_with_output()?;
        if output.status.success() {
            return Ok(Vec::new());
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        let diagnostics = stderr
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| PandocDiagnostic {
                message: line.trim().to_string(),
            })
            .collect();

        Ok(diagnostics)
    }
}
