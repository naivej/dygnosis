//! Bulk `eq_N` tags for the counted equations behind I208.
//!
//! The language server turns the plan into one workspace edit. This module
//! does not write a file.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::equations::{equations, heterogeneous_equations, EquationRow};
use crate::expand::{EquationOrigin, ExpandReport};
use crate::model::Model;
use crate::span::Span;
use crate::workspace::Workspace;

pub(crate) struct NameTagEdit {
    pub file: String,
    pub span: Span,
    pub new_text: String,
}

pub(crate) struct NameTagPlan {
    pub title: String,
    pub edits: Vec<NameTagEdit>,
}

pub(crate) fn equation_name_plan(ws: &mut Workspace, uri: &str) -> Option<NameTagPlan> {
    let model = ws.get_effective_model(uri)?.clone();
    let report = ws.expand_report(uri)?.clone();
    let mut sources = BTreeMap::<String, String>::new();
    for origin in &report.origins {
        let Some(file) = origin.origin_uri.as_deref() else {
            continue;
        };
        if sources.contains_key(file) {
            continue;
        }
        if let Some(text) = ws.get_source(file) {
            sources.insert(file.to_string(), text.to_string());
        }
    }
    plan_edits(&model, &report, &sources)
}

fn plan_edits(
    model: &Model,
    report: &ExpandReport,
    sources: &BTreeMap<String, String>,
) -> Option<NameTagPlan> {
    let (aggregate, heterogeneous) = counted_rows(model, report)?;
    let mut shared: HashMap<(String, u32, u32), usize> = HashMap::new();
    for row in aggregate.iter().chain(&heterogeneous) {
        *shared.entry(span_key(row.origin)).or_insert(0) += 1;
    }
    let mut taken = taken_names(model);
    let mut edits = Vec::new();
    let mut skipped = 0usize;
    for row in aggregate.iter().chain(&heterogeneous) {
        if !row.unnamed {
            continue;
        }
        if !can_edit(row.origin, &shared) {
            skipped += 1;
            continue;
        }
        let file = row.origin.origin_uri.as_deref()?;
        let Some(source) = sources.get(file) else {
            skipped += 1;
            continue;
        };
        let name = allocate(&format!("eq_{}", row.number), &taken);
        let Some((span, new_text)) = tag_edit(source, row.origin.written_span, &name) else {
            skipped += 1;
            continue;
        };
        taken.insert(name);
        edits.push(NameTagEdit {
            file: file.to_string(),
            span,
            new_text,
        });
    }
    if edits.is_empty() {
        return None;
    }
    let title = if skipped == 0 {
        "Name counted equations".to_string()
    } else {
        format!("Name counted equations ({skipped} skipped)")
    };
    Some(NameTagPlan { title, edits })
}

struct Counted<'a> {
    number: usize,
    unnamed: bool,
    origin: &'a EquationOrigin,
}

fn counted_rows<'a>(
    model: &Model,
    report: &'a ExpandReport,
) -> Option<(Vec<Counted<'a>>, Vec<Counted<'a>>)> {
    let aggregate_rows = equations(model);
    if aggregate_rows.len() != report.aggregate_origins.len() {
        return None;
    }
    let aggregate = aggregate_rows
        .iter()
        .zip(&report.aggregate_origins)
        .map(|(row, origin)| counted(row, origin))
        .collect();

    let blocks = heterogeneous_equations(model);
    let mut heterogeneous = Vec::new();
    for block in &blocks {
        let origins = report.heterogeneous_origins.get(block.block_index)?;
        if origins.len() != block.equations.len() {
            return None;
        }
        for (row, origin) in block.equations.iter().zip(origins) {
            heterogeneous.push((block.dimension.clone(), counted(row, origin)));
        }
    }
    heterogeneous.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.origin.scope_index.cmp(&right.1.origin.scope_index))
    });
    let heterogeneous = heterogeneous.into_iter().map(|(_, row)| row).collect();
    Some((aggregate, heterogeneous))
}

fn counted<'a>(row: &EquationRow, origin: &'a EquationOrigin) -> Counted<'a> {
    Counted {
        number: origin.scope_index + 1,
        unnamed: is_unnamed(row),
        origin,
    }
}

fn is_unnamed(row: &EquationRow) -> bool {
    let named = row.tags.get("name").is_some_and(|value| !value.is_empty());
    !named && row.name.is_empty()
}

fn taken_names(model: &Model) -> BTreeSet<String> {
    let mut taken = BTreeSet::new();
    let equations = model.equations.iter().chain(
        model
            .heterogeneous_models
            .iter()
            .flat_map(|block| block.equations.iter()),
    );
    for equation in equations {
        if let Some(value) = equation.tag_map.get("name") {
            if !value.is_empty() {
                taken.insert(value.clone());
            }
        }
        if !equation.name.is_empty() {
            taken.insert(equation.name.clone());
        }
    }
    taken
}

fn allocate(base: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_string();
    }
    let mut n = 2u32;
    loop {
        let candidate = format!("{base}_{n}");
        if !taken.contains(&candidate) {
            return candidate;
        }
        n += 1;
    }
}

fn span_key(origin: &EquationOrigin) -> (String, u32, u32) {
    (
        origin.origin_uri.clone().unwrap_or_default(),
        origin.written_span.start,
        origin.written_span.end,
    )
}

fn can_edit(origin: &EquationOrigin, shared: &HashMap<(String, u32, u32), usize>) -> bool {
    if origin.ambiguous || origin.loop_copy || origin.origin_uri.is_none() {
        return false;
    }
    shared.get(&span_key(origin)).copied() == Some(1)
}

enum ExistingName {
    None,
    Empty { at: u32, text: String },
    Kept,
}

fn tag_edit(source: &str, span: Span, name: &str) -> Option<(Span, String)> {
    let start = span.start as usize;
    let end = span.end as usize;
    if start >= end || end > source.len() {
        return None;
    }
    let groups = leading_groups(source, start, end);
    if groups.is_empty() {
        if source.as_bytes().get(start) == Some(&b'[') {
            return None;
        }
        return Some((
            Span {
                start: span.start,
                end: span.start,
            },
            format!("[name='{name}'] "),
        ));
    }
    let mut empty = None;
    for &(open, close) in &groups {
        match name_in_group(source, open, close, name) {
            ExistingName::Kept => return None,
            ExistingName::Empty { at, text } => {
                if empty.is_none() {
                    empty = Some((at, text));
                }
            }
            ExistingName::None => {}
        }
    }
    if let Some((at, text)) = empty {
        return Some((Span { start: at, end: at }, text));
    }
    let (open, close) = groups[0];
    let interior = source.get(open + 1..close)?;
    let text = if interior.trim().is_empty() {
        format!("name='{name}'")
    } else {
        format!(", name='{name}'")
    };
    let close = u32::try_from(close).ok()?;
    Some((
        Span {
            start: close,
            end: close,
        },
        text,
    ))
}

fn leading_groups(source: &str, start: usize, end: usize) -> Vec<(usize, usize)> {
    let bytes = source.as_bytes();
    let mut i = start;
    let mut groups = Vec::new();
    while i < end {
        while i < end && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= end || bytes[i] != b'[' {
            break;
        }
        let open = i;
        i += 1;
        let mut quote: Option<u8> = None;
        let mut close = None;
        while i < end {
            let byte = bytes[i];
            if let Some(q) = quote {
                if byte == q {
                    quote = None;
                }
                i += 1;
                continue;
            }
            if byte == b'\'' || byte == b'"' {
                quote = Some(byte);
                i += 1;
                continue;
            }
            if byte == b']' {
                close = Some(i);
                i += 1;
                break;
            }
            i += 1;
        }
        let Some(close) = close else {
            break;
        };
        groups.push((open, close));
    }
    groups
}

fn name_in_group(source: &str, open: usize, close: usize, name: &str) -> ExistingName {
    let bytes = source.as_bytes();
    let mut i = open + 1;
    let mut empty = None;
    while i < close {
        while i < close && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        if i >= close {
            break;
        }
        if !is_key_start(bytes[i]) {
            return ExistingName::Kept;
        }
        let key_at = i;
        i += 1;
        while i < close && is_key_cont(bytes[i]) {
            i += 1;
        }
        let is_name = source[key_at..i].eq_ignore_ascii_case("name");
        while i < close && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < close && bytes[i] == b'=' {
            i += 1;
            while i < close && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < close && (bytes[i] == b'\'' || bytes[i] == b'"') {
                let quote = bytes[i];
                let value_at = i + 1;
                let mut end = value_at;
                while end < close && bytes[end] != quote {
                    end += 1;
                }
                if end >= close {
                    return ExistingName::Kept;
                }
                let value = &source[value_at..end];
                i = end + 1;
                if is_name {
                    if value.is_empty() {
                        empty = Some(ExistingName::Empty {
                            at: value_at as u32,
                            text: name.to_string(),
                        });
                    } else {
                        return ExistingName::Kept;
                    }
                }
            } else if is_name {
                if i >= close || bytes[i] == b',' {
                    empty = Some(ExistingName::Empty {
                        at: i as u32,
                        text: format!("'{name}'"),
                    });
                } else {
                    return ExistingName::Kept;
                }
            } else {
                while i < close && bytes[i] != b',' && !bytes[i].is_ascii_whitespace() {
                    i += 1;
                }
            }
        } else if is_name {
            empty = Some(ExistingName::Empty {
                at: i as u32,
                text: format!("='{name}'"),
            });
        }
    }
    empty.unwrap_or(ExistingName::None)
}

fn is_key_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_key_cont(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(source: &str, edits: &[(Span, &str)]) -> String {
        let mut ordered: Vec<(Span, &str)> = edits.to_vec();
        ordered.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
        let mut out = source.to_string();
        for (span, text) in ordered {
            out.replace_range(span.start as usize..span.end as usize, text);
        }
        out
    }

    fn planned(source: &str) -> Option<(String, String)> {
        let mut ws = Workspace::new();
        let uri = r"C:\dygnosis-equation-names\plan.mod";
        ws.update_document(uri, source);
        let plan = equation_name_plan(&mut ws, uri)?;
        let edits: Vec<(Span, String)> = plan
            .edits
            .iter()
            .map(|edit| (edit.span, edit.new_text.clone()))
            .collect();
        let pairs: Vec<(Span, &str)> = edits
            .iter()
            .map(|(span, text)| (*span, text.as_str()))
            .collect();
        Some((plan.title, apply(source, &pairs)))
    }

    #[test]
    fn fills_an_empty_name_and_keeps_other_tags() {
        let source = include_str!("../tests/fixtures/equation_names/tags.mod");
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Name counted equations");
        assert_eq!(
            edited,
            include_str!("../tests/fixtures/equation_names/tags.named.mod")
        );
    }

    #[test]
    fn an_if_branch_names_each_equation() {
        let source = "\
var y, z;
@#if 1
model;
y = 1;
z = 2;
end;
@#endif
";
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Name counted equations");
        let model_at = edited.find("model;").unwrap();
        let first = edited.find("[name='eq_1']").unwrap();
        let second = edited.find("[name='eq_2']").unwrap();
        assert!(model_at < first && first < second, "{edited}");
        assert!(edited.contains("y = 1;"));
        assert!(edited.contains("z = 2;"));
    }

    #[test]
    fn an_if_branch_with_a_local_names_the_counted_equation() {
        let source = "\
var y;
@#if 1
model;
# z = 1;
y = z;
end;
@#endif
";
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Name counted equations");
        let local = edited.find("# z = 1;").unwrap();
        let named = edited.find("[name='eq_1']").unwrap();
        assert!(local < named, "{edited}");
        assert!(
            edited.contains("[name='eq_1'] y = z;") || edited.contains("[name='eq_1']\ny = z;"),
            "{edited}"
        );
        assert!(!edited[..=local].contains("[name="), "{edited}");
    }

    #[test]
    fn a_for_with_an_inner_if_is_a_copy() {
        let source = "\
var y;
@#define once = 1:1
model;
@#for i in once
@#if 1
y = @{i};
@#else
y = 0;
@#endif
@#endfor
end;
";
        assert!(
            planned(source).is_none(),
            "a one-iteration loop stays unedited"
        );
    }

    #[test]
    fn a_for_copy_is_skipped_and_keeps_its_number() {
        let source = include_str!("../tests/fixtures/equation_names/for_copy.mod");
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Name counted equations (2 skipped)");
        assert_eq!(
            edited,
            include_str!("../tests/fixtures/equation_names/for_copy.named.mod")
        );
    }

    #[test]
    fn a_name_on_a_skipped_loop_is_still_taken() {
        let source = include_str!("../tests/fixtures/equation_names/taken.mod");
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Name counted equations");
        assert_eq!(
            edited,
            include_str!("../tests/fixtures/equation_names/taken.named.mod")
        );
    }

    #[test]
    fn cross_scope_names_visit_aggregate_then_dimensions() {
        let source = include_str!("../tests/fixtures/equation_names/collision.mod");
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Name counted equations");
        assert_eq!(
            edited,
            include_str!("../tests/fixtures/equation_names/collision.named.mod")
        );
    }

    #[test]
    fn one_iteration_loop_is_skipped() {
        let source = "\
var y;
@#define is = 1:1
model;
y = y(-1);
@#for i in is
y = y(-1);
@#endfor
end;
";
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Name counted equations (1 skipped)");
        assert!(edited.contains("[name='eq_1'] y = y(-1);"));
        assert!(edited.contains("@#for i in is\ny = y(-1);"));
    }

    #[test]
    fn a_file_of_only_loop_copies_has_no_plan() {
        let source = include_str!("../tests/fixtures/equation_names/only_for.mod");
        assert!(planned(source).is_none());
    }

    #[test]
    fn a_shared_include_is_skipped_and_keeps_its_number() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/equation_names/shared_root.mod");
        let inc = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/equation_names/shared_eq.inc");
        let root_text = std::fs::read_to_string(&root)
            .unwrap()
            .replace("\r\n", "\n");
        let inc_text = std::fs::read_to_string(&inc).unwrap().replace("\r\n", "\n");
        let mut ws = Workspace::new();
        let root_uri = root.to_string_lossy();
        ws.update_document(&root_uri, &root_text);
        ws.update_document(&inc.to_string_lossy(), &inc_text);
        let plan = equation_name_plan(&mut ws, &root_uri).expect("action");
        assert_eq!(plan.title, "Name counted equations (2 skipped)");
        assert_eq!(plan.edits.len(), 1);
        let edit = &plan.edits[0];
        assert_eq!(
            crate::include_resolver::normalize_uri(&edit.file),
            crate::include_resolver::normalize_uri(&root_uri)
        );
        let edited = apply(&root_text, &[(edit.span, edit.new_text.as_str())]);
        let expected = include_str!("../tests/fixtures/equation_names/shared_root.named.mod")
            .replace("\r\n", "\n");
        assert_eq!(edited, expected);
        assert_eq!(
            ws.get_source(&inc.to_string_lossy()).unwrap(),
            inc_text.as_str()
        );
    }

    #[test]
    fn a_unique_include_is_edited_in_that_file() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/equation_names/one_inc.mod");
        let inc = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/equation_names/one_eq.inc");
        let root_text = std::fs::read_to_string(&root)
            .unwrap()
            .replace("\r\n", "\n");
        let inc_text = std::fs::read_to_string(&inc).unwrap().replace("\r\n", "\n");
        let mut ws = Workspace::new();
        let root_uri = root.to_string_lossy();
        let inc_uri = inc.to_string_lossy();
        ws.update_document(&root_uri, &root_text);
        ws.update_document(&inc_uri, &inc_text);
        let plan = equation_name_plan(&mut ws, &root_uri).expect("action");
        assert_eq!(plan.title, "Name counted equations");
        assert_eq!(plan.edits.len(), 1);
        let edit = &plan.edits[0];
        assert_eq!(
            crate::include_resolver::normalize_uri(&edit.file),
            crate::include_resolver::normalize_uri(&inc_uri)
        );
        let edited = apply(&inc_text, &[(edit.span, edit.new_text.as_str())]);
        let expected =
            include_str!("../tests/fixtures/equation_names/one_eq.named.inc").replace("\r\n", "\n");
        assert_eq!(edited, expected);
    }

    #[test]
    fn an_equation_from_two_files_is_skipped() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/equation_names/mixed.mod");
        let inc = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/equation_names/piece.inc");
        let root_text = std::fs::read_to_string(&root)
            .unwrap()
            .replace("\r\n", "\n");
        let inc_text = std::fs::read_to_string(&inc).unwrap().replace("\r\n", "\n");
        let mut ws = Workspace::new();
        let root_uri = root.to_string_lossy();
        ws.update_document(&root_uri, &root_text);
        ws.update_document(&inc.to_string_lossy(), &inc_text);
        let plan = equation_name_plan(&mut ws, &root_uri).expect("action");
        assert_eq!(plan.title, "Name counted equations (1 skipped)");
        assert_eq!(plan.edits.len(), 1);
        let edit = &plan.edits[0];
        let edited = apply(&root_text, &[(edit.span, edit.new_text.as_str())]);
        assert!(edited.contains("[name='eq_1'] y = y(-1);"));
        assert!(edited.contains("y =\n@#include \"piece.inc\""));
        assert_eq!(ws.get_source(&inc.to_string_lossy()).unwrap(), inc_text);
    }

    #[test]
    fn a_name_on_a_local_is_still_taken() {
        let source = "\
var y;
model;
[name='eq_1']
# x = 1;
y = y(-1);
end;
";
        let (title, edited) = planned(source).expect("action");
        assert_eq!(title, "Name counted equations");
        assert!(edited.contains("[name='eq_1']\n# x = 1;"));
        assert!(edited.contains("[name='eq_1_2'] y = y(-1);"));
    }

    #[test]
    fn a_taken_suffix_moves_to_the_next_free_name() {
        let source = "\
heterogeneity_dimension d;
var y;
var(heterogeneity=d) c;
model;
[name='eq_1']
y = y(-1);
[name='eq_1_2']
y = y(-1);
end;
model(heterogeneity=d);
c = c(-1);
end;
";
        let (_, edited) = planned(source).expect("action");
        assert!(edited.contains("[name='eq_1']\ny = y(-1);"));
        assert!(edited.contains("[name='eq_1_2']\ny = y(-1);"));
        assert!(edited.contains("[name='eq_1_3'] c = c(-1);"));
    }
}
