//! Open-document state held by the server.

use lsp_types::Url;
use pandocmd_analysis::{AnalyzeOptions, DocumentAnalysis, WorkspaceIndex};
use pandocmd_syntax::ParsedDocument;

/// A document opened by the client, with its analysis snapshot.
pub struct OpenDocument {
    pub uri: Url,
    pub version: i32,
    pub parsed: ParsedDocument,
    pub analysis: DocumentAnalysis,
    pub workspace: WorkspaceIndex,
    /// Diagnostics reported by external pandoc validation (if any).
    pub pandoc_diagnostics: Vec<lsp_types::Diagnostic>,
}

impl OpenDocument {
    /// Rebuild the workspace index and analysis for the stored text.
    pub fn reanalyze(&mut self, base_workspace: &WorkspaceIndex, options: &AnalyzeOptions) {
        let document_path = self.uri.to_file_path().ok();
        let workspace = base_workspace.for_document_with_extensions(
            document_path.as_deref(),
            self.parsed.text(),
            options.extensions,
        );
        self.analysis = DocumentAnalysis::analyze(&self.parsed, &workspace, options);
        self.workspace = workspace;
    }
}
