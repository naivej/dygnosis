//! Captured written-source comparison. Pairing is supplied proof, not inference.
//!
//! Inputs contain exactly one selected root and its executed includes, including
//! empty files. Capture failures must not enter this complete-input interface.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use crate::model_diff::ModelDiff;
use crate::parser::normalize_newlines;

use super::schema::*;

const CONTEXT_LINES: usize = 3;
/// Source detail allocations have a linear bound even for a one-sided file,
/// whose quadratic alignment charge would be zero.
pub const MAX_SOURCE_MATERIALIZED_LINES: usize = 100_000;
const MAX_SOURCE_MATERIALIZED_BYTES: usize = 8 * 1024 * 1024;

struct MaterializationBudget {
    lines: usize,
    text_bytes: usize,
}

/// The capture boundary remains visible even when its captured files are equal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureBoundary {
    RootAndExecutedIncludes,
    SuppliedRootAndExecutedIncludes,
}

/// An already complete captured side; the registry retains each file's exact
/// parser-normalized text for file-text actions after hunk omission.
#[derive(Clone, Copy, Debug)]
pub struct CapturedSourceInput<'a> {
    pub input_id: Option<&'a str>,
    pub root_key: &'a str,
    pub sources: &'a BTreeMap<String, String>,
    pub boundary: CaptureBoundary,
}

/// A trusted adapter establishes this identity before calling the comparator.
/// Equal suffixes, filenames, or text similarity do not supply this proof.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceIdentityProof {
    /// Both registries use the same repository key. The adapter also proves
    /// that the keys belong to the same selected repository across inputs.
    SameRepositoryKey,
    /// The adapter proves identity across registry aliases using the selected
    /// repository/written-file mapping, not suffix or text similarity.
    SameWrittenFileIdentity,
}

/// Includes need explicit identity proof. Root selection supplies its own pair.
#[derive(Clone, Copy, Debug)]
pub struct SourceFilePair<'a> {
    pub before_key: &'a str,
    pub after_key: &'a str,
    pub proof: SourceIdentityProof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceInputError {
    pub code: &'static str,
    pub reason: String,
}

/// Populate source changes from complete captured maps. Call the shared output
/// budget after all semantic/source producers have populated the comparison.
pub fn populate_captured_sources(
    diff: &mut ModelDiff,
    before: CapturedSourceInput<'_>,
    after: CapturedSourceInput<'_>,
    include_pairs: &[SourceFilePair<'_>],
) -> Result<(), SourceInputError> {
    if let Err(error) = validate_inputs(before, after, include_pairs) {
        diff.source_changes = SourceChanges {
            schema_version: 1,
            availability: Availability::NotAvailable,
            files: Vec::new(),
            limits: vec![ComparisonLimit::new(
                error.code,
                &error.reason,
                "source_changes",
            )],
        };
        set_boundary(diff, "capture_unavailable");
        diff.coverage.availability = Availability::Partial;
        diff.coverage.limits.push(ComparisonLimit::new(
            error.code,
            &error.reason,
            "source_changes",
        ));
        return Err(error);
    }

    let mut sources = SourceChanges {
        schema_version: 1,
        availability: Availability::Complete,
        files: Vec::new(),
        limits: Vec::new(),
    };
    let mut remaining_hunks = diff.semantic.budgets.source_hunks;
    let mut materialization = MaterializationBudget {
        lines: MAX_SOURCE_MATERIALIZED_LINES,
        text_bytes: diff
            .semantic
            .budgets
            .serialized_output_bytes
            .min(MAX_SOURCE_MATERIALIZED_BYTES),
    };
    let mut old_paired = BTreeSet::from([before.root_key]);
    let mut new_paired = BTreeSet::from([after.root_key]);
    compare_file(
        diff,
        &mut sources,
        Some((before, before.root_key)),
        Some((after, after.root_key)),
        SourceCorrespondence::SelectedRoots,
        &mut remaining_hunks,
        &mut materialization,
    );
    // Stable output order does not depend on the adapter's proof-list order.
    let mut pairs: Vec<_> = include_pairs.iter().collect();
    pairs.sort_by_key(|pair| (pair.before_key, pair.after_key));
    for pair in pairs {
        old_paired.insert(pair.before_key);
        new_paired.insert(pair.after_key);
        compare_file(
            diff,
            &mut sources,
            Some((before, pair.before_key)),
            Some((after, pair.after_key)),
            SourceCorrespondence::ProvenFileIdentity,
            &mut remaining_hunks,
            &mut materialization,
        );
    }
    for key in before
        .sources
        .keys()
        .filter(|key| !old_paired.contains(key.as_str()))
    {
        compare_file(
            diff,
            &mut sources,
            Some((before, key)),
            None,
            SourceCorrespondence::Unpaired,
            &mut remaining_hunks,
            &mut materialization,
        );
    }
    for key in after
        .sources
        .keys()
        .filter(|key| !new_paired.contains(key.as_str()))
    {
        compare_file(
            diff,
            &mut sources,
            None,
            Some((after, key)),
            SourceCorrespondence::Unpaired,
            &mut remaining_hunks,
            &mut materialization,
        );
    }
    if sources
        .files
        .iter()
        .any(|file| file.availability != Availability::Complete)
    {
        sources.availability = Availability::Partial;
        diff.coverage.availability = Availability::Partial;
    }
    diff.source_changes = sources;
    set_boundary(diff, boundary_name(before.boundary, after.boundary));
    diff.coverage.limits.retain(|limit| {
        !matches!(
            limit.code.as_str(),
            "sources_not_captured" | "supplied_source_boundary"
        )
    });
    diff.coverage.limits.push(ComparisonLimit::new(
        "sources_not_captured",
        "Source comparison covers the selected roots and executed includes. Unexecuted child files, external MATLAB functions and data files are not captured.",
        "source_changes",
    ));
    if before.boundary == CaptureBoundary::SuppliedRootAndExecutedIncludes
        || after.boundary == CaptureBoundary::SuppliedRootAndExecutedIncludes
    {
        diff.coverage.limits.push(ComparisonLimit::new(
            "supplied_source_boundary",
            "Supplied-text capture covers only the supplied root and supplied files actually used by executed includes. Other supplied or external files are not compared.",
            "source_changes",
        ));
    }
    Ok(())
}

fn validate_inputs(
    before: CapturedSourceInput<'_>,
    after: CapturedSourceInput<'_>,
    pairs: &[SourceFilePair<'_>],
) -> Result<(), SourceInputError> {
    let invalid = |reason: &str| SourceInputError {
        code: "invalid_captured_sources",
        reason: reason.into(),
    };
    if before.root_key.is_empty()
        || after.root_key.is_empty()
        || !before.sources.contains_key(before.root_key)
        || !after.sources.contains_key(after.root_key)
        || before.sources.keys().any(String::is_empty)
        || after.sources.keys().any(String::is_empty)
    {
        return Err(invalid(
            "Complete captured maps must contain both selected roots and nonempty file keys.",
        ));
    }
    if before.input_id.is_some() && before.input_id == after.input_id {
        return Err(invalid(
            "Captured Before and After input IDs must be distinct.",
        ));
    }
    let mut old = BTreeSet::new();
    let mut new = BTreeSet::new();
    for pair in pairs {
        if pair.before_key == before.root_key
            || pair.after_key == after.root_key
            || !before.sources.contains_key(pair.before_key)
            || !after.sources.contains_key(pair.after_key)
            || !old.insert(pair.before_key)
            || !new.insert(pair.after_key)
        {
            return Err(invalid(
                "Proven include pairs must be present, non-root and one-to-one.",
            ));
        }
        if pair.proof == SourceIdentityProof::SameRepositoryKey && pair.before_key != pair.after_key
        {
            return Err(invalid("Same-repository-key proof requires the same captured repository key on both sides."));
        }
    }
    Ok(())
}

fn set_boundary(diff: &mut ModelDiff, boundary: &str) {
    diff.coverage.source_boundary = boundary.into();
    diff.coverage.limits.retain(|limit| {
        !matches!(
            limit.code.as_str(),
            "capture_boundary_unavailable" | "invalid_captured_sources"
        )
    });
}

fn boundary_name(before: CaptureBoundary, after: CaptureBoundary) -> &'static str {
    match (before, after) {
        (CaptureBoundary::RootAndExecutedIncludes, CaptureBoundary::RootAndExecutedIncludes) => {
            "roots_and_executed_includes"
        }
        (
            CaptureBoundary::SuppliedRootAndExecutedIncludes,
            CaptureBoundary::SuppliedRootAndExecutedIncludes,
        ) => "supplied_roots_and_executed_includes",
        _ => "mixed_captured_and_supplied_roots_and_executed_includes",
    }
}

fn compare_file(
    diff: &mut ModelDiff,
    sources: &mut SourceChanges,
    before: Option<(CapturedSourceInput<'_>, &str)>,
    after: Option<(CapturedSourceInput<'_>, &str)>,
    correspondence: SourceCorrespondence,
    remaining_hunks: &mut usize,
    materialization: &mut MaterializationBudget,
) {
    let old_raw = before.map(|(input, key)| input.sources[key].as_str());
    let new_raw = after.map(|(input, key)| input.sources[key].as_str());
    if old_raw
        .zip(new_raw)
        .is_some_and(|(old, new)| normalized_equal(old, new))
    {
        return;
    }
    let side = |input: CapturedSourceInput<'_>, key: &str| SourceFileSide {
        input_id: input.input_id.map(str::to_owned),
        file_key: key.into(),
        exact_text_available: true,
    };
    let mut file = SourceFileChange {
        pointer: format!("/source_changes/files/{}", sources.files.len()),
        change: match (before, after) {
            (None, _) => ChangeKind::Added,
            (_, None) => ChangeKind::Removed,
            _ => ChangeKind::Changed,
        },
        correspondence,
        before: before.map(|(input, key)| side(input, key)),
        after: after.map(|(input, key)| side(input, key)),
        availability: Availability::Complete,
        hunks: Vec::new(),
        omitted_hunks: None,
        limits: Vec::new(),
    };
    let bytes = old_raw
        .unwrap_or("")
        .len()
        .checked_add(new_raw.unwrap_or("").len());
    let lines = bytes
        .filter(|bytes| *bytes <= materialization.text_bytes)
        .and_then(|_| {
            let old_count = normalized_line_count(old_raw.unwrap_or(""), materialization.lines)?;
            let new_count =
                normalized_line_count(new_raw.unwrap_or(""), materialization.lines - old_count)?;
            old_count.checked_add(new_count)
        });
    if bytes.is_none_or(|bytes| bytes > materialization.text_bytes)
        || lines.is_none_or(|lines| lines > materialization.lines)
    {
        file.availability = Availability::LimitExceeded;
        file.limits.push(ComparisonLimit::new(
            "source_materialization_limit",
            "Source detail exceeded its comparison-wide 100,000 materialized-line or min(serialized detail bytes, 8 MiB) raw-text-byte budget. Hunks were omitted; their count is unknown. Exact captured-file text diff remains available.",
            "source_changes",
        ));
        sources.files.push(file);
        return;
    }
    materialization.text_bytes -= bytes.expect("checked budget");
    materialization.lines -= lines.expect("checked budget");
    let old_text = old_raw.map(normalized_text);
    let new_text = new_raw.map(normalized_text);
    let old_lines: Vec<_> = old_text
        .as_deref()
        .unwrap_or("")
        .split_inclusive('\n')
        .collect();
    let new_lines: Vec<_> = new_text
        .as_deref()
        .unwrap_or("")
        .split_inclusive('\n')
        .collect();
    let prefix = old_lines
        .iter()
        .zip(&new_lines)
        .take_while(|(a, b)| a == b)
        .count();
    let suffix = old_lines[prefix..]
        .iter()
        .rev()
        .zip(new_lines[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let old_middle = &old_lines[prefix..old_lines.len() - suffix];
    let new_middle = &new_lines[prefix..new_lines.len() - suffix];
    if diff
        .semantic
        .charge_source_alignment(old_middle.len(), new_middle.len())
        != Availability::Complete
    {
        file.availability = Availability::LimitExceeded;
        file.limits.push(ComparisonLimit::new(
            "source_alignment_limit",
            "Source line alignment exceeded the comparison work limit. Hunks were omitted; their count is unknown. Exact captured-file text diff remains available.",
            "source_changes",
        ));
    } else {
        let mut edits = Vec::with_capacity(old_lines.len().saturating_add(new_lines.len()));
        edits.extend(
            old_lines[..prefix]
                .iter()
                .map(|line| (TokenRole::Unchanged, *line)),
        );
        align_lines(old_middle, new_middle, &mut edits);
        edits.extend(
            old_lines[old_lines.len() - suffix..]
                .iter()
                .map(|line| (TokenRole::Unchanged, *line)),
        );
        build_hunks(&edits, &mut file, remaining_hunks);
    }
    sources.files.push(file);
}

fn normalized_equal(before: &str, after: &str) -> bool {
    fn characters(text: &str) -> impl Iterator<Item = char> + '_ {
        let mut characters = text.chars().peekable();
        std::iter::from_fn(move || {
            characters.next().map(|character| {
                if character == '\r' && characters.peek() != Some(&'\n') {
                    '\n'
                } else {
                    character
                }
            })
        })
    }
    before == after || characters(before).eq(characters(after))
}

fn normalized_text(text: &str) -> Cow<'_, str> {
    let bytes = text.as_bytes();
    if bytes
        .iter()
        .enumerate()
        .any(|(index, byte)| *byte == b'\r' && bytes.get(index + 1) != Some(&b'\n'))
    {
        Cow::Owned(normalize_newlines(text))
    } else {
        Cow::Borrowed(text)
    }
}

fn normalized_line_count(text: &str, cap: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut count = 0_usize;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' || *byte == b'\r' && bytes.get(index + 1) != Some(&b'\n') {
            count += 1;
            if count > cap {
                return None;
            }
        }
    }
    if bytes
        .last()
        .is_some_and(|byte| !matches!(byte, b'\r' | b'\n'))
    {
        count += 1;
    }
    (count <= cap).then_some(count)
}

/// Inputs have already spent their comparison-wide N*M work charge. Empty
/// sides need no matrix; common prefixes/suffixes are outside this alignment.
fn align_lines<'a>(before: &[&'a str], after: &[&'a str], out: &mut Vec<(TokenRole, &'a str)>) {
    if before.is_empty() {
        out.extend(after.iter().map(|line| (TokenRole::Added, *line)));
        return;
    }
    if after.is_empty() {
        out.extend(before.iter().map(|line| (TokenRole::Removed, *line)));
        return;
    }
    let width = after.len() + 1;
    let mut lengths = vec![0_usize; (before.len() + 1) * width];
    for i in (0..before.len()).rev() {
        for j in (0..after.len()).rev() {
            lengths[i * width + j] = if before[i] == after[j] {
                lengths[(i + 1) * width + j + 1] + 1
            } else {
                lengths[(i + 1) * width + j].max(lengths[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    while i < before.len() || j < after.len() {
        if i < before.len() && j < after.len() && before[i] == after[j] {
            out.push((TokenRole::Unchanged, before[i]));
            i += 1;
            j += 1;
        } else if i < before.len()
            && (j == after.len() || lengths[(i + 1) * width + j] >= lengths[i * width + j + 1])
        {
            out.push((TokenRole::Removed, before[i]));
            i += 1;
        } else {
            out.push((TokenRole::Added, after[j]));
            j += 1;
        }
    }
}

fn build_hunks(edits: &[(TokenRole, &str)], file: &mut SourceFileChange, remaining: &mut usize) {
    let changed: Vec<_> = edits
        .iter()
        .enumerate()
        .filter_map(|(index, (role, _))| (*role != TokenRole::Unchanged).then_some(index))
        .collect();
    let mut intervals: Vec<(usize, usize)> = Vec::new();
    for index in changed {
        let first = index.saturating_sub(CONTEXT_LINES);
        let last = (index + CONTEXT_LINES + 1).min(edits.len());
        if let Some(interval) = intervals.last_mut().filter(|interval| first <= interval.1) {
            interval.1 = interval.1.max(last);
        } else {
            intervals.push((first, last));
        }
    }
    let omitted = intervals.len().saturating_sub(*remaining);
    if omitted > 0 {
        file.omitted_hunks = Some(omitted);
        file.availability = Availability::Partial;
        let mut limit = ComparisonLimit::new(
            "source_hunk_limit",
            "Source hunks were omitted; exact captured-file text diff remains available.",
            "source_changes",
        );
        limit.omitted = Some(omitted);
        file.limits.push(limit);
    }
    let mut old_line = 1_u32;
    let mut new_line = 1_u32;
    let mut cursor = 0;
    for (first, last) in intervals.into_iter().take(*remaining) {
        for (role, _) in &edits[cursor..first] {
            old_line += u32::from(*role != TokenRole::Added);
            new_line += u32::from(*role != TokenRole::Removed);
        }
        let lines = &edits[first..last];
        file.hunks.push(SourceHunk {
            before_start: old_line,
            before_lines: lines
                .iter()
                .filter(|(role, _)| *role != TokenRole::Added)
                .count() as u32,
            after_start: new_line,
            after_lines: lines
                .iter()
                .filter(|(role, _)| *role != TokenRole::Removed)
                .count() as u32,
            lines: lines
                .iter()
                .map(|(role, text)| SourceLine {
                    role: *role,
                    text: (*text).into(),
                })
                .collect(),
        });
        for (role, _) in lines {
            old_line += u32::from(*role != TokenRole::Added);
            new_line += u32::from(*role != TokenRole::Removed);
        }
        cursor = last;
        *remaining -= 1;
    }
}
