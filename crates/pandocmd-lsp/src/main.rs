use anyhow::{Context, Result};
use lsp_server::{Connection, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, Notification as _,
};
use lsp_types::request::{
    Completion, DocumentHighlightRequest, DocumentSymbolRequest, FoldingRangeRequest,
    GotoDefinition, HoverRequest, References, Request as _,
};
use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionOptions, CompletionResponse, CompletionTextEdit,
    Diagnostic as LspDiagnostic, DiagnosticSeverity, DidChangeTextDocumentParams,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DocumentHighlight,
    DocumentHighlightKind, DocumentHighlightParams, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, FoldingRange, FoldingRangeParams, FoldingRangeProviderCapability,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverContents, HoverParams,
    InitializeParams, Location, MarkupContent, MarkupKind, NumberOrString, OneOf, Position,
    PublishDiagnosticsParams, Range, ReferenceParams, ServerCapabilities, SymbolKind,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextEdit, Url,
};
use pandocmd_analysis::{
    BibliographyEntry, Citation, Diagnostic, DocumentAnalysis, FencedDiv, HeadingLink, Severity,
    SymbolAtOffset, WorkspaceIndex,
};
use pandocmd_pandoc::PandocValidator;
use pandocmd_syntax::{LineIndex, PandocMarkdownParser, ParsedDocument, TextPosition, TextRange};
use std::collections::{BTreeSet, HashMap};
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
        document_highlight_provider: Some(OneOf::Left(true)),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(vec!["@".to_string()]),
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
        let root = params
            .root_uri
            .and_then(|uri| uri.to_file_path().ok())
            .or_else(|| {
                params.workspace_folders.and_then(|folders| {
                    folders
                        .into_iter()
                        .find_map(|folder| folder.uri.to_file_path().ok())
                })
            });
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
            DocumentHighlightRequest::METHOD => self
                .document_highlight(request.params)
                .and_then(|result| Ok(serde_json::to_value(result)?)),
            FoldingRangeRequest::METHOD => self
                .folding_range(request.params)
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
        let workspace = self.document_workspace(&uri, parsed.text());
        let analysis = DocumentAnalysis::analyze(&parsed, &workspace);
        let document = OpenDocument {
            version,
            parsed,
            analysis,
            workspace,
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
        let base_workspace = self.workspace.clone();
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
        let document_path = uri.to_file_path().ok();
        let workspace = base_workspace.for_document(document_path.as_deref(), parsed.text());
        let analysis = DocumentAnalysis::analyze(&parsed, &workspace);
        existing.version = version;
        existing.parsed = parsed;
        existing.analysis = analysis;
        existing.workspace = workspace;
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

    fn document_workspace(&self, uri: &Url, text: &str) -> WorkspaceIndex {
        let document_path = uri.to_file_path().ok();
        self.workspace.for_document(document_path.as_deref(), text)
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

        #[allow(deprecated)]
        fn div_symbol(
            document: &OpenDocument,
            div: &pandocmd_analysis::FencedDiv,
        ) -> DocumentSymbol {
            DocumentSymbol {
                name: div.label(),
                detail: Some(div.detail()),
                kind: SymbolKind::OBJECT,
                tags: None,
                deprecated: None,
                range: to_lsp_range(
                    document.parsed.text(),
                    document.parsed.line_index(),
                    div.range,
                ),
                selection_range: to_lsp_range(
                    document.parsed.text(),
                    document.parsed.line_index(),
                    div.selection_range,
                ),
                children: None,
            }
        }

        let params: DocumentSymbolParams = serde_json::from_value(params)?;
        let Some(document) = self.documents.get(&params.text_document.uri) else {
            return Ok(None);
        };

        let mut symbols = document
            .analysis
            .headings
            .iter()
            .map(|heading| (heading.range.start, heading_symbol(document, heading)))
            .chain(
                document
                    .analysis
                    .fenced_divs
                    .iter()
                    .map(|div| (div.range.start, div_symbol(document, div))),
            )
            .collect::<Vec<_>>();
        symbols.sort_by_key(|(start, _)| *start);
        let symbols = symbols.into_iter().map(|(_, symbol)| symbol).collect();

        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    fn definition(&self, params: serde_json::Value) -> Result<Option<GotoDefinitionResponse>> {
        let params: GotoDefinitionParams = serde_json::from_value(params)?;
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let Some((document, symbol)) = self.symbol_at_lsp_position(&uri, position) else {
            return Ok(None);
        };

        let location = match symbol {
            SymbolAtOffset::Heading(heading) => Some(document_location(
                &uri,
                document,
                heading.id_range.unwrap_or(heading.selection_range),
            )),
            SymbolAtOffset::FencedDiv(div) => Some(document_location(
                &uri,
                document,
                div.id_range.unwrap_or(div.selection_range),
            )),
            SymbolAtOffset::LocalReference(reference) => {
                Some(document_location(&uri, document, reference.id_range))
            }
            SymbolAtOffset::ReferenceDefinition(definition) => {
                Some(document_location(&uri, document, definition.label_range))
            }
            SymbolAtOffset::FootnoteDefinition(definition) => {
                Some(document_location(&uri, document, definition.label_range))
            }
            SymbolAtOffset::ReferenceLink(link) => document
                .analysis
                .reference_definition(&link.label)
                .map(|definition| document_location(&uri, document, definition.label_range)),
            SymbolAtOffset::FootnoteReference(reference) => document
                .analysis
                .footnote_definition(&reference.label)
                .map(|definition| document_location(&uri, document, definition.label_range)),
            SymbolAtOffset::HeadingLink(link) => document
                .analysis
                .anchor_target_range(&link.anchor)
                .map(|range| document_location(&uri, document, range)),
            SymbolAtOffset::Citation(citation) => {
                bibliography_definition_location(&document.workspace, citation).or_else(|| {
                    document
                        .analysis
                        .anchor_target_range(&citation.key)
                        .map(|range| document_location(&uri, document, range))
                })
            }
        };

        Ok(location.map(GotoDefinitionResponse::Scalar))
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
                .local_reference_ranges_for_id(&heading.anchor),
            SymbolAtOffset::FencedDiv(div) => div
                .id
                .as_deref()
                .map(|id| document.analysis.local_reference_ranges_for_id(id))
                .unwrap_or_else(|| vec![div.selection_range]),
            SymbolAtOffset::LocalReference(reference) => document
                .analysis
                .local_reference_ranges_for_id(&reference.id),
            SymbolAtOffset::HeadingLink(link) => document
                .analysis
                .local_reference_ranges_for_id(&link.anchor),
            SymbolAtOffset::Citation(citation) => {
                if document.analysis.local_reference(&citation.key).is_some() {
                    document
                        .analysis
                        .local_reference_ranges_for_id(&citation.key)
                } else {
                    document
                        .analysis
                        .citations
                        .iter()
                        .filter(|other| other.key == citation.key)
                        .map(|other| other.key_range)
                        .collect()
                }
            }
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

    fn document_highlight(
        &self,
        params: serde_json::Value,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let params: DocumentHighlightParams = serde_json::from_value(params)?;
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let Some((document, SymbolAtOffset::FencedDiv(div))) =
            self.symbol_at_lsp_position(&uri, position)
        else {
            return Ok(None);
        };

        let highlights = fenced_div_highlights(document, div);
        if highlights.is_empty() {
            Ok(None)
        } else {
            Ok(Some(highlights))
        }
    }

    fn folding_range(&self, params: serde_json::Value) -> Result<Option<Vec<FoldingRange>>> {
        let params: FoldingRangeParams = serde_json::from_value(params)?;
        let Some(document) = self.documents.get(&params.text_document.uri) else {
            return Ok(None);
        };

        let ranges = folding_ranges(document);
        if ranges.is_empty() {
            Ok(None)
        } else {
            Ok(Some(ranges))
        }
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
                format!(
                    "Heading: `{}`\n\nAnchor: `#{}`",
                    heading.title, heading.anchor
                ),
                heading.selection_range,
            ),
            SymbolAtOffset::FencedDiv(div) => (fenced_div_hover_text(div), div.selection_range),
            SymbolAtOffset::LocalReference(reference) => {
                (local_reference_hover_text(reference), reference.id_range)
            }
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
            SymbolAtOffset::HeadingLink(link) => {
                (heading_link_hover_text(document, link), link.anchor_range)
            }
            SymbolAtOffset::Citation(citation) => (
                citation_hover_text(document, &document.workspace, citation),
                citation.key_range,
            ),
        };

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: contents,
            }),
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
        let position = params.text_document_position.position;
        let Some(document) = self.documents.get(&uri) else {
            return Ok(None);
        };

        if let Some(context) = citation_completion_context(document, position) {
            return Ok(Some(CompletionResponse::Array(citation_completion_items(
                document,
                &document.workspace,
                Some(context.edit_range),
                Some(context.prefix.as_str()),
            ))));
        }

        Ok(None)
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

fn document_location(uri: &Url, document: &OpenDocument, range: TextRange) -> Location {
    Location::new(
        uri.clone(),
        to_lsp_range(document.parsed.text(), document.parsed.line_index(), range),
    )
}

fn bibliography_definition_location(
    workspace: &WorkspaceIndex,
    citation: &Citation,
) -> Option<Location> {
    let entry = workspace.citation_entry(&citation.key)?;
    let source = entry.source.as_ref()?;
    let text = std::fs::read_to_string(&source.path).ok()?;
    let line_index = LineIndex::new(&text);
    let uri = Url::from_file_path(&source.path).ok()?;

    Some(Location::new(
        uri,
        to_lsp_range(&text, &line_index, source.key_range),
    ))
}

fn folding_ranges(document: &OpenDocument) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();

    push_heading_folding_ranges(document, &mut ranges);
    for div in &document.analysis.fenced_divs {
        push_folding_range(
            &mut ranges,
            document,
            div.opening_range.start,
            div.range.end,
            None,
        );
    }
    push_metadata_folding_ranges(document, &mut ranges);
    push_code_fence_folding_ranges(document, &mut ranges);

    ranges.sort_by_key(|range| (range.start_line, range.end_line));
    ranges.dedup_by_key(|range| (range.start_line, range.end_line));
    ranges
}

fn push_heading_folding_ranges(document: &OpenDocument, ranges: &mut Vec<FoldingRange>) {
    let text = document.parsed.text();
    let line_index = document.parsed.line_index();
    let last_line = last_content_line(line_index, text);

    for (index, heading) in document.analysis.headings.iter().enumerate() {
        let start_line = line_index
            .offset_to_position(text, heading.range.start)
            .line;
        let end_line = document
            .analysis
            .headings
            .iter()
            .skip(index + 1)
            .find(|next| next.level <= heading.level)
            .map(|next| {
                line_index
                    .offset_to_position(text, next.range.start)
                    .line
                    .saturating_sub(1)
            })
            .unwrap_or(last_line);

        push_line_folding_range(ranges, start_line, end_line, None);
    }
}

fn push_metadata_folding_ranges(document: &OpenDocument, ranges: &mut Vec<FoldingRange>) {
    let mut lines = document.parsed.text().lines().enumerate();
    let Some((start_line, first_line)) = lines.next() else {
        return;
    };
    let delimiter = match first_line.trim_end_matches('\r') {
        "---" => "---",
        "+++" => "+++",
        _ => return,
    };

    for (line_number, line) in lines {
        if line.trim_end_matches('\r').trim() == delimiter {
            push_line_folding_range(ranges, start_line as u32, line_number as u32, None);
            return;
        }
    }
}

fn push_code_fence_folding_ranges(document: &OpenDocument, ranges: &mut Vec<FoldingRange>) {
    let mut open = None::<(usize, char, usize)>;

    for (line_number, line) in document.parsed.text().lines().enumerate() {
        let Some((delimiter, len)) = code_fence_marker(line) else {
            continue;
        };

        if let Some((open_line, open_delimiter, open_len)) = open {
            if delimiter == open_delimiter && len >= open_len {
                push_line_folding_range(ranges, open_line as u32, line_number as u32, None);
                open = None;
            }
        } else {
            open = Some((line_number, delimiter, len));
        }
    }
}

fn code_fence_marker(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start_matches([' ', '\t']);
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let delimiter = trimmed.chars().next()?;
    if !matches!(delimiter, '`' | '~') {
        return None;
    }
    let len = trimmed.chars().take_while(|ch| *ch == delimiter).count();
    (len >= 3).then_some((delimiter, len))
}

fn push_folding_range(
    ranges: &mut Vec<FoldingRange>,
    document: &OpenDocument,
    start_offset: usize,
    end_offset: usize,
    collapsed_text: Option<String>,
) {
    let text = document.parsed.text();
    let line_index = document.parsed.line_index();
    let start_line = line_index.offset_to_position(text, start_offset).line;
    let end_line = line_index
        .offset_to_position(text, end_offset.saturating_sub(1))
        .line;
    push_line_folding_range(ranges, start_line, end_line, collapsed_text);
}

fn push_line_folding_range(
    ranges: &mut Vec<FoldingRange>,
    start_line: u32,
    end_line: u32,
    collapsed_text: Option<String>,
) {
    if end_line <= start_line {
        return;
    }

    ranges.push(FoldingRange {
        start_line,
        start_character: None,
        end_line,
        end_character: None,
        kind: None,
        collapsed_text,
    });
}

fn last_content_line(line_index: &LineIndex, text: &str) -> u32 {
    let line_count = line_index.line_count();
    if line_count == 0 {
        return 0;
    }
    if text.ends_with('\n') && line_count > 1 {
        (line_count - 2) as u32
    } else {
        (line_count - 1) as u32
    }
}

fn fenced_div_highlights(document: &OpenDocument, div: &FencedDiv) -> Vec<DocumentHighlight> {
    let mut ranges = vec![div.opening_range];
    if let Some(closing_range) = div.closing_range {
        if closing_range != div.opening_range {
            ranges.push(closing_range);
        }
    }

    ranges
        .into_iter()
        .map(|range| DocumentHighlight {
            range: to_lsp_range(document.parsed.text(), document.parsed.line_index(), range),
            kind: Some(DocumentHighlightKind::TEXT),
        })
        .collect()
}

fn citation_hover_text(
    document: &OpenDocument,
    workspace: &WorkspaceIndex,
    citation: &Citation,
) -> String {
    if let Some(entry) = workspace.citation_entry(&citation.key) {
        return bibliography_entry_hover_text(&citation.key, entry);
    }

    if let Some(div) = document.analysis.div_by_id(&citation.key) {
        return fenced_div_hover_text(div);
    }

    if let Some(heading) = document.analysis.heading_by_anchor(&citation.key) {
        return format!(
            "Heading: `{}`\n\nAnchor: `#{}`",
            heading.title, heading.anchor
        );
    }

    if let Some(reference) = document.analysis.local_reference(&citation.key) {
        return local_reference_hover_text(reference);
    }

    format!("Unresolved citation `@{}`", citation.key)
}

fn heading_link_hover_text(document: &OpenDocument, link: &HeadingLink) -> String {
    if let Some(heading) = document.analysis.heading_by_anchor(&link.anchor) {
        return format!(
            "Heading: `{}`\n\nAnchor: `#{}`",
            heading.title, heading.anchor
        );
    }

    if let Some(div) = document.analysis.div_by_id(&link.anchor) {
        return fenced_div_hover_text(div);
    }

    if let Some(reference) = document.analysis.local_reference(&link.anchor) {
        return local_reference_hover_text(reference);
    }

    format!("Unresolved heading link `#{}`", link.anchor)
}

fn bibliography_entry_hover_text(key: &str, entry: &BibliographyEntry) -> String {
    let mut details = Vec::new();
    if let Some(authors) = &entry.authors {
        details.push(format!("Author: {authors}"));
    }
    if let Some(title) = &entry.title {
        details.push(format!("Title: {title}"));
    }
    if let Some(year) = &entry.year {
        details.push(format!("Year: {year}"));
    }

    if details.is_empty() {
        format!("Citation `@{key}`")
    } else {
        format!("`@{key}`\n\n{}", details.join("\n"))
    }
}

fn local_reference_hover_text(reference: &pandocmd_analysis::LocalReference) -> String {
    let detail = local_reference_display_detail(&reference.detail);
    format!("{detail}: `#{}`", reference.id)
}

fn local_reference_display_detail(detail: &str) -> String {
    let mut chars = detail.chars();
    let Some(first) = chars.next() else {
        return "Local Pandoc reference".to_string();
    };
    first.to_uppercase().chain(chars).collect()
}

fn fenced_div_hover_text(div: &FencedDiv) -> String {
    let mut sections = vec![fenced_div_summary(div)];
    let mut details = Vec::new();

    if let Some(id) = &div.id {
        details.push(format!("ID: {}", markdown_code(&format!("#{id}"))));
    }
    if !div.classes.is_empty() {
        let classes = div
            .classes
            .iter()
            .map(|class| markdown_code(class))
            .collect::<Vec<_>>()
            .join(", ");
        details.push(format!("Classes: {classes}"));
    }
    if !div.attributes.is_empty() {
        let attributes = div
            .attributes
            .iter()
            .map(|attribute| {
                let text = if let Some(value) = &attribute.value {
                    format!("{}=\"{}\"", attribute.key, value)
                } else {
                    attribute.key.clone()
                };
                markdown_code(&text)
            })
            .collect::<Vec<_>>()
            .join(", ");
        details.push(format!("Attributes: {attributes}"));
    }

    if !details.is_empty() {
        sections.push(details.join("\n"));
    }

    sections.join("\n\n")
}

fn fenced_div_summary(div: &FencedDiv) -> String {
    let kind = div
        .classes
        .first()
        .map(String::as_str)
        .unwrap_or("fenced div");
    let kind = markdown_code(kind);

    if let Some(title) = div.title() {
        format!("{kind}: {title}")
    } else {
        kind
    }
}

fn markdown_code(text: &str) -> String {
    format!("`{}`", text.replace('`', "\\`"))
}

struct CitationCompletionContext {
    edit_range: Range,
    prefix: String,
}

fn citation_completion_context(
    document: &OpenDocument,
    position: Position,
) -> Option<CitationCompletionContext> {
    let text = document.parsed.text();
    let offset = document
        .parsed
        .line_index()
        .position_to_offset(text, TextPosition::new(position.line, position.character));
    let start = citation_completion_start(text, offset)?;
    let prefix = text.get(start + 1..offset)?.to_string();

    Some(CitationCompletionContext {
        edit_range: to_lsp_range(
            text,
            document.parsed.line_index(),
            TextRange::new(start, offset),
        ),
        prefix,
    })
}

fn citation_completion_start(text: &str, offset: usize) -> Option<usize> {
    let mut start = offset;
    while let Some((idx, ch)) = previous_char(text, start) {
        if is_citation_key_char(ch) {
            start = idx;
        } else {
            break;
        }
    }

    let (at_idx, ch) = previous_char(text, start)?;
    if ch == '@' && can_start_citation(text, at_idx) {
        Some(at_idx)
    } else {
        None
    }
}

fn previous_char(text: &str, offset: usize) -> Option<(usize, char)> {
    text.get(..offset)?.char_indices().next_back()
}

fn can_start_citation(text: &str, at_offset: usize) -> bool {
    match previous_char(text, at_offset) {
        Some((_, '[')) => true,
        Some((dash_offset, '-')) => matches!(previous_char(text, dash_offset), Some((_, '['))),
        _ => false,
    }
}

fn is_citation_key_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || matches!(
            ch,
            '_' | ':' | '.' | '#' | '$' | '%' | '&' | '+' | '-' | '?' | '<' | '>' | '~' | '/'
        )
}

fn citation_completion_items(
    document: &OpenDocument,
    workspace: &WorkspaceIndex,
    text_edit_range: Option<Range>,
    prefix: Option<&str>,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();

    let mut local_references = document.analysis.local_references(document.parsed.text());
    local_references.sort_by(|left, right| left.id.cmp(&right.id));
    for reference in local_references {
        push_citation_completion_item(
            &mut items,
            &mut seen,
            reference.id,
            &reference.detail,
            text_edit_range.clone(),
            prefix,
        );
    }

    let mut citations = workspace.citation_entries().collect::<Vec<_>>();
    citations.sort_by(|left, right| left.key.cmp(&right.key));
    for citation in citations {
        let detail = citation
            .completion_detail()
            .unwrap_or_else(|| "Bibliography citation".to_string());
        push_citation_completion_item(
            &mut items,
            &mut seen,
            citation.key.clone(),
            &detail,
            text_edit_range.clone(),
            prefix,
        );
    }

    items
}

fn push_citation_completion_item(
    items: &mut Vec<CompletionItem>,
    seen: &mut BTreeSet<String>,
    key: String,
    detail: &str,
    text_edit_range: Option<Range>,
    prefix: Option<&str>,
) {
    if prefix.is_some_and(|prefix| !key.starts_with(prefix)) || !seen.insert(key.clone()) {
        return;
    }

    let label = format!("@{key}");
    let text_edit = text_edit_range.map(|range| {
        CompletionTextEdit::Edit(TextEdit {
            range,
            new_text: label.clone(),
        })
    });
    let insert_text = if text_edit.is_some() {
        None
    } else {
        Some(label.clone())
    };

    items.push(CompletionItem {
        label: label.clone(),
        kind: Some(CompletionItemKind::REFERENCE),
        detail: Some(detail.to_string()),
        filter_text: Some(label),
        insert_text,
        text_edit,
        ..CompletionItem::default()
    });
}

struct OpenDocument {
    version: i32,
    parsed: ParsedDocument,
    analysis: DocumentAnalysis,
    workspace: WorkspaceIndex,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn citation_completion_uses_local_cross_reference_ids() -> Result<()> {
        let document = test_document("![Plot](plot.png){#fig-plot}\n\nSee [@fi]\n")?;
        let workspace = WorkspaceIndex::empty();
        let context = citation_completion_context(&document, Position::new(2, 8)).unwrap();
        let expected_range = Range::new(Position::new(2, 5), Position::new(2, 8));

        assert_eq!(context.prefix, "fi");
        assert_eq!(context.edit_range, expected_range);

        let items = citation_completion_items(
            &document,
            &workspace,
            Some(context.edit_range),
            Some(context.prefix.as_str()),
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "@fig-plot");
        assert_eq!(items[0].detail.as_deref(), Some("figure"));
        assert_eq!(items[0].insert_text, None);
        match &items[0].text_edit {
            Some(CompletionTextEdit::Edit(edit)) => {
                assert_eq!(edit.range, expected_range);
                assert_eq!(edit.new_text, "@fig-plot");
            }
            other => panic!("expected text edit, got {other:?}"),
        }

        Ok(())
    }

    #[test]
    fn citation_completion_uses_bibliography_author_and_year() -> Result<()> {
        let document = test_document("See [@do]\n")?;
        let mut workspace = WorkspaceIndex::empty();
        workspace.add_bibliography_text(
            "@article{doe2024,\n author = {Jane Doe and John Smith},\n year = {2024}\n}",
        );
        let context = citation_completion_context(&document, Position::new(0, 8)).unwrap();

        let items = citation_completion_items(
            &document,
            &workspace,
            Some(context.edit_range),
            Some(context.prefix.as_str()),
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "@doe2024");
        assert_eq!(items[0].detail.as_deref(), Some("Doe and Smith 2024"));

        Ok(())
    }

    #[test]
    fn citation_hover_uses_bibliography_title() -> Result<()> {
        let document = test_document("See [@doe2024]\n")?;
        let mut workspace = WorkspaceIndex::empty();
        workspace.add_bibliography_text(
            "@article{doe2024,\n author = {Jane Doe and John Smith},\n year = {2024},\n title = {Useful Result}\n}",
        );
        let citation = &document.analysis.citations[0];

        let hover = citation_hover_text(&document, &workspace, citation);

        assert!(hover.contains("Author: Doe and Smith"));
        assert!(hover.contains("Title: Useful Result"));
        assert!(hover.contains("Year: 2024"));

        Ok(())
    }

    #[test]
    fn citation_definition_uses_bibliography_source_location() -> Result<()> {
        let root = std::env::temp_dir().join(format!(
            "pandocmd-lsp-bib-definition-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root)?;
        let bibliography = root.join("refs.bib");
        std::fs::write(
            &bibliography,
            "@article{first,\n title = {First}\n}\n@book{doe2024,\n title = {Useful Result}\n}\n",
        )?;

        let document = test_document("See [@doe2024]\n")?;
        let mut workspace = WorkspaceIndex::empty();
        workspace.add_bibliography_file(&bibliography);
        let citation = &document.analysis.citations[0];

        let location = bibliography_definition_location(&workspace, citation).unwrap();

        assert_eq!(location.uri, Url::from_file_path(&bibliography).unwrap());
        assert_eq!(
            location.range,
            Range::new(Position::new(3, 6), Position::new(3, 13))
        );

        let _ = std::fs::remove_dir_all(root);
        Ok(())
    }

    #[test]
    fn citation_hover_uses_fenced_div_attributes() -> Result<()> {
        let document = test_document(
            "::: {#lem-main .Lemma title=\"Main result\" role=\"claim\"}\n:::\n\nSee [@lem-main]\n",
        )?;
        let workspace = WorkspaceIndex::empty();
        let citation = &document.analysis.citations[0];

        let hover = citation_hover_text(&document, &workspace, citation);

        assert!(hover.contains("`Lemma`: Main result"));
        assert!(hover.contains("ID: `#lem-main`"));
        assert!(hover.contains("Classes: `Lemma`"));
        assert!(hover.contains("Attributes: `title=\"Main result\"`, `role=\"claim\"`"));

        Ok(())
    }

    #[test]
    fn citation_hover_uses_local_cross_reference_kinds() -> Result<()> {
        let document = test_document("![Plot](plot.png){#fig-plot}\n\nSee [@fig-plot].\n")?;
        let workspace = WorkspaceIndex::empty();
        let citation = &document.analysis.citations[0];

        let hover = citation_hover_text(&document, &workspace, citation);

        assert_eq!(hover, "Figure: `#fig-plot`");

        Ok(())
    }

    #[test]
    fn fenced_div_hover_preserves_unbraced_class_name() -> Result<()> {
        let document = test_document("::: Lemma\n:::\n")?;
        let div = &document.analysis.fenced_divs[0];

        assert!(fenced_div_hover_text(div).starts_with("`Lemma`"));

        Ok(())
    }

    #[test]
    fn document_highlight_pairs_fenced_div_open_and_close() -> Result<()> {
        let document = test_document("::: Lemma\ncontent\n:::\n")?;
        let div = &document.analysis.fenced_divs[0];

        let highlights = fenced_div_highlights(&document, div);

        assert_eq!(highlights.len(), 2);
        assert_eq!(
            highlights[0].range,
            Range::new(Position::new(0, 0), Position::new(0, 9))
        );
        assert_eq!(
            highlights[1].range,
            Range::new(Position::new(2, 0), Position::new(2, 3))
        );

        Ok(())
    }

    #[test]
    fn folding_ranges_cover_document_blocks() -> Result<()> {
        let document = test_document(
            "---\ntitle: T\n---\n\n# Intro\npara\n\n```rust\nfn main() {}\n```\n\n::: {.note}\nbody\n:::\n\n## Nested\ntext\n\n# Next\n",
        )?;

        let ranges = folding_ranges(&document)
            .into_iter()
            .map(|range| (range.start_line, range.end_line))
            .collect::<Vec<_>>();

        assert!(ranges.contains(&(0, 2)), "YAML metadata should fold");
        assert!(ranges.contains(&(4, 17)), "top-level heading should fold");
        assert!(ranges.contains(&(7, 9)), "code fence should fold");
        assert!(ranges.contains(&(11, 13)), "fenced div should fold");
        assert!(ranges.contains(&(15, 17)), "nested heading should fold");
        assert!(
            !ranges.contains(&(18, 18)),
            "single-line final heading should not fold"
        );

        Ok(())
    }

    #[test]
    fn citation_completion_uses_fenced_div_title() -> Result<()> {
        let document =
            test_document("::: {#thm-main .theorem title=\"Main theorem\"}\n:::\n\nSee [@th]\n")?;
        let workspace = WorkspaceIndex::empty();
        let context = citation_completion_context(&document, Position::new(3, 8)).unwrap();

        let items = citation_completion_items(
            &document,
            &workspace,
            Some(context.edit_range),
            Some(context.prefix.as_str()),
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "@thm-main");
        assert_eq!(items[0].detail.as_deref(), Some("theorem: Main theorem"));

        Ok(())
    }

    #[test]
    fn citation_completion_uses_fenced_div_inline_caption() -> Result<()> {
        let document = test_document(
            "::: {.table #tbl:applications} Cogirth-strength ratio bounds.\nsome table\n:::\n\nSee [@tbl]\n",
        )?;
        let workspace = WorkspaceIndex::empty();
        let context = citation_completion_context(&document, Position::new(4, 9)).unwrap();

        let items = citation_completion_items(
            &document,
            &workspace,
            Some(context.edit_range),
            Some(context.prefix.as_str()),
        );

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].label, "@tbl:applications");
        assert_eq!(
            items[0].detail.as_deref(),
            Some("table: Cogirth-strength ratio bounds.")
        );

        Ok(())
    }

    #[test]
    fn citation_completion_excludes_footnote_labels() -> Result<()> {
        let document = test_document("[^note]: Footnote\n\nSee [@no]\n")?;
        let workspace = WorkspaceIndex::empty();
        let context = citation_completion_context(&document, Position::new(2, 8)).unwrap();

        let items = citation_completion_items(
            &document,
            &workspace,
            Some(context.edit_range),
            Some(context.prefix.as_str()),
        );

        assert!(items.is_empty());

        Ok(())
    }

    #[test]
    fn citation_completion_context_only_accepts_bracketed_citations() {
        assert_eq!(
            citation_completion_start("email@example", "email@example".len()),
            None
        );
        assert_eq!(
            citation_completion_start("See @fig", "See @fig".len()),
            None
        );
        assert_eq!(
            citation_completion_start("See -@fig", "See -@fig".len()),
            None
        );
        assert_eq!(
            citation_completion_start("See [@fig", "See [@fig".len()),
            Some(5)
        );
        assert_eq!(
            citation_completion_start("See [-@fig", "See [-@fig".len()),
            Some(6)
        );
    }

    fn test_document(text: &str) -> Result<OpenDocument> {
        let mut parser = PandocMarkdownParser::new()?;
        let parsed = parser.parse(text.to_string())?;
        let workspace = WorkspaceIndex::empty();
        let analysis = DocumentAnalysis::analyze(&parsed, &workspace);

        Ok(OpenDocument {
            version: 0,
            parsed,
            analysis,
            workspace,
        })
    }
}
