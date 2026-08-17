//! Pandoc Markdown language server.
//!
//! A stdio LSP server for Pandoc Markdown with rust-analyzer-style
//! features: navigation (definition/references/highlight), hover,
//! completion, rename, code actions, folding, document symbols, document
//! links, semantic tokens, and extension-aware diagnostics driven by the
//! full Pandoc extension model.
//!
//! The server is also usable as a library: [`run_stdio`] starts a server on
//! the current process's stdio, while [`Server`] can be driven over any
//! [`lsp_server::Connection`] (see the integration tests).

use anyhow::{Context, Result};
use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::notification::{
    DidChangeConfiguration, DidChangeTextDocument, DidChangeWatchedFiles, DidCloseTextDocument,
    DidOpenTextDocument, DidSaveTextDocument, Initialized, Notification as _,
};
use lsp_types::request::{
    CodeActionRequest, Completion, DocumentHighlightRequest, DocumentLinkRequest,
    DocumentSymbolRequest, FoldingRangeRequest, GotoDefinition, HoverRequest, PrepareRenameRequest,
    References, Rename as RenameRequest, Request as _, SemanticTokensFullRequest,
};
use lsp_types::{
    CodeActionContext, CompletionOptions, Diagnostic, DidChangeConfigurationParams,
    DidChangeTextDocumentParams, DidChangeWatchedFilesParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentSymbol, DocumentSymbolParams,
    DocumentSymbolResponse, FoldingRange, FoldingRangeParams, FoldingRangeProviderCapability,
    InitializeParams, Location, OneOf, Position, PublishDiagnosticsParams, RenameParams,
    SaveOptions, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, Url,
    WorkspaceEdit,
};
use pandocmd_analysis::{AnalyzeOptions, WorkspaceIndex};
use pandocmd_pandoc::PandocValidator;
use pandocmd_syntax::{PandocMarkdownParser, TextPosition, TextRange};
use std::collections::HashMap;
use tracing::{debug, error, info, warn};

pub mod config;
pub mod document;
pub mod features;

use config::{PandocValidationMode, PandocmdConfig, ResolvedSettings};
use document::OpenDocument;

/// Semantic token legend advertised by the server.
pub const SEMANTIC_TOKENS_LEGEND: &[&str] = features::SEMANTIC_TOKEN_LEGEND;

/// Run the language server over stdio (the binary entrypoint).
pub fn run_stdio() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let (connection, io_threads) = Connection::stdio();
    serve(connection)?;
    io_threads.join()?;
    Ok(())
}

/// Serve one connection: perform the LSP initialize handshake and run the
/// message loop until shutdown.
pub fn serve(connection: Connection) -> Result<()> {
    let capabilities = serde_json::to_value(server_capabilities())?;
    let initialization = connection.initialize(capabilities)?;
    let initialize_params: InitializeParams =
        serde_json::from_value(initialization).context("failed to decode initialize params")?;
    let mut server = Server::new(initialize_params, connection.sender.clone())?;
    server.run(connection)?;
    Ok(())
}

fn server_capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            lsp_types::TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::INCREMENTAL),
                save: Some(lsp_types::TextDocumentSyncSaveOptions::SaveOptions(
                    SaveOptions {
                        include_text: Some(false),
                    },
                )),
                ..lsp_types::TextDocumentSyncOptions::default()
            },
        )),
        document_symbol_provider: Some(OneOf::Left(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
        hover_provider: Some(lsp_types::HoverProviderCapability::Simple(true)),
        completion_provider: Some(CompletionOptions {
            resolve_provider: Some(false),
            trigger_characters: Some(["@", "#", "^", "]"].iter().map(|s| s.to_string()).collect()),
            ..CompletionOptions::default()
        }),
        document_link_provider: Some(lsp_types::DocumentLinkOptions {
            resolve_provider: Some(false),
            work_done_progress_options: Default::default(),
        }),
        semantic_tokens_provider: Some(
            lsp_types::SemanticTokensServerCapabilities::SemanticTokensOptions(
                lsp_types::SemanticTokensOptions {
                    legend: lsp_types::SemanticTokensLegend {
                        token_types: SEMANTIC_TOKENS_LEGEND
                            .iter()
                            .map(|name| lsp_types::SemanticTokenType::new(name))
                            .collect(),
                        token_modifiers: vec![],
                    },
                    full: Some(lsp_types::SemanticTokensFullOptions::Bool(true)),
                    range: Some(false),
                    ..Default::default()
                },
            ),
        ),
        code_action_provider: Some(lsp_types::CodeActionProviderCapability::Simple(true)),
        rename_provider: Some(lsp_types::OneOf::Right(lsp_types::RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: Default::default(),
        })),
        workspace: Some(lsp_types::WorkspaceServerCapabilities {
            workspace_folders: Some(lsp_types::WorkspaceFoldersServerCapabilities {
                supported: Some(false),
                change_notifications: None,
            }),
            file_operations: None,
        }),
        ..ServerCapabilities::default()
    }
}

/// The language server state.
pub struct Server {
    parser: PandocMarkdownParser,
    workspace: WorkspaceIndex,
    documents: HashMap<Url, OpenDocument>,
    config: PandocmdConfig,
    settings: ResolvedSettings,
    pandoc: Option<PandocValidator>,
    /// Request ids we are waiting for client responses on.
    pending_requests: HashMap<RequestId, PendingRequest>,
    next_request_id: i32,
    /// Sender for server-initiated messages (cloned from the connection).
    sender: crossbeam_channel::Sender<Message>,
    /// Extensions reported as unknown in the config (logged once).
    reported_unknown_extensions: Vec<String>,
}

enum PendingRequest {
    WorkspaceConfiguration,
}

impl Server {
    /// Build a server for an initialize handshake's parameters.
    pub fn new(
        params: InitializeParams,
        sender: crossbeam_channel::Sender<Message>,
    ) -> Result<Self> {
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

        let config: PandocmdConfig = params
            .initialization_options
            .and_then(|options| serde_json::from_value(options).ok())
            .unwrap_or_default();

        let mut server = Server {
            parser: PandocMarkdownParser::new()?,
            workspace,
            documents: HashMap::new(),
            settings: ResolvedSettings::from_config(&config),
            config,
            pandoc,
            pending_requests: HashMap::new(),
            next_request_id: 0,
            sender,
            reported_unknown_extensions: Vec::new(),
        };
        server.log_extension_config_issues();
        Ok(server)
    }

    /// The runtime settings (exposed for tests and embedders).
    pub fn settings(&self) -> &ResolvedSettings {
        &self.settings
    }

    fn log_extension_config_issues(&mut self) {
        let (_, unknown) = self.settings.extension_config.resolve();
        for item in unknown {
            if !self.reported_unknown_extensions.contains(&item.name) {
                warn!("unknown Pandoc extension `{}` in settings", item.name);
                self.reported_unknown_extensions.push(item.name);
            }
        }
    }

    fn analyze_options(&self) -> AnalyzeOptions {
        let (extensions, _) = self.settings.extension_config.resolve();
        AnalyzeOptions {
            extensions,
            unresolved_references: self.settings.unresolved_references,
            disabled_extensions: self.settings.disabled_extensions,
        }
    }

    /// Main message loop.
    pub fn run(&mut self, connection: Connection) -> Result<()> {
        for message in &connection.receiver {
            match message {
                Message::Request(request) => {
                    if connection.handle_shutdown(&request)? {
                        return Ok(());
                    }
                    let response = self.handle_request(request);
                    self.sender.send(Message::Response(response))?;
                }
                Message::Notification(notification) => {
                    if let Err(err) = self.handle_notification(notification) {
                        error!("{err:#}");
                    }
                }
                Message::Response(response) => {
                    self.handle_response(response);
                }
            }
        }
        Ok(())
    }

    fn handle_response(&mut self, response: lsp_server::Response) {
        match self.pending_requests.remove(&response.id) {
            Some(PendingRequest::WorkspaceConfiguration) => {
                let Some(value) = response.result else {
                    if let Some(error) = response.error {
                        debug!("workspace/configuration failed: {}", error.message);
                    }
                    return;
                };
                let items = match value.as_array() {
                    Some(items) => items.to_vec(),
                    None => return,
                };
                if let Some(item) = items.first() {
                    if let Ok(config) = serde_json::from_value::<PandocmdConfig>(item.clone()) {
                        self.apply_config(config);
                    }
                }
            }
            None => debug!("unexpected response {:?}", response.id),
        }
    }

    fn apply_config(&mut self, config: PandocmdConfig) {
        self.config.merge(config);
        self.settings = ResolvedSettings::from_config(&self.config);
        self.log_extension_config_issues();
        let options = self.analyze_options();
        for document in self.documents.values_mut() {
            document.reanalyze(&self.workspace, &options);
        }
        self.publish_all_diagnostics();
    }

    fn publish_all_diagnostics(&self) {
        let documents: Vec<_> = self.documents.keys().cloned().collect();
        for uri in documents {
            if let Some(document) = self.documents.get(&uri) {
                publish_diagnostics(&self.sender, &uri, document);
            }
        }
    }

    fn handle_request(&mut self, request: Request) -> Response {
        let id = request.id.clone();
        let method = request.method.clone();

        let result: Result<Option<serde_json::Value>> = match method.as_str() {
            DocumentSymbolRequest::METHOD => {
                self.document_symbols(request.params).and_then(to_value)
            }
            GotoDefinition::METHOD => self.definition(request.params).and_then(to_value),
            References::METHOD => self.references(request.params).and_then(to_value),
            DocumentHighlightRequest::METHOD => {
                self.document_highlight(request.params).and_then(to_value)
            }
            FoldingRangeRequest::METHOD => self.folding_range(request.params).and_then(to_value),
            HoverRequest::METHOD => self.hover(request.params).and_then(to_value),
            Completion::METHOD => self.completion(request.params).and_then(to_value),
            CodeActionRequest::METHOD => self.code_action(request.params).and_then(to_value),
            DocumentLinkRequest::METHOD => self.document_link(request.params).and_then(to_value),
            SemanticTokensFullRequest::METHOD => {
                self.semantic_tokens_full(request.params).and_then(to_value)
            }
            RenameRequest::METHOD => self.rename(request.params).and_then(to_value),
            PrepareRenameRequest::METHOD => self.prepare_rename(request.params).and_then(to_value),
            _ => {
                return Response::new_err(
                    id,
                    lsp_server::ErrorCode::MethodNotFound as i32,
                    format!("method not found: {method}"),
                )
            }
        };

        match result {
            Ok(value) => Response::new_ok(id, value),
            Err(err) => Response::new_err(
                id,
                lsp_server::ErrorCode::InternalError as i32,
                format!("{err:#}"),
            ),
        }
    }

    fn handle_notification(&mut self, notification: Notification) -> Result<()> {
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let params: DidOpenTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                self.open_document(params)?;
            }
            DidChangeTextDocument::METHOD => {
                let params: DidChangeTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                self.change_document(params)?;
            }
            DidCloseTextDocument::METHOD => {
                let params: DidCloseTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                self.close_document(params)?;
            }
            DidSaveTextDocument::METHOD => {
                let params: DidSaveTextDocumentParams =
                    serde_json::from_value(notification.params)?;
                self.save_document(params)?;
            }
            DidChangeWatchedFiles::METHOD => {
                let params: DidChangeWatchedFilesParams =
                    serde_json::from_value(notification.params)?;
                self.watched_files_changed(params)?;
            }
            DidChangeConfiguration::METHOD => {
                let params: DidChangeConfigurationParams =
                    serde_json::from_value(notification.params)?;
                self.configuration_changed(params);
            }
            Initialized::METHOD => self.on_initialized(),
            _ => {}
        }
        Ok(())
    }

    fn on_initialized(&mut self) {
        self.register_capability_watchers();
        self.pull_workspace_configuration();
    }

    fn register_capability_watchers(&mut self) {
        let registration = lsp_types::Registration {
            id: "pandocmd-watch-bibliographies".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options: Some(serde_json::json!({
                "watchers": [{ "globPattern": "**/*.bib" }]
            })),
        };
        self.send_client_request(
            "client/registerCapability",
            serde_json::json!({ "registrations": [registration] }),
            None,
        );
    }

    fn pull_workspace_configuration(&mut self) {
        let params = serde_json::json!({ "items": [{ "section": "pandoc" }] });
        self.send_client_request(
            "workspace/configuration",
            params,
            Some(PendingRequest::WorkspaceConfiguration),
        );
    }

    fn send_client_request(
        &mut self,
        method: &str,
        params: serde_json::Value,
        pending: Option<PendingRequest>,
    ) {
        self.next_request_id += 1;
        let id = RequestId::from(self.next_request_id);
        if let Some(pending) = pending {
            self.pending_requests.insert(id.clone(), pending);
        }
        let request = Request::new(id, method.to_string(), params);
        if self.sender.send(Message::Request(request)).is_err() {
            debug!("failed to send client request {method}");
        }
    }

    fn configuration_changed(&mut self, params: DidChangeConfigurationParams) {
        if let Some(config) = PandocmdConfig::from_settings(&params.settings) {
            self.apply_config(config);
        }
        // Some clients only push empty settings; poll as a fallback.
        self.pull_workspace_configuration();
    }

    // ------------------------------------------------------ document events

    fn open_document(&mut self, params: DidOpenTextDocumentParams) -> Result<()> {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let analyze_options = self.analyze_options();
        let parsed = self.parser.parse(params.text_document.text)?;
        let mut document = OpenDocument {
            uri: uri.clone(),
            version,
            parsed,
            analysis: Default::default(),
            workspace: WorkspaceIndex::empty(),
            pandoc_diagnostics: Vec::new(),
        };
        document.reanalyze(&self.workspace, &analyze_options);
        publish_diagnostics(&self.sender, &uri, &document);
        self.documents.insert(uri, document);
        Ok(())
    }

    fn change_document(&mut self, params: DidChangeTextDocumentParams) -> Result<()> {
        let uri = params.text_document.uri;
        let version = params.text_document.version;
        let analyze_options = self.analyze_options();
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
        existing.parsed = parsed;
        existing.version = version;
        existing.pandoc_diagnostics.clear();
        existing.reanalyze(&self.workspace, &analyze_options);
        publish_diagnostics(&self.sender, &uri, existing);
        Ok(())
    }

    fn close_document(&mut self, params: DidCloseTextDocumentParams) -> Result<()> {
        self.documents.remove(&params.text_document.uri);
        let notification = Notification::new(
            "textDocument/publishDiagnostics".to_string(),
            PublishDiagnosticsParams {
                uri: params.text_document.uri,
                diagnostics: Vec::new(),
                version: None,
            },
        );
        self.sender.send(Message::Notification(notification))?;
        Ok(())
    }

    fn save_document(&mut self, params: DidSaveTextDocumentParams) -> Result<()> {
        let uri = params.text_document.uri;
        if self.settings.pandoc_validation == PandocValidationMode::OnSave {
            if let Some(pandoc) = self.pandoc.clone() {
                if let Some(document) = self.documents.get_mut(&uri) {
                    match pandoc.validate_markdown_with_format(
                        document.parsed.text(),
                        &self.settings.extension_config,
                    ) {
                        Ok(diagnostics) => {
                            document.pandoc_diagnostics = pandoc_diagnostics_to_lsp(diagnostics);
                        }
                        Err(err) => debug!("pandoc validation failed: {err}"),
                    }
                }
            }
        }
        if let Some(document) = self.documents.get(&uri) {
            publish_diagnostics(&self.sender, &uri, document);
        }
        Ok(())
    }

    fn watched_files_changed(&mut self, params: DidChangeWatchedFilesParams) -> Result<()> {
        let bibliographies_changed = params.changes.iter().any(|change| {
            change
                .uri
                .to_file_path()
                .ok()
                .and_then(|path| {
                    path.extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e == "bib")
                })
                .unwrap_or(false)
        });
        if bibliographies_changed {
            let options = self.analyze_options();
            for document in self.documents.values_mut() {
                document.reanalyze(&self.workspace, &options);
                publish_diagnostics(&self.sender, &document.uri, document);
            }
        }
        Ok(())
    }

    // --------------------------------------------------------- LSP handlers

    fn document_symbols(
        &self,
        params: serde_json::Value,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let params: DocumentSymbolParams = serde_json::from_value(params)?;
        let Some(document) = self.documents.get(&params.text_document.uri) else {
            return Ok(None);
        };

        let mut symbols = document
            .analysis
            .headings
            .iter()
            .map(|heading| {
                #[allow(deprecated)]
                DocumentSymbol {
                    name: heading.title.clone(),
                    detail: heading.anchor.as_ref().map(|a| format!("#{a}")),
                    kind: lsp_types::SymbolKind::STRING,
                    tags: None,
                    deprecated: None,
                    range: to_lsp_range(document, heading.range),
                    selection_range: to_lsp_range(document, heading.selection_range),
                    children: None,
                }
            })
            .chain(document.analysis.fenced_divs.iter().map(|div| {
                #[allow(deprecated)]
                DocumentSymbol {
                    name: div.label(),
                    detail: Some(div.detail()),
                    kind: lsp_types::SymbolKind::OBJECT,
                    tags: None,
                    deprecated: None,
                    range: to_lsp_range(document, div.range),
                    selection_range: to_lsp_range(document, div.selection_range),
                    children: None,
                }
            }))
            .collect::<Vec<_>>();
        symbols.sort_by_key(|symbol| symbol.range.start);
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    fn definition(
        &self,
        params: serde_json::Value,
    ) -> Result<Option<lsp_types::GotoDefinitionResponse>> {
        let params: lsp_types::GotoDefinitionParams = serde_json::from_value(params)?;
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let Some((document, symbol)) = self.symbol_at(&uri, position) else {
            return Ok(None);
        };

        let location = match symbol {
            pandocmd_analysis::SymbolAtOffset::Citation(citation) => {
                // Bibliography sources win over in-document fallbacks.
                self.bibliography_definition_location(document, citation)
                    .or_else(|| {
                        document
                            .analysis
                            .anchor_target_range(&citation.key)
                            .map(|range| Location::new(uri.clone(), to_lsp_range(document, range)))
                    })
            }
            _ => {
                let range = Some(match symbol {
                    pandocmd_analysis::SymbolAtOffset::Heading(heading) => {
                        heading.id_range.unwrap_or(heading.selection_range)
                    }
                    pandocmd_analysis::SymbolAtOffset::FencedDiv(div) => {
                        div.id_range.unwrap_or(div.selection_range)
                    }
                    pandocmd_analysis::SymbolAtOffset::LocalReference(reference) => {
                        reference.id_range
                    }
                    pandocmd_analysis::SymbolAtOffset::ReferenceDefinition(definition) => {
                        definition.label_range
                    }
                    pandocmd_analysis::SymbolAtOffset::FootnoteDefinition(definition) => {
                        definition.label_range
                    }
                    pandocmd_analysis::SymbolAtOffset::ReferenceLink(link) => document
                        .analysis
                        .reference_definition(&link.label)
                        .map(|definition| definition.label_range)
                        .unwrap_or(link.label_range),
                    pandocmd_analysis::SymbolAtOffset::FootnoteReference(reference) => document
                        .analysis
                        .footnote_definition(&reference.label)
                        .map(|definition| definition.label_range)
                        .unwrap_or(reference.label_range),
                    pandocmd_analysis::SymbolAtOffset::HeadingLink(link) => document
                        .analysis
                        .anchor_target_range(&link.anchor)
                        .unwrap_or(link.anchor_range),
                    pandocmd_analysis::SymbolAtOffset::InlineNote(note) => note.range,
                    pandocmd_analysis::SymbolAtOffset::Citation(_) => unreachable!(),
                });
                range.map(|range| Location::new(uri.clone(), to_lsp_range(document, range)))
            }
        };

        Ok(location.map(lsp_types::GotoDefinitionResponse::Scalar))
    }

    fn bibliography_definition_location(
        &self,
        document: &OpenDocument,
        citation: &pandocmd_analysis::Citation,
    ) -> Option<Location> {
        let entry = document.workspace.citation_entry(&citation.key)?;
        let source = entry.source.as_ref()?;
        let text = std::fs::read_to_string(&source.path).ok()?;
        let line_index = pandocmd_syntax::LineIndex::new(&text);
        let uri = Url::from_file_path(&source.path).ok()?;
        let (start, end) = line_index.range_to_positions(&text, source.key_range);

        Some(Location::new(
            uri,
            lsp_types::Range::new(
                Position::new(start.line, start.character),
                Position::new(end.line, end.character),
            ),
        ))
    }

    fn references(&self, params: serde_json::Value) -> Result<Option<Vec<Location>>> {
        let params: lsp_types::ReferenceParams = serde_json::from_value(params)?;
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some((document, symbol)) = self.symbol_at(&uri, position) else {
            return Ok(None);
        };

        use pandocmd_analysis::SymbolAtOffset as S;
        let ranges = match symbol {
            S::ReferenceDefinition(definition) => document
                .analysis
                .reference_ranges_for_label(&definition.label),
            S::ReferenceLink(link) => document.analysis.reference_ranges_for_label(&link.label),
            S::FootnoteDefinition(definition) => document
                .analysis
                .footnote_ranges_for_label(&definition.label),
            S::FootnoteReference(reference) => document
                .analysis
                .footnote_ranges_for_label(&reference.label),
            S::Heading(heading) => heading
                .anchor
                .as_deref()
                .map(|anchor| document.analysis.local_reference_ranges_for_id(anchor))
                .unwrap_or_default(),
            S::FencedDiv(div) => div
                .id
                .as_deref()
                .map(|id| document.analysis.local_reference_ranges_for_id(id))
                .unwrap_or_else(|| vec![div.selection_range]),
            S::LocalReference(reference) => document
                .analysis
                .local_reference_ranges_for_id(&reference.id),
            S::HeadingLink(link) => document
                .analysis
                .local_reference_ranges_for_id(&link.anchor),
            S::Citation(citation) => {
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
            S::InlineNote(note) => vec![note.range],
        };

        let locations = ranges
            .into_iter()
            .map(|range| Location::new(uri.clone(), to_lsp_range(document, range)))
            .collect();
        Ok(Some(locations))
    }

    fn document_highlight(
        &self,
        params: serde_json::Value,
    ) -> Result<Option<Vec<lsp_types::DocumentHighlight>>> {
        let params: lsp_types::DocumentHighlightParams = serde_json::from_value(params)?;
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let Some((document, symbol)) = self.symbol_at(&uri, position) else {
            return Ok(None);
        };

        let highlights = match symbol {
            pandocmd_analysis::SymbolAtOffset::FencedDiv(div) => {
                let mut ranges = vec![div.opening_range];
                if let Some(closing) = div.closing_range {
                    if closing != div.opening_range {
                        ranges.push(closing);
                    }
                }
                ranges
            }
            _ => Vec::new(),
        }
        .into_iter()
        .map(|range| lsp_types::DocumentHighlight {
            range: to_lsp_range(document, range),
            kind: Some(lsp_types::DocumentHighlightKind::TEXT),
        })
        .collect::<Vec<_>>();

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

    fn hover(&self, params: serde_json::Value) -> Result<Option<lsp_types::Hover>> {
        let params: lsp_types::HoverParams = serde_json::from_value(params)?;
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        let Some((document, symbol)) = self.symbol_at(&uri, position) else {
            return Ok(None);
        };

        let contents = features::hover_text(document, &document.workspace, symbol);
        let range = match symbol {
            pandocmd_analysis::SymbolAtOffset::Heading(heading) => heading.selection_range,
            pandocmd_analysis::SymbolAtOffset::FencedDiv(div) => div.selection_range,
            pandocmd_analysis::SymbolAtOffset::LocalReference(reference) => reference.id_range,
            pandocmd_analysis::SymbolAtOffset::ReferenceDefinition(definition) => {
                definition.label_range
            }
            pandocmd_analysis::SymbolAtOffset::ReferenceLink(link) => link.label_range,
            pandocmd_analysis::SymbolAtOffset::FootnoteDefinition(definition) => {
                definition.label_range
            }
            pandocmd_analysis::SymbolAtOffset::FootnoteReference(reference) => {
                reference.label_range
            }
            pandocmd_analysis::SymbolAtOffset::InlineNote(note) => note.range,
            pandocmd_analysis::SymbolAtOffset::HeadingLink(link) => link.anchor_range,
            pandocmd_analysis::SymbolAtOffset::Citation(citation) => citation.key_range,
        };

        Ok(Some(lsp_types::Hover {
            contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: contents,
            }),
            range: Some(to_lsp_range(document, range)),
        }))
    }

    fn completion(
        &self,
        params: serde_json::Value,
    ) -> Result<Option<lsp_types::CompletionResponse>> {
        let params: lsp_types::CompletionParams = serde_json::from_value(params)?;
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some(document) = self.documents.get(&uri) else {
            return Ok(None);
        };

        let text = document.parsed.text();
        let offset = document
            .parsed
            .line_index()
            .position_to_offset(text, TextPosition::new(position.line, position.character));

        let Some(context) = features::completion_context_at(text, offset) else {
            return Ok(None);
        };

        let start = offset.saturating_sub(match &context {
            features::CompletionContext::Citation { prefix } => prefix.len() + 1,
            features::CompletionContext::Anchor { prefix } => prefix.len(),
            features::CompletionContext::Footnote { prefix } => prefix.len() + 2,
            features::CompletionContext::ReferenceLabel { prefix } => prefix.len(),
        });
        let edit_range = {
            let (start_pos, end_pos) = document
                .parsed
                .line_index()
                .range_to_positions(text, TextRange::new(start, offset));
            lsp_types::Range::new(
                Position::new(start_pos.line, start_pos.character),
                Position::new(end_pos.line, end_pos.character),
            )
        };

        let items = features::completion_items(
            document,
            &document.workspace,
            &context,
            edit_range,
            &self.settings,
        );
        if items.is_empty() {
            Ok(None)
        } else {
            Ok(Some(lsp_types::CompletionResponse::Array(items)))
        }
    }

    fn code_action(
        &self,
        params: serde_json::Value,
    ) -> Result<Option<lsp_types::CodeActionResponse>> {
        let params: lsp_types::CodeActionParams = serde_json::from_value(params)?;
        let Some(document) = self.documents.get(&params.text_document.uri) else {
            return Ok(None);
        };

        let matching: Vec<pandocmd_analysis::Diagnostic> = document
            .analysis
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic_matches(diagnostic, &params.context))
            .filter(|diagnostic| {
                let lsp_range = diagnostic_range(document, diagnostic);
                !(lsp_range.start >= params.range.end || lsp_range.end <= params.range.start)
            })
            .cloned()
            .collect();

        let actions = features::code_actions(document, &matching);
        Ok(Some(actions))
    }

    fn document_link(
        &self,
        params: serde_json::Value,
    ) -> Result<Option<Vec<lsp_types::DocumentLink>>> {
        let params: lsp_types::DocumentLinkParams = serde_json::from_value(params)?;
        let Some(document) = self.documents.get(&params.text_document.uri) else {
            return Ok(None);
        };
        let links = features::document_links(document);
        if links.is_empty() {
            Ok(None)
        } else {
            Ok(Some(links))
        }
    }

    fn semantic_tokens_full(
        &self,
        params: serde_json::Value,
    ) -> Result<Option<lsp_types::SemanticTokensResult>> {
        let params: lsp_types::SemanticTokensParams = serde_json::from_value(params)?;
        let Some(document) = self.documents.get(&params.text_document.uri) else {
            return Ok(None);
        };
        let data = features::semantic_tokens(document);
        Ok(Some(lsp_types::SemanticTokensResult::Tokens(
            lsp_types::SemanticTokens {
                result_id: None,
                data,
            },
        )))
    }

    fn rename(&self, params: serde_json::Value) -> Result<Option<WorkspaceEdit>> {
        let params: RenameParams = serde_json::from_value(params)?;
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let Some((document, symbol)) = self.symbol_at(&uri, position) else {
            return Ok(None);
        };

        let ranges = match features::rename_ranges(document, symbol) {
            Ok(ranges) => ranges,
            Err(message) => return Err(anyhow::anyhow!(message)),
        };

        let edits = ranges
            .into_iter()
            .map(|range| lsp_types::TextEdit {
                range: to_lsp_range(document, range),
                new_text: params.new_name.clone(),
            })
            .collect::<Vec<_>>();

        Ok(Some(WorkspaceEdit {
            changes: Some([(uri.clone(), edits)].into_iter().collect()),
            ..WorkspaceEdit::default()
        }))
    }

    fn prepare_rename(
        &self,
        params: serde_json::Value,
    ) -> Result<Option<lsp_types::PrepareRenameResponse>> {
        let params: lsp_types::TextDocumentPositionParams = serde_json::from_value(params)?;
        let uri = params.text_document.uri;
        let position = params.position;
        let Some((document, symbol)) = self.symbol_at(&uri, position) else {
            return Ok(None);
        };

        match features::rename_ranges(document, symbol) {
            Ok(ranges) if !ranges.is_empty() => {
                let range = to_lsp_range(document, ranges[0]);
                Ok(Some(lsp_types::PrepareRenameResponse::Range(range)))
            }
            Ok(_) => Ok(None),
            Err(_) => Ok(None),
        }
    }

    fn symbol_at(
        &self,
        uri: &Url,
        position: Position,
    ) -> Option<(&OpenDocument, pandocmd_analysis::SymbolAtOffset<'_>)> {
        let document = self.documents.get(uri)?;
        let offset = document.parsed.line_index().position_to_offset(
            document.parsed.text(),
            TextPosition::new(position.line, position.character),
        );
        let symbol = document.analysis.symbol_at(offset)?;
        Some((document, symbol))
    }
}

fn diagnostic_severity(severity: pandocmd_analysis::Severity) -> lsp_types::DiagnosticSeverity {
    match severity {
        pandocmd_analysis::Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
        pandocmd_analysis::Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
        pandocmd_analysis::Severity::Information => lsp_types::DiagnosticSeverity::INFORMATION,
        pandocmd_analysis::Severity::Hint => lsp_types::DiagnosticSeverity::HINT,
    }
}

fn diagnostic_matches(
    diagnostic: &pandocmd_analysis::Diagnostic,
    context: &CodeActionContext,
) -> bool {
    if context.only.as_ref().is_some_and(|only| {
        !only
            .iter()
            .any(|filter| filter.as_str().starts_with("quickfix"))
    }) {
        return false;
    }
    matches!(
        diagnostic.code,
        "unresolved-footnote" | "unresolved-reference" | "extension-disabled"
    )
}

fn diagnostic_range(
    document: &OpenDocument,
    diagnostic: &pandocmd_analysis::Diagnostic,
) -> lsp_types::Range {
    let (start, end) = document
        .parsed
        .line_index()
        .range_to_positions(document.parsed.text(), diagnostic.range);
    lsp_types::Range::new(
        Position::new(start.line, start.character),
        Position::new(end.line, end.character),
    )
}

fn to_value<T: serde::Serialize>(value: Option<T>) -> Result<Option<serde_json::Value>> {
    Ok(match value {
        Some(value) => Some(serde_json::to_value(value)?),
        None => None,
    })
}

fn pandoc_diagnostics_to_lsp(
    diagnostics: Vec<pandocmd_pandoc::PandocDiagnostic>,
) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            let (line, character) = diagnostic
                .line_column
                .map(|(line, column)| (line.saturating_sub(1), column.saturating_sub(1)))
                .unwrap_or((0, 0));
            Diagnostic {
                range: lsp_types::Range::new(
                    Position::new(line, character),
                    Position::new(line, character + 1),
                ),
                severity: Some(lsp_types::DiagnosticSeverity::WARNING),
                code: Some(lsp_types::NumberOrString::String("pandoc".to_string())),
                source: Some("pandoc".to_string()),
                message: diagnostic.message,
                ..Diagnostic::default()
            }
        })
        .collect()
}

/// Convert one analysis diagnostic into its LSP representation.
pub fn analysis_diagnostic_to_lsp(
    document: &OpenDocument,
    diagnostic: &pandocmd_analysis::Diagnostic,
) -> Diagnostic {
    let (start, end) = document
        .parsed
        .line_index()
        .range_to_positions(document.parsed.text(), diagnostic.range);
    Diagnostic {
        range: lsp_types::Range::new(
            Position::new(start.line, start.character),
            Position::new(end.line, end.character),
        ),
        severity: Some(diagnostic_severity(diagnostic.severity)),
        code: Some(lsp_types::NumberOrString::String(
            diagnostic.code.to_string(),
        )),
        source: Some("pandocmd".to_string()),
        message: diagnostic.message.clone(),
        data: diagnostic
            .extension
            .map(|extension| serde_json::json!({ "extension": extension })),
        ..Diagnostic::default()
    }
}

fn publish_diagnostics(
    sender: &crossbeam_channel::Sender<Message>,
    uri: &Url,
    document: &OpenDocument,
) {
    let mut diagnostics = features::diagnostics_from_analysis(document);
    diagnostics.extend(document.pandoc_diagnostics.iter().cloned());

    let notification = Notification::new(
        "textDocument/publishDiagnostics".to_string(),
        PublishDiagnosticsParams {
            uri: uri.clone(),
            diagnostics,
            version: Some(document.version),
        },
    );
    let _ = sender.send(Message::Notification(notification));
}

fn to_lsp_range(document: &OpenDocument, range: TextRange) -> lsp_types::Range {
    let (start, end) = document
        .parsed
        .line_index()
        .range_to_positions(document.parsed.text(), range);
    lsp_types::Range::new(
        Position::new(start.line, start.character),
        Position::new(end.line, end.character),
    )
}

// ----------------------------------------------------------- folding ranges

fn folding_ranges(document: &OpenDocument) -> Vec<FoldingRange> {
    let mut ranges = Vec::new();
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
        push_line_folding_range(&mut ranges, start_line, end_line);
    }

    for div in &document.analysis.fenced_divs {
        let start_line = line_index
            .offset_to_position(text, div.opening_range.start)
            .line;
        let end_line = line_index
            .offset_to_position(text, div.range.end.saturating_sub(1))
            .line;
        push_line_folding_range(&mut ranges, start_line, end_line);
    }

    push_metadata_folding_ranges(text, &mut ranges);
    push_code_fence_folding_ranges(text, &mut ranges);

    ranges.sort_by_key(|range| (range.start_line, range.end_line));
    ranges.dedup_by_key(|range| (range.start_line, range.end_line));
    ranges
}

fn push_metadata_folding_ranges(text: &str, ranges: &mut Vec<FoldingRange>) {
    let mut lines = text.lines().enumerate();
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
            push_line_folding_range(ranges, start_line as u32, line_number as u32);
            return;
        }
    }
}

fn push_code_fence_folding_ranges(text: &str, ranges: &mut Vec<FoldingRange>) {
    let mut open = None::<(usize, char, usize)>;

    for (line_number, line) in text.lines().enumerate() {
        let Some((delimiter, len)) = code_fence_marker(line) else {
            continue;
        };

        if let Some((open_line, open_delimiter, open_len)) = open {
            if delimiter == open_delimiter && len >= open_len {
                push_line_folding_range(ranges, open_line as u32, line_number as u32);
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

fn push_line_folding_range(ranges: &mut Vec<FoldingRange>, start_line: u32, end_line: u32) {
    if end_line <= start_line {
        return;
    }
    ranges.push(FoldingRange {
        start_line,
        start_character: None,
        end_line,
        end_character: None,
        kind: None,
        collapsed_text: None,
    });
}

fn last_content_line(line_index: &pandocmd_syntax::LineIndex, text: &str) -> u32 {
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

fn apply_change(text: &mut String, range: Option<lsp_types::Range>, replacement: &str) {
    if let Some(range) = range {
        let line_index = pandocmd_syntax::LineIndex::new(text);
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
