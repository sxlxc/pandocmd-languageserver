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
    /// 1-based (line, column) inside the validated input, when recoverable.
    pub line_column: Option<(u32, u32)>,
}

impl PandocDiagnostic {
    fn parse(stderr_line: &str) -> PandocDiagnostic {
        // Pandoc reader errors commonly look like:
        //   [error] Error parsing YAML metadata at (line 2, column 8): ...
        // or reference "<input>" line 3: ...
        let line_column = extract_line_column(stderr_line);
        PandocDiagnostic {
            message: stderr_line.trim().to_string(),
            line_column,
        }
    }
}

fn extract_line_column(message: &str) -> Option<(u32, u32)> {
    let start = message.find("(line ")?;
    let rest = &message[start + "(line ".len()..];
    let end = rest.find(')')?;
    let body = &rest[..end];
    let mut parts = body.split(',');
    let line = parts.next()?.trim().strip_prefix("line ")?.parse().ok()?;
    let column = match parts.next()?.trim().strip_prefix("column ") {
        Some(column) => column.parse().ok()?,
        None => 1,
    };
    Some((line, column))
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

    /// Validate with plain `markdown` as the reader format.
    pub fn validate_markdown(&self, text: &str) -> Result<Vec<PandocDiagnostic>, PandocError> {
        self.validate_markdown_with_format(text, &pandocmd_extensions::ExtensionConfig::default())
    }

    /// Validate using a Pandoc format spec derived from the extension
    /// configuration, so diagnostics reflect the enabled extensions.
    pub fn validate_markdown_with_format(
        &self,
        text: &str,
        config: &pandocmd_extensions::ExtensionConfig,
    ) -> Result<Vec<PandocDiagnostic>, PandocError> {
        let mut format = config
            .flavor
            .clone()
            .unwrap_or_else(|| "markdown".to_string());
        let (extensions, _) = config.resolve();
        let defaults = pandocmd_extensions::ExtensionSet::flavor_defaults(
            config
                .flavor
                .as_deref()
                .and_then(pandocmd_extensions::Flavor::from_name)
                .unwrap_or_default(),
        );
        let diff = extensions.diff_from(defaults);
        if !diff.is_empty() {
            format.push('+');
            format.push_str(&diff);
        }

        let mut child = Command::new(&self.executable)
            .args(["--from", &format, "--to", "native"])
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
            .map(PandocDiagnostic::parse)
            .collect();

        Ok(diagnostics)
    }
}
