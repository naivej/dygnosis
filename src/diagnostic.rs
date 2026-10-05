//! Thin diagnostics. The pinned official Dynare preprocessor is the ground
//! truth for language refusals and warnings before MATLAB or Octave.

use crate::model::Model;
use crate::parser::parse;
use crate::span::{LineIndex, Span};
use crate::workspace::Workspace;
use std::collections::HashMap;
use std::sync::Arc;

pub use crate::diagnostic_links::{RelatedDiagnostic, RelatedFrame, RelatedOccurrence};

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
    /// Independently mapped earlier occurrences; primary wording is unchanged.
    pub related: Vec<RelatedDiagnostic>,
    /// LSP DiagnosticTag values (1 = Unnecessary, 2 = Deprecated).
    pub tags: Vec<i32>,
    /// Heterogeneous count scope supplied by its producer. Macro executions
    /// can share a written span while belonging to different dimensions.
    pub model_dimension: Option<String>,
    /// Producer-selected keyword used only after analysis and safe mapping.
    pub display_keyword: Option<(Span, &'static str)>,
}

impl Diagnostic {
    pub fn new(
        span: Span,
        severity: Severity,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        let code = code.into();
        let tags = if severity == Severity::Warning && matches!(code.as_str(), "W020" | "W022") {
            vec![1]
        } else {
            Vec::new()
        };
        Self {
            span,
            severity,
            code,
            message: message.into(),
            fix: None,
            related: Vec::new(),
            tags,
            model_dimension: None,
            display_keyword: None,
        }
    }

    pub fn with_related(mut self, related: RelatedDiagnostic) -> Self {
        self.related.push(related);
        self
    }

    pub fn with_model_dimension(mut self, dimension: impl Into<String>) -> Self {
        self.model_dimension = Some(dimension.into());
        self
    }

    pub(crate) fn with_display_keyword(mut self, span: Span, keyword: &'static str) -> Self {
        self.display_keyword = Some((span, keyword));
        self
    }
}

/// Source of a compilation-unit writing summary after include expansion.
#[derive(Clone, Debug)]
pub struct WritingOrigin {
    pub file: String,
    pub text: String,
}

/// Written location of one diagnostic from an include-spliced model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticOrigin {
    pub file: String,
    /// One shared text snapshot per owning file in this analysis.
    pub text: Arc<str>,
    pub span: Span,
}

/// Diagnostics for one root, with written-source locations for presentation.
#[derive(Clone, Debug)]
pub struct DiagnosticSet {
    pub root: String,
    pub diagnostics: Vec<Diagnostic>,
    pub writing_origins: HashMap<String, WritingOrigin>,
    /// Parallel to `diagnostics`. `None` means the span is already in root text.
    pub origins: Vec<Option<DiagnosticOrigin>>,
}

/// Compose diagnostic families. Families share `Model` and run as peers.
///
/// When `check_parse` is nonempty, later families are skipped (cascade).
/// Thin families for one parsed model. If `check_parse` is nonempty, return
/// those E001 rows only (cascade). Otherwise concatenate equation-count (W013), E020, E030,
/// OccBin, written clash, estimation, shape, W010, E062–E065, W070, W090, W100, W110 (includes W060), W120, W130, symbol lists, D-block.
/// W062 / E061 / W061 / W160 and I050 quiet are workspace-only (`check_file`), not here.
pub fn analyze(model: &Model) -> Vec<Diagnostic> {
    let mut diagnostics = analyze_positions(model);
    crate::diagnostic_anchors::apply(model, &mut diagnostics, |_, _| true);
    diagnostics
}

/// Keep analysis positions through every selection and suppression decision.
/// Only the completed collection receives display ranges.
fn analyze_positions(model: &Model) -> Vec<Diagnostic> {
    let macro_syntax = crate::check_e060::check_e062(model);
    if !macro_syntax.is_empty() {
        return macro_syntax;
    }
    // Macro processing runs before the .mod parser. A failed definition may
    // otherwise turn its later interpolation into a spurious equation error.
    if !model.macro_type_errors.is_empty() {
        return model
            .macro_type_errors
            .iter()
            .map(|(span, code, message)| {
                Diagnostic::new(*span, Severity::Error, *code, message.clone())
            })
            .collect();
    }
    if let Some(span) = model.macro_incomplete_span {
        // The remaining source still contains macro syntax, so any ordinary
        // parse/name/count diagnostic could describe a tree Dynare never sees.
        let explicit_error = crate::check_e060::check_e064(model);
        if !explicit_error.is_empty() {
            return explicit_error;
        }
        return vec![Diagnostic::new(
            span,
            Severity::Information,
            "I211",
            "Macro expansion is incomplete; some model checks were withheld.",
        )];
    }
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
    let steady_state_diags = crate::check_w130::check_w130(model);
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
        || block_diags.iter().any(|diag| {
            matches!(
                diag.code.as_str(),
                "E020" | "E182" | "E271" | "E280" | "E281" | "E294"
            )
        })
        || steady_state_diags.iter().any(|diag| diag.code == "E481")
        || open_diags.iter().any(|diag| {
            matches!(
                diag.code.as_str(),
                "E058" | "E059" | "E288" | "E289" | "E290" | "E291" | "E292" | "E293" | "E294"
            )
        });
    let mut out = Vec::new();
    out.extend(crate::e010::check_e010(model));
    out.extend(crate::check_d_hank::check_square(model));
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
    out.extend(steady_state_diags);
    out.extend(crate::check_symbol_list::check_symbol_list(model));
    // Captured replacement uses also retain unknown names that a later
    // declaration hides from the equation pass. Keep its richer message when
    // both passes found the same written E020 range.
    for diag in block_diags {
        if diag.code != "E020"
            || !out
                .iter()
                .any(|existing| existing.code == "E020" && existing.span == diag.span)
        {
            out.push(diag);
        }
    }
    out.extend(shock_diags);
    out.extend(open_diags);
    // E271 is already emitted for each repeated option by check_shape. The
    // dotted parse walk uses the first duplicate only to stop a later head
    // type refusal from pre-empting that statement.
    out.extend(ms_diags.into_iter().filter(|d| d.code != "E271"));
    out.extend(surgery_parse);
    // The moment row-scope pass reads the same captured model-expression
    // uses as the generic role/timing passes. Keep one diagnostic per refusal.
    for diag in crate::check_mom::check_mom(model) {
        if !out.iter().any(|existing| {
            existing.code == diag.code
                && existing.span == diag.span
                && existing.message == diag.message
        }) {
            out.push(diag);
        }
    }
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
    if !out.iter().any(|d| d.severity == Severity::Error) {
        out.extend(crate::check_d_surgery::check_w212(model));
    }
    if !out.iter().any(|d| d.code == "E001") {
        out.extend(crate::check_writing::writing_summaries(model));
        out.extend(crate::check_w211::exogenous_leads(model));
    }
    // These written-file transform cases run only after every earlier refusal
    // we currently model is quiet. Keep the official transform source order in
    // `check_written_transform` and leave rewritten-only cases to W013/W208.
    let has_earlier_error = out.iter().any(|diag| {
        diag.severity == Severity::Error
            && !clashes
                .iter()
                .any(|clash| clash.code == diag.code && clash.span == diag.span)
    });
    if !has_earlier_error {
        let early = crate::check_written_transform::check_early(model);
        // Constant simplification and unused-endogenous checks precede all
        // current written clashes, including PAC target rewrites.
        if !early.is_empty() {
            out.retain(|diag| {
                !clashes
                    .iter()
                    .any(|clash| clash.code == diag.code && clash.span == diag.span)
            });
        }
        let written = if early.is_empty() && !out.iter().any(|d| d.severity == Severity::Error) {
            crate::check_written_transform::check_late(model)
        } else {
            early
        };
        if let Some(first) = written.first() {
            match first.code.as_str() {
                "E186" => out.retain(|d| {
                    d.code != "W013"
                        && !(d.code == "W020" && written.iter().any(|e| e.span == d.span))
                }),
                "E188" => out.retain(|d| d.code != "W013"),
                "E192" => out.retain(|d| d.code != "W208"),
                _ => {}
            }
            out.extend(written);
        }
    }
    out
}

/// One-document workspace check: `analyze()` plus W062 / E061 / W061 / W160 and I050 quiet.
///
/// `abs_path` is the workspace key (includes resolve against that file's
/// directory). Falls back to `analyze(&parse(text))` if setup fails.
/// Does not call `check_e060_family` (that would double-emit E062–E065).
pub fn check_file(text: &str, abs_path: &str) -> Vec<Diagnostic> {
    check_file_with_origins(text, abs_path).diagnostics
}

pub fn check_file_with_origins(text: &str, abs_path: &str) -> DiagnosticSet {
    let mut ws = Workspace::new();
    ws.update_document(abs_path, text);
    check_in_workspace_with_origins(&mut ws, abs_path)
}

/// Same families as [`check_file`] on an existing workspace (open overlays),
/// with the written source of each diagnostic retained for presentation.
pub(crate) fn check_in_workspace_with_origins(ws: &mut Workspace, abs_path: &str) -> DiagnosticSet {
    try_workspace_check(ws, abs_path).unwrap_or_else(|| {
        let text = ws.get_source(abs_path).unwrap_or("").to_string();
        let model = parse(&text);
        DiagnosticSet {
            root: root_key(ws, abs_path),
            diagnostics: analyze(&model),
            writing_origins: HashMap::new(),
            origins: Vec::new(),
        }
    })
}

fn try_workspace_check(ws: &mut Workspace, abs_path: &str) -> Option<DiagnosticSet> {
    let model = ws.get_effective_model(abs_path)?.clone();
    let mut diags = analyze_positions(&model);
    if model.macro_incomplete() {
        // An unevaluated macro can change declarations, command options, and
        // which includes exist. Keep only the macro result already established
        // by analyze; file and companion checks would use an unfinished tree.
        let writing_origins = HashMap::new();
        let mut source_texts = HashMap::new();
        let origins = diags
            .iter()
            .map(|diag| diagnostic_origin(ws, abs_path, diag, &writing_origins, &mut source_texts))
            .collect();
        return Some(DiagnosticSet {
            root: root_key(ws, abs_path),
            diagnostics: diags,
            writing_origins,
            origins,
        });
    }
    diags.extend(crate::check_d_open::check_workspace_d_open(
        ws, &model, abs_path,
    ));
    let records = ws.include_records(abs_path).cloned().unwrap_or_default();
    // Missing and cyclic includes are workspace records. The spliced model no
    // longer contains the directive, so `model_structure_incomplete` cannot see them.
    let expansion_blocked = !records.unresolved.is_empty() || !records.cycles.is_empty();
    if expansion_blocked {
        diags.retain(|d| {
            d.code != "W060"
                && d.code != "W208"
                && d.code != "W211"
                && !matches!(d.code.as_str(), "E186" | "E188" | "E189" | "E190" | "E192")
                && !crate::check_writing::is_writing_code(&d.code)
        });
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
    let writing_origins = if expansion_blocked {
        HashMap::new()
    } else {
        place_writing_anchors(ws, abs_path, &mut diags)
    };
    let mut extra = Vec::new();
    extra.extend(crate::check_e060::check_e060(&records));
    if !ws.is_virtual_root(abs_path) {
        extra.extend(crate::check_e060::check_e061(&records));
    }
    extra.extend(crate::check_e060::check_w061(ws, abs_path));
    let companions = ws
        .companion_records(abs_path)
        .map(|r| r.to_vec())
        .unwrap_or_default();
    extra.extend(crate::check_w160::check_w160(&companions));
    diags.extend(extra);
    crate::check_w160::quiet_i050(&mut diags, &companions);
    crate::diagnostic_anchors::apply(&model, &mut diags, |span, keyword| {
        safely_mapped_keyword(ws, abs_path, span, keyword)
    });
    let mut source_texts: HashMap<String, Arc<str>> = HashMap::new();
    crate::diagnostic_links::map_related(ws, abs_path, &mut diags, &mut source_texts);
    let origins: Vec<Option<DiagnosticOrigin>> = diags
        .iter()
        .map(|diag| diagnostic_origin(ws, abs_path, diag, &writing_origins, &mut source_texts))
        .collect();
    if !records.resolved.is_empty() {
        for (diag, origin) in diags.iter_mut().zip(&origins) {
            let Some(fix) = diag.fix.take() else { continue };
            diag.fix = origin
                .as_ref()
                .and_then(|owner| remap_stored_fix(ws, abs_path, &model.source, &fix, owner));
        }
    }
    Some(DiagnosticSet {
        root: root_key(ws, abs_path),
        diagnostics: diags,
        writing_origins,
        origins,
    })
}

/// A display keyword must be continuous and have the same spelling in its
/// written file. Otherwise the existing diagnostic range remains in force.
fn safely_mapped_keyword(ws: &mut Workspace, root: &str, span: Span, keyword: &str) -> bool {
    let Some((file, mapped)) = ws.map_effective_origin(root, span) else {
        return false;
    };
    let tail = Span {
        start: span.end - 1,
        end: span.end,
    };
    let continuous = ws
        .map_effective_origin(root, tail)
        .is_some_and(|(tail_file, tail_span)| file == tail_file && mapped.end == tail_span.end);
    continuous
        && ws
            .get_source(&file)
            .and_then(|text| text.get(mapped.start as usize..mapped.end as usize))
            .is_some_and(|text| text.eq_ignore_ascii_case(keyword))
}

/// These workspace checks already use root-file coordinates.
pub(crate) fn is_root_text_code(code: &str) -> bool {
    matches!(code, "W060" | "W061" | "W062" | "W160" | "E061")
}

fn diagnostic_origin(
    ws: &mut Workspace,
    root: &str,
    diag: &Diagnostic,
    writing_origins: &HashMap<String, WritingOrigin>,
    source_texts: &mut HashMap<String, Arc<str>>,
) -> Option<DiagnosticOrigin> {
    if crate::check_writing::is_writing_code(&diag.code) {
        let writing = writing_origins.get(&diag.code)?;
        let text = source_texts
            .entry(writing.file.clone())
            .or_insert_with(|| Arc::from(writing.text.as_str()))
            .clone();
        return Some(DiagnosticOrigin {
            file: writing.file.clone(),
            text,
            span: diag.span,
        });
    }
    if is_root_text_code(&diag.code) {
        return None;
    }
    let (file, mut span) = ws.map_effective_origin(root, diag.span)?;
    let text = if let Some(cached) = source_texts.get(&file) {
        cached.clone()
    } else {
        let snapshot: Arc<str> = Arc::from(ws.get_source(&file)?);
        source_texts.insert(file.clone(), snapshot.clone());
        snapshot
    };
    // An anchor spanning two splice segments has no continuous source range.
    // Keep the first source character so the diagnostic still points at its cause.
    if diag.span.end > diag.span.start.saturating_add(1) {
        let tail = Span {
            start: diag.span.end - 1,
            end: diag.span.end,
        };
        let contiguous =
            ws.map_effective_origin(root, tail)
                .is_some_and(|(tail_file, tail_span)| {
                    tail_file == file
                        && tail_span.start == span.start + (diag.span.end - diag.span.start - 1)
                });
        if !contiguous {
            let size = text
                .get(span.start as usize..)
                .and_then(|remaining| remaining.chars().next())
                .map(|ch| ch.len_utf8() as u32)
                .unwrap_or(0);
            span.end = span.start.saturating_add(size);
        }
    }
    Some(DiagnosticOrigin { file, text, span })
}

/// Stored fixes use scalar line/column positions in the effective file. Keep
/// one only when both ends are a contiguous range in the diagnostic's source.
fn remap_stored_fix(
    ws: &mut Workspace,
    root: &str,
    effective: &str,
    fix: &TextEdit,
    owner: &DiagnosticOrigin,
) -> Option<TextEdit> {
    let effective_index = LineIndex::new(effective);
    let start = effective_index.offset(
        effective,
        crate::span::Position {
            line: fix.start_line,
            character: fix.start_char,
        },
    );
    let end = effective_index.offset(
        effective,
        crate::span::Position {
            line: fix.end_line,
            character: fix.end_char,
        },
    );
    if end < start {
        return None;
    }
    let mapped = ws.map_effective_origin(root, Span { start, end: start });
    let (start_file, start_span) = match mapped {
        Some((file, span)) if file == owner.file => (file, span),
        _ if start == end && start > 0 => {
            // An insertion at an include's written EOF is also the following
            // splice segment's start. The preceding byte proves the left owner.
            let (file, tail) = ws.map_effective_origin(
                root,
                Span {
                    start: start - 1,
                    end: start,
                },
            )?;
            if file != owner.file {
                return None;
            }
            (
                file,
                Span {
                    start: tail.end,
                    end: tail.end,
                },
            )
        }
        _ => return None,
    };
    debug_assert_eq!(start_file, owner.file);
    let end_pos = if end == start {
        start_span.start
    } else {
        let (last_file, last_span) = ws.map_effective_origin(
            root,
            Span {
                start: end - 1,
                end,
            },
        )?;
        if last_file != owner.file || last_span.start != start_span.start + end - start - 1 {
            return None;
        }
        last_span.end
    };
    if end_pos as usize > owner.text.len() {
        return None;
    }
    let owner_index = LineIndex::new(&owner.text);
    let mapped_start = owner_index.position(&owner.text, start_span.start);
    let mapped_end = owner_index.position(&owner.text, end_pos);
    Some(TextEdit {
        start_line: mapped_start.line,
        start_char: mapped_start.character,
        end_line: mapped_end.line,
        end_char: mapped_end.character,
        new_text: fix.new_text.clone(),
    })
}

fn root_key(ws: &Workspace, path: &str) -> String {
    if ws.is_overlay_only() {
        path.to_string()
    } else {
        crate::include_resolver::normalize_uri(path)
    }
}

/// Rewrite I208–I210 onto the file that owns the first site.
/// A span that crosses two files keeps only its first byte.
fn place_writing_anchors(
    ws: &mut Workspace,
    root: &str,
    diags: &mut Vec<Diagnostic>,
) -> HashMap<String, WritingOrigin> {
    let mut origins = HashMap::new();
    diags.retain_mut(|diag| {
        if !crate::check_writing::is_writing_code(&diag.code) {
            return true;
        }
        let Some((file, span)) = mapped_writing_span(ws, root, diag.span) else {
            return false;
        };
        let Some(text) = ws.get_source(&file) else {
            return false;
        };
        origins.insert(
            diag.code.clone(),
            WritingOrigin {
                file,
                text: text.to_string(),
            },
        );
        diag.span = span;
        true
    });
    origins
}

fn mapped_writing_span(ws: &mut Workspace, root: &str, span: Span) -> Option<(String, Span)> {
    let (file, origin) = ws.map_effective_origin(root, span)?;
    if span.end > span.start.saturating_add(1) {
        let tail = Span {
            start: span.end - 1,
            end: span.end,
        };
        if let Some((end_file, _)) = ws.map_effective_origin(root, tail) {
            if end_file != file {
                let short = Span {
                    start: span.start,
                    end: span.start.saturating_add(1).min(span.end),
                };
                return ws.map_effective_origin(root, short);
            }
        }
    }
    Some((file, origin))
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
    format_check_lines_with_origins(
        path,
        &DiagnosticSet {
            root: String::new(),
            diagnostics: diags.to_vec(),
            writing_origins: HashMap::new(),
            origins: Vec::new(),
        },
        src,
    )
}

pub fn format_check_lines_with_origins(path: &str, set: &DiagnosticSet, src: &str) -> String {
    let diags = &set.diagnostics;
    if diags.is_empty() {
        return format!("No issues found in {path}\n");
    }
    let index = LineIndex::new(src);
    let mut errors = 0usize;
    let mut warnings = 0usize;
    let mut out = String::new();
    for (i, d) in diags.iter().enumerate() {
        let (display_path, pos) = if let Some(owner) = set.origins.get(i).and_then(Option::as_ref) {
            let owner_index = LineIndex::new(&owner.text);
            let display = if owner.file == set.root {
                path
            } else {
                &owner.file
            };
            (display, owner_index.position(&owner.text, owner.span.start))
        } else {
            (path, index.position(src, d.span.start))
        };
        let line = pos.line + 1;
        let col = pos.character + 1;
        let severity = severity_label(d.severity);
        out.push_str(&format!(
            "{display_path}:{line}:{col}: {severity} [{}] {}\n",
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
