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
/// those E001 rows only (cascade). Otherwise concatenate equation-count (W013), E020, E030,
/// OccBin, written clash, estimation, shape, W010, E062–E065, W070, W090, W100, W110 (includes W060), W120, W130, symbol lists, D-block.
/// W062 / E061 / W061 / W160 and I050 quiet are workspace-only (`check_file`), not here.
pub fn analyze(model: &Model) -> Vec<Diagnostic> {
    let parse_diags = crate::check_parse::check_parse(model);
    if !parse_diags.is_empty() {
        return parse_diags;
    }
    let pac_parse_diags = crate::check_d_pac::check_parse(model);
    if !pac_parse_diags.is_empty() {
        return pac_parse_diags;
    }
    let hank_parse_diags = crate::check_d_hank::check_parse(model);
    if !hank_parse_diags.is_empty() {
        return hank_parse_diags;
    }
    let shape_diags = crate::diag_shape::check_shape(model);
    let shape_syntax: Vec<Diagnostic> = shape_diags
        .iter()
        .filter(|d| d.code == "E001")
        .cloned()
        .collect();
    if !shape_syntax.is_empty() {
        return shape_syntax;
    }
    let ms_diags = crate::check_d_ms::check_d_ms(model);
    let recorded_syntax: Vec<Diagnostic> = ms_diags
        .iter()
        .filter(|d| d.code == "E001")
        .cloned()
        .collect();
    if !recorded_syntax.is_empty() {
        return recorded_syntax;
    }
    if ms_diags.first().is_some_and(|d| {
        matches!(
            d.code.as_str(),
            "E058" | "E059" | "E317" | "E378" | "E426" | "E427" | "E428" | "E429" | "E430"
        )
    }) {
        return ms_diags;
    }
    let shock_diags = crate::check_d_shocks::check_d_shocks(model);
    let shock_parse_diags: Vec<Diagnostic> = shock_diags
        .iter()
        .filter(|d| d.code != "E420")
        .cloned()
        .collect();
    if !shock_parse_diags.is_empty() {
        return shock_parse_diags;
    }
    let mut clashes = crate::check_clash::check_clash(model);
    clashes.extend(crate::check_d_pac::check_transform(model));
    let equation_parse = crate::check_e020::check_e020(model);
    let declaration_parse = crate::check_e030::check_e030(model);
    let occbin_diags = crate::check_occbin::check_occbin(model);
    let observed_diags = crate::check_w090::check_w090(model);
    let block_diags = crate::check_d_block::check_d_block(model);
    let open_diags = crate::check_d_open::check_d_open(model);
    let surgery_parse = crate::check_d_surgery::check_d_surgery(model);
    // The equation/declaration/surgery families are parsed before checkPass.
    // Mixed families contribute only the named parse refusals below. In
    // particular, a check-stage Error elsewhere must not suppress an earlier
    // PAC checkPass refusal.
    let parse_refused = equation_parse
        .iter()
        .chain(&declaration_parse)
        .chain(&surgery_parse)
        .any(|diag| diag.severity == Severity::Error)
        || occbin_diags.iter().any(|diag| diag.code == "E182")
        || observed_diags
            .iter()
            .any(|diag| matches!(diag.code.as_str(), "E093" | "E261"))
        || block_diags.iter().any(|diag| diag.code == "E271")
        || open_diags.iter().any(|diag| {
            matches!(
                diag.code.as_str(),
                "E058" | "E059" | "E288" | "E289" | "E290" | "E291" | "E292" | "E293" | "E294"
            )
        });
    let mut out = Vec::new();
    out.extend(crate::e010::check_e010(model));
    out.extend(equation_parse);
    out.extend(declaration_parse);
    out.extend(occbin_diags);
    out.extend(clashes.iter().cloned());
    out.extend(crate::check_estimation::check_estimation(model));
    out.extend(crate::check_context::check_context(model));
    out.extend(shape_diags);
    out.extend(crate::check_w010::check_w010_family(model));
    out.extend(crate::check_e060::check_e060_family_on_model(model));
    out.extend(crate::check_w070::check_w070(model));
    out.extend(observed_diags);
    out.extend(crate::check_estimated_params::check_estimated_params(model));
    out.extend(crate::check_w100::check_w100(model));
    out.extend(crate::check_w110::check_w110(model));
    out.extend(crate::check_w120::check_w120_family(model));
    out.extend(crate::check_w130::check_w130(model));
    out.extend(crate::check_symbol_list::check_symbol_list(model));
    out.extend(block_diags);
    out.extend(shock_diags);
    out.extend(open_diags);
    // E271 is already emitted for each repeated option by check_shape. The
    // dotted parse walk uses the first duplicate only to stop a later head
    // type refusal from pre-empting that statement.
    out.extend(ms_diags.into_iter().filter(|d| d.code != "E271"));
    out.extend(surgery_parse);
    out.extend(crate::check_mom::check_mom(model));
    if !parse_refused {
        out.extend(crate::check_d_pac::check_check(model));
    }
    let hank_mcp = crate::check_d_hank::check_mcp(model);
    let hank_parse_stopped = parse_refused || hank_mcp.iter().any(|diag| diag.code == "E479");
    out.extend(hank_mcp);
    out.extend(crate::check_d_hank::check_second_dimension(model));
    if !hank_parse_stopped {
        out.extend(crate::check_d_hank::check_check(model));
    }
    // The subsample type gate is in writeOutput, after every parse, check and
    // transform refusal. A prior Error keeps the writer from running.
    if out
        .iter()
        .any(|d| d.severity == Severity::Error && d.code != "E431")
    {
        out.retain(|d| d.code != "E431");
    }
    // These generic parse refusals can enter through later diagnostic passes.
    // Dynare finishes parsing the whole file before checkPass, so any of them
    // stops E420 regardless of its written position.
    if parse_refused {
        out.retain(|d| d.code != "E420");
    }
    // A refusal in an earlier statement also stops a later shock_paths
    // checkPass from reporting the circular self reference.
    let earlier_nonclash_errors: Vec<Span> = out
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code != "E420"
                && !clashes
                    .iter()
                    .any(|clash| clash.code == d.code && clash.span == d.span)
        })
        .map(|d| d.span)
        .collect();
    out.retain(|d| {
        d.code != "E420"
            || !earlier_nonclash_errors
                .iter()
                .any(|span| span.start < d.span.start)
    });
    // A parse or check Error stops the file before transformPass. Warnings
    // and other transform clashes do not prevent a written clash from firing.
    if out.iter().any(|d| {
        d.severity == Severity::Error
            && !clashes
                .iter()
                .any(|clash| clash.code == d.code && clash.span == d.span)
    }) {
        out.retain(|d| {
            !clashes
                .iter()
                .any(|clash| clash.code == d.code && clash.span == d.span)
        });
    }
    out
}

/// One-document workspace check: `analyze()` plus W062 / E061 / W061 / W160 and I050 quiet.
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
        let model = parse(&text);
        crate::suppress::apply_model(&model, analyze(&model))
    })
}

fn try_workspace_check(ws: &mut Workspace, abs_path: &str) -> Option<Vec<Diagnostic>> {
    let model = ws.get_effective_model(abs_path)?.clone();
    let mut diags = analyze(&model);
    diags.extend(crate::check_d_open::check_workspace_d_open(
        &model, abs_path,
    ));
    diags = crate::suppress::apply_effective(ws, abs_path, diags);
    let records = ws.include_records(abs_path).cloned().unwrap_or_default();
    if !records.unresolved.is_empty() || !records.cycles.is_empty() {
        diags.retain(|d| d.code != "W060");
    } else if !records.resolved.is_empty() {
        diags.retain_mut(|d| {
            if d.code != "W060" {
                return true;
            }
            if let Some(span) = ws.map_effective_span_to_root(abs_path, d.span) {
                d.span = span;
                true
            } else {
                false
            }
        });
    }
    let mut extra = Vec::new();
    extra.extend(crate::check_e060::check_e060(&records));
    extra.extend(crate::check_e060::check_e061(&records));
    extra.extend(crate::check_e060::check_w061(ws, abs_path));
    let companions = ws
        .companion_records(abs_path)
        .map(|r| r.to_vec())
        .unwrap_or_default();
    extra.extend(crate::check_w160::check_w160(&companions));
    if let Some(root) = ws.get_model(abs_path) {
        let source = root.source.clone();
        let native = root.ms_unparsed_spans.clone();
        extra = crate::suppress::apply_source(&source, &native, extra);
    }
    diags.extend(extra);
    crate::check_w160::quiet_i050(&mut diags, &companions);
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
