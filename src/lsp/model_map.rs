//! LSP locations and file-local presentation over the shared written model map.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::{Value, json};
use tower_lsp::lsp_types::*;

use crate::engine::analysis::source_inventory::{SourceInventory, SourceItem, SourceItemKind};
use crate::engine::analysis::timing::TimingAnalysis;
use crate::engine::analysis_configuration::SourceAvailability;
use crate::engine::language::model::{Model, StatementKind, WrittenDeclaration};
use crate::engine::language::span::{LineIndex, Span};
use crate::engine::workspace::Workspace;
use crate::engine::workspace::include_resolver::{is_virtual_uri, normalize_uri};
use crate::engine::workspace::model_map::{SourceOccurrence, WrittenModelMap, WrittenSegment};
use crate::lsp::settings::PresentationSettings;
use std::sync::Arc;

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
    index: Arc<LineIndex>,
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
    let normalized = crate::engine::language::parser::normalize_newlines(text);
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

fn declaration_icon(kind: &str) -> SymbolKind {
    match kind {
        "parameters" => SymbolKind::NUMBER,
        "varexo" | "varexo_det" => SymbolKind::FIELD,
        "external_function" => SymbolKind::FUNCTION,
        "heterogeneity_dimension" => SymbolKind::NAMESPACE,
        _ => SymbolKind::VARIABLE,
    }
}

fn declaration_detail(
    kind: &str,
    written_kind: &str,
    long_name: Option<&str>,
    tex_name: Option<&str>,
    preferences: &PresentationSettings,
) -> Option<String> {
    let mut details = Vec::new();
    if kind != written_kind {
        details.push(kind.to_owned());
    }
    if preferences.name_details.long_name
        && let Some(long) = long_name
    {
        details.push(long.to_owned());
    }
    if preferences.name_details.tex
        && let Some(tex) = tex_name
    {
        details.push(format!("${tex}$"));
    }
    (!details.is_empty()).then(|| details.join(" · "))
}

/// Reuse written Outline labels over verified token positions in one display document.
pub(crate) fn preview_symbols(
    uri: &Url,
    text: &str,
    model: &Model,
    map: &WrittenModelMap,
    token_spans: &[Option<Span>],
    preferences: &PresentationSettings,
    authoritative: bool,
) -> Vec<DocumentSymbol> {
    if token_spans.len() != model.expanded_tokens.len() {
        return Vec::new();
    }
    let segment = |range: &std::ops::Range<usize>| -> Option<WrittenSegment> {
        if range.is_empty() {
            return None;
        }
        let first = token_spans.get(range.start)?.as_ref()?;
        let last = token_spans.get(range.end - 1)?.as_ref()?;
        Some(WrittenSegment {
            file: None,
            span: Span {
                start: first.start,
                end: last.end,
            },
        })
    };
    let source =
        |range: &std::ops::Range<usize>, anchor: &std::ops::Range<usize>| SourceOccurrence {
            segments: segment(range).into_iter().collect(),
            anchor: segment(anchor),
            origin_frames: Vec::new(),
        };
    let mut display = map.clone();
    (display.statement_objects, display.pac_children) =
        crate::engine::workspace::model_map::outline_occurrences(model, source);
    display.statements = model
        .statements
        .iter()
        .map(|statement| source(&statement.token_range, &statement.opener_range))
        .collect();
    display.declarations = model
        .written_declarations
        .iter()
        .map(|declaration| source(&declaration.token_range, &declaration.token_range))
        .collect();
    for (row, written) in display.equations.iter_mut().zip(&model.written_equations) {
        row.source = source(
            &written.token_range,
            &(written.token_range.start..written.token_range.start + 1),
        );
    }
    let mut symbols = WrittenView::new(uri, text, model, &display, None, authoritative, true)
        .symbols(preferences);
    let index = LineIndex::new(text);
    let anchors: BTreeMap<_, _> = token_spans
        .iter()
        .flatten()
        .map(|span| {
            let range = super::span_range(&index, text, *span);
            (range.start, range)
        })
        .collect();
    fn select_token(rows: &mut [DocumentSymbol], anchors: &BTreeMap<Position, Range>) {
        for row in rows {
            if let Some(range) = anchors
                .get(&row.selection_range.start)
                .filter(|range| contains(row.range, **range))
            {
                row.selection_range = *range;
            }
            if let Some(children) = &mut row.children {
                select_token(children, anchors);
            }
        }
    }
    // Whole equations can cross source regions. A single token keeps Go to written source usable.
    select_token(&mut symbols, &anchors);
    symbols
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
        text.get(segment.span.start as usize..segment.span.end as usize)?;
        let index = self
            .workspace
            .and_then(|workspace| workspace.source_document(self.uri.as_str(), key))
            .filter(|source| std::ptr::eq(source.source(), text))
            .map(|source| source.source_index())
            .unwrap_or_else(|| {
                Arc::new(LineIndex::new(
                    &crate::engine::language::parser::normalize_newlines(text),
                ))
            });
        let start = index.position_utf16(text, segment.span.start);
        let end = index.position_utf16(text, segment.span.end);
        let range = Range::new(
            Position::new(start.line, start.character),
            Position::new(end.line, end.character),
        );
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

    /// Outline has one display extent per written file. Navigation and model
    /// facts keep the exact, disjoint source segments in source_json.
    fn outline_location(&self, source: &SourceOccurrence, require_owner: bool) -> Option<Location> {
        if require_owner {
            let owner = source.anchor.as_ref().or_else(|| source.segments.first())?;
            if self.location(owner)?.uri != *self.uri {
                return None;
            }
        }
        let mut locations = source
            .segments
            .iter()
            .filter_map(|segment| self.location(segment))
            .filter(|location| location.uri == *self.uri);
        let mut location = locations.next()?;
        for next in locations {
            location.range.start = location.range.start.min(next.range.start);
            location.range.end = location.range.end.max(next.range.end);
        }
        Some(location)
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

    pub fn facts_json(&self, timing: Option<&TimingAnalysis>) -> Value {
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
                row["final_kind"] = json!(
                    self.authoritative
                        .then(|| self.model.final_symbol_kind(declaration.name))
                        .flatten()
                );
                row["long_name"] = json!(declaration.long_name);
                row["tex_name"] = json!(declaration.tex_name);
                row["log_transform"] = json!(declaration.log_transform);
                row["written_dimension"] = json!(
                    declaration
                        .heterogeneity
                        .map(|(name, _)| self.model.name(name))
                );
                row["dimension"] = json!(
                    self.authoritative
                        .then(|| self
                            .model
                            .final_heterogeneity(declaration)
                            .map(|name| self.model.name(name)))
                        .flatten()
                );
                row["timing"] = if self.authoritative {
                    timing
                        .and_then(|timing| timing.all.get(self.model.name(declaration.name)))
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
    ) -> DocumentSymbol {
        let declaration = &written.declaration;
        let kind = if self.final_types && self.authoritative {
            self.model
                .final_symbol_kind(declaration.name)
                .unwrap_or(&written.written_kind)
        } else {
            &written.written_kind
        };
        symbol(
            self.model.name(declaration.name).to_string(),
            declaration_detail(
                kind,
                &written.written_kind,
                declaration.long_name.as_deref(),
                declaration.tex_name.as_deref(),
                preferences,
            ),
            declaration_icon(kind),
            location.range,
            location.range,
            Vec::new(),
        )
    }

    pub fn symbols(&self, preferences: &PresentationSettings) -> Vec<DocumentSymbol> {
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
            if statement.kind == StatementKind::Assignment {
                continue;
            }
            if let Some(location) = self.outline_location(source, false) {
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
        let mut groups: Vec<_> = groups.into_iter().collect();
        groups.sort_by_key(|(_, group)| range_key(group.range));
        let mut output = Vec::new();
        for ((kind, name, _), StatementGroup { ids, range, anchor }) in groups {
            let mut children = Vec::new();
            let objects: Vec<_> = self
                .map
                .statement_objects
                .iter()
                .filter(|object| ids.contains(&object.statement_id))
                .collect();
            let object_names: BTreeSet<_> =
                objects.iter().map(|object| object.name.as_str()).collect();
            let object_target = objects.iter().find_map(|object| {
                object
                    .source
                    .anchor
                    .as_ref()
                    .and_then(|segment| self.location(segment))
                    .filter(|location| location.uri == *self.uri && contains(range, location.range))
                    .map(|location| location.range)
            });
            if enabled("blocks") {
                for member in self
                    .map
                    .pac_children
                    .iter()
                    .filter(|member| ids.contains(&member.statement_id))
                {
                    for location in self
                        .local_locations(&member.source)
                        .into_iter()
                        .filter(|location| contains(range, location.range))
                    {
                        let selection = member
                            .source
                            .anchor
                            .as_ref()
                            .and_then(|segment| self.location(segment))
                            .filter(|target| {
                                target.uri == *self.uri && contains(location.range, target.range)
                            })
                            .map(|target| target.range)
                            .unwrap_or(Range::new(location.range.start, location.range.start));
                        let child = symbol(
                            member.name.clone(),
                            None,
                            SymbolKind::OBJECT,
                            location.range,
                            selection,
                            Vec::new(),
                        );
                        if !children.contains(&child) {
                            children.push(child);
                        }
                    }
                }
            }
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
                        let child = self.declaration_symbol(declaration, location, preferences);
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
                        .outline_location(&row.source, true)
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
                        SymbolKind::OPERATOR,
                        row_range,
                        row_range,
                        Vec::new(),
                    ));
                }
            }
            children.sort_by_key(|child| range_key(child.range));
            let dimensions: BTreeSet<_> =
                if kind == "declaration" && matches!(name.as_str(), "var" | "varexo") {
                    self.model
                        .written_declarations
                        .iter()
                        .filter(|row| ids.contains(&row.statement_id))
                        .filter_map(|row| row.declaration.heterogeneity)
                        .map(|(dimension, _)| self.model.name(dimension))
                        .collect()
                } else {
                    ids.iter()
                        .filter_map(|id| self.model.statements[*id].dimension.as_deref())
                        .collect()
                };
            let show = match kind {
                "declaration" => enabled("declarations"),
                "dimension" => enabled("dimensions"),
                "block" => enabled("blocks") || (!dimensions.is_empty() && enabled("dimensions")),
                _ => enabled("commands"),
            };
            if show && !(kind == "block" && anchor.is_none()) {
                let title = if (name == "model"
                    || (kind == "declaration" && matches!(name.as_str(), "var" | "varexo")))
                    && dimensions.len() == 1
                {
                    format!("{name}(heterogeneity={})", dimensions.first().unwrap())
                } else if dimensions.len() > 1 || object_names.len() > 1 {
                    format!("{name} (multiple expansions)")
                } else if let Some(object) = object_names.first() {
                    format!("{name}({object})")
                } else {
                    name
                };
                let detail = if !self.authoritative
                    && kind == "block"
                    && ids
                        .iter()
                        .any(|id| self.model.statements[*id].name == "model")
                {
                    Some("recovered written structure".to_string())
                } else {
                    None
                };
                let icon = match kind {
                    "declaration" => SymbolKind::NAMESPACE,
                    "dimension" => SymbolKind::NAMESPACE,
                    "block" => SymbolKind::MODULE,
                    _ => SymbolKind::EVENT,
                };
                let row = symbol(
                    title,
                    detail,
                    icon,
                    range,
                    object_target
                        .filter(|_| object_names.len() == 1)
                        .or(anchor)
                        .or_else(|| children.first().map(|child| child.selection_range))
                        .unwrap_or(Range::new(range.start, range.start)),
                    children,
                );
                output.push(row);
            } else {
                output.extend(children);
            }
        }
        if enabled("declarations") {
            for (name, target, span, function) in
                crate::lsp::syntax_intel::macro_definitions(self.text)
            {
                if let (Some(range), Some(selection)) = (
                    written_range(self.text, span),
                    written_range(self.text, target),
                ) {
                    output.push(symbol(
                        name,
                        None,
                        if function {
                            SymbolKind::FUNCTION
                        } else {
                            SymbolKind::CONSTANT
                        },
                        range,
                        selection,
                        Vec::new(),
                    ));
                }
            }
        }
        output.sort_by_key(|row| range_key(row.range));
        output
    }
}

/// Merge written rows by their source locations. Accepted rows keep their
/// semantic labels and equation numbers; added rows retain literal metadata.
pub(crate) fn add_source_symbols(
    rows: &mut Vec<DocumentSymbol>,
    inventory: &SourceInventory,
    text: &str,
    preferences: &PresentationSettings,
    availability: impl Fn(&SourceItem) -> SourceAvailability,
) {
    let enabled = |section: &str| {
        preferences
            .outline
            .sections
            .iter()
            .any(|value| value == section)
    };
    let normalized = crate::engine::language::parser::normalize_newlines(text);
    let index = LineIndex::new(&normalized);
    let range = |span: Span| super::span_range(&index, &normalized, span);
    struct OutlineRow {
        symbol: DocumentSymbol,
        availability: SourceAvailability,
    }
    fn flatten(mut row: DocumentSymbol, flat: &mut Vec<OutlineRow>) {
        let children = row.children.take();
        flat.push(OutlineRow {
            symbol: row,
            availability: SourceAvailability::Unknown,
        });
        if let Some(children) = children {
            for child in children {
                flatten(child, flat);
            }
        }
    }
    let mut flat = Vec::new();
    for row in rows.drain(..) {
        flatten(row, &mut flat);
    }
    let mut selections: BTreeMap<_, _> = flat
        .iter()
        .enumerate()
        .map(|(index, row)| (range_key(row.symbol.selection_range), index))
        .collect();
    let mut spans: BTreeMap<_, _> = flat
        .iter()
        .enumerate()
        .map(|(index, row)| (range_key(row.symbol.range), index))
        .collect();
    for item in &inventory.items {
        let show = match item.kind {
            SourceItemKind::Declaration | SourceItemKind::MacroDefinition => {
                enabled("declarations")
            }
            SourceItemKind::Block
            | SourceItemKind::Conditional
            | SourceItemKind::Branch
            | SourceItemKind::Loop => enabled("blocks"),
            SourceItemKind::Equation => enabled("equations"),
            SourceItemKind::Command => enabled("commands"),
            _ => false,
        };
        let selection = range(item.selection);
        let span = range(Span {
            start: item.span.start,
            end: item.branch_end.unwrap_or(item.span.end),
        });
        let state = availability(item);
        if let Some(&existing) = selections
            .get(&range_key(selection))
            .or_else(|| spans.get(&range_key(span)))
        {
            flat[existing].availability = state;
            continue;
        }
        if !show {
            continue;
        }
        let literal_candidate =
            item.kind == SourceItemKind::Declaration && item.span == item.selection;
        let detail = if matches!(
            item.kind,
            SourceItemKind::Conditional | SourceItemKind::Branch | SourceItemKind::Loop
        ) {
            Some(
                match state {
                    SourceAvailability::Active => "active",
                    SourceAvailability::Inactive => "inactive",
                    SourceAvailability::Mixed => "active in some expansions",
                    SourceAvailability::Unknown => "unknown",
                }
                .to_owned(),
            )
        } else if literal_candidate {
            let kind = item.written_kind.as_deref().unwrap_or("var");
            declaration_detail(
                kind,
                kind,
                item.declaration_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.long_name.as_deref()),
                item.declaration_metadata
                    .as_ref()
                    .and_then(|metadata| metadata.tex_name.as_deref()),
                preferences,
            )
        } else {
            None
        };
        let name = item.name.clone().unwrap_or_else(|| {
            text.get(item.span.start as usize..item.span.end as usize)
                .unwrap_or("source template")
                .trim()
                .chars()
                .take(80)
                .collect()
        });
        let name = if matches!(
            item.kind,
            SourceItemKind::Conditional | SourceItemKind::Branch | SourceItemKind::Loop
        ) {
            text.get(item.selection.start as usize..item.selection.end as usize)
                .unwrap_or(&name)
                .trim()
                .chars()
                .take(120)
                .collect()
        } else {
            name
        };
        let icon = match item.kind {
            SourceItemKind::Declaration if literal_candidate => {
                declaration_icon(item.written_kind.as_deref().unwrap_or("var"))
            }
            SourceItemKind::Declaration => SymbolKind::NAMESPACE,
            SourceItemKind::Equation => SymbolKind::OPERATOR,
            SourceItemKind::MacroDefinition if item.function => SymbolKind::FUNCTION,
            SourceItemKind::MacroDefinition => SymbolKind::CONSTANT,
            SourceItemKind::Command => SymbolKind::EVENT,
            _ => SymbolKind::MODULE,
        };
        selections.insert(range_key(selection), flat.len());
        spans.insert(range_key(span), flat.len());
        flat.push(OutlineRow {
            symbol: symbol(name, detail, icon, span, selection, Vec::new()),
            availability: state,
        });
    }
    // A single ordered stack groups only ranges that prove containment. It
    // avoids searching every existing Outline row for each source candidate.
    flat.sort_by_key(|row| {
        (
            row.symbol.range.start,
            std::cmp::Reverse(row.symbol.range.end),
        )
    });
    let mut stack: Vec<OutlineRow> = Vec::new();
    fn finish(
        stack: &mut Vec<OutlineRow>,
        output: &mut Vec<DocumentSymbol>,
        activity: super::settings::OutlineActivity,
    ) {
        let row = stack.pop().expect("nonempty Outline stack");
        // Filter as the existing tree stack closes each row. Keep a container
        // only when it matches or contains retained children.
        if !activity.includes(row.availability)
            && row.symbol.children.as_ref().is_none_or(Vec::is_empty)
        {
            return;
        }
        if let Some(parent) = stack.last_mut() {
            parent
                .symbol
                .children
                .get_or_insert_with(Vec::new)
                .push(row.symbol);
        } else {
            output.push(row.symbol);
        }
    }
    for row in flat {
        while stack.last().is_some_and(|parent| {
            parent.symbol.range == row.symbol.range
                || !contains(parent.symbol.range, row.symbol.range)
        }) {
            finish(&mut stack, rows, preferences.outline.activity);
        }
        stack.push(row);
    }
    while !stack.is_empty() {
        finish(&mut stack, rows, preferences.outline.activity);
    }
}

pub(crate) fn common_leaves(views: Vec<Vec<DocumentSymbol>>) -> Vec<DocumentSymbol> {
    fn leaves(rows: Vec<DocumentSymbol>) -> Vec<DocumentSymbol> {
        rows.into_iter()
            .flat_map(|mut row| {
                if let Some(mut children) = row.children.take() {
                    // Shared includes flatten the tree, so declaration headings disappear.
                    if row.kind == SymbolKind::NAMESPACE {
                        for child in &mut children {
                            child.detail = Some(match child.detail.take() {
                                Some(detail) => format!("{} · {detail}", row.name),
                                None => row.name.clone(),
                            });
                        }
                    }
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

/// Named declaration children share the engine's accepted binding/source proof.
/// Repeated executions at one literal source target create one Outline leaf.
pub(crate) fn add_binding_symbols(
    rows: &mut Vec<DocumentSymbol>,
    model: &crate::engine::analysis::symbols::SemanticModel,
    uri: &Url,
    text: &str,
    preferences: &PresentationSettings,
) {
    fn present(rows: &[DocumentSymbol], name: &str, target: Range) -> bool {
        rows.iter().any(|row| {
            (row.name == name && row.selection_range == target)
                || row
                    .children
                    .as_ref()
                    .is_some_and(|children| present(children, name, target))
        })
    }
    fn insert(rows: &mut [DocumentSymbol], leaf: &DocumentSymbol) -> bool {
        let owner = rows
            .iter_mut()
            .filter(|row| contains(row.range, leaf.range))
            .min_by_key(|row| {
                (
                    row.range.end.line - row.range.start.line,
                    row.range
                        .end
                        .character
                        .saturating_sub(row.range.start.character),
                )
            });
        let Some(owner) = owner else {
            return false;
        };
        let children = owner.children.get_or_insert_with(Vec::new);
        if !insert(children, leaf) {
            children.push(leaf.clone());
            children.sort_by_key(|row| range_key(row.range));
        }
        true
    }
    let file = normalize_uri(uri.as_str());
    for binding in model.bindings() {
        let (section, icon) = match binding.kind.as_str() {
            "heterogeneity_dimension" => ("dimensions", SymbolKind::NAMESPACE),
            "database" => ("declarations", SymbolKind::NAMESPACE),
            "named_subsample" => ("commands", SymbolKind::CONSTANT),
            "trend_var" | "log_trend_var" => ("declarations", SymbolKind::VARIABLE),
            "external_function" => ("declarations", SymbolKind::FUNCTION),
            _ => continue,
        };
        if !preferences
            .outline
            .sections
            .iter()
            .any(|value| value == section)
        {
            continue;
        }
        let Some((target_file, span)) = &binding.declaration else {
            continue;
        };
        if target_file != &file {
            continue;
        }
        let Some(range) = written_range(text, *span) else {
            continue;
        };
        if present(rows, &binding.name, range) {
            continue;
        }
        let detail = binding
            .namespace
            .as_ref()
            .map(|namespace| format!("{} · {namespace}", binding.kind.replace('_', " ")))
            .unwrap_or_else(|| binding.kind.replace('_', " "));
        let leaf = symbol(binding.name, Some(detail), icon, range, range, Vec::new());
        if !insert(rows, &leaf) {
            rows.push(leaf);
        }
    }
    rows.sort_by_key(|row| range_key(row.range));
}

#[cfg(test)]
mod source_index_tests {
    use super::*;

    #[test]
    fn written_locations_share_the_document_index() {
        // Guards first written-location projection constructing two line
        // indexes, or retaining a separate index despite an identical Doc.
        let uri = Url::parse("untitled:written-index.mod").unwrap();
        let mut workspace = Workspace::new();
        workspace.update_document(uri.as_str(), "var y;\r\nmodel; y=0; end;\r\n");
        let model = workspace.effective_model_snapshot(uri.as_str()).unwrap();
        let report = workspace.expand_report_snapshot(uri.as_str()).unwrap();
        let key = normalize_uri(uri.as_str());
        let source = workspace.source_document(uri.as_str(), &key).unwrap();
        let view = WrittenView::new(
            &uri,
            source.source(),
            &model,
            &report.model_map,
            Some(&workspace),
            true,
            true,
        );
        let segment = &report.model_map.declarations[0].segments[0];
        view.location(segment).unwrap();
        let sources = view.sources.borrow();
        assert!(Arc::ptr_eq(&sources[&key].index, &source.source_index()));
    }
}
