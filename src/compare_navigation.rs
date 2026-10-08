//! Additive navigation for the existing structural comparison. Pairing is owned
//! by model_diff; every navigation id is a JSON pointer into that unchanged diff.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{json, Value};
use tower_lsp::lsp_types::Url;

use crate::include_resolver::{is_virtual_uri, normalize_uri};
use crate::model::{AssignmentIndex, Model};
use crate::model_diff::{EquationChange, IndexedEquation, ModelDiff, ShockSetting};
use crate::model_map::{SourceOccurrence, WrittenSegment};
use crate::parser::normalize_newlines;
use crate::span::LineIndex;
use crate::workspace::Workspace;

pub(crate) enum Coordinates {
    Mcp,
    Lsp,
    SnapshotMcp,
    SnapshotLsp,
}

#[derive(Clone)]
struct Target {
    occurrence_id: String,
    dimension: Option<String>,
    segments: Vec<WrittenSegment>,
}

/// Captured while the comparison models are current. Rendering never reads disk
/// or changes a workspace, so a target and revision belong to the same snapshot.
pub(crate) struct ComparisonInput {
    root_uri: Option<String>,
    root_key: String,
    revision: Option<String>,
    complete: bool,
    sources: HashMap<String, String>,
    file_names: HashMap<String, String>,
    equations: BTreeMap<(Option<String>, usize), Target>,
    parameters: BTreeMap<String, Target>,
    symbols: BTreeMap<String, Target>,
    shocks: HashMap<usize, Target>,
    snapshot_id: Option<String>,
    snapshot_commit: Option<String>,
}

impl ComparisonInput {
    pub(crate) fn capture<'a>(
        workspace: &mut Workspace,
        root: &str,
        root_uri: Option<&str>,
        revision: Option<String>,
        model: &Model,
        shocks: impl Iterator<Item = Option<&'a ShockSetting>>,
    ) -> Self {
        let root_key = if workspace.snapshot_lookup.is_some() {
            root.to_owned()
        } else {
            normalize_uri(root)
        };
        let report = workspace.expand_report(root).cloned();
        let complete = report
            .as_ref()
            .is_some_and(|report| report.complete && report.model_map.complete)
            && workspace.includes_complete(root);
        let mut input = Self {
            root_uri: root_uri.map(str::to_owned),
            root_key,
            revision,
            complete,
            sources: HashMap::new(),
            file_names: HashMap::new(),
            equations: BTreeMap::new(),
            parameters: BTreeMap::new(),
            symbols: BTreeMap::new(),
            shocks: HashMap::new(),
            snapshot_id: None,
            snapshot_commit: None,
        };
        let Some(report) = report.filter(|_| complete) else {
            return input;
        };
        for row in &report.model_map.equations {
            if let Some(number) = row.number {
                input.equations.insert(
                    (row.dimension.clone(), number - 1),
                    target(format!("e{}", row.id), &row.source, row.dimension.clone()),
                );
            }
        }
        // Match the exact assignment index used by legacy last_assignment. A
        // later macro copy can share its written span and still be the winner.
        for (index, assignment) in model.param_assignments.iter().enumerate() {
            let name = model.name(assignment.name).to_owned();
            input.parameters.remove(&name);
            if let Some(statement) = model.statements.iter().find(|statement| {
                matches!(statement.assignment, Some(AssignmentIndex::Parameter(i)) if i == index)
            }) {
                if let Some(source) = report.model_map.statements.get(statement.id) {
                    input.parameters.insert(name, target(format!("s{}", statement.id), source, None));
                }
            }
        }
        // The metadata comparison takes the first final declaration after this
        // same span ordering. Match its parse-order occurrence, never its name
        // or a repeated written span alone.
        let mut declarations = model.final_decls(&["var", "varexo", "varexo_det", "parameters"]);
        declarations.sort_by_key(|decl| (decl.span.start, decl.span.end));
        for decl in declarations {
            let name = model.name(decl.name).to_owned();
            if input.symbols.contains_key(&name) {
                continue;
            }
            let source = model
                .written_declarations
                .iter()
                .position(|written| {
                    written.declaration.name == decl.name
                        && written.declaration.parse_order == decl.parse_order
                })
                .and_then(|index| report.model_map.declarations.get(index))
                .or_else(|| {
                    // A retyped implicit local has a proven type-event site,
                    // although no declaration keyword was written there.
                    let event = model
                        .type_event_occurrences
                        .iter()
                        .find(|(index, range)| {
                            let event = &model.symbol_type_events[*index];
                            !event.changed
                                && event.name == decl.name
                                && range.end == decl.parse_order
                        })?
                        .0;
                    report
                        .model_map
                        .type_events
                        .iter()
                        .find(|(index, _)| *index == event)
                        .map(|(_, source)| source)
                });
            if let Some(source) = source {
                input.symbols.insert(
                    name,
                    target(
                        format!("d{}", decl.parse_order),
                        source,
                        model
                            .final_heterogeneity(decl)
                            .map(|dimension| model.name(dimension).to_owned()),
                    ),
                );
            }
        }
        for (name, parameter) in &mut input.parameters {
            parameter.dimension = input
                .symbols
                .get(name)
                .and_then(|symbol| symbol.dimension.clone());
        }
        for setting in shocks.flatten() {
            if let Some(span) = setting.source_span {
                input.shocks.insert(
                    setting.occurrence_id,
                    Target {
                        occurrence_id: format!("h{}", setting.occurrence_id),
                        dimension: setting.heterogeneity.clone(),
                        segments: workspace.map_effective_segments(root, span),
                    },
                );
            }
        }
        let files: BTreeSet<_> = input
            .equations
            .values()
            .chain(input.parameters.values())
            .chain(input.symbols.values())
            .chain(input.shocks.values())
            .flat_map(|target| target.segments.iter())
            .map(|segment| {
                segment
                    .file
                    .as_deref()
                    .unwrap_or(&input.root_key)
                    .to_owned()
            })
            .collect();
        for file in files {
            if let Some(text) = workspace.source_for_normalized_key(&file) {
                input.sources.insert(file, text.to_owned());
            }
        }
        input
    }

    pub(crate) fn with_snapshot_identity(mut self, input_id: &str, commit: Option<&str>) -> Self {
        self.snapshot_id = Some(input_id.to_owned());
        self.snapshot_commit = commit.map(str::to_owned);
        // All new source actions use the same normalized text as parsing.
        for text in self.sources.values_mut() {
            *text = normalize_newlines(text);
        }
        self
    }

    pub(crate) fn with_file_names<'a>(mut self, files: impl Iterator<Item = &'a String>) -> Self {
        for file in files {
            self.file_names.insert(normalize_uri(file), file.clone());
        }
        self
    }

    fn location(&self, segment: &WrittenSegment, coordinates: &Coordinates) -> Option<Value> {
        let key = segment.file.as_deref().unwrap_or(&self.root_key);
        let text = self.sources.get(key)?;
        text.get(segment.span.start as usize..segment.span.end as usize)?;
        if segment.span.is_empty() {
            return None;
        }
        let normalized = normalize_newlines(text);
        let index = LineIndex::new(&normalized);
        match coordinates {
            Coordinates::SnapshotMcp => {
                let start = index.position(&normalized, segment.span.start);
                let end = index.position(&normalized, segment.span.end);
                let mut location = json!({"input_id":self.snapshot_id, "file_key":key,
                    "line":start.line + 1, "column":start.character + 1,
                    "end_line":end.line + 1, "end_column":end.character + 1});
                if let Some(commit) = &self.snapshot_commit {
                    location["commit"] = json!(commit);
                }
                Some(location)
            }
            Coordinates::SnapshotLsp => {
                let start = index.position_utf16(&normalized, segment.span.start);
                let end = index.position_utf16(&normalized, segment.span.end);
                let mut location = json!({"input_id":self.snapshot_id, "file_key":key,
                    "range":{"start":{"line":start.line,"character":start.character},
                    "end":{"line":end.line,"character":end.character}}});
                if let Some(commit) = &self.snapshot_commit {
                    location["commit"] = json!(commit);
                }
                Some(location)
            }
            Coordinates::Mcp => {
                let start = index.position(text, segment.span.start);
                let end = index.position(text, segment.span.end);
                let file = if key == self.root_key {
                    self.root_uri.as_deref()
                } else {
                    Some(self.file_names.get(key).map(String::as_str).unwrap_or(key))
                };
                Some(
                    json!({"file": file, "line": start.line + 1, "column": start.character + 1,
                    "end_line": end.line + 1, "end_column": end.character + 1}),
                )
            }
            Coordinates::Lsp => {
                let uri = if key == self.root_key {
                    Url::parse(self.root_uri.as_deref()?).ok()?
                } else if is_virtual_uri(key) {
                    Url::parse(key).ok()?
                } else {
                    Url::from_file_path(key).ok()?
                };
                let start = index.position_utf16(text, segment.span.start);
                let end = index.position_utf16(text, segment.span.end);
                Some(
                    json!({"uri": uri, "range": {"start": {"line": start.line, "character": start.character},
                    "end": {"line": end.line, "character": end.character}}}),
                )
            }
        }
    }

    fn render(&self, target: Option<&Target>, coordinates: &Coordinates) -> Value {
        let Some(target) = target else {
            return Value::Null;
        };
        let locations: Vec<_> = target
            .segments
            .iter()
            .filter_map(|segment| self.location(segment, coordinates))
            .collect();
        if locations.is_empty() {
            return Value::Null;
        }
        json!({"occurrence_id": target.occurrence_id, "written_locations": locations,
            "domain": if target.dimension.is_some() { "heterogeneous" } else { "aggregate" }, "dimension": target.dimension})
    }

    fn envelope(&self) -> Value {
        if let Some(input_id) = &self.snapshot_id {
            return json!({"input_id":input_id,"root_file":self.root_key,"revision":self.revision,"complete":self.complete});
        }
        json!({"root_uri": self.root_uri, "revision": self.revision, "complete": self.complete})
    }
}

fn target(occurrence_id: String, source: &SourceOccurrence, dimension: Option<String>) -> Target {
    Target {
        occurrence_id,
        dimension,
        segments: source.segments.clone(),
    }
}

pub(crate) fn navigation_json(
    diff: &ModelDiff,
    before: &ComparisonInput,
    after: &ComparisonInput,
    coordinates: Coordinates,
) -> Value {
    let mut builder = Rows {
        before,
        after,
        coordinates,
        rows: Vec::new(),
    };
    for (list, names, old, new) in [
        ("added_endogenous", &diff.added_endogenous, false, true),
        ("removed_endogenous", &diff.removed_endogenous, true, false),
        ("common_endogenous", &diff.common_endogenous, true, true),
        ("added_exogenous", &diff.added_exogenous, false, true),
        ("removed_exogenous", &diff.removed_exogenous, true, false),
        ("common_exogenous", &diff.common_exogenous, true, true),
        ("added_parameters", &diff.added_parameters, false, true),
        ("removed_parameters", &diff.removed_parameters, true, false),
        ("common_parameters", &diff.common_parameters, true, true),
    ] {
        for (index, name) in names.iter().enumerate() {
            builder.symbol(&format!("/{list}/{index}"), name, old, new);
        }
    }
    for (index, change) in diff.changed_parameter_values.iter().enumerate() {
        builder.push(json!({"id": format!("/changed_parameter_values/{index}"), "kind": "parameter", "name": change.name}),
            before.parameters.get(&change.name), after.parameters.get(&change.name));
    }
    for (index, change) in diff.symbols_changed.iter().enumerate() {
        builder.symbol(
            &format!("/symbols_changed/{index}"),
            &change.name,
            true,
            true,
        );
    }
    builder.equations(
        "",
        &diff.added_equations,
        &diff.removed_equations,
        &diff.changed_equations,
    );
    builder.unmatched("", &diff.unmatched_same_name);
    for (index, dimension) in diff.heterogeneous_equations.iter().enumerate() {
        let prefix = format!("/heterogeneous_equations/{index}");
        builder.equations(
            &prefix,
            &dimension.added,
            &dimension.removed,
            &dimension.changed,
        );
        builder.unmatched(&prefix, &dimension.unmatched_same_name);
    }
    for (index, change) in diff.shock_setup_changes.iter().enumerate() {
        builder.push(json!({"id": format!("/shock_setup_changes/{index}"), "kind": "shock", "form": change.form,
            "role": change.role, "name": change.target, "dimension": change.before.as_ref().or(change.after.as_ref()).and_then(|setting| setting.heterogeneity.as_deref())}),
            change.before.as_ref().and_then(|setting| before.shocks.get(&setting.occurrence_id)),
            change.after.as_ref().and_then(|setting| after.shocks.get(&setting.occurrence_id)));
    }
    json!({"schema_version": if matches!(builder.coordinates, Coordinates::SnapshotLsp | Coordinates::SnapshotMcp) { 2 } else { 1 }, "before": before.envelope(), "after": after.envelope(), "rows": builder.rows})
}

struct Rows<'a> {
    before: &'a ComparisonInput,
    after: &'a ComparisonInput,
    coordinates: Coordinates,
    rows: Vec<Value>,
}

impl Rows<'_> {
    fn push(&mut self, mut row: Value, before: Option<&Target>, after: Option<&Target>) {
        row["before"] = self.before.render(before, &self.coordinates);
        row["after"] = self.after.render(after, &self.coordinates);
        self.rows.push(row);
    }

    fn symbol(&mut self, id: &str, name: &str, old: bool, new: bool) {
        self.push(
            json!({"id": id, "kind": "symbol", "name": name}),
            old.then(|| self.before.symbols.get(name)).flatten(),
            new.then(|| self.after.symbols.get(name)).flatten(),
        );
    }

    fn single_equation(&mut self, id: String, row: &IndexedEquation, old: bool) {
        let key = (row.dimension.clone(), row.index);
        self.push(
            json!({"id": id, "kind": "equation", "domain": row.domain, "dimension": row.dimension,
            "index_old": old.then_some(row.index), "index_new": (!old).then_some(row.index)}),
            old.then(|| self.before.equations.get(&key)).flatten(),
            (!old).then(|| self.after.equations.get(&key)).flatten(),
        );
    }

    fn equations(
        &mut self,
        prefix: &str,
        added: &[IndexedEquation],
        removed: &[IndexedEquation],
        changed: &[EquationChange],
    ) {
        let suffix = if prefix.is_empty() { "_equations" } else { "" };
        for (index, row) in added.iter().enumerate() {
            self.single_equation(format!("{prefix}/added{suffix}/{index}"), row, false);
        }
        for (index, row) in removed.iter().enumerate() {
            self.single_equation(format!("{prefix}/removed{suffix}/{index}"), row, true);
        }
        for (index, row) in changed.iter().enumerate() {
            self.push(json!({"id": format!("{prefix}/changed{suffix}/{index}"), "kind": "equation", "domain": row.domain,
                "dimension": row.dimension, "index_old": row.index_old, "index_new": row.index_new}),
                self.before.equations.get(&(row.dimension.clone(), row.index_old)),
                self.after.equations.get(&(row.dimension.clone(), row.index_new)));
        }
    }

    fn unmatched(&mut self, prefix: &str, groups: &[crate::model_diff::UnmatchedSameName]) {
        for (index, group) in groups.iter().enumerate() {
            for (side, rows, old) in [
                ("removed", &group.removed, true),
                ("added", &group.added, false),
            ] {
                for (row_index, row) in rows.iter().enumerate() {
                    self.single_equation(
                        format!("{prefix}/unmatched_same_name/{index}/{side}/{row_index}"),
                        row,
                        old,
                    );
                }
            }
        }
    }
}
