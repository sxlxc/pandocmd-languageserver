use anyhow::{Context, Result};
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
};
use lsp_types::request::{
    Completion, DocumentSymbolRequest, GotoDefinition, HoverRequest, References, Request as _,
};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionResponse,
    Diagnostic as LspDiagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents,
    HoverParams, InitializeParams, Location, MarkedString, NumberOrString, OneOf, Position,
    PublishDiagnosticsParams, Range, ReferenceParams, ServerCapabilities, SymbolKind,
    TextDocumentSyncCapability, TextDocumentSyncKind, Url,
};
use pandocmd_analysis::{Diagnostic, DocumentAnalysis, Severity, SymbolAtOffset, WorkspaceIndex};
use pandocmd_pandoc::PandocValidator;
use pandocmd_syntax::{LineIndex, PandocMarkdownParser, ParsedDocument, TextPosition, TextRange};
use std::collections::HashMap;
use tracing::{error, info};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let (connection, io_threads) = Connection::stdio();
    let capabilities = serde_json::to_value(server_capabilities())?;
    let initialization = connection.initialize(capabilities)?;
    let initialize_params: InitializeParams =
        serde_json::from_value(initialization).context("failed to decode initialize params")?;

    let mut server = Server::new(initialize_params)?;
    server.run(connection)?;
    io_threads.join()?;
    Ok(())
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),
        document_symbol_provider: Some(OneOf::Left(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec![
                "[".to_string(),
                "#".to_string(),
                "^".to_string(),
                "@".to_string(),
            ]),
            ..CompletionOptions::default()
        }),
        ..ServerCapabilities::default()
    }
}

struct Server {
    parser: PandocMarkdownParser,
    documents: HashMap<Url, OpenDocument>,
    workspace: WorkspaceIndex,
    pandoc: Option<PandocValidator>,
}

impl Server {
    fn new(params: InitializeParams) -> Result<Self> {
        let root = params.root_uri.and_then(|uri| uri.to_file_path().ok());
        let workspace = root
            .as_deref()
            .map(WorkspaceIndex::from_root)
            .unwrap_or_else(WorkspaceIndex::empty);
        let pandoc = PandocValidator::detect();

        if pandoc.is_some() {
            info!("pandoc executable detected");
        }

        Ok(Self {
            parser: PandocMarkdownParser::new()?,
            documents: HashMap::new(),
            workspace,
            pandoc,
        })
    }

    fn run(&mut self, connection: Connection) -> Result<()> {
        for message in &connection.receiver {
            match message {
                Message::Request(request) => {
                    if connection.handle_shutdown(&request)? {
                        return Ok(());
                    }
                    let response = self.handle_request(request);
                    connection.sender.send(Message::Response(response))?;
                }
                Message::Notification(notification) => {
                    if let Err(err) = self.handle_notification(notification, &connection) {
                        error!("{err:#}");
                    }
                }
                Message::Response(_) => {}
            }
        }

        Ok(())
    }

    fn handle_request(&self, request: Request) -> Response {
        let id = request.id.clone();
        let result = match request.method.as_str() {
            DocumentSymbolRequest::METHOD => self
                .document_symbols(request.params)
                .and_then(|result| Ok(serde_json::to_value(result)?)),
            GotoDefinition::METHOD => self
                .definition(request.params)
                .and_then(|result| Ok(serde_json::to_value(result)?)),
            References::METHOD => self
                .references(request.params)
                .and_then(|result| Ok(serde_json::to_value(result)?)),
            HoverRequest::METHOD => self
                .hover(request.params)
                .and_then(|result| Ok(serde_json::to_value(result)?)),
            Completion::METHOD => self
                .completion(request.params)
                .and_then(|result| Ok(serde_json::to_value(result)?)),
            _ => Ok(serde_json::Value::Null),
        };

        match result {
            Ok(value) => Response::new_ok(id, value),
            Err(err) => Response::new_err(
                id,
                lsp_server::ErrorCode::InternalError as i32,
                err.to_string(),
            ),
        }
    }

    fn handle_notification(
        &mut self,
        notification: Notification,
        connection: &Connection,
    ) -> Result<()> {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let params: DidOpenTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                self.open_document(params, connection)?;
            }
            DidChangeTextDocument::METHOD => {
                let params: DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                self.change_document(params, connection)?;
            }
            DidCloseTextDocument::METHOD => {
                let params: DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                self.close_document(params, connection)?;
            }
            _ => {}
        }

        Ok(())
    }

    fn open_document(
        &mut self,
        params: DidOpenTextDocumentParams,
        connection: &Connection,
    ) -> Result<()> {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let parsed = self.parser.parse(params.text_document.text)?;
        let analysis = DocumentAnalysis::analyze(&parsed, &self.workspace);
        let document = OpenDocument {
            version,
            parsed,
            analysis,
        };
        self.publish_diagnostics(&uri, &document, connection)?;
        self.documents.insert(uri, document);
        Ok(())
    }

    fn change_document(
        &mut self,
        params: DidChangeTextDocumentParams,
        connection: &Connection,
    ) -> Result<()> {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let pandoc = self.pandoc.clone();
        let workspace = self.workspace.clone();
        let Some(existing) = self.documents.get_mut(&uri) else {
            return Ok(());
        };

        let mut text = existing.parsed.text().to_string();
        for change in params.content_changes {
            apply_change(&mut text, change.range, &change.text);
        }

        let mut old_tree = existing.parsed.tree().clone();
        if text != existing.parsed.text() {
            let old_text = existing.parsed.text();
            let range = TextRange::new(0, old_text.len());
            let edit = pandocmd_syntax::input_edit_for_replacement(old_text, range, &text);
            old_tree.edit(&edit);
        }

        let parsed = self.parser.reparse(text, Some(&old_tree))?;
        let analysis = DocumentAnalysis::analyze(&parsed, &workspace);
        existing.version = version;
        existing.parsed = parsed;
        existing.analysis = analysis;
        let diagnostics = build_diagnostics(pandoc.as_ref(), existing);
        let version = existing.version;
        publish_diagnostics(connection, &uri, version, diagnostics)?;
        Ok(())
    }

    fn close_document(
        &mut self,
        params: DidCloseTextDocumentParams,
        connection: &Connection,
    ) -> Result<()> {
        self.documents.remove(&params.text_document.uri);
        let notification = Notification::new(
            "textDocument/publishDiagnostics".to_string(),
            PublishDiagnosticsParams {
                uri: params.text_document.uri,
                diagnostics: Vec::new(),
                version: None,
            },
        );
        connection
            .sender
            .send(Message::Notification(notification))?;
        Ok(())
    }

    fn publish_diagnostics(
        &self,
        uri: &Url,
        document: &OpenDocument,
        connection: &Connection,
    ) -> Result<()> {
        publish_diagnostics(
            connection,
            uri,
            document.version,
            build_diagnostics(self.pandoc.as_ref(), document),
        )
    }

    fn document_symbols(
        &self,
        params: serde_json::Value,
    ) -> Result<Option<DocumentSymbolResponse>> {
        #[allow(deprecated)]
        fn heading_symbol(
            document: &OpenDocument,
            heading: &pandocmd_analysis::Heading,
        ) -> DocumentSymbol {
            DocumentSymbol {
                name: heading.title.clone(),
                detail: Some(format!("#{}", heading.anchor)),
                kind: SymbolKind::STRING,
                tags: None,
                deprecated: None,
                range: to_lsp_range(
                    document.parsed.text(),
                    document.parsed.line_index(),
                    heading.range,
                ),
                selection_range: to_lsp_range(
                    document.parsed.text(),
                    document.parsed.line_index(),
                    heading.selection_range,
                ),
                children: None,
            }
        }

        let params: DocumentSymbolParams = serde_json::from_value(params)?;
        let Some(document) = self.documents.get(&params.text_document.uri) else {
            return Ok(None);
        };

        let symbols = document
            .analysis
            .headings
            .iter()
            .map(|heading| heading_symbol(document, heading))
            .collect();

        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    fn definition(&self, params: serde_json::Value) -> Result<Option<GotoDefinitionResponse>> {
        let params: GotoDefinitionParams = serde_json::from_value(params)?;
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let Some((document, symbol)) = self.symbol_at_lsp_position(&uri, position) else {
            return Ok(None);
        };

        let target = match symbol {
            SymbolAtOffset::Heading(heading) => Some(heading.selection_range),
            SymbolAtOffset::ReferenceDefinition(definition) => Some(definition.label_range),
            SymbolAtOffset::FootnoteDefinition(definition) => Some(definition.label_range),
            SymbolAtOffset::ReferenceLink(link) => document
                .analysis
                .reference_definition(&link.label)
                .map(|definition| definition.label_range),
            SymbolAtOffset::FootnoteReference(reference) => document
                .analysis
                .footnote_definition(&reference.label)
                .map(|definition| definition.label_range),
            SymbolAtOffset::HeadingLink(link) => document
                .analysis
                .heading_by_anchor(&link.anchor)
                .map(|heading| heading.selection_range),
            SymbolAtOffset::Citation(_) => None,
        };

        Ok(target.map(|range| {
            GotoDefinitionResponse::Scalar(Location::new(
                uri,
                to_lsp_range(document.parsed.text(), document.parsed.line_index(), range),
            ))
        }))
    }

    fn references(&self, params: serde_json::Value) -> Result<Option<Vec<Location>>> {
        let params: ReferenceParams = serde_json::from_value(params)?;
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some((document, symbol)) = self.symbol_at_lsp_position(&uri, position) else {
            return Ok(None);
        };

        let ranges = match symbol {
            SymbolAtOffset::ReferenceDefinition(definition) => document
                .analysis
                .reference_ranges_for_label(&definition.label),
            SymbolAtOffset::ReferenceLink(link) => {
                document.analysis.reference_ranges_for_label(&link.label)
            }
            SymbolAtOffset::FootnoteDefinition(definition) => document
                .analysis
                .footnote_ranges_for_label(&definition.label),
            SymbolAtOffset::FootnoteReference(reference) => document
                .analysis
                .footnote_ranges_for_label(&reference.label),
            SymbolAtOffset::Heading(heading) => document
                .analysis
                .heading_link_ranges_for_anchor(&heading.anchor),
            SymbolAtOffset::HeadingLink(link) => document
                .analysis
                .heading_link_ranges_for_anchor(&link.anchor),
            SymbolAtOffset::Citation(citation) => document
                .analysis
                .citations
                .iter()
                .filter(|other| other.key == citation.key)
                .map(|other| other.key_range)
                .collect(),
        };

        let locations = ranges
            .into_iter()
            .map(|range| {
                Location::new(
                    uri.clone(),
                    to_lsp_range(document.parsed.text(), document.parsed.line_index(), range),
                )
            })
            .collect();
        Ok(Some(locations))
    }

    fn hover(&self, params: serde_json::Value) -> Result<Option<Hover>> {
        let params: HoverParams = serde_json::from_value(params)?;
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let Some((document, symbol)) = self.symbol_at_lsp_position(&uri, position) else {
            return Ok(None);
        };

        let (contents, range) = match symbol {
            SymbolAtOffset::Heading(heading) => (
                format!("Heading anchor: `#{}`", heading.anchor),
                heading.selection_range,
            ),
            SymbolAtOffset::ReferenceDefinition(definition) => (
                format!("Reference target: `{}`", definition.target),
                definition.label_range,
            ),
            SymbolAtOffset::ReferenceLink(link) => {
                if let Some(definition) = document.analysis.reference_definition(&link.label) {
                    (
                        format!("Reference target: `{}`", definition.target),
                        link.label_range,
                    )
                } else {
                    (
                        format!("Unresolved reference `{}`", link.label),
                        link.label_range,
                    )
                }
            }
            SymbolAtOffset::FootnoteDefinition(definition) => (
                format!("Footnote definition `{}`", definition.label),
                definition.label_range,
            ),
            SymbolAtOffset::FootnoteReference(reference) => (
                format!("Footnote reference `{}`", reference.label),
                reference.label_range,
            ),
            SymbolAtOffset::HeadingLink(link) => (
                format!("Heading link `#{}`", link.anchor),
                link.anchor_range,
            ),
            SymbolAtOffset::Citation(citation) => {
                (format!("Citation `@{}`", citation.key), citation.key_range)
            }
        };

        Ok(Some(Hover {
            contents: HoverContents::Scalar(MarkedString::String(contents)),
            range: Some(to_lsp_range(
                document.parsed.text(),
                document.parsed.line_index(),
                range,
            )),
        }))
    }

    fn completion(&self, params: serde_json::Value) -> Result<Option<CompletionResponse>> {
        let params: lsp_types::CompletionParams = serde_json::from_value(params)?;
        let uri = params.text_document_position.text_document.uri;
        let Some(document) = self.documents.get(&uri) else {
            return Ok(None);
        };

        let mut items = Vec::new();
        for heading in &document.analysis.headings {
            items.push(CompletionItem {
                label: format!("#{}", heading.anchor),
                kind: Some(CompletionItemKind::REFERENCE),
                detail: Some(heading.title.clone()),
                insert_text: Some(format!("#{}", heading.anchor)),
                ..CompletionItem::default()
            });
        }
        for definition in &document.analysis.reference_definitions {
            items.push(CompletionItem {
                label: definition.label.clone(),
                kind: Some(CompletionItemKind::REFERENCE),
                detail: Some(definition.target.clone()),
                insert_text: Some(definition.label.clone()),
                ..CompletionItem::default()
            });
        }
        for definition in &document.analysis.footnote_definitions {
            items.push(CompletionItem {
                label: format!("^{}", definition.label),
                kind: Some(CompletionItemKind::REFERENCE),
                insert_text: Some(format!("^{}", definition.label)),
                ..CompletionItem::default()
            });
        }
        for key in self.workspace.citation_keys() {
            items.push(CompletionItem {
                label: format!("@{key}"),
                kind: Some(CompletionItemKind::REFERENCE),
                insert_text: Some(format!("@{key}")),
                ..CompletionItem::default()
            });
        }

        Ok(Some(CompletionResponse::Array(items)))
    }

    fn symbol_at_lsp_position(
        &self,
        uri: &Url,
        position: Position,
    ) -> Option<(&OpenDocument, SymbolAtOffset<'_>)> {
        let document = self.documents.get(uri)?;
        let offset = document.parsed.line_index().position_to_offset(
            document.parsed.text(),
            TextPosition::new(position.line, position.character),
        );
        let symbol = document.analysis.symbol_at(offset)?;
        Some((document, symbol))
    }
}

struct OpenDocument {
    version: i32,
    parsed: ParsedDocument,
    analysis: DocumentAnalysis,
}

fn apply_change(text: &mut String, range: Option<Range>, replacement: &str) {
    if let Some(range) = range {
        let line_index = LineIndex::new(text);
        let start = line_index.position_to_offset(
            text,
            TextPosition::new(range.start.line, range.start.character),
        );
        let end = line_index
            .position_to_offset(text, TextPosition::new(range.end.line, range.end.character));
        text.replace_range(start..end, replacement);
    } else {
        text.clear();
        text.push_str(replacement);
    }
}

fn build_diagnostics(
    pandoc: Option<&PandocValidator>,
    document: &OpenDocument,
) -> Vec<LspDiagnostic> {
    let mut diagnostics = document
        .analysis
        .diagnostics
        .iter()
        .map(|diagnostic| {
            to_lsp_diagnostic(
                document.parsed.text(),
                document.parsed.line_index(),
                diagnostic,
            )
        })
        .collect::<Vec<_>>();

    if let Some(pandoc) = pandoc {
        if let Ok(pandoc_diagnostics) = pandoc.validate_markdown(document.parsed.text()) {
            diagnostics.extend(
                pandoc_diagnostics
                    .into_iter()
                    .map(|diagnostic| LspDiagnostic {
                        range: Range::new(Position::new(0, 0), Position::new(0, 0)),
                        severity: Some(DiagnosticSeverity::WARNING),
                        code: Some(NumberOrString::String("pandoc".to_string())),
                        source: Some("pandoc".to_string()),
                        message: diagnostic.message,
                        ..LspDiagnostic::default()
                    }),
            );
        }
    }

    diagnostics
}

fn publish_diagnostics(
    connection: &Connection,
    uri: &Url,
    version: i32,
    diagnostics: Vec<LspDiagnostic>,
) -> Result<()> {
    let notification = Notification::new(
        "textDocument/publishDiagnostics".to_string(),
        PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics,
            version: Some(version),
        },
    );
    connection
        .sender
        .send(Message::Notification(notification))?;
    Ok(())
}

fn to_lsp_diagnostic(text: &str, line_index: &LineIndex, diagnostic: &Diagnostic) -> LspDiagnostic {
    LspDiagnostic {
        range: to_lsp_range(text, line_index, diagnostic.range),
        severity: Some(match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Information => DiagnosticSeverity::INFORMATION,
            Severity::Hint => DiagnosticSeverity::HINT,
        }),
        code: Some(NumberOrString::String(diagnostic.code.to_string())),
        source: Some("pandocmd".to_string()),
        message: diagnostic.message.clone(),
        ..LspDiagnostic::default()
    }
}

fn to_lsp_range(text: &str, line_index: &LineIndex, range: TextRange) -> Range {
    let (start, end) = line_index.range_to_positions(text, range);
    Range::new(
        Position::new(start.line, start.character),
        Position::new(end.line, end.character),
    )
}
