use std::collections::HashMap;

use crate::model_diff::ModelDiff;

use super::schema::*;

/// Multiplication overflow is a limit, never permission for unbounded work.
pub fn alignment_availability(before: usize, after: usize, cells: usize) -> Availability {
    if before.checked_mul(after).is_some_and(|work| work <= cells) {
        Availability::Complete
    } else {
        Availability::LimitExceeded
    }
}

impl SemanticDiff {
    pub fn charge_token_alignment(&mut self, before: usize, after: usize) -> Availability {
        charge(
            &mut self.work.token_alignment_cells,
            before,
            after,
            self.budgets.token_alignment_cells,
        )
    }
    pub fn charge_source_alignment(&mut self, before: usize, after: usize) -> Availability {
        charge(
            &mut self.work.source_alignment_cells,
            before,
            after,
            self.budgets.source_alignment_cells,
        )
    }
}

fn charge(used: &mut usize, before: usize, after: usize, limit: usize) -> Availability {
    let Some(cells) = before
        .checked_mul(after)
        .and_then(|cells| used.checked_add(cells))
    else {
        return Availability::LimitExceeded;
    };
    if cells > limit {
        return Availability::LimitExceeded;
    }
    *used = cells;
    Availability::Complete
}

fn json_bytes<T: serde::Serialize>(value: &T) -> usize {
    serde_json::to_vec(value)
        .map(|value| value.len())
        .unwrap_or(usize::MAX)
}

fn detail_bytes(diff: &ModelDiff) -> usize {
    diff.semantic
        .rows
        .iter()
        .map(json_bytes)
        .chain(diff.semantic.references.iter().map(json_bytes))
        .chain(
            diff.source_changes
                .files
                .iter()
                .flat_map(|file| file.hunks.iter().map(json_bytes)),
        )
        .fold(0, usize::saturating_add)
}

fn add_limit(
    limits: &mut Vec<ComparisonLimit>,
    code: &str,
    reason: &str,
    omitted: usize,
    owner: &str,
) {
    if omitted == 0 {
        return;
    }
    if let Some(existing) = limits.iter_mut().find(|limit| limit.code == code) {
        existing.omitted = Some(existing.omitted.unwrap_or(0).saturating_add(omitted));
    } else {
        let mut limit = ComparisonLimit::new(code, reason, owner);
        limit.omitted = Some(omitted);
        limits.push(limit);
    }
}

/// Apply comparison-wide reference, hunk and optional serialized-detail caps.
/// Complete captured-file actions survive hunk omission. Legacy rows survive
/// every cap; the resulting availability cannot imply a complete review.
pub fn enforce_output_budget(diff: &mut ModelDiff) {
    let cap = diff.semantic.budgets.references_per_side;
    let mut counts = [0_usize; 2];
    let old_references = diff.semantic.references.len();
    diff.semantic.references.retain(|reference| {
        let side = if reference.side == Side::Before { 0 } else { 1 };
        counts[side] += 1;
        counts[side] <= cap
    });
    let omitted_references = old_references - diff.semantic.references.len();
    add_limit(
        &mut diff.semantic.limits,
        "reference_limit",
        "Direct equation references were omitted; the reference list is partial.",
        omitted_references,
        "semantic_equations",
    );
    if omitted_references > 0 {
        diff.semantic.availability = Availability::Partial;
        diff.coverage.availability = Availability::Partial;
        add_limit(
            &mut diff.coverage.limits,
            "reference_limit",
            "Direct equation references were omitted; the comparison detail is partial.",
            omitted_references,
            "semantic_equations",
        );
    }

    let mut remaining_hunks = diff.semantic.budgets.source_hunks;
    for file in &mut diff.source_changes.files {
        let keep = file.hunks.len().min(remaining_hunks);
        remaining_hunks -= keep;
        let omitted = file.hunks.len() - keep;
        file.hunks.truncate(keep);
        omit_hunks(file, omitted, "source_hunk_limit");
    }

    let limit = diff.semantic.budgets.serialized_output_bytes;
    let mut bytes = detail_bytes(diff);
    let mut omitted_rows = 0;
    let mut omitted_reference_bytes = 0;
    let mut omitted_hunk_bytes = 0;
    // Remove optional highlights first, retaining exact expression text.
    if bytes > limit {
        for row in &mut diff.semantic.rows {
            let old_size = json_bytes(row);
            for expression in &mut row.expressions {
                for side in [&mut expression.before, &mut expression.after]
                    .into_iter()
                    .flatten()
                {
                    side.runs = vec![TokenRun {
                        text: side.text.clone(),
                        role: TokenRole::Unchanged,
                    }];
                }
                expression.availability = Availability::LimitExceeded;
                expression.highlight_basis = HighlightBasis::None;
                expression.reason = Some("Serialized detail limit; exact expression text is retained without highlights.".into());
            }
            bytes = bytes
                .saturating_sub(old_size)
                .saturating_add(json_bytes(row));
        }
    }
    while bytes > limit && !diff.semantic.references.is_empty() {
        let reference = diff.semantic.references.pop().expect("nonempty");
        bytes = bytes.saturating_sub(json_bytes(&reference));
        omitted_reference_bytes += 1;
    }
    for file in diff.source_changes.files.iter_mut().rev() {
        let mut omitted = 0;
        while bytes > limit && !file.hunks.is_empty() {
            let hunk = file.hunks.pop().expect("nonempty");
            bytes = bytes.saturating_sub(json_bytes(&hunk));
            omitted += 1;
        }
        omit_hunks(file, omitted, "serialized_output_limit");
        omitted_hunk_bytes += omitted;
    }
    while bytes > limit && !diff.semantic.rows.is_empty() {
        let row = diff.semantic.rows.pop().expect("nonempty");
        bytes = bytes.saturating_sub(json_bytes(&row));
        omitted_rows += 1;
    }
    add_limit(&mut diff.semantic.limits, "serialized_output_limit", "Semantic rows were omitted by the serialized detail limit; legacy structural rows remain available.", omitted_rows, "semantic_diff");
    add_limit(
        &mut diff.semantic.limits,
        "reference_output_limit",
        "References were omitted by the serialized detail limit.",
        omitted_reference_bytes,
        "semantic_equations",
    );
    if omitted_rows > 0 || omitted_reference_bytes > 0 || omitted_hunk_bytes > 0 {
        diff.semantic.availability = Availability::Partial;
        diff.coverage.availability = Availability::Partial;
        diff.coverage.limits.push(ComparisonLimit::new(
            "detail_output_partial",
            "Optional comparison detail was omitted by an output limit.",
            "semantic_diff",
        ));
    }
    let retained: HashMap<_, _> = diff
        .semantic
        .references
        .iter_mut()
        .enumerate()
        .map(|(index, reference)| {
            let old_pointer = reference.pointer.clone();
            reference.pointer = format!("/semantic/references/{index}");
            (old_pointer, reference.pointer.clone())
        })
        .collect();
    for reference in &mut diff.semantic.references {
        if let Some(pointer) = retained.get(&reference.equation_pointer) {
            reference.equation_pointer = pointer.clone();
        }
    }
    for row in &mut diff.semantic.rows {
        let old_count = row.references.len();
        row.references = row
            .references
            .iter()
            .filter_map(|pointer| retained.get(pointer).cloned())
            .collect();
        add_limit(
            &mut row.limits,
            "references_partial",
            "Direct equation references for this row were omitted.",
            old_count - row.references.len(),
            "semantic_equations",
        );
    }
    // Link/limit repair changes row JSON sizes. Enforce the byte cap after it.
    let mut final_bytes = detail_bytes(diff);
    let mut additional_rows = 0;
    while final_bytes > limit && !diff.semantic.rows.is_empty() {
        let row = diff.semantic.rows.pop().expect("nonempty");
        final_bytes = final_bytes.saturating_sub(json_bytes(&row));
        additional_rows += 1;
    }
    add_limit(&mut diff.semantic.limits, "serialized_output_limit", "Semantic rows were omitted by the serialized detail limit; legacy structural rows remain available.", additional_rows, "semantic_diff");
    if additional_rows > 0 {
        diff.semantic.availability = Availability::Partial;
        diff.coverage.availability = Availability::Partial;
    }
    if diff
        .source_changes
        .files
        .iter()
        .any(|file| file.availability != Availability::Complete)
        && !diff.source_changes.files.is_empty()
    {
        diff.source_changes.availability = Availability::Partial;
        diff.coverage.availability = Availability::Partial;
    }
}

fn omit_hunks(file: &mut SourceFileChange, omitted: usize, code: &str) {
    if omitted == 0 {
        return;
    }
    file.omitted_hunks = Some(file.omitted_hunks.unwrap_or(0).saturating_add(omitted));
    file.availability = Availability::Partial;
    add_limit(&mut file.limits, code, "Source hunks were omitted; the captured-file text diff remains available when exact_text_available is true.", omitted, "source_changes");
}
