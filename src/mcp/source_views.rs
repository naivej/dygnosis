//! MCP projections of shared written-source records. No model facts are inferred here.

use rmcp::ErrorData;
use serde_json::{Value, json};
use std::collections::HashSet;

use crate::engine::analysis::source_inventory::{SourceItem, SourceItemKind};
use crate::engine::analysis_configuration::{
    AvailabilityCause, SourceFileSelection, SourceFileStructure, SourceStructureQuery,
};
use crate::engine::language::span::Span;

use super::canonical::View;
use super::request::{Params, byte_position};
use super::result::ToolResult;
use super::source::SourceSnapshot;

pub(super) struct Inspection {
    pub structure: SourceStructureQuery,
    pub declaration_target: Option<super::request::Target>,
}

fn invalid(message: &str) -> ErrorData {
    ErrorData::invalid_params(message.to_owned(), None)
}

pub(super) fn inspection(tool: &str, params: &Params) -> Result<Option<Inspection>, ErrorData> {
    let source_view = params.view == Some(View::Source);
    if params.file.is_some()
        && (tool == "dynare_equations" && !source_view
            || tool == "dynare_model_info"
                && !params
                    .output
                    .sections
                    .iter()
                    .any(|section| section == "source_structure"))
    {
        return Err(invalid(
            "file selects written source inspection. Select view=source for equations, or include source_structure in Model Info sections.",
        ));
    }
    if source_view
        && tool == "dynare_equations"
        && (params.equation_id.is_some()
            || params.equation.is_some()
            || params.index.is_some()
            || params.symbol.is_some()
            || params.binding_id.is_some()
            || params.target.is_some()
            || params.scope.is_some()
            || params.dimension.is_some())
    {
        return Err(invalid(
            "Source equation view accepts name, tags and file filters. It has no effective equation, binding, scope or index selectors.",
        ));
    }
    if source_view && params.binding_id.is_some() {
        return Err(invalid(
            "Source view accepts a literal name or written target, not an accepted binding identity.",
        ));
    }
    let needed = source_view
        || tool == "dynare_model_info"
            && params
                .output
                .sections
                .iter()
                .any(|section| section == "source_structure");
    if !needed {
        return Ok(None);
    }
    let file = params.file.as_ref().or_else(|| {
        params
            .target
            .as_ref()
            .and_then(|target| target.file.as_ref())
    });
    Ok(Some(Inspection {
        structure: SourceStructureQuery {
            files: file
                .map(|file| vec![SourceFileSelection::File(file.clone())])
                .unwrap_or_else(|| {
                    if source_view && tool == "dynare_symbol_info" && params.target.is_some() {
                        vec![SourceFileSelection::Root]
                    } else {
                        Vec::new()
                    }
                }),
        },
        declaration_target: (source_view && tool == "dynare_symbol_info")
            .then(|| params.target.clone())
            .flatten(),
    }))
}

fn range(snapshot: &SourceSnapshot, file: &SourceFileStructure, span: Span) -> Value {
    snapshot
        .range(file.file.as_deref().unwrap_or(&snapshot.root), span)
        .unwrap_or(Value::Null)
}

fn cause(snapshot: &SourceSnapshot, cause: &AvailabilityCause) -> Value {
    json!({"code":cause.code,"message":cause.message,"file":cause.file,"range":cause.span.and_then(|span|snapshot.range(cause.file.as_deref().unwrap_or(&snapshot.root),span)),"target":cause.site,"branch_id":cause.branch_id})
}

fn item_json(
    snapshot: &SourceSnapshot,
    file: &SourceFileStructure,
    index: usize,
    item: &SourceItem,
) -> Value {
    if crate::engine::control::cancelled() {
        return Value::Null;
    }
    let kind = match item.kind {
        SourceItemKind::Declaration => "declaration",
        SourceItemKind::Block => "block",
        SourceItemKind::Command => "command",
        SourceItemKind::Equation => "equation",
        SourceItemKind::MacroDefinition => "macro_definition",
        SourceItemKind::MacroDirective => "macro_directive",
        SourceItemKind::Conditional => "conditional",
        SourceItemKind::Branch => "branch",
        SourceItemKind::Loop => "loop",
        SourceItemKind::Fragment => "fragment",
    };
    let availability = file
        .availability
        .get(index)
        .filter(|row| row.source_id == item.source_id)
        .or_else(|| item.parent.and_then(|index| file.availability.get(index)));
    let occurrences: Vec<_> = availability.into_iter().flat_map(|row| &row.occurrences).map(|row|json!({
        "occurrence_id":row.occurrence_id,"include_call":row.include_call,"parent_include_call":row.parent_include_call,
        "loops":row.loops.iter().map(|step|json!({"frame_id":step.frame_id,"variable":step.variable,"value":step.value})).collect::<Vec<_>>(),
        "availability":row.availability,"cause":row.cause.as_deref().map(|value|cause(snapshot,value))})).collect();
    json!({"source_id":item.source_id,"file":file.file,"source_revision":file.source_revision,
        "kind":kind,"name":item.name,"written_kind":item.written_kind,"template":item.template,
        "long_name":item.declaration_metadata.as_ref().and_then(|metadata|metadata.long_name.as_ref()),
        "tex_name":item.declaration_metadata.as_ref().and_then(|metadata|metadata.tex_name.as_ref()),
        "namespace":item.source_namespace,
        "expression":item.local_definition.and_then(|span|file.text.get(span.start as usize..span.end as usize)),
        "range":range(snapshot,file,item.span),"selection_range":range(snapshot,file,item.selection),
        "text":matches!(item.kind,SourceItemKind::Equation|SourceItemKind::Fragment).then(||file.text.get(item.span.start as usize..item.span.end as usize)).flatten(),"tags":item.tags,
        "parent_source_id":item.parent.and_then(|index|file.inventory.items.get(index)).map(|parent|&parent.source_id),
        "conditions":item.path.iter().map(|step|json!({"opener":range(snapshot,file,step.opener),"branch":range(snapshot,file,step.branch),"loop_body":step.loop_body})).collect::<Vec<_>>(),
        "availability":availability.map(|row|row.availability).unwrap_or_default(),
        "cause":availability.and_then(|row|row.cause.as_deref()).map(|value|cause(snapshot,value)),"occurrences":occurrences})
}

fn coverage(snapshot: &SourceSnapshot) -> Value {
    let Some(structure) = &snapshot.source_structure else {
        return Value::Null;
    };
    json!({"complete":structure.coverage.complete,
        "indexed_files":structure.coverage.indexed_files.iter().map(|file|json!({"file":file.file,"source_revision":file.source_revision,"acquired":file.acquired,"executed":file.executed,"explicitly_selected":file.explicitly_selected,"complete":file.complete,"cause":file.cause})).collect::<Vec<_>>(),
        "unresolved_targets":structure.coverage.unresolved_targets.iter().map(|target|json!({"file":target.file,"expression":target.expression,"literal_target":target.literal_target,"cause":target.cause,"lookup_context":target.lookup_context,
            "range":snapshot.range(target.file.as_deref().unwrap_or(&snapshot.root),target.span)})).collect::<Vec<_>>()})
}

pub(super) fn structure_json(snapshot: &SourceSnapshot) -> Value {
    let Some(structure) = &snapshot.source_structure else {
        return Value::Null;
    };
    let items: Vec<_> = structure
        .files
        .iter()
        .flat_map(|file| {
            file.inventory
                .items
                .iter()
                .enumerate()
                .map(|(index, item)| item_json(snapshot, file, index, item))
        })
        .collect();
    let directives: Vec<_> = structure.files.iter().flat_map(|file|file.sites.iter().map(|site|json!({
        "target":site.target,"kind":site.kind,"name":site.name,"range":range(snapshot,file,site.span),"directive_range":range(snapshot,file,site.directive_span),"expected_text":site.expected_text,
        "branches":site.branches.iter().map(|branch|json!({"branch_id":branch.branch_id,"directive_range":range(snapshot,file,branch.directive_span),"body_range":range(snapshot,file,branch.body_span),"expected_text":branch.expected_text})).collect::<Vec<_>>() }))).collect();
    json!({"revision":structure.revision,"analysis_revision":structure.analysis_revision,"source_item_count":structure.coverage.complete.then_some(items.len()),"available_source_item_count":items.len(),"items":items,"directives":directives,"coverage":coverage(snapshot)})
}

pub(super) fn equations(
    snapshot: &SourceSnapshot,
    params: &Params,
) -> Result<ToolResult, ErrorData> {
    let structure = snapshot
        .source_structure
        .as_ref()
        .expect("source view acquired");
    let rows: Vec<_> = structure
        .files
        .iter()
        .flat_map(|file| {
            file.inventory
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.kind == SourceItemKind::Equation)
                .filter(|(_, item)| {
                    params
                        .name
                        .as_ref()
                        .is_none_or(|name| item.tags.get("name") == Some(name))
                        && params
                            .tags
                            .iter()
                            .all(|(key, value)| item.tags.get(key) == Some(value))
                })
                .map(|(index, item)| item_json(snapshot, file, index, item))
        })
        .collect();
    Ok(ToolResult::new(
        json!({"view":"source","label":"written equation templates","source_item_count":structure.coverage.complete.then_some(rows.len()),"available_source_item_count":rows.len(),"equation_templates":rows,"source_coverage":coverage(snapshot)}),
    ))
}

pub(super) fn symbols(snapshot: &SourceSnapshot, params: &Params) -> Result<ToolResult, ErrorData> {
    let structure = snapshot
        .source_structure
        .as_ref()
        .expect("source view acquired");
    let target = params
        .target
        .as_ref()
        .map(|target| {
            let file = target.file.as_deref();
            let source = structure
                .files
                .iter()
                .find(|source| {
                    source.file.as_deref() == file
                        || file.is_none() && source.file == snapshot.root_file
                })
                .ok_or_else(|| {
                    invalid("The written target file is unavailable under this source policy.")
                })?;
            Ok::<_, ErrorData>((
                source.file.clone(),
                byte_position(&source.text, target.line, target.column)?,
            ))
        })
        .transpose()?;
    let mut rows: Vec<_> = structure
        .files
        .iter()
        .flat_map(|file| {
            file.inventory
                .items
                .iter()
                .enumerate()
                .filter(|(_, item)| {
                    matches!(
                        item.kind,
                        SourceItemKind::Declaration | SourceItemKind::MacroDefinition
                    )
                })
                .filter(|(_, item)| {
                    params
                        .symbol
                        .as_ref()
                        .is_none_or(|name| item.name.as_ref() == Some(name))
                })
                .filter(|(_, item)| {
                    target.as_ref().is_none_or(|(selected, byte)| {
                        selected == &file.file
                            && item.selection.start <= *byte
                            && *byte < item.selection.end
                    })
                })
                .map(|(index, item)| {
                    let mut row = item_json(snapshot, file, index, item);
                    row["label"] = json!("source candidate");
                    row["verified_binding"] = json!(false);
                    row
                })
        })
        .collect();
    let mut row_ids: HashSet<_> = rows
        .iter()
        .map(|row| {
            (
                row["file"].as_str().map(str::to_owned),
                row["source_id"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect();
    for file in &structure.files {
        for site in file
            .inventory
            .named_sites
            .iter()
            .filter(|site| site.declaration)
        {
            if params.symbol.as_ref().is_some_and(|name| {
                file.text
                    .get(site.span.start as usize..site.span.end as usize)
                    .map(|value| value.trim_matches(['\'', '"']))
                    != Some(name.as_str())
            }) {
                continue;
            }
            let Some(item) = file.inventory.named_declaration(&file.text, site) else {
                continue;
            };
            if params
                .symbol
                .as_ref()
                .is_some_and(|name| item.name.as_ref() != Some(name))
                || target.as_ref().is_some_and(|(selected, byte)| {
                    selected != &file.file || *byte < site.span.start || *byte >= site.span.end
                })
            {
                continue;
            }
            if !row_ids.insert((file.file.clone(), item.source_id.clone())) {
                continue;
            }
            let mut row = item_json(snapshot, file, usize::MAX, &item);
            row["label"] = json!("source candidate");
            row["verified_binding"] = json!(false);
            rows.push(row);
        }
    }
    if let Some(declaration) = snapshot
        .source_declaration
        .as_ref()
        .and_then(|lookup| lookup.declaration.as_ref())
    {
        let existing = structure
            .files
            .iter()
            .find(|file| file.file.as_deref().unwrap_or(&snapshot.root) == declaration.file);
        let acquired = snapshot.source_declaration.as_ref().and_then(|lookup| {
            lookup
                .sources
                .iter()
                .find(|file| file.file == declaration.file)
        });
        let owned = acquired.and_then(|source| {
            source
                .inventory
                .as_ref()
                .map(|inventory| SourceFileStructure {
                    file: Some(source.file.clone()),
                    source_revision: source.revision.clone(),
                    text: std::sync::Arc::clone(&source.text),
                    inventory: std::sync::Arc::clone(inventory),
                    sites: std::sync::Arc::from([]),
                    availability: std::sync::Arc::from([]),
                    lookup: None,
                })
        });
        if let Some(file) = existing.or(owned.as_ref()) {
            let index = file
                .inventory
                .items
                .iter()
                .position(|item| item.source_id == declaration.item.source_id)
                .unwrap_or(usize::MAX);
            let mut row = item_json(snapshot, file, index, &declaration.item);
            row["label"] = json!("source candidate");
            row["verified_binding"] = json!(false);
            if row_ids.insert((file.file.clone(), declaration.item.source_id.clone())) {
                rows.push(row);
            }
        }
    }
    let empty = rows.is_empty();
    let mut result = ToolResult::new(
        json!({"view":"source","label":"source candidates","source_item_count":structure.coverage.complete.then_some(rows.len()),"available_source_item_count":rows.len(),"candidates":rows,"source_coverage":coverage(snapshot)}),
    );
    if let Some(cause) = snapshot
        .source_declaration
        .as_ref()
        .and_then(|lookup| lookup.cause.as_ref())
    {
        result.cause("partial", "SOURCE_DECLARATION_UNAVAILABLE", cause.clone());
    } else if empty {
        let label = params
            .symbol
            .as_deref()
            .map(|name| format!("'{name}'"))
            .unwrap_or_else(|| "the selected source position".into());
        result.cause(
            if structure.coverage.complete {
                "unchanged"
            } else {
                "partial"
            },
            "SOURCE_CANDIDATE_UNAVAILABLE",
            format!(
                "No source declaration matches {label} in {}.{}",
                params
                    .file
                    .as_deref()
                    .or(snapshot.root_file.as_deref())
                    .unwrap_or("the supplied source"),
                if structure.coverage.complete {
                    ""
                } else {
                    " Source inspection is incomplete; see source_coverage for the affected files."
                }
            ),
        );
    }
    Ok(result)
}

pub(super) fn outcomes(snapshot: &SourceSnapshot, result: &mut ToolResult) {
    if let Some(structure) = &snapshot.source_structure
        && !structure.coverage.complete
        && result.outcome.status != "refused"
    {
        let first = structure
            .coverage
            .unresolved_targets
            .first()
            .map(|target| target.cause.as_str())
            .or_else(|| {
                structure
                    .coverage
                    .indexed_files
                    .iter()
                    .find_map(|file| file.cause.as_deref())
            })
            .unwrap_or("Source inspection stopped before all requested files were indexed.");
        result.cause("partial","SOURCE_COVERAGE_INCOMPLETE",format!("Source inspection for {} is partial: {first} Available source records are retained.",snapshot.root_file.as_deref().unwrap_or("unnamed model")));
    }
    if result.outcome.status != "refused" {
        for receipt in snapshot.inputs["analysis"]["overrides"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if receipt["application"] == "not_reached" || receipt["application"] == "unknown" {
                let operation = &receipt["operation"];
                let label = if operation["kind"] == "branch" {
                    format!(
                        "Branch {}",
                        receipt["branch_label"].as_str().unwrap_or("preview")
                    )
                } else {
                    format!(
                        "Definition {}",
                        receipt["name"].as_str().unwrap_or("override")
                    )
                };
                let file = operation["target"]["file"]
                    .as_str()
                    .unwrap_or("unnamed model");
                let cause = receipt["cause"].as_str().unwrap_or(
                    if receipt["application"] == "not_reached" {
                        "The selected directive was not reached."
                    } else {
                        "Application is unknown."
                    },
                );
                result.cause_for(
                    "partial",
                    if receipt["application"] == "not_reached" {
                        "ANALYSIS_OVERRIDE_NOT_REACHED"
                    } else {
                        "ANALYSIS_OVERRIDE_UNKNOWN"
                    },
                    format!("{label} in {file}: {cause}"),
                    operation["target"].clone(),
                );
            }
        }
    }
}
