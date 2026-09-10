//! LSP server (stdio). Wave a: document loop. Wave b: navigation / edit. Wave c: intel / format / commands.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{json, Value};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, ClientSocket, LanguageServer, LspService, Server};

use crate::catalog::{command_options, option_doc};
use crate::diagnostic::{check_file, check_in_workspace};
use crate::explain;
use crate::format::{format_range, format_text};
use crate::lexer::{tokenize, TokenKind};
use crate::model::{Decl, Equation, Model};
use crate::model_diff::compare_models;
use crate::model_info::{
    assigned_number, classify_variable_timing, format_structure_lens, format_timing_line,
    structure_summary, TimingClass,
};
use crate::preprocessor::{
    find_preprocessor, reconcile_diagnostics, result_to_structured, run_preprocessor,
    run_workspace_preprocessor, DEFAULT_TIMEOUT, MISSING_BINARY_MESSAGE,
};
use crate::refs::{ident_at, is_legal_ident, occurrences, option_command_at};
use crate::span::{LineIndex, Span};
use crate::workspace::Workspace;
use crate::{Severity, VERSION};

const DYNARE_KEYWORDS: &[(&str, &str)] = &[
    ("var", "Declare endogenous variables"),
    ("varexo", "Declare exogenous variables"),
    ("parameters", "Declare parameters"),
    ("model", "Begin model equation block"),
    ("end", "End a block"),
    ("steady_state_model", "Define steady state computation"),
    ("initval", "Set initial values for steady state computation"),
    ("endval", "Set terminal values"),
    ("shocks", "Define shock processes"),
    ("steady", "Compute the steady state"),
    ("check", "Check Blanchard-Kahn conditions"),
    ("stoch_simul", "Compute stochastic simulation"),
    ("simul", "Compute deterministic simulation"),
    ("resid", "Compute residuals of model equations"),
    ("estimated_params", "Define parameters to estimate"),
    ("estimation", "Run Bayesian or ML estimation"),
    ("varobs", "Declare observed variables"),
    ("calib_smoother", "Run the calibrated smoother"),
    ("forecast", "Compute forecasts"),
    ("osr", "Optimal simple rules"),
    ("ramsey_model", "Ramsey optimal policy model"),
    ("planner_objective", "Define planner's objective for Ramsey"),
    ("identification", "Run identification analysis"),
    ("sensitivity", "Run sensitivity analysis"),
];

const BUILTIN_FNS: &[(&str, &str)] = &[
    ("exp", "Exponential function exp(x)"),
    ("log", "Natural logarithm log(x)"),
    ("ln", "Natural logarithm ln(x)"),
    ("sqrt", "Square root sqrt(x)"),
    ("abs", "Absolute value abs(x)"),
    ("sin", "Sine function"),
    ("cos", "Cosine function"),
];

const OUT_CODES: &[&str] = &[
    "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
];

struct OpenDoc {
    text: String,
    version: i32,
    diagnostics: Vec<Diagnostic>,
    library: Vec<crate::Diagnostic>,
}

struct Inner {
    docs: HashMap<Url, OpenDoc>,
    workspace: Workspace,
    format_indent_unit: String,
    preprocessor_path: Option<String>,
    search_paths: Vec<PathBuf>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            docs: HashMap::new(),
            workspace: Workspace::new(),
            format_indent_unit: "\t".into(),
            preprocessor_path: None,
            search_paths: Vec::new(),
        }
    }
}

/// Language server backend (document overlay + diagnostics).
pub struct Backend {
    client: Client,
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Backend").finish_non_exhaustive()
    }
}

impl Backend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            inner: Mutex::new(Inner::default()),
        }
    }

    fn lock_inner(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn upsert(&self, uri: Url, text: String, version: i32) -> (Url, i32, Vec<Diagnostic>) {
        let mut inner = self.lock_inner();
        inner.workspace.update_document(uri.as_str(), &text);
        let library = check_in_workspace(&mut inner.workspace, uri.as_str());
        let diagnostics = library_to_lsp(&text, &library);
        inner.docs.insert(
            uri.clone(),
            OpenDoc {
                text,
                version,
                diagnostics: diagnostics.clone(),
                library,
            },
        );
        (uri, version, diagnostics)
    }

    fn save_reconcile(&self, uri: Url, text: String, version: i32) -> (Url, i32, Vec<Diagnostic>) {
        let (configured, source_dir, own, files) = {
            let mut inner = self.lock_inner();
            inner.workspace.update_document(uri.as_str(), &text);
            let own = check_in_workspace(&mut inner.workspace, uri.as_str());
            let configured = inner.preprocessor_path.clone();
            let source_dir = uri.to_file_path().ok().and_then(|p| file_parent_dir(&p));
            let files = open_overlay_files(&inner, &uri, &text);
            (configured, source_dir, own, files)
        };
        let configured_path = configured.as_ref().map(PathBuf::from);
        let pre = find_preprocessor(configured_path.as_deref()).map(|pp| {
            if files.len() > 1 {
                run_workspace_preprocessor(uri.as_str(), &files, &pp, DEFAULT_TIMEOUT)
            } else {
                run_preprocessor(&text, &pp, source_dir.as_deref(), DEFAULT_TIMEOUT)
            }
        });
        let library = reconcile_diagnostics(&own, pre.as_ref());
        let diagnostics = library_to_lsp(&text, &library);
        {
            let mut inner = self.lock_inner();
            inner.docs.insert(
                uri.clone(),
                OpenDoc {
                    text,
                    version,
                    diagnostics: diagnostics.clone(),
                    library,
                },
            );
        }
        (uri, version, diagnostics)
    }

    fn run_preprocessor_command(&self, arguments: &[Value]) -> Value {
        let Some(uri) = extract_command_uri(arguments) else {
            return json!({"success": false, "message": "Missing or invalid URI argument"});
        };
        let (text, configured, source_dir, files) = {
            let inner = self.lock_inner();
            let Some(doc) = inner.docs.get(&uri) else {
                return json!({"success": false, "message": "Document not available"});
            };
            let configured = inner.preprocessor_path.clone();
            let source_dir = uri.to_file_path().ok().and_then(|p| file_parent_dir(&p));
            let files = open_overlay_files(&inner, &uri, &doc.text);
            (doc.text.clone(), configured, source_dir, files)
        };
        let configured_path = configured.as_ref().map(PathBuf::from);
        let Some(pp) = find_preprocessor(configured_path.as_deref()) else {
            return json!({
                "success": false,
                "message": MISSING_BINARY_MESSAGE,
                "exit_code": null,
                "diagnostics": [],
                "raw_stdout": "",
                "raw_stderr": "",
            });
        };
        let result = if files.len() > 1 {
            run_workspace_preprocessor(uri.as_str(), &files, &pp, DEFAULT_TIMEOUT)
        } else {
            run_preprocessor(&text, &pp, source_dir.as_deref(), DEFAULT_TIMEOUT)
        };
        result_to_structured(&result, &text)
    }

    fn pull_items(&self, uri: &Url) -> Vec<Diagnostic> {
        let inner = self.lock_inner();
        inner
            .docs
            .get(uri)
            .map(|d| d.diagnostics.clone())
            .unwrap_or_default()
    }

    fn hover_at(&self, pos: &TextDocumentPositionParams) -> Option<Hover> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset(&doc.text, span_pos(pos.position));
        let (word, span) = ident_at(&doc.text, byte)?;
        let range = Some(span_range(&index, &doc.text, span));
        if let Some(cmd) = option_command_at(&doc.text, byte) {
            if command_options(&cmd).iter().any(|(n, _)| *n == word) {
                let mut md = format!("**`{cmd}` option**: `{word}`");
                let description = option_doc(&word);
                if !description.is_empty() {
                    md.push_str("\n\n");
                    md.push_str(description);
                }
                return Some(markdown_hover(md, range));
            }
        }
        let model = inner.workspace.get_model(pos.text_document.uri.as_str())?;
        let md = decl_hover_markdown(model, &word)?;
        Some(markdown_hover(md, range))
    }

    fn doc_symbols(&self, uri: &Url) -> Option<Vec<DocumentSymbol>> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(uri)?;
        let model = inner.workspace.get_model(uri.as_str())?;
        Some(document_symbols_for(&doc.text, model))
    }

    fn workspace_symbols(&self, query: &str) -> Vec<SymbolInformation> {
        let inner = self.lock_inner();
        let q = query.to_ascii_lowercase();
        let mut out = Vec::new();
        for (uri, doc) in &inner.docs {
            let Some(model) = inner.workspace.get_model(uri.as_str()) else {
                continue;
            };
            let nested = document_symbols_for(&doc.text, model);
            flatten_workspace_symbols(uri, &nested, None, &q, &mut out);
        }
        out
    }

    fn decl_location(&self, pos: &TextDocumentPositionParams) -> Option<Location> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset(&doc.text, span_pos(pos.position));
        let (word, _) = ident_at(&doc.text, byte)?;
        let model = inner.workspace.get_model(pos.text_document.uri.as_str())?;
        let decl = find_decl(model, &word)?;
        Some(Location::new(
            pos.text_document.uri.clone(),
            span_range(&index, &doc.text, decl.span),
        ))
    }

    fn ident_locations(&self, pos: &TextDocumentPositionParams) -> Option<Vec<Location>> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset(&doc.text, span_pos(pos.position));
        let (word, _) = ident_at(&doc.text, byte)?;
        let locs = occurrences(&doc.text, &word)
            .into_iter()
            .map(|span| {
                Location::new(
                    pos.text_document.uri.clone(),
                    span_range(&index, &doc.text, span),
                )
            })
            .collect::<Vec<_>>();
        if locs.is_empty() {
            None
        } else {
            Some(locs)
        }
    }

    fn ident_highlights(&self, pos: &TextDocumentPositionParams) -> Option<Vec<DocumentHighlight>> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset(&doc.text, span_pos(pos.position));
        let (word, _) = ident_at(&doc.text, byte)?;
        let hits = occurrences(&doc.text, &word)
            .into_iter()
            .map(|span| DocumentHighlight {
                range: span_range(&index, &doc.text, span),
                kind: Some(DocumentHighlightKind::TEXT),
            })
            .collect::<Vec<_>>();
        if hits.is_empty() {
            None
        } else {
            Some(hits)
        }
    }

    fn complete(&self, pos: &TextDocumentPositionParams) -> Option<CompletionResponse> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset(&doc.text, span_pos(pos.position));
        if let Some(cmd) = option_command_at(&doc.text, byte) {
            let items = command_options(&cmd)
                .iter()
                .map(|(name, doc_str)| CompletionItem {
                    label: (*name).into(),
                    kind: Some(CompletionItemKind::PROPERTY),
                    detail: Some(format!("{cmd} option")),
                    documentation: Some(Documentation::String((*doc_str).into())),
                    ..CompletionItem::default()
                })
                .collect::<Vec<_>>();
            if items.is_empty() {
                return None;
            }
            return Some(CompletionResponse::Array(items));
        }
        let model = inner.workspace.get_model(pos.text_document.uri.as_str())?;
        Some(CompletionResponse::Array(default_completions(model)))
    }

    fn prepare_rename_at(&self, pos: &TextDocumentPositionParams) -> Option<Range> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset(&doc.text, span_pos(pos.position));
        let (word, span) = ident_at(&doc.text, byte)?;
        if !is_legal_ident(&word) {
            return None;
        }
        if !is_declared_in_open(&inner, &word) {
            return None;
        }
        Some(span_range(&index, &doc.text, span))
    }

    fn rename_at(&self, pos: &TextDocumentPositionParams, new_name: &str) -> Option<WorkspaceEdit> {
        if !is_legal_ident(new_name) {
            return None;
        }
        let inner = self.lock_inner();
        let doc = inner.docs.get(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset(&doc.text, span_pos(pos.position));
        let (word, _) = ident_at(&doc.text, byte)?;
        if !is_legal_ident(&word) || !is_declared_in_open(&inner, &word) {
            return None;
        }
        let mut changes = HashMap::new();
        for (uri, open) in &inner.docs {
            let idx = LineIndex::new(&open.text);
            let mut spans = occurrences(&open.text, &word);
            if spans.is_empty() {
                continue;
            }
            spans.sort_by_key(|s| std::cmp::Reverse(s.start));
            let edits = spans
                .into_iter()
                .map(|span| TextEdit {
                    range: span_range(&idx, &open.text, span),
                    new_text: new_name.to_string(),
                })
                .collect::<Vec<_>>();
            if !edits.is_empty() {
                changes.insert(uri.clone(), edits);
            }
        }
        if changes.is_empty() {
            None
        } else {
            Some(WorkspaceEdit {
                changes: Some(changes),
                ..WorkspaceEdit::default()
            })
        }
    }

    fn quick_fixes(&self, params: &CodeActionParams) -> Option<CodeActionResponse> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(&params.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let mut actions = Vec::new();
        for lib in &doc.library {
            let Some(fix) = &lib.fix else {
                continue;
            };
            let diag_range = span_range(&index, &doc.text, lib.span);
            if !ranges_overlap(diag_range, params.range) {
                continue;
            }
            let title = fix_title(lib);
            let edit = TextEdit {
                range: Range::new(
                    Position::new(fix.start_line, fix.start_char),
                    Position::new(fix.end_line, fix.end_char),
                ),
                new_text: fix.new_text.clone(),
            };
            let mut changes = HashMap::new();
            changes.insert(params.text_document.uri.clone(), vec![edit]);
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title,
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: None,
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..WorkspaceEdit::default()
                }),
                command: None,
                is_preferred: Some(true),
                disabled: None,
                data: None,
            }));
        }
        if actions.is_empty() {
            None
        } else {
            Some(actions)
        }
    }

    fn linked_ranges(&self, pos: &TextDocumentPositionParams) -> Option<LinkedEditingRanges> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(&pos.text_document.uri)?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset(&doc.text, span_pos(pos.position));
        let (word, _) = ident_at(&doc.text, byte)?;
        if !is_declared_in_open(&inner, &word) {
            return None;
        }
        let spans = occurrences(&doc.text, &word);
        if spans.len() < 2 {
            return None;
        }
        Some(LinkedEditingRanges {
            ranges: spans
                .into_iter()
                .map(|span| span_range(&index, &doc.text, span))
                .collect(),
            word_pattern: None,
        })
    }

    fn apply_settings(&self, settings: &Value) {
        let dynare = settings.get("dynare").unwrap_or(settings);
        let Some(obj) = dynare.as_object() else {
            return;
        };
        let mut inner = self.lock_inner();
        if let Some(value) = obj.get("formatIndent") {
            if let Some(unit) = parse_format_indent(value) {
                inner.format_indent_unit = unit;
            }
        }
        if let Some(value) = obj.get("preprocessorPath") {
            if let Some(s) = value.as_str() {
                let trimmed = s.trim();
                inner.preprocessor_path = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                };
            }
        }
        let mut paths_changed = false;
        if let Some(raw) = obj.get("searchPaths") {
            if let Some(paths) = parse_path_array(raw) {
                inner.search_paths = paths;
                paths_changed = true;
            }
        }
        if let Some(raw) = obj.get("searchPathsByRoot") {
            if let Some(extras) = parse_search_paths_by_root(raw) {
                for extra in extras {
                    if !inner.search_paths.iter().any(|p| p == &extra) {
                        inner.search_paths.push(extra);
                        paths_changed = true;
                    }
                }
            }
        }
        if paths_changed {
            let paths = inner.search_paths.clone();
            inner.workspace.set_search_paths(paths);
        }
    }

    fn inlay_hints(&self, uri: &Url, range: Range) -> Option<Vec<InlayHint>> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(uri)?;
        let model = inner.workspace.get_model(uri.as_str())?;
        let index = LineIndex::new(&doc.text);
        let mut hints = Vec::new();
        for a in &model.param_assignments {
            let start = index.position(&doc.text, a.span.start);
            if !pos_in_range(Position::new(start.line, start.character), range) {
                continue;
            }
            let name = model.name(a.name);
            let Some(n) = assigned_number(model, name) else {
                continue;
            };
            let end = index.position(&doc.text, a.span.end);
            hints.push(InlayHint {
                position: Position::new(end.line, end.character),
                label: InlayHintLabel::String(format!(" → {}", g_format(n))),
                kind: Some(InlayHintKind::TYPE),
                text_edits: None,
                tooltip: None,
                padding_left: Some(true),
                padding_right: None,
                data: None,
            });
        }
        if hints.is_empty() {
            None
        } else {
            Some(hints)
        }
    }

    fn folding_ranges(&self, uri: &Url) -> Option<Vec<FoldingRange>> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(uri)?;
        let model = inner.workspace.get_model(uri.as_str())?;
        let index = LineIndex::new(&doc.text);
        let mut ranges = Vec::new();
        let blocks = [
            (model.model_block, "model"),
            (model.ss_block, "steady_state_model"),
            (model.initval_block, "initval"),
            (model.endval_block, "endval"),
            (model.shocks_block, "shocks"),
        ];
        for (span, keyword) in blocks {
            let Some(span) = span else {
                continue;
            };
            let start = index.position(&doc.text, span.start);
            let end = index.position(&doc.text, span.end);
            if start.line < end.line {
                ranges.push(FoldingRange {
                    start_line: start.line,
                    start_character: None,
                    end_line: end.line,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Region),
                    collapsed_text: Some(format!("{keyword} ... end;")),
                });
            }
        }
        let mut stack: Vec<&crate::model::MacroDirective> = Vec::new();
        const COND: &[&str] = &["if", "ifdef", "ifndef"];
        for directive in &model.macro_directives {
            let kind = directive.kind.as_str();
            if COND.contains(&kind) || kind == "for" {
                stack.push(directive);
            } else if kind == "endif"
                && stack
                    .last()
                    .is_some_and(|d| COND.contains(&d.kind.as_str()))
            {
                let opener = stack.pop().unwrap();
                let start = index.position(&doc.text, opener.span.start);
                let end = index.position(&doc.text, directive.span.end);
                if start.line < end.line {
                    ranges.push(FoldingRange {
                        start_line: start.line,
                        start_character: None,
                        end_line: end.line,
                        end_character: None,
                        kind: Some(FoldingRangeKind::Region),
                        collapsed_text: Some(format!("@#{} ... @#endif", opener.kind)),
                    });
                }
            } else if kind == "endfor" && stack.last().is_some_and(|d| d.kind == "for") {
                let opener = stack.pop().unwrap();
                let start = index.position(&doc.text, opener.span.start);
                let end = index.position(&doc.text, directive.span.end);
                if start.line < end.line {
                    ranges.push(FoldingRange {
                        start_line: start.line,
                        start_character: None,
                        end_line: end.line,
                        end_character: None,
                        kind: Some(FoldingRangeKind::Region),
                        collapsed_text: Some("@#for ... @#endfor".into()),
                    });
                }
            }
        }
        for (start_line, end_line) in block_comment_folds(&doc.text) {
            ranges.push(FoldingRange {
                start_line,
                start_character: None,
                end_line,
                end_character: None,
                kind: Some(FoldingRangeKind::Comment),
                collapsed_text: None,
            });
        }
        if ranges.is_empty() {
            None
        } else {
            Some(ranges)
        }
    }

    fn selection_ranges(&self, uri: &Url, positions: &[Position]) -> Option<Vec<SelectionRange>> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(uri)?;
        let model = inner.workspace.get_model(uri.as_str())?;
        let index = LineIndex::new(&doc.text);
        let file_range = full_document_range(&doc.text);
        let mut out = Vec::new();
        for &pos in positions {
            let mut chain = Vec::new();
            let byte = index.offset(&doc.text, span_pos(pos));
            if let Some((_, span)) = ident_at(&doc.text, byte) {
                chain.push(span_range(&index, &doc.text, span));
            }
            if let Some(eq) = equation_at(model, &index, &doc.text, pos) {
                chain.push(span_range(&index, &doc.text, eq.span));
            }
            for span in [
                model.model_block,
                model.ss_block,
                model.initval_block,
                model.endval_block,
                model.shocks_block,
            ]
            .into_iter()
            .flatten()
            {
                let rng = span_range(&index, &doc.text, span);
                if range_has_pos(rng, pos) {
                    chain.push(rng);
                }
            }
            chain.push(file_range);
            chain.sort_by(|a, b| {
                (b.start.line, b.start.character, a.end.line, a.end.character).cmp(&(
                    a.start.line,
                    a.start.character,
                    b.end.line,
                    b.end.character,
                ))
            });
            let mut nested: Vec<Range> = Vec::new();
            for rng in chain {
                if nested.is_empty() || range_strictly_contains(rng, nested[nested.len() - 1]) {
                    nested.push(rng);
                }
            }
            let mut node: Option<SelectionRange> = None;
            for rng in nested.into_iter().rev() {
                node = Some(SelectionRange {
                    range: rng,
                    parent: node.map(Box::new),
                });
            }
            out.push(node.unwrap_or(SelectionRange {
                range: Range::new(pos, pos),
                parent: None,
            }));
        }
        Some(out)
    }

    fn document_links(&self, uri: &Url) -> Option<Vec<DocumentLink>> {
        let mut inner = self.lock_inner();
        let text = inner.docs.get(uri)?.text.clone();
        let includes = {
            let model = inner.workspace.get_model(uri.as_str())?;
            if model.includes.is_empty() {
                return None;
            }
            model.includes.clone()
        };
        let index = LineIndex::new(&text);
        let records = inner
            .workspace
            .include_records(uri.as_str())
            .cloned()
            .unwrap_or_default();
        let mut links = Vec::new();
        for inc in &includes {
            let resolved = records.resolved.iter().find(|r| {
                (r.span.start == inc.span.start && r.span.end == inc.span.end)
                    || r.filename == inc.filename
            });
            let Some(resolved) = resolved else {
                continue;
            };
            let Ok(target) = Url::from_file_path(&resolved.path) else {
                continue;
            };
            links.push(DocumentLink {
                range: span_range(&index, &text, inc.span),
                target: Some(target),
                tooltip: Some(format!("Open {}", inc.filename)),
                data: None,
            });
        }
        if links.is_empty() {
            None
        } else {
            Some(links)
        }
    }

    fn semantic_tokens(&self, uri: &Url, range: Option<Range>) -> Option<SemanticTokens> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(uri)?;
        let model = inner.workspace.get_model(uri.as_str())?;
        let index = LineIndex::new(&doc.text);
        let endo: HashMap<String, ()> = model
            .endogenous
            .iter()
            .map(|d| (model.name(d.name).to_string(), ()))
            .collect();
        let exo: HashMap<String, ()> = model
            .exogenous
            .iter()
            .map(|d| (model.name(d.name).to_string(), ()))
            .collect();
        let params: HashMap<String, ()> = model
            .parameters
            .iter()
            .map(|d| (model.name(d.name).to_string(), ()))
            .collect();
        let local: HashMap<String, ()> = model
            .equations
            .iter()
            .filter(|eq| eq.is_local || eq.model_local)
            .filter_map(|eq| {
                let name = eq.lhs.trim();
                if name.is_empty() {
                    None
                } else {
                    Some((name.to_string(), ()))
                }
            })
            .collect();
        let timing = classify_variable_timing(model);
        let mut decl_starts = HashMap::new();
        for d in model
            .endogenous
            .iter()
            .chain(model.exogenous.iter())
            .chain(model.parameters.iter())
        {
            decl_starts.insert(d.span.start, ());
        }
        let mut raw = Vec::new();
        for tok in tokenize(&doc.text) {
            if tok.kind != TokenKind::Ident {
                continue;
            }
            let name = tok.text(&doc.text);
            let start = index.position(&doc.text, tok.span.start);
            if let Some(range) = range {
                if !pos_in_range_half_open(Position::new(start.line, start.character), range) {
                    continue;
                }
            }
            let ttype = if local.contains_key(name) {
                3u32
            } else if endo.contains_key(name) {
                0
            } else if exo.contains_key(name) {
                1
            } else if params.contains_key(name) {
                2
            } else {
                continue;
            };
            let mut mods = 0u32;
            if decl_starts.contains_key(&tok.span.start) {
                mods |= 1;
            }
            if ttype == 0 {
                if let Some(info) = timing.get(name) {
                    match info.class {
                        TimingClass::ForwardLooking | TimingClass::Mixed => mods |= 1 << 1,
                        _ => {}
                    }
                    match info.class {
                        TimingClass::Predetermined | TimingClass::Mixed => mods |= 1 << 2,
                        _ => {}
                    }
                }
            }
            let end = index.position(&doc.text, tok.span.end);
            let length = if start.line == end.line {
                end.character.saturating_sub(start.character)
            } else {
                tok.span.end - tok.span.start
            };
            raw.push((start.line, start.character, length, ttype, mods));
        }
        raw.sort_by_key(|t| (t.0, t.1));
        let mut data = Vec::new();
        let mut prev_line = 0u32;
        let mut prev_col = 0u32;
        for (line, col, length, ttype, mods) in raw {
            let delta_line = line - prev_line;
            let delta_start = if delta_line == 0 { col - prev_col } else { col };
            data.push(SemanticToken {
                delta_line,
                delta_start,
                length,
                token_type: ttype,
                token_modifiers_bitset: mods,
            });
            prev_line = line;
            prev_col = col;
        }
        Some(SemanticTokens {
            result_id: None,
            data,
        })
    }

    fn code_lenses(&self, uri: &Url) -> Option<Vec<CodeLens>> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(uri)?;
        let model = inner.workspace.get_model(uri.as_str())?;
        let block = model.model_block?;
        if model.endogenous.is_empty() {
            return None;
        }
        let index = LineIndex::new(&doc.text);
        let start = index.position(&doc.text, block.start);
        let summary = structure_summary(model);
        let title = format_structure_lens(&summary);
        let range = Range::new(Position::new(start.line, 0), Position::new(start.line, 0));
        Some(vec![
            CodeLens {
                range,
                command: Some(Command {
                    title,
                    command: String::new(),
                    arguments: None,
                }),
                data: None,
            },
            CodeLens {
                range,
                command: Some(Command {
                    title: "Run preprocessor".into(),
                    command: "dynare/runPreprocessor".into(),
                    arguments: Some(vec![json!({"uri": uri.as_str()})]),
                }),
                data: None,
            },
        ])
    }

    fn format_document(&self, uri: &Url) -> Option<Vec<TextEdit>> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(uri)?;
        let formatted = format_text(&doc.text, &inner.format_indent_unit)?;
        Some(vec![full_document_edit(&doc.text, formatted)])
    }

    fn format_line_range(&self, uri: &Url, range: Range) -> Option<Vec<TextEdit>> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(uri)?;
        let mut end_line = range.end.line;
        if range.end.character == 0 && end_line > range.start.line {
            end_line -= 1;
        }
        let (start_line, end_line, replacement) = format_range(
            &doc.text,
            range.start.line,
            end_line,
            &inner.format_indent_unit,
        )?;
        Some(vec![line_range_edit(
            &doc.text,
            start_line,
            end_line,
            replacement,
        )])
    }

    fn execute(&self, command: &str, arguments: &[Value]) -> Value {
        match command {
            "dynare/explainDiagnostic" => explain_command(arguments),
            "dynare/compareModels" => self.compare_command(arguments),
            "dynare/runPreprocessor" => self.run_preprocessor_command(arguments),
            _ => json!({"error": format!("unknown command {command}"), "code": "UNKNOWN_COMMAND"}),
        }
    }

    fn compare_command(&self, arguments: &[Value]) -> Value {
        let (uri_a, uri_b) = match parse_compare_args(arguments) {
            Ok(pair) => pair,
            Err(v) => return v,
        };
        let inner = self.lock_inner();
        let Some(model_a) = inner.workspace.get_model(uri_a.as_str()) else {
            return json!({"error": format!("No parsed model for uri_a: {uri_a}"), "code": "URI_A_NOT_FOUND"});
        };
        let Some(model_b) = inner.workspace.get_model(uri_b.as_str()) else {
            return json!({"error": format!("No parsed model for uri_b: {uri_b}"), "code": "URI_B_NOT_FOUND"});
        };
        compare_models(model_a, model_b).to_json()
    }

    fn prepare_hierarchy(
        &self,
        pos: &TextDocumentPositionParams,
    ) -> Option<Vec<CallHierarchyItem>> {
        let inner = self.lock_inner();
        let doc = inner.docs.get(&pos.text_document.uri)?;
        let model = inner.workspace.get_model(pos.text_document.uri.as_str())?;
        let index = LineIndex::new(&doc.text);
        let byte = index.offset(&doc.text, span_pos(pos.position));
        let mut items = Vec::new();
        if let Some((word, _)) = ident_at(&doc.text, byte) {
            if find_decl(model, &word).is_some() {
                items.push(ch_variable_item(
                    model,
                    &index,
                    &doc.text,
                    &pos.text_document.uri,
                    &word,
                ));
            }
        }
        if let Some(eq) = equation_at(model, &index, &doc.text, pos.position) {
            items.push(ch_equation_item(
                model,
                &index,
                &doc.text,
                &pos.text_document.uri,
                eq,
            ));
        }
        if items.is_empty() {
            None
        } else {
            Some(items)
        }
    }

    fn incoming(&self, item: &CallHierarchyItem) -> Option<Vec<CallHierarchyIncomingCall>> {
        let data = item.data.as_ref()?;
        if data.get("dynare").and_then(|v| v.as_str()) != Some("variable") {
            return Some(Vec::new());
        }
        let name = data.get("name")?.as_str()?.to_string();
        let inner = self.lock_inner();
        let doc = inner.docs.get(&item.uri)?;
        let model = inner.workspace.get_model(item.uri.as_str())?;
        let index = LineIndex::new(&doc.text);
        let mut calls = Vec::new();
        for eq in &model.equations {
            if eq.is_local || eq.model_local {
                continue;
            }
            let in_eq: Vec<Span> = model
                .ident_refs(eq)
                .into_iter()
                .filter(|r| model.name(r.name) == name)
                .map(|r| r.span)
                .collect();
            if in_eq.is_empty() {
                continue;
            }
            calls.push(CallHierarchyIncomingCall {
                from: ch_equation_item(model, &index, &doc.text, &item.uri, eq),
                from_ranges: in_eq
                    .into_iter()
                    .map(|s| span_range(&index, &doc.text, s))
                    .collect(),
            });
        }
        Some(calls)
    }

    fn outgoing(&self, item: &CallHierarchyItem) -> Option<Vec<CallHierarchyOutgoingCall>> {
        let data = item.data.as_ref()?;
        if data.get("dynare").and_then(|v| v.as_str()) != Some("equation") {
            return Some(Vec::new());
        }
        let start = data.get("start")?.as_u64()? as u32;
        let inner = self.lock_inner();
        let doc = inner.docs.get(&item.uri)?;
        let model = inner.workspace.get_model(item.uri.as_str())?;
        let eq = model.equations.iter().find(|e| e.span.start == start)?;
        let index = LineIndex::new(&doc.text);
        let mut by_name: HashMap<String, Vec<Span>> = HashMap::new();
        for r in model.ident_refs(eq) {
            let n = model.name(r.name).to_string();
            if find_decl(model, &n).is_none() {
                continue;
            }
            by_name.entry(n).or_default().push(r.span);
        }
        let mut names: Vec<String> = by_name.keys().cloned().collect();
        names.sort();
        let mut calls = Vec::new();
        for name in names {
            let occs = by_name.remove(&name).unwrap_or_default();
            calls.push(CallHierarchyOutgoingCall {
                to: ch_variable_item(model, &index, &doc.text, &item.uri, &name),
                from_ranges: occs
                    .into_iter()
                    .map(|s| span_range(&index, &doc.text, s))
                    .collect(),
            });
        }
        Some(calls)
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        if let Some(opts) = params.initialization_options {
            self.apply_settings(&opts);
        }
        Ok(initialize_result())
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "dygnosis initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        let (uri, version, diagnostics) = self.upsert(doc.uri, doc.text, doc.version);
        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let Some(change) = params.content_changes.last() else {
            return;
        };
        if change.range.is_some() {
            return;
        }
        let (uri, version, diagnostics) = self.upsert(
            params.text_document.uri,
            change.text.clone(),
            params.text_document.version,
        );
        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        let snapshot = {
            let inner = self.lock_inner();
            inner.docs.get(&uri).map(|d| (d.text.clone(), d.version))
        };
        let (text, version) = match (params.text, snapshot) {
            (Some(text), Some((_, version))) => (text, version),
            (Some(text), None) => (text, 0),
            (None, Some((text, version))) => (text, version),
            (None, None) => return,
        };
        let (uri, version, diagnostics) = self.save_reconcile(uri, text, version);
        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        {
            let mut inner = self.lock_inner();
            inner.docs.remove(&uri);
            inner.workspace.remove_document(uri.as_str());
        }
        self.client.publish_diagnostics(uri, Vec::new(), None).await;
    }

    async fn diagnostic(
        &self,
        params: DocumentDiagnosticParams,
    ) -> Result<DocumentDiagnosticReportResult> {
        let items = self.pull_items(&params.text_document.uri);
        Ok(DocumentDiagnosticReportResult::Report(
            DocumentDiagnosticReport::Full(RelatedFullDocumentDiagnosticReport {
                related_documents: None,
                full_document_diagnostic_report: FullDocumentDiagnosticReport {
                    result_id: None,
                    items,
                },
            }),
        ))
    }

    async fn workspace_diagnostic(
        &self,
        _: WorkspaceDiagnosticParams,
    ) -> Result<WorkspaceDiagnosticReportResult> {
        let inner = self.lock_inner();
        let items = inner
            .docs
            .iter()
            .map(|(uri, doc)| {
                WorkspaceDocumentDiagnosticReport::Full(WorkspaceFullDocumentDiagnosticReport {
                    uri: uri.clone(),
                    version: Some(i64::from(doc.version)),
                    full_document_diagnostic_report: FullDocumentDiagnosticReport {
                        result_id: None,
                        items: doc.diagnostics.clone(),
                    },
                })
            })
            .collect();
        Ok(WorkspaceDiagnosticReportResult::Report(
            WorkspaceDiagnosticReport { items },
        ))
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        let to_publish: Vec<(Url, i32, Vec<Diagnostic>)> = {
            let mut inner = self.lock_inner();
            for event in &params.changes {
                inner.workspace.remove_document(event.uri.as_str());
            }
            let snapshots: Vec<(Url, String, i32)> = inner
                .docs
                .iter()
                .map(|(uri, doc)| (uri.clone(), doc.text.clone(), doc.version))
                .collect();
            let mut out = Vec::new();
            for (uri, text, version) in snapshots {
                inner.workspace.update_document(uri.as_str(), &text);
                let library = check_in_workspace(&mut inner.workspace, uri.as_str());
                let diagnostics = library_to_lsp(&text, &library);
                if let Some(doc) = inner.docs.get_mut(&uri) {
                    doc.diagnostics = diagnostics.clone();
                    doc.library = library;
                }
                out.push((uri, version, diagnostics));
            }
            out
        };
        for (uri, version, diagnostics) in to_publish {
            self.client
                .publish_diagnostics(uri, diagnostics, Some(version))
                .await;
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        Ok(self.hover_at(&params.text_document_position_params))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        Ok(self
            .doc_symbols(&params.text_document.uri)
            .map(DocumentSymbolResponse::Nested))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<Vec<SymbolInformation>>> {
        Ok(Some(self.workspace_symbols(&params.query)))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        Ok(self
            .decl_location(&params.text_document_position_params)
            .map(GotoDefinitionResponse::Scalar))
    }

    async fn goto_declaration(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        Ok(self
            .decl_location(&params.text_document_position_params)
            .map(GotoDefinitionResponse::Scalar))
    }

    async fn goto_type_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        Ok(self
            .decl_location(&params.text_document_position_params)
            .map(GotoDefinitionResponse::Scalar))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        Ok(self.ident_locations(&params.text_document_position))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        Ok(self.ident_highlights(&params.text_document_position_params))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        Ok(self.complete(&params.text_document_position))
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        Ok(self
            .prepare_rename_at(&params)
            .map(PrepareRenameResponse::Range))
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        Ok(self.rename_at(&params.text_document_position, &params.new_name))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        Ok(self.quick_fixes(&params))
    }

    async fn linked_editing_range(
        &self,
        params: LinkedEditingRangeParams,
    ) -> Result<Option<LinkedEditingRanges>> {
        Ok(self.linked_ranges(&params.text_document_position_params))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        Ok(self.inlay_hints(&params.text_document.uri, params.range))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        Ok(self.folding_ranges(&params.text_document.uri))
    }

    async fn selection_range(
        &self,
        params: SelectionRangeParams,
    ) -> Result<Option<Vec<SelectionRange>>> {
        Ok(self.selection_ranges(&params.text_document.uri, &params.positions))
    }

    async fn document_link(&self, params: DocumentLinkParams) -> Result<Option<Vec<DocumentLink>>> {
        Ok(self.document_links(&params.text_document.uri))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        Ok(self
            .semantic_tokens(&params.text_document.uri, None)
            .map(SemanticTokensResult::Tokens))
    }

    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> Result<Option<SemanticTokensRangeResult>> {
        Ok(self
            .semantic_tokens(&params.text_document.uri, Some(params.range))
            .map(SemanticTokensRangeResult::Tokens))
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        Ok(self.code_lenses(&params.text_document.uri))
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        Ok(self.format_document(&params.text_document.uri))
    }

    async fn range_formatting(
        &self,
        params: DocumentRangeFormattingParams,
    ) -> Result<Option<Vec<TextEdit>>> {
        Ok(self.format_line_range(&params.text_document.uri, params.range))
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<Value>> {
        if params.command == "dynare/runPreprocessor" {
            let value = self.run_preprocessor_command(&params.arguments);
            if value.get("message").and_then(|v| v.as_str()) == Some(MISSING_BINARY_MESSAGE) {
                self.client
                    .show_message(MessageType::WARNING, MISSING_BINARY_MESSAGE)
                    .await;
            }
            return Ok(Some(value));
        }
        Ok(Some(self.execute(&params.command, &params.arguments)))
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        self.apply_settings(&params.settings);
    }

    async fn prepare_call_hierarchy(
        &self,
        params: CallHierarchyPrepareParams,
    ) -> Result<Option<Vec<CallHierarchyItem>>> {
        Ok(self.prepare_hierarchy(&params.text_document_position_params))
    }

    async fn incoming_calls(
        &self,
        params: CallHierarchyIncomingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyIncomingCall>>> {
        Ok(self.incoming(&params.item))
    }

    async fn outgoing_calls(
        &self,
        params: CallHierarchyOutgoingCallsParams,
    ) -> Result<Option<Vec<CallHierarchyOutgoingCall>>> {
        Ok(self.outgoing(&params.item))
    }
}

/// Wave c initialize payload: FULL sync + diagnostic pull + navigation + intel / format / commands.
pub fn initialize_result() -> InitializeResult {
    InitializeResult {
        capabilities: ServerCapabilities {
            text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
            diagnostic_provider: Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
                identifier: None,
                inter_file_dependencies: true,
                workspace_diagnostics: true,
                work_done_progress_options: WorkDoneProgressOptions::default(),
            })),
            hover_provider: Some(HoverProviderCapability::Simple(true)),
            document_symbol_provider: Some(OneOf::Left(true)),
            workspace_symbol_provider: Some(OneOf::Left(true)),
            definition_provider: Some(OneOf::Left(true)),
            declaration_provider: Some(DeclarationCapability::Simple(true)),
            type_definition_provider: Some(TypeDefinitionProviderCapability::Simple(true)),
            references_provider: Some(OneOf::Left(true)),
            document_highlight_provider: Some(OneOf::Left(true)),
            completion_provider: Some(CompletionOptions {
                trigger_characters: Some(vec!["(".into(), ",".into()]),
                ..CompletionOptions::default()
            }),
            rename_provider: Some(OneOf::Right(RenameOptions {
                prepare_provider: Some(true),
                work_done_progress_options: WorkDoneProgressOptions::default(),
            })),
            code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
            linked_editing_range_provider: Some(LinkedEditingRangeServerCapabilities::Simple(true)),
            inlay_hint_provider: Some(OneOf::Left(true)),
            folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
            selection_range_provider: Some(SelectionRangeProviderCapability::Simple(true)),
            document_link_provider: Some(DocumentLinkOptions {
                resolve_provider: Some(false),
                work_done_progress_options: WorkDoneProgressOptions::default(),
            }),
            semantic_tokens_provider: Some(
                SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
                    legend: SemanticTokensLegend {
                        token_types: vec![
                            SemanticTokenType::VARIABLE,
                            SemanticTokenType::TYPE,
                            SemanticTokenType::MACRO,
                            SemanticTokenType::PARAMETER,
                        ],
                        token_modifiers: vec![
                            SemanticTokenModifier::DECLARATION,
                            SemanticTokenModifier::new("forwardLooking"),
                            SemanticTokenModifier::new("predetermined"),
                        ],
                    },
                    range: Some(true),
                    full: Some(SemanticTokensFullOptions::Bool(true)),
                    work_done_progress_options: WorkDoneProgressOptions::default(),
                }),
            ),
            code_lens_provider: Some(CodeLensOptions {
                resolve_provider: Some(false),
            }),
            document_formatting_provider: Some(OneOf::Left(true)),
            document_range_formatting_provider: Some(OneOf::Left(true)),
            execute_command_provider: Some(ExecuteCommandOptions {
                commands: vec![
                    "dynare/explainDiagnostic".into(),
                    "dynare/compareModels".into(),
                    "dynare/runPreprocessor".into(),
                ],
                work_done_progress_options: WorkDoneProgressOptions::default(),
            }),
            call_hierarchy_provider: Some(CallHierarchyServerCapability::Simple(true)),
            ..ServerCapabilities::default()
        },
        server_info: Some(ServerInfo {
            name: "dygnosis".into(),
            version: Some(VERSION.into()),
        }),
    }
}

/// Library diagnostics as LSP items. Uses [`check_file`] (same families as CLI).
pub fn diagnostics_for(uri: &str, text: &str) -> Vec<Diagnostic> {
    library_to_lsp(text, &check_file(text, uri))
}

fn library_to_lsp(text: &str, diags: &[crate::Diagnostic]) -> Vec<Diagnostic> {
    let index = LineIndex::new(text);
    diags
        .iter()
        .filter(|d| !is_dropped_code(&d.code))
        .map(|d| {
            let start = index.position(text, d.span.start);
            let end = index.position(text, d.span.end);
            Diagnostic {
                range: Range::new(
                    Position::new(start.line, start.character),
                    Position::new(end.line, end.character),
                ),
                severity: Some(lsp_severity(d.severity)),
                code: Some(NumberOrString::String(d.code.clone())),
                source: Some("dygnosis".into()),
                message: d.message.clone(),
                tags: lsp_tags(&d.tags),
                ..Diagnostic::default()
            }
        })
        .collect()
}

fn span_pos(pos: Position) -> crate::span::Position {
    crate::span::Position {
        line: pos.line,
        character: pos.character,
    }
}

fn span_range(index: &LineIndex, text: &str, span: Span) -> Range {
    let start = index.position(text, span.start);
    let end = index.position(text, span.end);
    Range::new(
        Position::new(start.line, start.character),
        Position::new(end.line, end.character),
    )
}

fn markdown_hover(value: String, range: Option<Range>) -> Hover {
    Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value,
        }),
        range,
    }
}

fn decl_hover_markdown(model: &Model, word: &str) -> Option<String> {
    if find_named(&model.endogenous, model, word).is_some() {
        let mut parts = vec![format!("**Endogenous variable**: `{word}`")];
        if let Some(info) = classify_variable_timing(model).get(word) {
            parts.push(format_timing_line(info));
        }
        return Some(parts.join("\n\n"));
    }
    if find_named(&model.exogenous, model, word).is_some() {
        return Some(format!("**Exogenous variable**: `{word}`"));
    }
    if find_named(&model.parameters, model, word).is_some() {
        let mut parts = vec![format!("**Parameter**: `{word}`")];
        match assigned_number(model, word) {
            Some(n) => parts.push(format!("Value: `{n}`")),
            None => parts.push("Value: *not assigned*".into()),
        }
        return Some(parts.join("\n\n"));
    }
    None
}

fn find_named<'a>(decls: &'a [Decl], model: &Model, word: &str) -> Option<&'a Decl> {
    decls.iter().find(|d| model.name(d.name) == word)
}

fn find_decl<'a>(model: &'a Model, word: &str) -> Option<&'a Decl> {
    find_named(&model.endogenous, model, word)
        .or_else(|| find_named(&model.exogenous, model, word))
        .or_else(|| find_named(&model.parameters, model, word))
}

fn is_declared_in_open(inner: &Inner, word: &str) -> bool {
    inner.docs.keys().any(|uri| {
        inner
            .workspace
            .get_model(uri.as_str())
            .and_then(|m| find_decl(m, word))
            .is_some()
    })
}

fn fix_title(d: &crate::Diagnostic) -> String {
    if let Some(rest) = d.message.split_once("Fix:") {
        let suffix = rest.1.trim();
        if !suffix.is_empty() {
            if suffix.len() > 120 {
                return format!("{}...", suffix.chars().take(117).collect::<String>().trim());
            }
            return suffix.to_string();
        }
    }
    format!("Apply fix for {}", d.code)
}

fn ranges_overlap(left: Range, right: Range) -> bool {
    let left_start = (left.start.line, left.start.character);
    let left_end = (left.end.line, left.end.character);
    let right_start = (right.start.line, right.start.character);
    let right_end = (right.end.line, right.end.character);
    if right_start == right_end {
        left_start <= right_start && right_start <= left_end
    } else {
        left_start < right_end && right_start < left_end
    }
}

fn default_completions(model: &Model) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for (kw, doc) in DYNARE_KEYWORDS {
        items.push(CompletionItem {
            label: (*kw).into(),
            kind: Some(CompletionItemKind::KEYWORD),
            detail: Some("Dynare keyword".into()),
            documentation: Some(Documentation::String((*doc).into())),
            ..CompletionItem::default()
        });
    }
    for d in &model.endogenous {
        let name = model.name(d.name);
        items.push(CompletionItem {
            label: name.into(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some("endogenous variable".into()),
            ..CompletionItem::default()
        });
    }
    for d in &model.exogenous {
        let name = model.name(d.name);
        items.push(CompletionItem {
            label: name.into(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some("exogenous variable".into()),
            ..CompletionItem::default()
        });
    }
    for d in &model.parameters {
        let name = model.name(d.name);
        items.push(CompletionItem {
            label: name.into(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some("parameter".into()),
            ..CompletionItem::default()
        });
    }
    for (name, doc) in BUILTIN_FNS {
        items.push(CompletionItem {
            label: (*name).into(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some("built-in function".into()),
            documentation: Some(Documentation::String((*doc).into())),
            ..CompletionItem::default()
        });
    }
    items.push(CompletionItem {
        label: "model".into(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some("model equation block".into()),
        insert_text: Some("model;\n$0\nend;".into()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..CompletionItem::default()
    });
    items
}

#[allow(deprecated)]
fn document_symbols_for(text: &str, model: &Model) -> Vec<DocumentSymbol> {
    let index = LineIndex::new(text);
    let mut symbols = Vec::new();
    if !model.endogenous.is_empty() {
        symbols.push(group_symbol(
            "var (endogenous)",
            &model.endogenous,
            model,
            &index,
            text,
            SymbolKind::VARIABLE,
        ));
    }
    if !model.exogenous.is_empty() {
        symbols.push(group_symbol(
            "varexo (exogenous)",
            &model.exogenous,
            model,
            &index,
            text,
            SymbolKind::VARIABLE,
        ));
    }
    if !model.parameters.is_empty() {
        symbols.push(group_symbol(
            "parameters",
            &model.parameters,
            model,
            &index,
            text,
            SymbolKind::NUMBER,
        ));
    }
    if !model.equations.is_empty() {
        let children: Vec<DocumentSymbol> = model
            .equations
            .iter()
            .map(|eq| {
                let label = if !eq.name.is_empty() {
                    eq.name.clone()
                } else if !eq.lhs.is_empty() {
                    eq.lhs.clone()
                } else {
                    eq.text.chars().take(60).collect()
                };
                leaf_symbol(
                    &label,
                    SymbolKind::FUNCTION,
                    span_range(&index, text, eq.span),
                )
            })
            .collect();
        let first = children.first().map(|c| c.range).unwrap_or_default();
        let last = children.last().map(|c| c.range.end).unwrap_or(first.end);
        symbols.push(DocumentSymbol {
            name: "model".into(),
            detail: Some(format!("{} equations", model.equations.len())),
            kind: SymbolKind::MODULE,
            tags: None,
            deprecated: None,
            range: Range::new(first.start, last),
            selection_range: first,
            children: Some(children),
        });
    }
    symbols
}

#[allow(deprecated)]
fn group_symbol(
    name: &str,
    decls: &[Decl],
    model: &Model,
    index: &LineIndex,
    text: &str,
    child_kind: SymbolKind,
) -> DocumentSymbol {
    let children: Vec<DocumentSymbol> = decls
        .iter()
        .map(|d| {
            leaf_symbol(
                model.name(d.name),
                child_kind,
                span_range(index, text, d.span),
            )
        })
        .collect();
    let first = span_range(index, text, decls[0].span);
    let last = span_range(index, text, decls[decls.len() - 1].span);
    DocumentSymbol {
        name: name.into(),
        detail: Some(if name == "parameters" {
            format!("{} parameters", decls.len())
        } else {
            format!("{} variables", decls.len())
        }),
        kind: SymbolKind::NAMESPACE,
        tags: None,
        deprecated: None,
        range: Range::new(first.start, last.end),
        selection_range: first,
        children: Some(children),
    }
}

#[allow(deprecated)]
fn leaf_symbol(name: &str, kind: SymbolKind, range: Range) -> DocumentSymbol {
    DocumentSymbol {
        name: name.into(),
        detail: None,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range: range,
        children: None,
    }
}

#[allow(deprecated)]
fn flatten_workspace_symbols(
    uri: &Url,
    symbols: &[DocumentSymbol],
    container: Option<&str>,
    query: &str,
    out: &mut Vec<SymbolInformation>,
) {
    for sym in symbols {
        let matches = query.is_empty() || sym.name.to_ascii_lowercase().contains(query);
        if matches {
            out.push(SymbolInformation {
                name: sym.name.clone(),
                kind: sym.kind,
                tags: None,
                deprecated: None,
                location: Location::new(uri.clone(), sym.range),
                container_name: container.map(str::to_string),
            });
        }
        if let Some(children) = &sym.children {
            flatten_workspace_symbols(uri, children, Some(&sym.name), query, out);
        }
    }
}

fn lsp_severity(severity: Severity) -> DiagnosticSeverity {
    match severity {
        Severity::Error => DiagnosticSeverity::ERROR,
        Severity::Warning => DiagnosticSeverity::WARNING,
        Severity::Information => DiagnosticSeverity::INFORMATION,
        Severity::Hint => DiagnosticSeverity::HINT,
    }
}

fn lsp_tags(tags: &[i32]) -> Option<Vec<DiagnosticTag>> {
    if tags.contains(&2) {
        Some(vec![DiagnosticTag::DEPRECATED])
    } else {
        None
    }
}

fn is_dropped_code(code: &str) -> bool {
    OUT_CODES.contains(&code)
}

fn file_parent_dir(path: &std::path::Path) -> Option<PathBuf> {
    let parent = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    parent.is_dir().then_some(parent)
}

fn open_overlay_files(inner: &Inner, uri: &Url, text: &str) -> HashMap<String, String> {
    let mut files = HashMap::new();
    for (u, d) in &inner.docs {
        files.insert(u.as_str().to_string(), d.text.clone());
    }
    files.insert(uri.as_str().to_string(), text.to_string());
    files
}

fn extract_command_uri(arguments: &[Value]) -> Option<Url> {
    let first = arguments.first()?;
    let obj = if first.is_array() {
        first.as_array()?.first()?
    } else {
        first
    };
    let s = if let Some(s) = obj.as_str() {
        s.to_string()
    } else {
        obj.get("uri")?.as_str()?.to_string()
    };
    Url::parse(&s).ok().or_else(|| Url::from_file_path(&s).ok())
}

/// In-process server for tests. Stdio entry is [`run_stdio`].
pub fn new_service() -> (LspService<Backend>, ClientSocket) {
    LspService::new(Backend::new)
}

/// Run the language server over stdin/stdout.
pub async fn run_stdio() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let (service, socket) = new_service();
    Server::new(stdin, stdout, socket).serve(service).await;
}

/// Debug TCP listener (one accept). Not the editor ship path.
pub async fn run_tcp(host: &str, port: u16) {
    let listener = tokio::net::TcpListener::bind((host, port))
        .await
        .unwrap_or_else(|e| panic!("TCP bind {host}:{port}: {e}"));
    let (stream, _) = listener
        .accept()
        .await
        .unwrap_or_else(|e| panic!("TCP accept: {e}"));
    let (read, write) = tokio::io::split(stream);
    let (service, socket) = new_service();
    Server::new(read, write, socket).serve(service).await;
}

fn g_format(n: f64) -> String {
    let mut s = format!("{n:.6}");
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

fn pos_in_range(pos: Position, range: Range) -> bool {
    let p = (pos.line, pos.character);
    p >= (range.start.line, range.start.character) && p <= (range.end.line, range.end.character)
}

fn pos_in_range_half_open(pos: Position, range: Range) -> bool {
    let p = (pos.line, pos.character);
    p >= (range.start.line, range.start.character) && p < (range.end.line, range.end.character)
}

fn range_has_pos(range: Range, pos: Position) -> bool {
    pos_in_range(pos, range)
}

fn range_strictly_contains(outer: Range, inner: Range) -> bool {
    if outer.start == inner.start && outer.end == inner.end {
        return false;
    }
    pos_in_range(inner.start, outer) && pos_in_range(inner.end, outer)
}

fn full_document_range(text: &str) -> Range {
    let lines: Vec<&str> = text.split('\n').collect();
    let last = lines.last().copied().unwrap_or("");
    let last = last.strip_suffix('\r').unwrap_or(last);
    Range::new(
        Position::new(0, 0),
        Position::new(
            lines.len().saturating_sub(1) as u32,
            last.chars().count() as u32,
        ),
    )
}

fn full_document_edit(old_text: &str, new_text: String) -> TextEdit {
    TextEdit {
        range: full_document_range(old_text),
        new_text,
    }
}

fn line_range_edit(text: &str, start_line: u32, end_line: u32, replacement: String) -> TextEdit {
    let lines: Vec<&str> = text.split('\n').collect();
    let end = lines
        .get(end_line as usize)
        .map(|l| l.strip_suffix('\r').unwrap_or(l).chars().count() as u32)
        .unwrap_or(0);
    TextEdit {
        range: Range::new(Position::new(start_line, 0), Position::new(end_line, end)),
        new_text: replacement,
    }
}

fn block_comment_folds(text: &str) -> Vec<(u32, u32)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'/' && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            if i + 1 < bytes.len() {
                i += 2;
                let start_line = text[..start].bytes().filter(|&b| b == b'\n').count() as u32;
                let end_line = text[..i].bytes().filter(|&b| b == b'\n').count() as u32;
                if start_line < end_line {
                    out.push((start_line, end_line));
                }
            }
            continue;
        }
        i += 1;
    }
    out
}

fn equation_at<'a>(
    model: &'a Model,
    index: &LineIndex,
    text: &str,
    pos: Position,
) -> Option<&'a Equation> {
    for eq in model
        .equations
        .iter()
        .chain(model.steady_state_equations.iter())
    {
        let rng = span_range(index, text, eq.span);
        if range_has_pos(rng, pos) {
            return Some(eq);
        }
    }
    None
}

fn parse_format_indent(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        let s = s.trim();
        if s.eq_ignore_ascii_case("tab") {
            return Some("\t".into());
        }
        if s.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = s.parse::<usize>() {
                if (1..=8).contains(&n) {
                    return Some(" ".repeat(n));
                }
            }
        }
        return None;
    }
    if let Some(n) = value.as_u64() {
        if (1..=8).contains(&n) {
            return Some(" ".repeat(n as usize));
        }
    }
    if let Some(n) = value.as_i64() {
        if (1..=8).contains(&n) {
            return Some(" ".repeat(n as usize));
        }
    }
    None
}

fn parse_path_array(value: &Value) -> Option<Vec<PathBuf>> {
    let arr = value.as_array()?;
    let mut out = Vec::new();
    for item in arr {
        if let Some(s) = item.as_str() {
            let s = s.trim();
            if !s.is_empty() {
                out.push(PathBuf::from(s));
            }
        }
    }
    Some(out)
}

fn parse_search_paths_by_root(value: &Value) -> Option<Vec<PathBuf>> {
    let obj = value.as_object()?;
    let mut out = Vec::new();
    for extras in obj.values() {
        if let Some(paths) = parse_path_array(extras) {
            for p in paths {
                if !out.iter().any(|e| e == &p) {
                    out.push(p);
                }
            }
        }
    }
    Some(out)
}

fn explain_command(arguments: &[Value]) -> Value {
    let code = extract_explain_code(arguments);
    let Some(code) = code else {
        return Value::Null;
    };
    match explain::render_markdown(&code) {
        Some(md) => Value::String(md),
        None => {
            let known = explain::known_codes().join(", ");
            Value::String(format!(
                "Diagnostic `{code}` is not documented. Known codes: {known}"
            ))
        }
    }
}

fn extract_explain_code(arguments: &[Value]) -> Option<String> {
    let first = arguments.first()?;
    let obj = if first.is_array() {
        first.as_array()?.first()?
    } else {
        first
    };
    if let Some(s) = obj.as_str() {
        return Some(s.to_string());
    }
    obj.get("code")?.as_str().map(|s| s.to_string())
}

fn parse_compare_args(arguments: &[Value]) -> std::result::Result<(String, String), Value> {
    let values: &[Value] = if arguments.len() == 1 && arguments[0].is_array() {
        arguments[0]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or(arguments)
    } else {
        arguments
    };
    if values.len() >= 2 {
        if let (Some(a), Some(b)) = (values[0].as_str(), values[1].as_str()) {
            return Ok((a.to_string(), b.to_string()));
        }
    }
    if let Some(obj) = values.first().and_then(|v| v.as_object()) {
        let a = obj
            .get("uri_a")
            .or_else(|| obj.get("uriA"))
            .and_then(|v| v.as_str());
        let b = obj
            .get("uri_b")
            .or_else(|| obj.get("uriB"))
            .and_then(|v| v.as_str());
        if let (Some(a), Some(b)) = (a, b) {
            return Ok((a.to_string(), b.to_string()));
        }
    }
    Err(json!({
        "error": "compareModels requires uri_a and uri_b arguments",
        "code": "BAD_ARGS",
    }))
}

fn ch_variable_item(
    model: &Model,
    index: &LineIndex,
    text: &str,
    uri: &Url,
    name: &str,
) -> CallHierarchyItem {
    let rng = find_decl(model, name)
        .map(|d| span_range(index, text, d.span))
        .unwrap_or_else(|| Range::new(Position::new(0, 0), Position::new(0, 0)));
    CallHierarchyItem {
        name: name.to_string(),
        kind: SymbolKind::VARIABLE,
        tags: None,
        detail: Some("variable".into()),
        uri: uri.clone(),
        range: rng,
        selection_range: rng,
        data: Some(json!({"dynare": "variable", "name": name})),
    }
}

fn ch_equation_item(
    _model: &Model,
    index: &LineIndex,
    text: &str,
    uri: &Url,
    eq: &Equation,
) -> CallHierarchyItem {
    let rng = span_range(index, text, eq.span);
    let name = if !eq.name.is_empty() {
        eq.name.clone()
    } else if !eq.lhs.trim().is_empty() {
        format!("{} = ...", eq.lhs.trim())
    } else {
        eq.text
            .lines()
            .next()
            .unwrap_or("equation")
            .chars()
            .take(48)
            .collect()
    };
    CallHierarchyItem {
        name,
        kind: SymbolKind::FUNCTION,
        tags: None,
        detail: Some("equation".into()),
        uri: uri.clone(),
        range: rng,
        selection_range: rng,
        data: Some(json!({"dynare": "equation", "start": eq.span.start})),
    }
}
