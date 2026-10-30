//! Selected source edits over captured inputs. Every edit is a proposal; this
//! adapter verifies diagnostic ownership and never saves a file.

use std::collections::{BTreeMap, HashMap};

use rmcp::ErrorData;
use serde_json::{Value, json};

use crate::engine::analysis::symbols::Edit;
use crate::engine::diagnostics::{Diagnostic, DiagnosticSet, TextEdit};
use crate::engine::editing::{auto_fix, equation_names, extract, format, shock_template};
use crate::engine::language::span::Span;
use crate::engine::workspace::Workspace;

use super::request::{Params, location, proposal_result};
use super::result::{OUTPUT_BYTES, ToolResult};
use super::source::{SourceSnapshot, sha256};

pub(super) fn edit_request(
    tool: &str,
    params: &Params,
    snapshot: &mut SourceSnapshot,
) -> Result<ToolResult, ErrorData> {
    snapshot
        .workspace
        .check_cancelled()
        .map_err(|_| cancelled_error())?;
    if snapshot.model.is_analysis_preview() && tool == "dynare_auto_fix" {
        return Ok(ToolResult::refused(
            "ANALYSIS_PREVIEW_EDIT_UNSUPPORTED",
            snapshot
                .workspace
                .root_analysis_configuration(&snapshot.root)
                .preview_edit_cause("Diagnostic fixes"),
        ));
    }
    if snapshot.model.is_analysis_preview()
        && tool == "dynare_code_actions"
        && params.action_id.is_some()
    {
        return Ok(ToolResult::refused(
            "ANALYSIS_PREVIEW_EDIT_UNSUPPORTED",
            snapshot
                .workspace
                .root_analysis_configuration(&snapshot.root)
                .preview_edit_cause("Source actions"),
        ));
    }
    crate::engine::control::run(snapshot.workspace.cancellation(), || match tool {
        "dynare_format" => format_request(params, snapshot),
        "dynare_auto_fix" => fix_request(params, snapshot),
        "dynare_code_actions" => actions_request(params, snapshot),
        "dynare_extract" => extract_request(params, snapshot),
        _ => Err(ErrorData::internal_error("Unknown source-edit tool", None)),
    })
    .map_err(|_| cancelled_error())?
}

fn cancelled_error() -> ErrorData {
    ErrorData::internal_error(
        "The request was cancelled",
        Some(json!({"code":"CANCELLED"})),
    )
}

fn checked_diagnostics(
    workspace: &mut Workspace,
    root: &str,
) -> Result<DiagnosticSet, crate::engine::control::Cancelled> {
    match workspace.cancellation() {
        Some(flag) => crate::engine::diagnostics::check_in_workspace_with_origins_controlled(
            workspace, root, &flag,
        ),
        None => Ok(crate::engine::diagnostics::check_in_workspace_with_origins(
            workspace, root,
        )),
    }
}

fn invalid(message: impl Into<String>) -> ErrorData {
    ErrorData::invalid_params(message.into(), None)
}

fn chosen_file(params: &Params, snapshot: &mut SourceSnapshot) -> Result<String, ErrorData> {
    let file = params
        .file
        .as_deref()
        .or_else(|| {
            params
                .target
                .as_ref()
                .and_then(|target| target.file.as_deref())
        })
        .unwrap_or(&snapshot.root)
        .to_owned();
    if !snapshot
        .workspace
        .snapshot_sources(&snapshot.root)
        .contains_key(&file)
    {
        return Err(invalid(
            "The selected source file does not belong to the selected model",
        ));
    }
    Ok(file)
}

fn source_edit(
    snapshot: &SourceSnapshot,
    file: &str,
    span: Span,
    replacement: String,
) -> Result<Edit, ErrorData> {
    let expected = snapshot
        .text(Some(file))
        .and_then(|text| text.get(span.start as usize..span.end as usize))
        .ok_or_else(|| {
            ErrorData::internal_error("The proposed written range is unavailable", None)
        })?;
    Ok(Edit {
        file: file.into(),
        span,
        expected: expected.into(),
        replacement,
    })
}

fn proposed(snapshot: &SourceSnapshot, edits: &[Edit]) -> Result<ToolResult, ErrorData> {
    let (data, proposal) = proposal_result(snapshot, edits)?;
    let mut result = ToolResult::new(data);
    if edits.is_empty() {
        result.cause("unchanged", "NO_CHANGE", "No source text needs to change.");
    } else {
        result.outcome.status = "changed".into();
        result.proposal = Some(proposal);
    }
    Ok(result)
}

fn format_request(params: &Params, snapshot: &mut SourceSnapshot) -> Result<ToolResult, ErrorData> {
    let file = chosen_file(params, snapshot)?;
    let text = snapshot.text(Some(&file)).expect("checked source file");
    let indent = params
        .format_indent
        .as_ref()
        .map(format::parse_format_indent)
        .unwrap_or_else(|| Some("\t".into()))
        .ok_or_else(|| invalid("formatIndent must be tab or a whole number from 1 to 8"))?;
    let edit = if let Some(range) = &params.line_range {
        let starts: Vec<_> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        if range.start_line == 0
            || range.end_line < range.start_line
            || range.end_line as usize > starts.len()
        {
            return Err(invalid(
                "line_range must be ordered, one-based and inside the selected file",
            ));
        }
        match format::format_range_outcome_with_limit(
            text,
            range.start_line - 1,
            range.end_line - 1,
            &indent,
            snapshot.workspace.root_macro_work_limit(&snapshot.root),
        ) {
            format::RangeFormatOutcome::Changed(start, end, replacement) => {
                let start = starts[start as usize];
                let end = starts
                    .get(end as usize + 1)
                    .map_or(text.len(), |end| end - 1);
                Some(source_edit(
                    snapshot,
                    &file,
                    Span::new(start, end),
                    replacement,
                )?)
            }
            format::RangeFormatOutcome::Unchanged => None,
            format::RangeFormatOutcome::Unsupported(message) => {
                return Ok(ToolResult::refused("FORMAT_UNSUPPORTED", message));
            }
        }
    } else {
        match format::format_outcome_with_limit(
            text,
            &indent,
            snapshot.workspace.root_macro_work_limit(&snapshot.root),
        ) {
            format::FormatOutcome::Changed(replacement) => Some(source_edit(
                snapshot,
                &file,
                Span::new(0, text.len()),
                replacement,
            )?),
            format::FormatOutcome::Unchanged => None,
            format::FormatOutcome::Unsupported(message) => {
                return Ok(ToolResult::refused("FORMAT_UNSUPPORTED", message));
            }
        }
    };
    let mut result = proposed(snapshot, &edit.into_iter().collect::<Vec<_>>())?;
    result.data["status"] = json!(result.outcome.status);
    if let Some(file_text) = result
        .data
        .get("text")
        .cloned()
        .or_else(|| result.data["files"].get(&file).cloned())
    {
        result.data["formatted_text"] = file_text;
    }
    Ok(result)
}

struct OwnedDiagnostic {
    diagnostic: Diagnostic,
    file: String,
    span: Span,
    id: String,
}

fn owned_diagnostics(
    snapshot: &mut SourceSnapshot,
) -> Result<(DiagnosticSet, Vec<OwnedDiagnostic>), ErrorData> {
    let set = checked_diagnostics(&mut snapshot.workspace, &snapshot.root)
        .map_err(|_| cancelled_error())?;
    let map: HashMap<_, _> = snapshot
        .sources
        .iter()
        .map(|(file, text)| (file.clone(), text.clone()))
        .collect();
    let rows = super::diagnostics_to_json_with_origins(&snapshot.raw, &set, &map);
    let mut rows = rows.into_iter();
    let owned = set
        .diagnostics
        .iter()
        .enumerate()
        .filter(|(_, diagnostic)| !super::is_dropped_code(&diagnostic.code))
        .map(|(index, diagnostic)| {
            let origin = set.origins.get(index).and_then(Option::as_ref);
            let wire = serde_json::to_value(
                rows.next()
                    .expect("diagnostic rows correspond to retained diagnostics"),
            )
            .expect("diagnostic JSON");
            let id = format!(
                "{}:diagnostic:{}",
                snapshot.revision,
                sha256(wire.to_string())
            );
            OwnedDiagnostic {
                diagnostic: diagnostic.clone(),
                file: origin.map_or_else(|| snapshot.root.clone(), |origin| origin.file.clone()),
                span: origin.map_or(diagnostic.span, |origin| origin.span),
                id,
            }
        })
        .collect();
    Ok((set, owned))
}

fn select_diagnostics<'a>(
    params: &Params,
    snapshot: &mut SourceSnapshot,
    rows: &'a [OwnedDiagnostic],
) -> Result<Vec<&'a OwnedDiagnostic>, ErrorData> {
    let target = if let Some(target) = &params.target {
        let file = target.file.as_deref().unwrap_or(&snapshot.root).to_owned();
        if !snapshot
            .workspace
            .snapshot_sources(&snapshot.root)
            .contains_key(&file)
        {
            return Err(invalid("target.file does not belong to the selected model"));
        }
        let text = snapshot.text(Some(&file)).expect("checked target file");
        Some((
            file,
            super::request::byte_position(text, target.line, target.column)?,
        ))
    } else {
        None
    };
    if params
        .diagnostic_id
        .as_ref()
        .is_some_and(|id| !rows.iter().any(|row| &row.id == id))
    {
        return Err(invalid(
            "diagnostic_id does not belong to this input revision",
        ));
    }
    if params
        .diagnostic_ids
        .iter()
        .any(|id| !rows.iter().any(|row| &row.id == id))
    {
        return Err(invalid(
            "A selected diagnostic identity does not belong to this input revision",
        ));
    }
    Ok(rows
        .iter()
        .filter(|row| {
            params
                .code
                .as_ref()
                .is_none_or(|code| row.diagnostic.code == *code)
                && params.diagnostic_id.as_ref().is_none_or(|id| row.id == *id)
                && (params.diagnostic_ids.is_empty() || params.diagnostic_ids.contains(&row.id))
                && target.as_ref().is_none_or(|(file, byte)| {
                    row.file == *file && row.span.start <= *byte && *byte <= row.span.end
                })
        })
        .collect())
}

fn owner_count(set: &DiagnosticSet, owner: &Diagnostic) -> usize {
    set.diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.code == owner.code && diagnostic.message == owner.message)
        .count()
}

fn fix_span(text: &str, fix: &TextEdit) -> Option<Span> {
    let starts: Vec<_> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect();
    let at = |line: u32, column: u32| {
        let start = *starts.get(line as usize)?;
        let end = starts
            .get(line as usize + 1)
            .map_or(text.len(), |end| end - 1);
        let body = &text[start..end];
        Some(
            start
                + body
                    .char_indices()
                    .nth(column as usize)
                    .map_or(body.len(), |(at, _)| at),
        )
    };
    let start = at(fix.start_line, fix.start_char)?;
    let end = if fix.end_line as usize >= starts.len() {
        text.len()
    } else {
        at(fix.end_line, fix.end_char)?
    };
    (start <= end).then(|| Span::new(start, end))
}

fn candidate_workspace(
    snapshot: &SourceSnapshot,
    owner: &str,
    edits: &[Edit],
) -> Option<Workspace> {
    let mut workspace = snapshot.workspace.snapshot_with_root_settings(owner);
    let mut grouped = BTreeMap::<String, Vec<&Edit>>::new();
    for edit in edits {
        grouped.entry(edit.file.clone()).or_default().push(edit);
    }
    for (file, mut edits) in grouped {
        let mut source = workspace.get_source(&file)?.to_owned();
        edits.sort_by_key(|edit| std::cmp::Reverse((edit.span.start, edit.span.end)));
        for edit in edits {
            if source.get(edit.span.start as usize..edit.span.end as usize)
                != Some(edit.expected.as_str())
            {
                return None;
            }
            source.replace_range(
                edit.span.start as usize..edit.span.end as usize,
                &edit.replacement,
            );
        }
        workspace.update_document(&file, source);
    }
    Some(workspace)
}

fn checked_candidate(
    snapshot: &SourceSnapshot,
    owner: &str,
    edits: &[Edit],
) -> Result<DiagnosticSet, &'static str> {
    let Some(mut workspace) = candidate_workspace(snapshot, owner, edits) else {
        return Err("The proposed edit cannot be applied to its captured source.");
    };
    let set =
        checked_diagnostics(&mut workspace, owner).map_err(|_| "The request was cancelled")?;
    if workspace.acquisition_limit().is_some() {
        return Err("The proposed source exceeds the source analysis limit.");
    }
    let guards = workspace.input_guards(owner);
    if guards.iter().any(|guard| {
        !snapshot
            .guards
            .iter()
            .any(|original| original.file == guard.file)
    }) {
        return Err(
            "The proposed source requires a file that was not checked in the original input.",
        );
    }
    Ok(set)
}

fn verification_refusal(message: &'static str) -> ToolResult {
    let limited = message == "The proposed source exceeds the source analysis limit.";
    let mut result = ToolResult::refused(
        if limited {
            "SOURCE_LIMIT"
        } else {
            "FIX_VERIFICATION_UNAVAILABLE"
        },
        message,
    );
    if limited {
        result.limits.push(json!({"code":"SOURCE_LIMIT","affected":"proposed-source analysis","omitted":"the complete edit proposal"}));
    }
    result
}

fn repair(
    snapshot: &SourceSnapshot,
    set: &DiagnosticSet,
    row: &OwnedDiagnostic,
    macro_unsafe: bool,
) -> Result<Edit, &'static str> {
    if !auto_fix::eligible_code(&row.diagnostic.code) {
        return Err("This diagnostic is outside the supported automatic-fix set.");
    }
    let Some(fix) = &row.diagnostic.fix else {
        return Err("This diagnostic has no verified stored source fix.");
    };
    if macro_unsafe {
        return Err(
            "Automatic fixes were withheld because macro definitions or loops with identifier interpolation can change the written edit targets.",
        );
    }
    let Some(text) = snapshot.text(Some(&row.file)) else {
        return Err("The diagnostic's written source is unavailable.");
    };
    let Some(span) = fix_span(text, fix) else {
        return Err("The diagnostic's stored fix does not identify a written range.");
    };
    let edit = source_edit(snapshot, &row.file, span, fix.new_text.clone())
        .map_err(|_| "The diagnostic's stored edit is outside its written source.")?;
    if edit.expected == edit.replacement {
        return Err("The stored fix makes no source change.");
    }
    let candidate = checked_candidate(snapshot, &snapshot.root, std::slice::from_ref(&edit))?;
    if owner_count(&candidate, &row.diagnostic) >= owner_count(set, &row.diagnostic) {
        return Err("The stored fix did not reduce the diagnostic that owns it.");
    }
    for owner in snapshot
        .owner_roots
        .iter()
        .filter(|owner| *owner != &snapshot.root)
    {
        let mut before = snapshot.workspace.snapshot_with_root_settings(owner);
        if !before.snapshot_sources(owner).contains_key(&edit.file) {
            continue;
        }
        let before =
            checked_diagnostics(&mut before, owner).map_err(|_| "The request was cancelled")?;
        let after = checked_candidate(snapshot, owner, std::slice::from_ref(&edit))?;
        let errors = |set: &DiagnosticSet| {
            set.diagnostics
                .iter()
                .filter(|diagnostic| {
                    diagnostic.severity == crate::engine::diagnostics::Severity::Error
                })
                .count()
        };
        if errors(&after) > errors(&before) {
            return Err("The fix would add an error in another checked model owner.");
        }
    }
    Ok(edit)
}

fn remaining_json(snapshot: &SourceSnapshot, set: &DiagnosticSet, edits: &[Edit]) -> Value {
    let mut files: HashMap<_, _> = snapshot
        .sources
        .iter()
        .map(|(file, text)| (file.clone(), text.clone()))
        .collect();
    for edit in edits.iter().rev() {
        if let Some(text) = files.get_mut(&edit.file) {
            text.replace_range(
                edit.span.start as usize..edit.span.end as usize,
                &edit.replacement,
            );
        }
    }
    let root = files
        .get(&snapshot.root)
        .map_or(snapshot.raw.as_str(), String::as_str);
    serde_json::to_value(super::diagnostics_to_json_with_origins(root, set, &files))
        .expect("remaining diagnostics JSON")
}

fn fix_request(params: &Params, snapshot: &mut SourceSnapshot) -> Result<ToolResult, ErrorData> {
    if !params.agent_output
        && snapshot
            .workspace
            .root_analysis_configuration(&snapshot.root)
            .is_source()
        && snapshot.root_file.is_none()
        && params.target.is_none()
        && params.code.is_none()
        && params.diagnostic_id.is_none()
        && params.diagnostic_ids.is_empty()
    {
        let fixed_result = auto_fix::auto_fix_outcome_with_limit(
            &snapshot.raw,
            snapshot.workspace.root_macro_work_limit(&snapshot.root),
            snapshot.workspace.cancellation().as_ref(),
        );
        let fixed = match fixed_result {
            Ok(text) => text,
            Err(auto_fix::AutoFixFailure::Refused(message)) => {
                return Ok(ToolResult::refused("FIX_UNSAFE_MACRO", message));
            }
            Err(auto_fix::AutoFixFailure::Cancelled) => return Err(cancelled_error()),
        };
        let edits = if fixed == snapshot.raw {
            Vec::new()
        } else {
            vec![source_edit(
                snapshot,
                &snapshot.root,
                Span::new(0, snapshot.raw.len()),
                fixed,
            )?]
        };
        let after = match checked_candidate(snapshot, &snapshot.root, &edits) {
            Ok(after) => after,
            Err(message) => return Ok(verification_refusal(message)),
        };
        let mut result = proposed(snapshot, &edits)?;
        result.data["remaining_diagnostics"] = remaining_json(snapshot, &after, &edits);
        if after
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.fix.is_some())
            && !edits.is_empty()
        {
            result.cause("partial","FIXES_REMAIN","Verified fixes were proposed. Other diagnostics still have a stored fix and require a focused request.");
        }
        return Ok(result);
    }
    if params.target.is_none()
        && params.code.is_none()
        && params.diagnostic_id.is_none()
        && params.diagnostic_ids.is_empty()
    {
        return mapped_bulk_fix(snapshot);
    }
    fix_selected(params, snapshot, false)
}

fn macro_unsafe(snapshot: &SourceSnapshot) -> bool {
    auto_fix::refuse_macro_rewrite(
        &snapshot
            .sources
            .values()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
    )
}

fn fix_selected(
    params: &Params,
    snapshot: &mut SourceSnapshot,
    semantic_pass: bool,
) -> Result<ToolResult, ErrorData> {
    let (set, rows) = owned_diagnostics(snapshot)?;
    let mut selected = select_diagnostics(params, snapshot, &rows)?;
    if semantic_pass {
        selected.retain(|row| {
            row.diagnostic.fix.is_some()
                && row.diagnostic.code != "E001"
                && auto_fix::eligible_code(&row.diagnostic.code)
        });
    }
    let unsafe_macro = macro_unsafe(snapshot);
    let mut edits = Vec::<Edit>::new();
    let mut accepted = Vec::new();
    let mut omitted = Vec::new();
    for row in selected {
        if crate::engine::control::cancelled() {
            return Err(cancelled_error());
        }
        match repair(snapshot,&set,row,unsafe_macro) {
            Ok(edit) if edits.contains(&edit)=>accepted.push(json!({"diagnostic_id":row.id,"code":row.diagnostic.code})),
            Ok(edit) if edits.iter().any(|prior|prior.file==edit.file && ranges_overlap(prior.span,edit.span))=>omitted.push(json!({"diagnostic_id":row.id,"code":row.diagnostic.code,"reason":"The stored fix overlaps another selected fix."})),
            Ok(edit)=> {edits.push(edit);accepted.push(json!({"diagnostic_id":row.id,"code":row.diagnostic.code}));}
            Err(message)=>omitted.push(json!({"diagnostic_id":row.id,"code":row.diagnostic.code,"reason":message})),
        }
    }
    edits.sort_by_key(|edit| (edit.file.clone(), edit.span.start, edit.span.end));
    let after = if edits.is_empty() {
        set.clone()
    } else {
        match checked_candidate(snapshot, &snapshot.root, &edits) {
            Ok(after) => after,
            Err(message) => return Ok(verification_refusal(message)),
        }
    };
    for row in &rows {
        if accepted.iter().any(|item| item["diagnostic_id"] == row.id)
            && owner_count(&after, &row.diagnostic) >= owner_count(&set, &row.diagnostic)
        {
            return Ok(ToolResult::refused(
                "FIX_VERIFICATION_FAILED",
                "The combined fixes did not repair every selected diagnostic; no proposal was returned.",
            ));
        }
    }
    let mut result = proposed(snapshot, &edits)?;
    result.data["fixed_diagnostics"] = json!(accepted);
    result.data["omitted_actions"] = json!(omitted);
    result.data["remaining_diagnostics"] = remaining_json(snapshot, &after, &edits);
    if !omitted.is_empty() {
        result.cause(if edits.is_empty(){"unchanged"}else{"partial"},"FIXES_OMITTED",format!("{} verified source edits were proposed; {} selected diagnostics could not be fixed. See omitted_actions for each cause.",edits.len(),omitted.len()));
    }
    Ok(result)
}

fn proposed_capture(
    snapshot: &SourceSnapshot,
    edits: &[Edit],
) -> Result<SourceSnapshot, ErrorData> {
    let mut workspace = candidate_workspace(snapshot, &snapshot.root, edits)
        .ok_or_else(|| ErrorData::internal_error("Proposed input could not be captured", None))?;
    workspace
        .input_revision(&snapshot.root)
        .ok_or_else(|| ErrorData::internal_error("Proposed input revision is unavailable", None))?;
    let model = workspace
        .effective_model_snapshot(&snapshot.root)
        .ok_or_else(|| ErrorData::internal_error("Proposed model is unavailable", None))?;
    let report = workspace
        .expand_report_snapshot(&snapshot.root)
        .ok_or_else(|| ErrorData::internal_error("Proposed source map is unavailable", None))?;
    let mut sources = snapshot.sources.clone();
    for file in sources.keys().cloned().collect::<Vec<_>>() {
        if let Some(text) = workspace.get_source(&file) {
            sources.insert(file, text.to_owned());
        }
    }
    let raw = sources[&snapshot.root].clone();
    let line_indexes = sources
        .iter()
        .map(|(file, text)| {
            (
                file.clone(),
                crate::engine::language::span::LineIndex::new(text),
            )
        })
        .collect();
    Ok(SourceSnapshot {
        source_declaration: None,
        source_structure: None,
        workspace,
        root: snapshot.root.clone(),
        root_file: snapshot.root_file.clone(),
        owner_roots: snapshot.owner_roots.clone(),
        revision: snapshot.revision.clone(),
        model,
        report,
        raw,
        sources,
        line_indexes,
        analyses: BTreeMap::new(),
        inputs: snapshot.inputs.clone(),
        guards: snapshot.guards.clone(),
        coverage: snapshot.coverage.clone(),
    })
}

fn mapped_bulk_fix(snapshot: &mut SourceSnapshot) -> Result<ToolResult, ErrorData> {
    if macro_unsafe(snapshot) {
        return Ok(ToolResult::refused(
            "FIX_UNSAFE_MACRO",
            "Automatic fixes were withheld because macro definitions or loops with identifier interpolation can change the written edit targets.",
        ));
    }
    let mut current = proposed_capture(snapshot, &[])?;
    let mut fixed = Vec::new();
    let mut omitted = Vec::new();
    let mut pass = 0;
    while pass < 4 {
        if crate::engine::control::cancelled() {
            return Err(cancelled_error());
        }
        let params = Params {
            code: (pass < 3).then(|| "E001".into()),
            ..Default::default()
        };
        let result = fix_selected(&params, &mut current, pass == 3)?;
        if result.outcome.status == "refused" {
            return Ok(result);
        }
        if let Some(rows) = result.data["fixed_diagnostics"].as_array() {
            fixed.extend(rows.iter().cloned());
        }
        if let Some(rows) = result.data["omitted_actions"].as_array() {
            omitted.extend(rows.iter().cloned());
        }
        let changed_text = result.data["files"]
            .as_object()
            .into_iter()
            .flat_map(|files| files.iter())
            .filter_map(|(file, text)| text.as_str().map(|text| (file, text)))
            .chain(
                result.data["text"]
                    .as_str()
                    .map(|text| (&current.root, text)),
            );
        let edits = changed_text
            .map(|(file, text)| {
                source_edit(
                    &current,
                    file,
                    Span::new(0, current.sources[file].len()),
                    text.into(),
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        if !edits.is_empty() {
            current = proposed_capture(&current, &edits)?;
        }
        pass = if edits.is_empty() && pass < 3 {
            3
        } else {
            pass + 1
        };
    }
    let edits = current
        .sources
        .iter()
        .filter(|(file, text)| snapshot.sources.get(*file) != Some(*text))
        .map(|(file, text)| {
            source_edit(
                snapshot,
                file,
                Span::new(0, snapshot.sources[file].len()),
                text.clone(),
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    let after = match checked_candidate(snapshot, &snapshot.root, &edits) {
        Ok(after) => after,
        Err(message) => return Ok(verification_refusal(message)),
    };
    let mut result = proposed(snapshot, &edits)?;
    result.data["fixed_diagnostics"] = json!(fixed);
    result.data["remaining_diagnostics"] = remaining_json(snapshot, &after, &edits);
    omitted.sort_by_key(Value::to_string);
    omitted.dedup();
    result.data["omitted_actions"] = json!(omitted);
    if !omitted.is_empty()
        || after
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.fix.is_some())
    {
        result.cause(if edits.is_empty(){"unchanged"}else{"partial"},"FIXES_REMAIN","Verified fixes were proposed where safe. Some diagnostics could not be repaired; inspect remaining_diagnostics and omitted_actions.");
    }
    Ok(result)
}

fn ranges_overlap(left: Span, right: Span) -> bool {
    (left.start < right.end && right.start < left.end)
        || (left.is_empty() && right.is_empty() && left.start == right.start)
}

struct Action {
    id: String,
    kind: &'static str,
    title: String,
    diagnostic_id: Option<String>,
    edits: Vec<Edit>,
    skipped: usize,
    skipped_rows: Vec<Value>,
}

fn action(
    snapshot: &SourceSnapshot,
    kind: &'static str,
    title: String,
    diagnostic_id: Option<String>,
    edits: Vec<Edit>,
    skipped: usize,
) -> Action {
    let identity = json!({"kind":kind,"diagnostic_id":diagnostic_id,"edits":edits.iter().map(|edit|json!({"file":edit.file,"start":edit.span.start,"end":edit.span.end,"text":edit.replacement})).collect::<Vec<_>>()});
    Action {
        id: format!(
            "{}:action:{}",
            snapshot.revision,
            sha256(identity.to_string())
        ),
        kind,
        title,
        diagnostic_id,
        edits,
        skipped,
        skipped_rows: Vec::new(),
    }
}

fn metadata_action(snapshot: &mut SourceSnapshot, row: &OwnedDiagnostic) -> Result<Action, String> {
    let context = row
        .diagnostic
        .writing
        .as_ref()
        .ok_or("The diagnostic has no captured metadata selection.")?;
    let (kind, plan) = match row.diagnostic.code.as_str() {
        "I208" => (
            "equation_tags",
            equation_names::equation_name_plan_captured(
                &mut snapshot.workspace,
                &snapshot.root,
                context,
            )?,
        ),
        "I209" => (
            "long_names",
            equation_names::long_name_plan_captured(
                &mut snapshot.workspace,
                &snapshot.root,
                context,
            )?,
        ),
        _ => return Err("This diagnostic has no metadata action.".into()),
    };
    let edits = plan
        .edits
        .iter()
        .map(|edit| source_edit(snapshot, &edit.file, edit.span, edit.new_text.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|cause| format!("Metadata source edit was not prepared: {cause}"))?;
    for owner in snapshot
        .owner_roots
        .iter()
        .filter(|owner| *owner != &snapshot.root)
    {
        let mut workspace = snapshot.workspace.snapshot_with_root_settings(owner);
        if !edits
            .iter()
            .any(|edit| workspace.snapshot_sources(owner).contains_key(&edit.file))
        {
            continue;
        }
        let set =
            crate::engine::diagnostics::check_in_workspace_with_origins(&mut workspace, owner);
        let mut compatible = Vec::new();
        for diagnostic in &set.diagnostics {
            let Some(context) = diagnostic.writing.as_ref() else {
                continue;
            };
            let plan = match kind {
                "equation_tags" => {
                    equation_names::equation_name_plan_captured(&mut workspace, owner, context)
                }
                _ => equation_names::long_name_plan_captured(&mut workspace, owner, context),
            };
            if let Ok(plan) = plan {
                compatible.extend(plan.edits);
            }
        }
        if edits
            .iter()
            .filter(|edit| workspace.snapshot_sources(owner).contains_key(&edit.file))
            .any(|edit| {
                !compatible.iter().any(|other| {
                    other.file == edit.file
                        && other.span == edit.span
                        && other.new_text == edit.replacement
                })
            })
        {
            return Err(format!(
                "Model owner {owner} does not accept the same metadata edits."
            ));
        }
    }
    let mut action = action(
        snapshot,
        kind,
        plan.title,
        Some(row.id.clone()),
        edits,
        plan.skipped,
    );
    action.skipped_rows=plan.skipped_rows.iter().map(|row|{
        let source=row.source.as_ref().and_then(|(file,span)|location(snapshot,file,*span));
        let equation=snapshot.report.model_map.equations.iter().find(|equation|equation.id==row.row_id);
        let name=snapshot.model.written_declarations.iter().find(|declaration|declaration.declaration.parse_order==row.row_id).map(|declaration|snapshot.model.name(declaration.declaration.name));
        json!({"location":source,"equation_id":equation.map(|equation|super::request::equation_id(snapshot,equation)),"name":name,"reason":row.reason})
    }).collect();
    Ok(action)
}

fn actions_request(
    params: &Params,
    snapshot: &mut SourceSnapshot,
) -> Result<ToolResult, ErrorData> {
    if params.action_kind.as_deref().is_some_and(|kind| {
        !matches!(
            kind,
            "diagnostic_fix" | "equation_tags" | "long_names" | "shock_template"
        )
    }) {
        return Err(invalid(
            "action_kind must be diagnostic_fix, equation_tags, long_names or shock_template",
        ));
    }
    let (set, rows) = owned_diagnostics(snapshot)?;
    let selected = select_diagnostics(params, snapshot, &rows)?;
    if snapshot.model.is_analysis_preview() {
        let cause = snapshot
            .workspace
            .root_analysis_configuration(&snapshot.root)
            .preview_edit_cause("Source actions");
        let omitted: Vec<_> = selected.iter().filter(|row|row.diagnostic.fix.is_some() || matches!(row.diagnostic.code.as_str(),"I208"|"I209")).map(|row|json!({"diagnostic_id":row.id,"code":"ANALYSIS_PREVIEW_EDIT_UNSUPPORTED","reason":cause})).collect();
        let mut result = ToolResult::new(json!({"actions":[],"omitted_actions":omitted}));
        result.cause("partial", "ANALYSIS_PREVIEW_EDIT_UNSUPPORTED", cause);
        return Ok(result);
    }
    let unsafe_macro = macro_unsafe(snapshot);
    let mut actions = Vec::new();
    let mut omitted = Vec::new();
    for row in selected {
        if params
            .action_kind
            .as_deref()
            .is_none_or(|kind| kind == "diagnostic_fix")
            && row.diagnostic.fix.is_some()
        {
            match repair(snapshot, &set, row, unsafe_macro) {
                Ok(edit) => actions.push(action(
                    snapshot,
                    "diagnostic_fix",
                    format!("Fix {}: {}", row.diagnostic.code, row.diagnostic.message),
                    Some(row.id.clone()),
                    vec![edit],
                    0,
                )),
                Err(message) => omitted
                    .push(json!({"diagnostic_id":row.id,"kind":"diagnostic_fix","reason":message})),
            }
        }
        if matches!(row.diagnostic.code.as_str(), "I208" | "I209")
            && params.action_kind.as_deref().is_none_or(|kind| {
                kind == if row.diagnostic.code == "I208" {
                    "equation_tags"
                } else {
                    "long_names"
                }
            })
        {
            match metadata_action(snapshot, row) {
                Ok(action) => actions.push(action),
                Err(cause) => omitted.push(json!({"diagnostic_id":row.id,"code":"METADATA_PLAN_UNAVAILABLE","file":row.file,"kind":if row.diagnostic.code=="I208"{"equation_tags"}else{"long_names"},"reason":cause})),
            }
        }
    }
    if params
        .action_kind
        .as_deref()
        .is_none_or(|kind| kind == "shock_template")
        && params.diagnostic_id.is_none()
        && params.code.is_none()
        && params
            .target
            .as_ref()
            .and_then(|target| target.file.as_deref())
            .is_none_or(|file| file == snapshot.root)
        && snapshot.coverage["complete"] == true
        && !shock_template::has_ordinary_shocks(&snapshot.model)
    {
        for kind in shock_template::available_kinds(&snapshot.model) {
            let names = match kind {
                shock_template::TemplateKind::Stochastic => {
                    shock_template::aggregate_varexo_names(&snapshot.model)
                }
                shock_template::TemplateKind::Deterministic => {
                    shock_template::aggregate_deterministic_names(&snapshot.model)
                }
            };
            if let Some(insertion) = shock_template::insertion(&snapshot.raw, &names, kind) {
                let edit = source_edit(
                    snapshot,
                    &snapshot.root,
                    Span::new(insertion.byte as usize, insertion.byte as usize),
                    insertion.new_text,
                )?;
                actions.push(action(
                    snapshot,
                    "shock_template",
                    kind.title().into(),
                    None,
                    vec![edit],
                    0,
                ));
            }
        }
    }
    actions.sort_by(|left, right| left.id.cmp(&right.id));
    if let Some(id) = &params.action_id {
        let Some(action) = actions.iter().find(|action| &action.id == id) else {
            return Err(invalid(
                "action_id is unavailable for this input and selection",
            ));
        };
        let mut result = proposed(snapshot, &action.edits)?;
        result.data["action_id"] = json!(action.id);
        result.data["kind"] = json!(action.kind);
        result.data["title"] = json!(action.title);
        result.data["skipped_items"] = json!(action.skipped);
        result.data["skipped"] = json!(action.skipped_rows);
        if action.skipped > 0 {
            result.cause(
                "partial",
                "METADATA_ITEMS_SKIPPED",
                format!(
                    "{} source edits were proposed; {} items had no safe written edit.",
                    action.edits.len(),
                    action.skipped
                ),
            );
        }
        return Ok(result);
    }
    let mut wire = Vec::new();
    let alternative_kinds: Vec<_> = actions
        .iter()
        .filter(|action| {
            actions.iter().any(|other| {
                other.id != action.id
                    && other.kind == action.kind
                    && other.diagnostic_id == action.diagnostic_id
            })
        })
        .map(|action| action.kind)
        .collect();
    for action in actions {
        if params.agent_output {
            wire.push(json!({"action_id":action.id,"kind":action.kind,"title":action.title,"diagnostic_id":action.diagnostic_id,"changed_items":action.edits.len(),"skipped_items":action.skipped,"skipped":action.skipped_rows,"has_alternatives":alternative_kinds.contains(&action.kind)}));
            continue;
        }
        let (data, proposal) = proposal_result(snapshot, &action.edits)?;
        let row = json!({"action_id":action.id,"kind":action.kind,"title":action.title,"diagnostic_id":action.diagnostic_id,"changed_items":action.edits.len(),"skipped_items":action.skipped,"skipped":action.skipped_rows,"data":data,"proposal":proposal});
        if serde_json::to_vec(&row).expect("action JSON").len() > OUTPUT_BYTES {
            omitted.push(json!({"kind":action.kind,"reason":"The complete action proposal exceeds the output byte limit."}));
        } else {
            wire.push(row);
        }
    }
    let mut result = ToolResult::new(json!({"actions":wire,"omitted_actions":omitted}));
    if !omitted.is_empty() {
        result.cause("partial","ACTIONS_OMITTED","Available complete actions are listed. Other actions were withheld; omitted_actions gives each cause.");
    }
    Ok(result)
}

fn extract_request(
    params: &Params,
    snapshot: &mut SourceSnapshot,
) -> Result<ToolResult, ErrorData> {
    if params.equation_id.is_some() && (params.equation.is_some() || params.index.is_some())
        || params.equation.is_some() && params.index.is_some()
    {
        return Err(invalid(
            "Use one equation_id, equation or aggregate index selector",
        ));
    }
    let selector = if let Some(id) = &params.equation_id {
        let row =
            &snapshot.report.model_map.equations[super::request::selected_equation(snapshot, id)?];
        if row.local {
            return Err(invalid(
                "Extraction requires a model equation, not a model-local definition",
            ));
        }
        Some(extract::ExtractSelector::Occurrence(row.id))
    } else if let Some(equation) = &params.equation {
        let dimension = match equation.scope.as_str() {
            "aggregate" if equation.dimension.is_none() => None,
            "dimension" => Some(
                equation
                    .dimension
                    .clone()
                    .ok_or_else(|| invalid("Dimension equation selection requires dimension"))?,
            ),
            _ => {
                return Err(invalid(
                    "Equation scope must be aggregate or dimension; aggregate scope has no dimension",
                ));
            }
        };
        Some(extract::ExtractSelector::Counted {
            dimension,
            index: equation.index,
        })
    } else {
        params.index.map(|index| extract::ExtractSelector::Counted {
            dimension: None,
            index,
        })
    };
    let request = extract::ExtractRequest {
        file_content: snapshot.raw.clone(),
        active_file: snapshot.root_file.clone(),
        files: BTreeMap::new(),
        names: params.names.clone(),
        tags: params.tags.clone(),
        dimension: params.dimension.clone(),
    };
    let extraction = extract::extract_prepared(
        &request,
        &snapshot.model,
        &snapshot.report,
        snapshot.coverage["includes_complete"] != true,
        selector,
    )
    .map_err(|_| {
        invalid("Supply names, tags, equation_id, equation or aggregate index to select equations")
    })?;
    let map: HashMap<_, _> = snapshot
        .sources
        .iter()
        .map(|(file, text)| (file.clone(), text.clone()))
        .collect();
    let mut data = super::extract_json(&extraction, &snapshot.raw, Some(&map));
    if snapshot.root_file.is_none()
        && let Some(origins) = data["origins"].as_array_mut()
    {
        for origin in origins {
            origin["file"] = Value::Null;
            if let Some(frames) = origin["frames"].as_array_mut() {
                for frame in frames {
                    frame["file"] = Value::Null;
                }
            }
        }
    }
    let mut result = ToolResult::new(data);
    if extraction.status == extract::ExtractStatus::UnsupportedContext {
        result.cause("refused", "EXTRACTION_UNSUPPORTED", extraction.explanation);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::super::source::{RequestControl, SourceInput, resolve_source};
    use super::*;

    fn params(value: Value) -> Params {
        serde_json::from_value(value).unwrap()
    }
    fn text_snapshot(text: &str) -> SourceSnapshot {
        resolve_source(
            SourceInput::Text { text: text.into() },
            None,
            &RequestControl::default(),
        )
        .unwrap()
    }
    fn map_snapshot(root: &str, files: &[(&str, &str)]) -> SourceSnapshot {
        resolve_source(
            SourceInput::Map {
                root: root.into(),
                files: files
                    .iter()
                    .map(|(file, text)| ((*file).into(), (*text).into()))
                    .collect(),
                root_text: None,
                include_dirs: Vec::new(),
                owner_roots: Vec::new(),
            },
            None,
            &RequestControl::default(),
        )
        .unwrap()
    }

    #[test]
    fn cancelled_mapped_bulk_fix_returns_no_executable_subset() {
        // Guards returning a partial proposal after cancellation during the
        // proposed-input analysis of an already acquired mapped request.
        let mut snapshot = map_snapshot("root.mod", &[("root.mod", "var y;model;\ny=1\nend;")]);
        let flag = snapshot.workspace.cancellation().unwrap();
        crate::engine::control::cancel_after(&flag, 100);
        let result = edit_request("dynare_auto_fix", &Params::default(), &mut snapshot);
        let error = result.expect_err("a cancelled fix must return no proposal");
        assert_eq!(error.data.unwrap()["code"], "CANCELLED");
        assert!(flag.load(std::sync::atomic::Ordering::Relaxed));
        assert_eq!(snapshot.raw, "var y;model;\ny=1\nend;");
    }

    #[test]
    fn range_formatting_changes_only_selected_include_lines() {
        // Guards include selection editing the root and whole-line formatting changing outside source/native layout.
        let root = "@#include \"body.inc\"\n";
        let body = "// 😀 saved prefix\r\nvar x;\r\nmodel;\r\nx  =  0;\r\nend;\r\ndisp('leave   this');\r\n";
        let mut snapshot = map_snapshot("root.mod", &[("root.mod", root), ("body.inc", body)]);
        let result=edit_request("dynare_format",&params(json!({"file":"body.inc","line_range":{"start_line":4,"end_line":4},"formatIndent":2})),&mut snapshot).unwrap();
        assert_eq!(result.outcome.status, "changed");
        let proposed = result.data["files"]["body.inc"].as_str().unwrap();
        assert!(proposed.starts_with("// 😀 saved prefix\r\nvar x;\r\nmodel;\r\n"));
        assert!(proposed.ends_with("\r\nend;\r\ndisp('leave   this');\r\n"));
        assert!(result.data["files"].get("root.mod").is_none());
        assert_eq!(snapshot.raw, root);
        for range in [
            json!({"start_line":0,"end_line":1}),
            json!({"start_line":5,"end_line":4}),
            json!({"start_line":1,"end_line":99}),
        ] {
            assert!(
                edit_request(
                    "dynare_format",
                    &params(json!({"file":"body.inc","line_range":range})),
                    &mut snapshot
                )
                .is_err()
            );
        }
    }

    #[test]
    fn selected_include_fix_repairs_its_diagnostic_and_keeps_root_unchanged() {
        // Guards a stored include fix being applied to the root or accepted without repairing its exact owner.
        let root = "@#include \"body.inc\"\n";
        let body = "var y;\nmodel;\ny=0\nend;\n";
        let mut snapshot = map_snapshot("root.mod", &[("root.mod", root), ("body.inc", body)]);
        let (_, rows) = owned_diagnostics(&mut snapshot).unwrap();
        let row = rows
            .iter()
            .find(|row| row.diagnostic.code == "E001" && row.diagnostic.fix.is_some())
            .unwrap_or_else(|| {
                panic!(
                    "stored parse fix; diagnostics: {:?}",
                    rows.iter()
                        .map(|row| (
                            &row.diagnostic.code,
                            &row.diagnostic.message,
                            &row.diagnostic.fix,
                            &row.file,
                            row.span
                        ))
                        .collect::<Vec<_>>()
                )
            });
        let result = edit_request(
            "dynare_auto_fix",
            &params(json!({"diagnostic_id":row.id})),
            &mut snapshot,
        )
        .unwrap();
        assert_eq!(result.outcome.status, "changed", "{}", result.data);
        assert_eq!(
            result.data["files"]["body.inc"],
            "var y;\nmodel;\ny=0;\nend;\n"
        );
        assert!(result.data["files"].get("root.mod").is_none());
        assert!(
            !result.data["remaining_diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["code"] == "E001")
        );
        assert_eq!(snapshot.text(Some("body.inc")), Some(body));
        assert_eq!(result.proposal.unwrap()["edits"][0]["file"], "body.inc");
    }

    #[test]
    fn metadata_actions_keep_existing_names_and_select_one_complete_proposal() {
        // Guards metadata overwrite and treating alternative actions as one combined executable edit.
        let source = "var x (long_name='Output'), y;\nmodel;\n[name='eq1'] x=0;\ny=0;\nend;\n";
        let mut snapshot = text_snapshot(source);
        let listed =
            edit_request("dynare_code_actions", &params(json!({})), &mut snapshot).unwrap();
        assert!(listed.proposal.is_none());
        let actions = listed.data["actions"].as_array().unwrap();
        let tags = actions
            .iter()
            .find(|action| action["kind"] == "equation_tags")
            .expect("equation tags action");
        let text = tags["data"]["text"].as_str().unwrap();
        assert!(text.contains("[name='eq1'] x=0;"));
        assert!(text.contains("[name='eq2'] y=0;"));
        let labels = actions
            .iter()
            .find(|action| action["kind"] == "long_names")
            .expect("long names action");
        assert!(
            labels["data"]["text"]
                .as_str()
                .unwrap()
                .contains("x (long_name='Output')")
        );
        let selected = edit_request(
            "dynare_code_actions",
            &params(json!({"action_id":tags["action_id"],"action_kind":"equation_tags"})),
            &mut snapshot,
        )
        .unwrap();
        assert_eq!(selected.proposal.as_ref().unwrap(), &tags["proposal"]);
        assert_eq!(selected.outcome.status, "changed");
        assert_eq!(selected.proposal.unwrap()["edits"][0]["file"], Value::Null);
    }

    #[test]
    fn scoped_unnamed_extraction_keeps_local_dependencies() {
        // Guards an unnamed scope-qualified row selecting another scope and dropping its local dependency.
        let source = "heterogeneity_dimension h, g;\nvar y;\nvar(heterogeneity=h) c;\nvar(heterogeneity=g) d;\nmodel;\ny=0;\nend;\nmodel(heterogeneity=h);\n# helper=.9;\nc=helper*c(-1);\nend;\nmodel(heterogeneity=g);\nd=1;\nend;\n";
        let mut snapshot = text_snapshot(source);
        let result = edit_request(
            "dynare_extract",
            &params(json!({"equation":{"scope":"dimension","dimension":"h","index":0}})),
            &mut snapshot,
        )
        .unwrap();
        assert_eq!(result.outcome.status, "ok", "{}", result.data);
        let fragment = result.data["fragment"].as_str().unwrap();
        assert!(fragment.contains("# helper=.9;"), "{fragment}");
        assert!(fragment.contains("c=helper*c(-1);"), "{fragment}");
        assert!(!fragment.contains("d=1;"));
        assert!(!fragment.contains("y=0;"));
        assert_eq!(result.data["selected_equations"][0]["dimension"], "h");
        assert_eq!(result.data["origins"][0]["file"], Value::Null);
    }

    #[test]
    fn shock_actions_remain_commented_placeholders() {
        // Guards a source convenience inventing runnable shock sizes or periods.
        let source = "var x;\nvarexo e;\nmodel;\nx=e;\nend;\nstoch_simul(order=1);\n";
        let mut snapshot = text_snapshot(source);
        let result = edit_request(
            "dynare_code_actions",
            &params(json!({"action_kind":"shock_template"})),
            &mut snapshot,
        )
        .unwrap();
        let action = &result.data["actions"][0];
        assert_eq!(action["kind"], "shock_template");
        let inserted = action["proposal"]["edits"][0]["new_text"].as_str().unwrap();
        assert!(inserted.contains("// shocks;"));
        assert!(inserted.contains("// var e;"));
        assert!(inserted.contains("// stderr ;"));
        assert!(
            inserted
                .lines()
                .filter(|line| !line.trim().is_empty())
                .all(|line| line.starts_with("// "))
        );
        assert!(result.proposal.is_none());
    }

    #[test]
    fn mapped_bulk_fix_reports_semantic_diagnostics_exposed_by_parse_repair() {
        // Guards returning pre-repair diagnostics or inventing a stored fix after reparsing an include.
        let body = "var y;\nmodel;\ny=z\nend;\n";
        let mut snapshot = map_snapshot(
            "root.mod",
            &[("root.mod", "@#include \"body.inc\"\n"), ("body.inc", body)],
        );
        let result = edit_request("dynare_auto_fix", &params(json!({})), &mut snapshot).unwrap();
        assert_ne!(result.outcome.status, "refused", "{}", result.data);
        let fixed = result.data["files"]["body.inc"].as_str().unwrap();
        assert!(fixed.contains("y=z;"), "{fixed}");
        assert!(!fixed.contains("var z;"), "{}", result.data);
        assert!(
            !result.data["remaining_diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["code"] == "E001")
        );
        assert!(
            result.data["remaining_diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["code"] == "E020")
        );
        assert!(result.data["files"].get("root.mod").is_none());
    }

    #[test]
    fn partial_metadata_action_identifies_each_uneditable_macro_copy() {
        // Guards a count-only skip notice hiding which expanded equations had no safe source edit.
        let mut snapshot =
            text_snapshot("var x;\nmodel;\nx=0;\n@#for i in 1:2\nx=1;\n@#endfor\nend;\n");
        let listed = edit_request(
            "dynare_code_actions",
            &params(json!({"action_kind":"equation_tags"})),
            &mut snapshot,
        )
        .unwrap();
        let action = &listed.data["actions"][0];
        assert_eq!(action["skipped_items"], 2);
        let skipped = action["skipped"].as_array().unwrap();
        assert_eq!(skipped.len(), 2);
        assert!(skipped.iter().all(|row| {
            row["reason"].as_str().unwrap().contains("repeated")
                && row["equation_id"]
                    .as_str()
                    .unwrap()
                    .starts_with(&snapshot.revision)
        }));
        assert_ne!(skipped[0]["equation_id"], skipped[1]["equation_id"]);
        let selected = edit_request(
            "dynare_code_actions",
            &params(json!({"action_id":action["action_id"]})),
            &mut snapshot,
        )
        .unwrap();
        assert_eq!(selected.outcome.status, "partial");
        assert_eq!(
            selected.proposal.unwrap()["edits"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
