//! Thin diagnostics. Code, severity, and message match the Python oracle for
//! thin codes; a recorded Python bug is fixed on this surface, not preserved.

use crate::model::Model;
use crate::parser::parse;
use crate::span::{LineIndex, Span};
use crate::workspace::Workspace;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error = 1,
    Warning = 2,
    Information = 3,
    Hint = 4,
}

/// Suggested edit stored on a diagnostic (slice 07). Slice 20 applies it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub start_line: u32,
    pub start_char: u32,
    pub end_line: u32,
    pub end_char: u32,
    pub new_text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub span: Span,
    pub severity: Severity,
    pub code: String,
    pub message: String,
    pub fix: Option<TextEdit>,
    /// LSP DiagnosticTag values (2 = Deprecated). Empty for most codes.
    pub tags: Vec<i32>,
}

impl Diagnostic {
    pub fn new(
        span: Span,
        severity: Severity,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            span,
            severity,
            code: code.into(),
            message: message.into(),
            fix: None,
            tags: Vec::new(),
        }
    }
}

/// Compose diagnostic families. Families share `Model` and run as peers.
///
/// When `check_parse` is nonempty, later families are skipped (cascade).
/// Thin families for one parsed model. If `check_parse` is nonempty, return
/// those E001 rows only (cascade). Otherwise concatenate E010, E020, E030,
/// shape, W010, E062–E065, W070, W090, W100, W110 (includes W060), W120, W130.
/// E060 / E061 / W061 are workspace-only (`check_file`), not here.
pub fn analyze(model: &Model) -> Vec<Diagnostic> {
    let parse_diags = crate::check_parse::check_parse(model);
    if !parse_diags.is_empty() {
        return parse_diags;
    }
    let mut out = Vec::new();
    out.extend(crate::e010::check_e010(model));
    out.extend(crate::check_e020::check_e020(model));
    out.extend(crate::check_e030::check_e030(model));
    out.extend(crate::diag_shape::check_shape(model));
    out.extend(crate::check_w010::check_w010_family(model));
    out.extend(crate::check_e060::check_e060_family_on_model(model));
    out.extend(crate::check_w070::check_w070(model));
    out.extend(crate::check_w090::check_w090(model));
    out.extend(crate::check_w100::check_w100(model));
    out.extend(crate::check_w110::check_w110(model));
    out.extend(crate::check_w120::check_w120_family(model));
    out.extend(crate::check_w130::check_w130(model));
    out
}

/// One-document workspace check: `analyze()` plus E060 / E061 / W061.
///
/// `abs_path` is the workspace key (includes resolve against that file's
/// directory). Falls back to `analyze(&parse(text))` if setup fails.
/// Does not call `check_e060_family` (that would double-emit E062–E065).
pub fn check_file(text: &str, abs_path: &str) -> Vec<Diagnostic> {
    let mut ws = Workspace::new();
    ws.update_document(abs_path, text);
    check_in_workspace(&mut ws, abs_path)
}

/// Same families as [`check_file`] on an existing workspace (open overlays).
pub(crate) fn check_in_workspace(ws: &mut Workspace, abs_path: &str) -> Vec<Diagnostic> {
    try_workspace_check(ws, abs_path).unwrap_or_else(|| {
        let text = ws.get_source(abs_path).unwrap_or("").to_string();
        analyze(&parse(&text))
    })
}

fn try_workspace_check(ws: &mut Workspace, abs_path: &str) -> Option<Vec<Diagnostic>> {
    let model = ws.get_effective_model(abs_path)?.clone();
    let mut diags = analyze(&model);
    let records = ws.include_records(abs_path).cloned().unwrap_or_default();
    diags.extend(crate::check_e060::check_e060(&records));
    diags.extend(crate::check_e060::check_e061(&records));
    diags.extend(crate::check_e060::check_w061(ws, abs_path));
    Some(diags)
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "ERROR",
        Severity::Warning => "WARNING",
        Severity::Information => "INFO",
        Severity::Hint => "HINT",
    }
}

/// CLI check line template. Line and column are 1-based.
pub fn format_check_lines(path: &str, diags: &[Diagnostic], src: &str) -> String {
    if diags.is_empty() {
        return format!("No issues found in {path}\n");
    }
    let index = LineIndex::new(src);
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut out = String::new();
    for d in diags {
        let pos = index.position(src, d.span.start);
        let line = pos.line + 1;
        let col = pos.character + 1;
        let severity = severity_label(d.severity);
        out.push_str(&format!(
            "{path}:{line}:{col}: {severity} [{}] {}\n",
            d.code, d.message
        ));
        match d.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
            _ => {}
        }
    }
    out.push('\n');
    out.push_str(&format!(
        "{} issue(s): {errors} error(s), {warnings} warning(s)\n",
        diags.len()
    ));
    out
}
