//! LSP locations and file-local presentation over the shared written model map.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{json, Value};
use tower_lsp::lsp_types::*;

use crate::include_resolver::{is_virtual_uri, normalize_uri};
use crate::model::{Model, StatementKind, WrittenDeclaration};
use crate::model_info::{classify_variable_timing, TimingInfo};
use crate::model_map::{SourceOccurrence, WrittenModelMap, WrittenSegment};
use crate::server_settings::PresentationSettings;
use crate::span::{LineIndex, Span};
use crate::workspace::Workspace;

pub(crate) struct WrittenView<'a> {
    pub uri: &'a Url,
    pub text: &'a str,
    pub model: &'a Model,
    pub map: &'a WrittenModelMap,
    pub workspace: Option<&'a Workspace>,
    pub authoritative: bool,
    pub final_types: bool,
    root_key: String,
    sources: RefCell<HashMap<String, SourceIndex<'a>>>,
    known_uris: HashMap<String, Url>,
}

struct SourceIndex<'a> {
    uri: Url,
    text: &'a str,
    index: LineIndex,
}
struct StatementGroup {
    ids: Vec<usize>,
    range: Range,
    anchor: Option<Range>,
}
struct EquationLeaf {
    label: String,
    range: Range,
    numbers: BTreeSet<usize>,
    dimensions: BTreeSet<Option<String>>,
    states: BTreeSet<&'static str>,
    copies: usize,
}

pub(crate) fn written_range(text: &str, span: Span) -> Option<Range> {
    text.get(span.start as usize..span.end as usize)?;
    let normalized = crate::parser::normalize_newlines(text);
    let index = LineIndex::new(&normalized);
    debug_assert_eq!(normalized.len(), text.len());
    let start = index.position_utf16(text, span.start);
    let end = index.position_utf16(text, span.end);
    Some(Range::new(
        Position::new(start.line, start.character),
        Position::new(end.line, end.character),
    ))
}

fn contains(parent: Range, child: Range) -> bool {
    parent.start <= child.start && child.end <= parent.end
}

#[allow(deprecated)]
fn symbol(
    name: String,
    detail: Option<String>,
    kind: SymbolKind,
    range: Range,
    selection: Range,
    children: Vec<DocumentSymbol>,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range: selection,
        children: (!children.is_empty()).then_some(children),
    }
}

fn range_key(range: Range) -> (u32, u32, u32, u32) {
    (
        range.start.line,
        range.start.character,
        range.end.line,
        range.end.character,
    )
}

impl<'a> WrittenView<'a> {
    pub fn new(
        uri: &'a Url,
        text: &'a str,
        model: &'a Model,
        map: &'a WrittenModelMap,
        workspace: Option<&'a Workspace>,
        authoritative: bool,
        final_types: bool,
    ) -> Self {
        Self {
            uri,
            text,
            model,
            map,
            workspace,
            authoritative,
            final_types,
            root_key: normalize_uri(uri.as_str()),
            sources: RefCell::new(HashMap::new()),
            known_uris: HashMap::new(),
        }
    }

    pub fn with_known_uris<'b>(mut self, uris: impl IntoIterator<Item = &'b Url>) -> Self {
        for uri in uris {
            self.known_uris
                .insert(normalize_uri(uri.as_str()), uri.clone());
        }
        self
    }

    fn location(&self, segment: &WrittenSegment) -> Option<Location> {
        let own = segment
            .file
            .as_ref()
            .is_none_or(|file| file == &self.root_key);
        let key = segment.file.as_deref().unwrap_or(&self.root_key);
        let mut sources = self.sources.borrow_mut();
        if let Some(source) = sources.get(key) {
            source
                .text
                .get(segment.span.start as usize..segment.span.end as usize)?;
            let start = source.index.position_utf16(source.text, segment.span.start);
            let end = source.index.position_utf16(source.text, segment.span.end);
            return Some(Location::new(
                source.uri.clone(),
                Range::new(
                    Position::new(start.line, start.character),
                    Position::new(end.line, end.character),
                ),
            ));
        }
        let (uri, text) = if own {
            (self.uri.clone(), self.text)
        } else {
            let file = segment.file.as_deref()?;
            let text = self.workspace?.source_for_normalized_key(file)?;
            let uri = self.known_uris.get(file).cloned().or_else(|| {
                if is_virtual_uri(file) {
                    Url::parse(file).ok()
                } else {
                    Url::from_file_path(file).ok()
                }
            })?;
            (uri, text)
        };
        let range = written_range(text, segment.span)?;
        let normalized = crate::parser::normalize_newlines(text);
        debug_assert_eq!(normalized.len(), text.len());
        let index = LineIndex::new(&normalized);
        sources.insert(
            key.to_string(),
            SourceIndex {
                uri: uri.clone(),
                text,
                index,
            },
        );
        Some(Location::new(uri, range))
    }

    fn local_locations(&self, source: &SourceOccurrence) -> Vec<Location> {
        source
            .segments
            .iter()
            .filter_map(|segment| self.location(segment))
            .filter(|location| location.uri == *self.uri)
            .collect()
    }

    pub(crate) fn source_json(&self, source: &SourceOccurrence) -> Value {
        let locations: Vec<_> = source
            .segments
            .iter()
            .filter_map(|segment| self.location(segment))
            .collect();
        let anchor = source
            .anchor
            .as_ref()
            .and_then(|segment| self.location(segment));
        let single =
            (locations.len() == 1 && source.segments.len() == 1).then(|| locations[0].clone());
        let frames: Vec<_> = source.origin_frames.iter().map(|frame| json!({
            "kind": frame.kind, "variable": frame.variable, "value": frame.value,
            "segments": frame.segments.iter().filter_map(|segment| self.location(segment)).collect::<Vec<_>>()
        })).collect();
        json!({"location": single, "segments": locations, "anchor": anchor,
            "ambiguous": source.segments.len() != 1, "origin_frames": frames})
    }

    pub fn facts_json(&self) -> Value {
        let mut counted_by_statement: HashMap<usize, Vec<_>> = HashMap::new();
        for row in self.map.equations.iter().filter(|row| row.number.is_some()) {
            counted_by_statement
                .entry(row.statement_id)
                .or_default()
                .push(row);
        }
        let statements: Vec<Value> = self
            .model
            .statements
            .iter()
            .zip(&self.map.statements)
            .map(|(statement, source)| {
                let mut row = self.source_json(source);
                row["id"] = json!(format!("s{}", statement.id));
                row["kind"] = json!(statement.kind.as_str());
                row["name"] = json!(statement.name);
                row["complete"] = json!(statement.complete);
                row["native"] = json!(statement.native);
                row["category"] = json!(statement.category);
                row["subtype"] = json!(statement.subtype);
                row["dimension"] = json!(statement.dimension);
                let counted = counted_by_statement
                    .get(&statement.id)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let count_safe = counted.iter().all(|equation| {
                    equation.source.segments.len() == 1
                        && self.location(&equation.source.segments[0]).is_some()
                });
                let lens_safe =
                    self.authoritative && statement.complete && row["anchor"].is_object();
                row["lens_anchor"] = if lens_safe
                    && (statement.name == "model" || statement.kind == StatementKind::Declaration)
                {
                    row["anchor"].clone()
                } else {
                    Value::Null
                };
                row["equation_count"] = if lens_safe && count_safe && statement.name == "model" {
                    json!(counted.len())
                } else {
                    Value::Null
                };
                row
            })
            .collect();
        let timing = classify_variable_timing(self.model);
        let declarations: Vec<_> = self
            .model
            .written_declarations
            .iter()
            .zip(&self.map.declarations)
            .enumerate()
            .map(|(index, (written, source))| {
                let declaration = &written.declaration;
                let mut row = self.source_json(source);
                row["id"] = json!(format!("d{index}"));
                row["statement_id"] = json!(format!("s{}", written.statement_id));
                row["name"] = json!(self.model.name(declaration.name));
                row["written_kind"] = json!(written.written_kind);
                row["final_kind"] = json!(self
                    .authoritative
                    .then(|| self.model.final_symbol_kind(declaration.name))
                    .flatten());
                row["long_name"] = json!(declaration.long_name);
                row["tex_name"] = json!(declaration.tex_name);
                row["log_transform"] = json!(declaration.log_transform);
                row["written_dimension"] = json!(declaration
                    .heterogeneity
                    .map(|(name, _)| self.model.name(name)));
                row["dimension"] = json!(self
                    .authoritative
                    .then(|| self
                        .model
                        .final_heterogeneity(declaration)
                        .map(|name| self.model.name(name)))
                    .flatten());
                row["timing"] = if self.authoritative {
                    timing
                        .get(self.model.name(declaration.name))
                        .map(|info| json!({"class":info.class.label(),"offsets":info.offsets}))
                        .unwrap_or(Value::Null)
                } else {
                    Value::Null
                };
                row
            })
            .collect();
        let equations: Vec<_> = if self.authoritative {
            self.map
                .equations
                .iter()
                .enumerate()
                .filter(|(_, row)| row.active && row.number.is_some())
                .map(|(index, equation)| {
                    let mut row = self.source_json(&equation.source);
                    row["id"] = json!(format!("e{}", equation.id));
                    row["statement_id"] = json!(format!("s{}", equation.statement_id));
                    row["block_id"] = json!(format!("s{}", equation.statement_id));
                    row["scope"] = json!(if equation.dimension.is_some() {
                        "dimension"
                    } else {
                        "aggregate"
                    });
                    row["number"] = json!(equation.number);
                    row["dimension"] = json!(equation.dimension);
                    row["name"] = json!(equation.name);
                    row["text"] = json!(self.model.written_equations[index].equation.text);
                    row
                })
                .collect()
        } else {
            Vec::new()
        };
        let first = statements
            .iter()
            .find(|row| row["name"] == "model" && row["anchor"].is_object())
            .map(|row| row["anchor"].clone());
        json!({"statements":statements,"declarations":declarations,"equations":equations,"first_model_anchor":first,
            "equation_numbering":"Dygnosis numbers before transformation"})
    }

    fn declaration_symbol(
        &self,
        written: &WrittenDeclaration,
        location: Location,
        preferences: &PresentationSettings,
        timing: &HashMap<String, TimingInfo>,
    ) -> DocumentSymbol {
        let declaration = &written.declaration;
        let kind = if self.final_types && self.authoritative {
            self.model
                .final_symbol_kind(declaration.name)
                .unwrap_or(&written.written_kind)
        } else {
            &written.written_kind
        };
        let mut details = vec![kind.to_string()];
        if kind == "var"
            && self.final_types
            && self.authoritative
            && let Some(timing) = timing.get(self.model.name(declaration.name))
        {
            details.push(timing.class.label().to_string());
        }
        if preferences.name_details.long_name
            && let Some(long) = &declaration.long_name
        {
            details.push(long.clone());
        }
        if preferences.name_details.tex
            && let Some(tex) = &declaration.tex_name
        {
            details.push(format!("${tex}$"));
        }
        let icon = match kind {
            "parameters" => SymbolKind::NUMBER,
            "external_function" => SymbolKind::FUNCTION,
            "heterogeneity_dimension" => SymbolKind::NAMESPACE,
            _ => SymbolKind::VARIABLE,
        };
        symbol(
            self.model.name(declaration.name).to_string(),
            Some(details.join(" · ")),
            icon,
            location.range,
            location.range,
            Vec::new(),
        )
    }

    pub fn symbols(&self, preferences: &PresentationSettings) -> Vec<DocumentSymbol> {
        let timing = if self.final_types && self.authoritative {
            classify_variable_timing(self.model)
        } else {
            HashMap::new()
        };
        let enabled = |section: &str| {
            preferences
                .outline
                .sections
                .iter()
                .any(|value| value == section)
        };
        // Group equal written segments before labelling equations: repeated
        // openers must carry every expansion's number, not the first one.
        let mut groups: BTreeMap<_, StatementGroup> = BTreeMap::new();
        for (statement, source) in self.model.statements.iter().zip(&self.map.statements) {
            for location in self.local_locations(source) {
                let anchor = source
                    .anchor
                    .as_ref()
                    .and_then(|segment| self.location(segment))
                    .filter(|anchor| {
                        anchor.uri == *self.uri && contains(location.range, anchor.range)
                    })
                    .map(|anchor| anchor.range);
                let key = (
                    statement.kind.as_str(),
                    statement.name.clone(),
                    range_key(location.range),
                );
                groups
                    .entry(key)
                    .and_modify(|group| group.ids.push(statement.id))
                    .or_insert(StatementGroup {
                        ids: vec![statement.id],
                        range: location.range,
                        anchor,
                    });
            }
        }
        let mut output = Vec::new();
        for ((kind, name, _), StatementGroup { ids, range, anchor }) in groups {
            let mut children = Vec::new();
            if (kind == "declaration" && enabled("declarations"))
                || (kind == "dimension" && enabled("dimensions"))
            {
                for (declaration, source) in self
                    .model
                    .written_declarations
                    .iter()
                    .zip(&self.map.declarations)
                    .filter(|(row, _)| ids.contains(&row.statement_id))
                {
                    for location in self
                        .local_locations(source)
                        .into_iter()
                        .filter(|location| contains(range, location.range))
                    {
                        let child =
                            self.declaration_symbol(declaration, location, preferences, &timing);
                        if !children.contains(&child) {
                            children.push(child);
                        }
                    }
                }
            }
            if enabled("equations") {
                let mut rows: BTreeMap<_, EquationLeaf> = BTreeMap::new();
                for (index, row) in self
                    .map
                    .equations
                    .iter()
                    .enumerate()
                    .filter(|(_, row)| ids.contains(&row.statement_id))
                {
                    let written = &self.model.written_equations[index].equation;
                    let label = if !row.name.is_empty() {
                        row.name.clone()
                    } else if !written.lhs.is_empty() {
                        written.lhs.clone()
                    } else {
                        written.text.chars().take(60).collect()
                    };
                    for location in self
                        .local_locations(&row.source)
                        .into_iter()
                        .filter(|location| contains(range, location.range))
                    {
                        let item = rows
                            .entry((range_key(location.range), label.clone()))
                            .or_insert(EquationLeaf {
                                label: label.clone(),
                                range: location.range,
                                numbers: BTreeSet::new(),
                                dimensions: BTreeSet::new(),
                                states: BTreeSet::new(),
                                copies: 0,
                            });
                        item.copies += 1;
                        item.dimensions.insert(row.dimension.clone());
                        if self.authoritative
                            && preferences.outline.equation_numbers
                            && let Some(number) = row.number
                        {
                            item.numbers.insert(number);
                        }
                        if row.local {
                            item.states.insert("model-local definition");
                        } else if row.static_only {
                            item.states.insert("static-only equation");
                        } else if self.final_types && !row.active {
                            item.states.insert("removed equation");
                        }
                    }
                }
                for (
                    _,
                    EquationLeaf {
                        label,
                        range: row_range,
                        mut numbers,
                        dimensions,
                        states,
                        copies,
                    },
                ) in rows
                {
                    let multiple = dimensions.len() > 1
                        || (copies > 1 && (!states.is_empty() || numbers.len() < copies));
                    if multiple {
                        numbers.clear();
                    }
                    let number_label = if numbers.is_empty() {
                        String::new()
                    } else if numbers.len() > 1
                        && numbers.last().unwrap() - numbers.first().unwrap() + 1 == numbers.len()
                    {
                        format!(
                            "{}–{} · ",
                            numbers.first().unwrap(),
                            numbers.last().unwrap()
                        )
                    } else {
                        format!(
                            "{} · ",
                            numbers
                                .iter()
                                .map(usize::to_string)
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    };
                    let detail = if multiple {
                        Some("multiple expansions".to_string())
                    } else if states.is_empty() {
                        None
                    } else {
                        Some(states.into_iter().collect::<Vec<_>>().join(" · "))
                    };
                    children.push(symbol(
                        format!("{number_label}{label}"),
                        detail,
                        SymbolKind::FUNCTION,
                        row_range,
                        row_range,
                        Vec::new(),
                    ));
                }
            }
            children.sort_by_key(|child| range_key(child.range));
            let dimensions: BTreeSet<_> = ids
                .iter()
                .filter_map(|id| self.model.statements[*id].dimension.as_deref())
                .collect();
            let show = match kind {
                "declaration" => enabled("declarations"),
                "dimension" => enabled("dimensions"),
                "block" => enabled("blocks") || (!dimensions.is_empty() && enabled("dimensions")),
                _ => enabled("commands"),
            };
            if show {
                let title = if name == "model" && dimensions.len() == 1 && self.final_types {
                    format!("model ({})", dimensions.first().unwrap())
                } else if dimensions.len() > 1 {
                    format!("{name} (multiple expansions)")
                } else if anchor.is_none() && kind == "block" {
                    format!("{name} (fragment)")
                } else {
                    name
                };
                let detail = if kind == "block"
                    && ids
                        .iter()
                        .any(|id| self.model.statements[*id].name == "model")
                {
                    Some(
                        if self.authoritative {
                            "Dygnosis numbers before transformation"
                        } else {
                            "recovered written structure"
                        }
                        .to_string(),
                    )
                } else {
                    None
                };
                let icon = match kind {
                    "declaration" => SymbolKind::NAMESPACE,
                    "dimension" => SymbolKind::NAMESPACE,
                    "block" => SymbolKind::MODULE,
                    "assignment" => SymbolKind::VARIABLE,
                    _ => SymbolKind::EVENT,
                };
                output.push(symbol(
                    title,
                    detail,
                    icon,
                    range,
                    anchor
                        .or_else(|| children.first().map(|child| child.selection_range))
                        .unwrap_or(Range::new(range.start, range.start)),
                    children,
                ));
            } else {
                output.extend(children);
            }
        }
        output.sort_by_key(|row| range_key(row.range));
        output
    }

    pub fn folds(&self) -> Vec<FoldingRange> {
        let mut output = Vec::new();
        for (statement, source) in self
            .model
            .statements
            .iter()
            .zip(&self.map.statements)
            .filter(|(statement, _)| statement.kind == StatementKind::Block && statement.complete)
        {
            let local = self.local_locations(source);
            if local.len() != 1 || source.segments.len() != 1 {
                continue;
            }
            let range = local[0].range;
            if range.start.line < range.end.line {
                let key = (range.start.line, range.end.line);
                if output
                    .iter()
                    .any(|row: &FoldingRange| (row.start_line, row.end_line) == key)
                {
                    continue;
                }
                output.push(FoldingRange {
                    start_line: key.0,
                    end_line: key.1,
                    start_character: None,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Region),
                    collapsed_text: Some(format!("{} ... end;", statement.name)),
                });
            }
        }
        output
    }
}

pub(crate) fn common_leaves(views: Vec<Vec<DocumentSymbol>>) -> Vec<DocumentSymbol> {
    fn leaves(rows: Vec<DocumentSymbol>) -> Vec<DocumentSymbol> {
        rows.into_iter()
            .flat_map(|mut row| {
                if let Some(children) = row.children.take() {
                    leaves(children)
                } else {
                    vec![row]
                }
            })
            .collect()
    }
    let mut views = views.into_iter().map(leaves);
    let Some(mut common) = views.next() else {
        return Vec::new();
    };
    for view in views {
        common.retain_mut(|row| {
            let matching = view.iter().find(|other| {
                row.name == other.name
                    && row.kind == other.kind
                    && row.range == other.range
                    && row.selection_range == other.selection_range
            });
            if let Some(other) = matching {
                if row.detail != other.detail {
                    row.detail = Some("several model roots".to_string());
                }
                true
            } else {
                false
            }
        });
    }
    common.sort_by_key(|row| range_key(row.range));
    common.dedup();
    common
}
