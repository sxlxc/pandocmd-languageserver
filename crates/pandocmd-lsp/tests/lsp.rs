//! End-to-end tests for the Pandoc Markdown language server, driving a real
//! [`pandocmd_lsp::Server`] over in-memory LSP connections (the same pattern
//! rust-analyzer uses for its test harness).

use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use lsp_types::{
    notification::{
        DidChangeConfiguration, DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument,
        Initialized, Notification as _,
    },
    request::{
        CodeActionRequest, Completion, DocumentLinkRequest, DocumentSymbolRequest,
        FoldingRangeRequest, GotoDefinition, HoverRequest, PrepareRenameRequest, References,
        Rename, Request as _, SemanticTokensFullRequest,
    },
    ClientCapabilities, DidChangeTextDocumentParams, Position, Range,
    TextDocumentContentChangeEvent, TextDocumentIdentifier, TextDocumentPositionParams, Url,
    VersionedTextDocumentIdentifier,
};
use serde_json::json;

struct TestClient {
    client: Connection,
    next_id: i64,
    notifications: Vec<serde_json::Value>,
    /// Settings returned for workspace/configuration pulls.
    configuration: Option<serde_json::Value>,
}

impl TestClient {
    /// Start a server with the given initialization options and wait for the
    /// initialize handshake to complete.
    fn start(initialization_options: Option<serde_json::Value>) -> TestClient {
        Self::start_with_root(initialization_options, None)
    }

    fn start_with_root(
        initialization_options: Option<serde_json::Value>,
        root: Option<Url>,
    ) -> TestClient {
        let (client, server) = Connection::memory();
        let handle = std::thread::spawn(move || {
            pandocmd_lsp::serve(server).expect("server failed");
        });

        let mut test_client = TestClient {
            client,
            next_id: 1,
            notifications: Vec::new(),
            configuration: None,
        };

        let initialize_params = json!({
            "processId": null,
            "rootUri": root,
            "capabilities": ClientCapabilities::default(),
            "initializationOptions": initialization_options,
        });
        let response = test_client.request_raw("initialize", initialize_params);
        assert!(
            response.error.is_none(),
            "initialize failed: {:?}",
            response.error
        );
        let capabilities = response.result.unwrap();
        assert!(
            capabilities
                .get("capabilities")
                .and_then(|caps| caps.get("semanticTokensProvider"))
                .is_some(),
            "semantic tokens must be advertised"
        );
        test_client.notify(Initialized::METHOD, json!({}));
        // Let the server process `initialized` (registrations etc.).
        test_client.drain(Duration::from_millis(100));
        handle.detach_for_tests();
        test_client
    }

    fn request_raw(&mut self, method: &str, params: serde_json::Value) -> Response {
        let id = RequestId::from(self.next_id.to_string());
        self.next_id += 1;
        self.client
            .sender
            .send(Message::Request(Request::new(
                id.clone(),
                method.to_string(),
                params,
            )))
            .expect("send request");

        loop {
            let message = self
                .client
                .receiver
                .recv_timeout(Duration::from_secs(10))
                .expect("receive response");
            match message {
                Message::Response(response) if response.id == id => return response,
                Message::Response(other) => {
                    panic!("unexpected response id {:?} for {method}", other.id)
                }
                Message::Request(server_request) => {
                    self.answer_server_request(server_request);
                }
                Message::Notification(notification) => {
                    self.notifications.push(notification.params);
                }
            }
        }
    }

    fn answer_server_request(&mut self, request: lsp_server::Request) {
        match request.method.as_str() {
            "client/registerCapability" => {
                let _ = self.client.sender.send(Message::Response(Response::new_ok(
                    request.id,
                    serde_json::Value::Null,
                )));
            }
            "workspace/configuration" => {
                let result = json!([self
                    .configuration
                    .clone()
                    .unwrap_or(serde_json::Value::Null)]);
                let _ = self
                    .client
                    .sender
                    .send(Message::Response(Response::new_ok(request.id, result)));
            }
            other => {
                let _ = self.client.sender.send(Message::Response(Response::new_err(
                    request.id,
                    lsp_server::ErrorCode::MethodNotFound as i32,
                    format!("test client cannot handle {other}"),
                )));
            }
        }
    }

    fn notify(&mut self, method: &str, params: serde_json::Value) {
        self.client
            .sender
            .send(Message::Notification(Notification::new(
                method.to_string(),
                params,
            )))
            .expect("send notification");
    }

    /// Read pending notifications without blocking long.
    fn drain(&mut self, wait: Duration) {
        while let Ok(message) = self.client.receiver.recv_timeout(wait) {
            match message {
                Message::Notification(notification) => self.notifications.push(notification.params),
                Message::Request(server_request) => self.answer_server_request(server_request),
                Message::Response(_) => {}
            }
        }
    }

    fn diagnostics_for(&mut self, uri: &Url) -> Vec<lsp_types::Diagnostic> {
        self.drain(Duration::from_millis(50));
        self.notifications
            .iter()
            .filter_map(|params| {
                let parsed: lsp_types::PublishDiagnosticsParams =
                    serde_json::from_value(params.clone()).ok()?;
                (parsed.uri == *uri).then_some(parsed.diagnostics)
            })
            .next_back()
            .unwrap_or_default()
    }

    fn open(&mut self, uri: &Url, text: &str) {
        self.notify(
            DidOpenTextDocument::METHOD,
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "pandoc-markdown",
                    "version": 1,
                    "text": text,
                }
            }),
        );
        self.drain(Duration::from_millis(100));
    }

    fn edit(&mut self, uri: &Url, version: i32, range: Option<Range>, new_text: &str) {
        let params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range,
                range_length: None,
                text: new_text.to_string(),
            }],
        };
        self.notify(
            DidChangeTextDocument::METHOD,
            serde_json::to_value(params).unwrap(),
        );
        self.drain(Duration::from_millis(100));
    }

    fn close(&mut self, uri: &Url) {
        self.notify(
            DidCloseTextDocument::METHOD,
            json!({ "textDocument": { "uri": uri } }),
        );
        self.drain(Duration::from_millis(50));
    }

    fn request<R: lsp_types::request::Request>(
        &mut self,
        params: serde_json::Value,
    ) -> serde_json::Value
    where
        R::Params: serde::de::DeserializeOwned,
        R::Result: serde::Serialize,
    {
        let response = self.request_raw(R::METHOD, params);
        response.result.expect("request failed")
    }

    fn position_params(uri: &Url, position: Position) -> serde_json::Value {
        let params = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position,
        };
        serde_json::to_value(params).unwrap()
    }
}

trait JoinHandleExt {
    fn detach_for_tests(&self);
}

impl JoinHandleExt for std::thread::JoinHandle<()> {
    fn detach_for_tests(&self) {}
}

fn test_uri(name: &str) -> Url {
    Url::from_file_path(std::env::temp_dir().join(format!("pandocmd-lsp-test-{name}.md"))).unwrap()
}

fn codes(diagnostics: &[lsp_types::Diagnostic]) -> Vec<String> {
    diagnostics
        .iter()
        .map(|diagnostic| match &diagnostic.code {
            Some(lsp_types::NumberOrString::String(code)) => code.clone(),
            _ => String::new(),
        })
        .collect()
}

// ------------------------------------------------------------------- tests

#[test]
fn initialize_advertises_rust_analyzer_style_capabilities() {
    let client = TestClient::start(None);
    // If we got here the handshake worked; capabilities were asserted in start().
    drop(client);
}

#[test]
fn publishes_extension_aware_diagnostics() {
    let uri = test_uri("diagnostics");
    let mut client = TestClient::start(None);
    client.open(&uri, "# Title\n\nSee [missing][nope] and [^gone].\n");

    let diagnostics = client.diagnostics_for(&uri);
    let codes = codes(&diagnostics);
    assert!(codes.iter().any(|code| code == "unresolved-reference"));
    assert!(codes.iter().any(|code| code == "unresolved-footnote"));
}

#[test]
fn disabled_extensions_produce_actionable_diagnostics() {
    let uri = test_uri("disabled");
    let mut client = TestClient::start(Some(json!({
        "extensions": { "disabled": ["footnotes"] }
    })));
    client.open(&uri, "Text.[^note]\n");

    let diagnostics = client.diagnostics_for(&uri);
    assert!(codes(&diagnostics)
        .iter()
        .any(|code| code == "extension-disabled"));
    let disabled = diagnostics
        .iter()
        .find(|diagnostic| {
            matches!(&diagnostic.code, Some(lsp_types::NumberOrString::String(code)) if code == "extension-disabled")
        })
        .unwrap();
    assert_eq!(
        disabled.data.as_ref().unwrap().get("extension"),
        Some(&json!("footnotes"))
    );
    // No unresolved-footnote noise: the reference was not parsed at all.
    assert!(!codes(&diagnostics)
        .iter()
        .any(|code| code == "unresolved-footnote"));
}

#[test]
fn did_change_configuration_updates_analysis() {
    let uri = test_uri("config-change");
    let mut client = TestClient::start(None);
    client.open(&uri, "One.[^note]\n");
    assert!(client
        .diagnostics_for(&uri)
        .iter()
        .any(|diagnostic| matches!(&diagnostic.code, Some(lsp_types::NumberOrString::String(code)) if code == "unresolved-footnote")));

    client.notify(
        DidChangeConfiguration::METHOD,
        json!({ "settings": { "pandoc": { "extensions": { "disabled": ["footnotes"] } } } }),
    );
    client.drain(Duration::from_millis(200));

    let diagnostics = client.diagnostics_for(&uri);
    assert!(!diagnostics
        .iter()
        .any(|diagnostic| matches!(&diagnostic.code, Some(lsp_types::NumberOrString::String(code)) if code == "unresolved-footnote")));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| matches!(&diagnostic.code, Some(lsp_types::NumberOrString::String(code)) if code == "extension-disabled")));
}

#[test]
fn workspace_configuration_pull_applies_settings() {
    let uri = test_uri("config-pull");
    let mut client = TestClient::start(None);
    client.configuration = Some(json!({
        "diagnostics": { "unresolvedReferences": false }
    }));
    client.notify(
        DidChangeConfiguration::METHOD,
        json!({ "settings": {} }), // empty push triggers a pull
    );
    client.drain(Duration::from_millis(200));
    client.open(&uri, "See [missing][nope].\n");

    let diagnostics = client.diagnostics_for(&uri);
    assert!(!diagnostics
        .iter()
        .any(|diagnostic| matches!(&diagnostic.code, Some(lsp_types::NumberOrString::String(code)) if code == "unresolved-reference")));
}

#[test]
fn incremental_edits_reanalyze() {
    let uri = test_uri("edits");
    let mut client = TestClient::start(None);
    client.open(&uri, "# Hello\n\ntext\n");

    client.edit(
        &uri,
        2,
        Some(Range::new(Position::new(0, 7), Position::new(0, 7))),
        " World",
    );

    let symbols: serde_json::Value = client.request::<DocumentSymbolRequest>(json!({
        "textDocument": { "uri": uri }
    }));
    let symbols = symbols.as_array().unwrap();
    assert_eq!(symbols[0]["name"], "Hello World");
}

#[test]
fn hover_reports_heading_anchor() {
    let uri = test_uri("hover");
    let mut client = TestClient::start(None);
    client.open(&uri, "# Getting Started\n\nBody.\n");

    let hover: serde_json::Value =
        client.request::<HoverRequest>(TestClient::position_params(&uri, Position::new(0, 5)));
    let value = hover["contents"]["value"].as_str().unwrap();
    assert!(value.contains("Getting Started"));
    assert!(value.contains("getting-started"));
}

#[test]
fn goto_definition_navigates_links_and_footnotes() {
    let uri = test_uri("definition");
    let mut client = TestClient::start(None);
    client.open(
        &uri,
        "# Intro {#sec-intro}\n\nSee [text][label] and [^note] and [link](#sec-intro).\n\n[label]: https://example.com\n\n[^note]: Note text.\n",
    );

    // Reference link -> definition (`label` occupies columns 11-15).
    let definition: serde_json::Value = client.request::<GotoDefinition>(json!({
        "textDocument": { "uri": uri },
        "position": Position::new(2, 13),
    }));
    assert_eq!(definition["range"]["start"]["line"], 4);

    // Footnote reference -> definition.
    let definition: serde_json::Value = client.request::<GotoDefinition>(json!({
        "textDocument": { "uri": uri },
        "position": Position::new(2, 26),
    }));
    assert_eq!(definition["range"]["start"]["line"], 6);

    // Heading link -> heading.
    let definition: serde_json::Value = client.request::<GotoDefinition>(json!({
        "textDocument": { "uri": uri },
        "position": Position::new(2, 47),
    }));
    assert_eq!(definition["range"]["start"]["line"], 0);
}

#[test]
fn references_find_all_uses_of_an_anchor() {
    let uri = test_uri("references");
    let mut client = TestClient::start(None);
    client.open(
        &uri,
        "# Intro {#sec-intro}\n\nSee [link](#sec-intro) and [@sec-intro].\n",
    );

    let references: serde_json::Value = client.request::<References>(json!({
        "textDocument": { "uri": uri },
        "position": Position::new(0, 12),
        "context": { "includeDeclaration": true },
    }));
    let locations = references.as_array().unwrap();
    assert!(
        locations.len() >= 3,
        "id, (#sec-intro), [@sec-intro]: {references}"
    );
}

#[test]
fn completion_offers_citations_and_anchors() {
    let uri = test_uri("completion");
    let mut client = TestClient::start(None);
    client.open(&uri, "# Figures {#fig-main}\n\nSee [@fig]\n");

    let completion: serde_json::Value = client.request::<Completion>(json!({
        "textDocument": { "uri": uri },
        "position": Position::new(2, 8),
    }));
    let items = completion.as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["label"], "@fig-main");
    assert_eq!(
        items[0]["textEdit"]["newText"], "@fig-main",
        "completion should replace the partial key"
    );

    // Anchor completion after `](#`.
    client.edit(
        &uri,
        2,
        Some(Range::new(Position::new(2, 10), Position::new(2, 10))),
        "\n\nGo [there](#)",
    );
    let completion: serde_json::Value = client.request::<Completion>(json!({
        "textDocument": { "uri": uri },
        "position": Position::new(4, 12),
    }));
    let items = completion.as_array().unwrap();
    assert!(items
        .iter()
        .any(|item| item["textEdit"]["newText"].as_str() == Some("fig-main")));
}

#[test]
fn completion_offers_footnote_and_reference_labels() {
    let uri = test_uri("completion-labels");
    let mut client = TestClient::start(None);
    client.open(
        &uri,
        "Text [doc][ref] and [^fn]\n\n[ref]: https://example.com\n\n[^fn]: Note\n",
    );

    // Footnote labels after `[^`.
    let completion: serde_json::Value = client.request::<Completion>(json!({
        "textDocument": { "uri": uri },
        "position": Position::new(0, 23),
    }));
    let items = completion.as_array().unwrap();
    assert!(items.iter().any(|item| item["textEdit"]["newText"] == "fn"));

    // Reference labels after `][`.
    let completion: serde_json::Value = client.request::<Completion>(json!({
        "textDocument": { "uri": uri },
        "position": Position::new(0, 11),
    }));
    let items = completion.as_array().unwrap();
    assert!(items
        .iter()
        .any(|item| item["textEdit"]["newText"] == "ref"));
}

#[test]
fn code_actions_create_missing_definitions() {
    let uri = test_uri("code-action");
    let mut client = TestClient::start(None);
    client.open(&uri, "See [missing][nope] and [^gone].\n");

    let actions: serde_json::Value = client.request::<CodeActionRequest>(json!({
        "textDocument": { "uri": uri },
        "range": Range::new(Position::new(0, 0), Position::new(0, 34)),
        "context": { "diagnostics": [] },
    }));
    let actions = actions.as_array().unwrap();
    let titles: Vec<&str> = actions
        .iter()
        .filter_map(|action| action["title"].as_str())
        .collect();
    assert!(titles
        .iter()
        .any(|title| title.contains("reference definition")));
    assert!(titles
        .iter()
        .any(|title| title.contains("footnote definition")));
}

#[test]
fn code_actions_offer_extension_enablement() {
    let uri = test_uri("code-action-extension");
    let mut client = TestClient::start(Some(json!({
        "extensions": { "disabled": ["citations"] }
    })));
    client.open(&uri, "See [@doe2024].\n");

    let actions: serde_json::Value = client.request::<CodeActionRequest>(json!({
        "textDocument": { "uri": uri },
        "range": Range::new(Position::new(0, 0), Position::new(0, 16)),
        "context": { "diagnostics": [] },
    }));
    let actions = actions.as_array().unwrap();
    assert!(actions
        .iter()
        .any(|action| action["command"].as_str() == Some("pandocmd.enableExtension")));
}

#[test]
fn execute_command_enables_extension() {
    let uri = test_uri("execute-command");
    let mut client = TestClient::start(Some(json!({
        "extensions": { "disabled": ["citations"] }
    })));
    client.open(&uri, "See [@doe2024].\n");

    let before = codes(&client.diagnostics_for(&uri));
    assert!(
        before.iter().any(|code| code == "extension-disabled"),
        "expected an extension-disabled diagnostic first, got {before:?}"
    );

    let response = client.request_raw(
        "workspace/executeCommand",
        json!({
            "command": "pandocmd.enableExtension",
            "arguments": [{ "extension": "citations" }],
        }),
    );
    assert!(response.error.is_none(), "{:?}", response.error);

    let after = codes(&client.diagnostics_for(&uri));
    assert!(
        !after.iter().any(|code| code == "extension-disabled"),
        "extension-disabled diagnostics must clear after the command, got {after:?}"
    );
    assert!(client.notifications.iter().any(|params| params
        .get("message")
        .and_then(|message| message.as_str())
        .is_some_and(|message| message.contains("citations"))));
}

#[test]
fn execute_command_rejects_unknown_command_and_arguments() {
    let mut client = TestClient::start(None);

    let response = client.request_raw(
        "workspace/executeCommand",
        json!({ "command": "pandocmd.bogus" }),
    );
    assert!(response.error.is_some());

    let response = client.request_raw(
        "workspace/executeCommand",
        json!({ "command": "pandocmd.enableExtension", "arguments": [] }),
    );
    assert!(response.error.is_some());
}

#[test]
fn rename_updates_definitions_and_references() {
    let uri = test_uri("rename");
    let mut client = TestClient::start(None);
    client.open(&uri, "# Target {#sec-target}\n\nSee [x](#sec-target).\n");

    let rename: serde_json::Value = client.request::<Rename>(json!({
        "textDocument": { "uri": uri },
        "position": Position::new(0, 14),
        "newName": "sec-renamed",
    }));
    let edits = rename["changes"][uri.as_str()].as_array().unwrap();
    assert_eq!(edits.len(), 2, "id + link: {rename}");
    assert!(edits
        .iter()
        .all(|edit| edit["newText"].as_str() == Some("sec-renamed")));

    let prepare: serde_json::Value = client
        .request::<PrepareRenameRequest>(TestClient::position_params(&uri, Position::new(0, 14)));
    // PrepareRenameResponse::Range is untagged: the response IS the range.
    assert_eq!(prepare["start"]["line"], 0);
}

#[test]
fn rename_rejects_automatic_identifiers() {
    let uri = test_uri("rename-auto");
    let mut client = TestClient::start(None);
    client.open(&uri, "# Automatic Heading\n");

    let response = client.request_raw(
        Rename::METHOD,
        json!({
            "textDocument": { "uri": uri },
            "position": Position::new(0, 5),
            "newName": "new",
        }),
    );
    assert!(response.error.is_some(), "renaming auto anchors must fail");
}

#[test]
fn document_links_resolve_relative_targets() {
    let uri = test_uri("links");
    let mut client = TestClient::start(None);
    client.open(
        &uri,
        "See [docs](https://example.com) and [local](./other.md) and ![img](pic.png).\n",
    );

    let links: serde_json::Value = client.request::<DocumentLinkRequest>(json!({
        "textDocument": { "uri": uri }
    }));
    let links = links.as_array().unwrap();
    assert_eq!(links.len(), 3);
    let targets: Vec<&str> = links
        .iter()
        .filter_map(|link| link["target"].as_str())
        .collect();
    assert!(targets.contains(&"https://example.com/"));
    assert!(targets.iter().any(|target| target.ends_with("/other.md")));
    assert!(targets.iter().any(|target| target.ends_with("/pic.png")));
}

#[test]
fn semantic_tokens_encode_headings_and_citations() {
    let uri = test_uri("semantic-tokens");
    let mut client = TestClient::start(None);
    client.open(&uri, "# Head\n\nSee [@key] and [^fn].\n\n[^fn]: x\n");

    let tokens: serde_json::Value = client.request::<SemanticTokensFullRequest>(json!({
        "textDocument": { "uri": uri }
    }));
    let data = tokens["data"].as_array().unwrap();
    assert!(!data.is_empty(), "tokens: {tokens}");
    // The data array is a flat u32 sequence; the first token starts at the
    // beginning of the first line.
    assert_eq!(data[0].as_u64(), Some(0), "first delta_line");
    assert_eq!(data[1].as_u64(), Some(0), "first delta_start");
    assert!(data[2].as_u64().is_some_and(|len| len > 0), "length");
}

#[test]
fn semantic_tokens_never_cross_lines() {
    // A link whose text wraps onto the next line produces an analysis range
    // that spans two lines; LSP semantic tokens cannot, so the encoded
    // token must stay within the first line.
    let uri = test_uri("semantic-wrapped");
    let mut client = TestClient::start(None);
    client.open(
        &uri,
        "# Title\n\nSee [the wrapped\nlink](https://example.com) now.\n",
    );

    let tokens: serde_json::Value = client.request::<SemanticTokensFullRequest>(json!({
        "textDocument": { "uri": uri }
    }));
    let data = tokens["data"].as_array().unwrap();
    let line_lengths = [7usize, 0, 17, 32, 0];
    let mut line = 0u64;
    let mut character = 0u64;
    let mut decoded = 0usize;
    for chunk in data.chunks_exact(5) {
        let delta_line = chunk[0].as_u64().unwrap();
        let delta_start = chunk[1].as_u64().unwrap();
        let length = chunk[2].as_u64().unwrap();
        line += delta_line;
        character = if delta_line == 0 {
            character + delta_start
        } else {
            delta_start
        };
        let limit = line_lengths[line as usize];
        assert!(
            character + length <= limit as u64,
            "token at {line}:{character}+{length} escapes the line (length {limit})"
        );
        decoded += 1;
    }
    assert!(decoded >= 2, "expected heading and link tokens: {tokens}");
}

#[test]
fn document_links_cover_wrapped_links_and_continuation_definitions() {
    let uri = test_uri("links-wrapped");
    let mut client = TestClient::start(None);
    client.open(
        &uri,
        "See [the \ndocs](https://example.com/wrapped).\n\n[ref]:\n  https://example.com/continued \"Title\"\n\nUse [ref].\n",
    );

    let links: serde_json::Value = client.request::<DocumentLinkRequest>(json!({
        "textDocument": { "uri": uri }
    }));
    let links = links.as_array().unwrap();
    let wrapped = links
        .iter()
        .find(|link| link["target"].as_str().unwrap().ends_with("/wrapped"))
        .expect("wrapped inline link");
    assert_eq!(wrapped["range"]["start"]["line"], 1, "range: {wrapped}");
    assert_eq!(
        wrapped["range"]["start"]["character"], 6,
        "range: {wrapped}"
    );
    assert_eq!(wrapped["range"]["end"]["character"], 33, "range: {wrapped}");

    let continued = links
        .iter()
        .find(|link| link["target"].as_str().unwrap().ends_with("/continued"))
        .expect("continuation definition link");
    assert_eq!(continued["range"]["start"]["line"], 4, "range: {continued}");
    assert_eq!(
        continued["range"]["start"]["character"], 2,
        "range: {continued}"
    );
    assert_eq!(
        continued["range"]["end"]["character"], 31,
        "range: {continued}"
    );
}

#[test]
fn trailing_definition_at_eof_does_not_flag_references() {
    let uri = test_uri("def-eof");
    let mut client = TestClient::start(None);
    client.open(&uri, "Use [foo] here.\n\n[foo]:\n");

    let diagnostics = client.diagnostics_for(&uri);
    assert!(
        !codes(&diagnostics)
            .iter()
            .any(|code| code == "unresolved-reference"),
        "diagnostics: {diagnostics:?}"
    );
}

#[test]
fn folding_ranges_cover_metadata_and_divs() {
    let uri = test_uri("folding");
    let mut client = TestClient::start(None);
    client.open(
        &uri,
        "---\ntitle: T\n---\n\n# Intro\npara\n\n::: {.note}\nbody\n:::\n",
    );

    let ranges: serde_json::Value = client.request::<FoldingRangeRequest>(json!({
        "textDocument": { "uri": uri }
    }));
    let ranges = ranges.as_array().unwrap();
    let spans: Vec<(u64, u64)> = ranges
        .iter()
        .map(|range| {
            (
                range["startLine"].as_u64().unwrap(),
                range["endLine"].as_u64().unwrap(),
            )
        })
        .collect();
    assert!(spans.contains(&(0, 2)), "metadata folds: {spans:?}");
    assert!(spans.contains(&(7, 9)), "div folds: {spans:?}");
}

#[test]
fn document_symbols_list_headings_with_anchor_details() {
    let uri = test_uri("symbols");
    let mut client = TestClient::start(None);
    client.open(&uri, "# One\n\ntext\n\n## Two {#custom}\n");

    let symbols: serde_json::Value = client.request::<DocumentSymbolRequest>(json!({
        "textDocument": { "uri": uri }
    }));
    let symbols = symbols.as_array().unwrap();
    assert_eq!(symbols.len(), 2);
    assert_eq!(symbols[0]["detail"], "#one");
    assert_eq!(symbols[1]["detail"], "#custom");
}

#[test]
fn close_document_clears_diagnostics() {
    let uri = test_uri("close");
    let mut client = TestClient::start(None);
    client.open(&uri, "Broken [missing][nope]\n");
    assert!(!client.diagnostics_for(&uri).is_empty());
    client.close(&uri);
    assert!(client.diagnostics_for(&uri).is_empty());
}

#[test]
fn unknown_requests_return_method_not_found() {
    let uri = test_uri("unknown");
    let mut client = TestClient::start(None);
    client.open(&uri, "# x\n");

    let response = client.request_raw(
        "textDocument/typeDefinition",
        json!({
            "textDocument": { "uri": uri },
            "position": Position::new(0, 0),
        }),
    );
    let error = response.error.expect("expected MethodNotFound");
    assert_eq!(error.code, lsp_server::ErrorCode::MethodNotFound as i32);
}

#[test]
fn gflavor_uses_gfm_identifiers_via_config() {
    let uri = test_uri("gfm");
    let mut client = TestClient::start(Some(json!({
        "extensions": { "flavor": "gfm" }
    })));
    client.open(
        &uri,
        "# 1. Introduction\n\nBack to [1. Introduction](#1-introduction).\n",
    );

    let diagnostics = client.diagnostics_for(&uri);
    assert!(
        !codes(&diagnostics)
            .iter()
            .any(|code| code == "unresolved-heading"),
        "gfm slug 1-introduction should resolve, diagnostics: {diagnostics:?}"
    );
}

#[test]
fn did_save_runs_pandoc_validation_when_enabled() {
    let pandoc_available = std::process::Command::new("pandoc")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !pandoc_available {
        eprintln!("skipping: pandoc not installed");
        return;
    }

    let uri = test_uri("pandoc-save");
    let mut client = TestClient::start(Some(json!({
        "diagnostics": { "pandoc": "onSave" }
    })));
    client.open(&uri, "---\nbad yaml: [\n: : x\n---\n");
    // Default is off until save: no pandoc diagnostics yet.
    assert!(!client
        .diagnostics_for(&uri)
        .iter()
        .any(|diagnostic| diagnostic.source.as_deref() == Some("pandoc")));

    client.notify(
        lsp_types::notification::DidSaveTextDocument::METHOD,
        json!({ "textDocument": { "uri": uri } }),
    );
    client.drain(Duration::from_millis(500));

    let diagnostics = client.diagnostics_for(&uri);
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.source.as_deref() == Some("pandoc")),
        "expected pandoc diagnostics after save, got {diagnostics:?}"
    );
}
