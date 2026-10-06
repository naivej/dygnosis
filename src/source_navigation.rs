//! Source-layout preview regions for Go to written source.

use serde_json::{json, Value};

use crate::lexer::{tokenize, TokenKind};
use crate::macro_expand::{SourceFragment, SourceFragmentKind};
use crate::span::Span;

pub(crate) const SOURCE_NAVIGATION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RegionKind {
    Copy,
    Substitution,
    Identifier,
}

impl RegionKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Copy => "copy",
            Self::Substitution => "substitution",
            Self::Identifier => "identifier",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Region {
    display: Span,
    /// Span in include-spliced expander source.
    written: Span,
    kind: RegionKind,
}

/// Build source-navigation regions from recorded source-layout fragments.
///
/// `location` maps a spliced written span to one verified written_location JSON
/// value, or `None` when the span is empty, crosses files, or cannot be proven.
pub(crate) fn source_navigation_json(
    display: &str,
    fragments: &[SourceFragment],
    file_cuts: &[u32],
    mut location: impl FnMut(Span) -> Option<Value>,
    mut effective_range: impl FnMut(Span) -> Value,
) -> Value {
    let regions = build_regions(display, fragments, file_cuts);
    Value::Array(
        regions
            .into_iter()
            .enumerate()
            .filter_map(|(index, region)| {
                let written_location = location(region.written)?;
                Some(json!({
                    "id": format!("r{index}"),
                    "effective_range": effective_range(region.display),
                    "written_location": written_location,
                    "kind": region.kind.as_str(),
                }))
            })
            .collect(),
    )
}

fn build_regions(display: &str, fragments: &[SourceFragment], file_cuts: &[u32]) -> Vec<Region> {
    let split = split_copies_at_files(display, fragments, file_cuts);
    let identifiers = identifier_covers(display, &split, file_cuts);
    let mut leftovers = Vec::new();
    for fragment in &split {
        let Some(written) = fragment.written else {
            continue;
        };
        if written.is_empty() || fragment.display.is_empty() {
            continue;
        }
        for gap in subtract_spans(fragment.display, &identifiers) {
            let Some(projected) = project_fragment(fragment, display, gap.start, gap.end)
            else {
                continue;
            };
            let kind = match fragment.kind {
                SourceFragmentKind::Copy => RegionKind::Copy,
                SourceFragmentKind::Substitution => RegionKind::Substitution,
            };
            leftovers.push(Region {
                display: gap,
                written: projected,
                kind,
            });
        }
    }
    leftovers.sort_by_key(|region| region.display.start);
    let mut coalesced = coalesce_copies(leftovers, file_cuts);
    for (display_span, written) in identifiers {
        coalesced.push(Region {
            display: display_span,
            written,
            kind: RegionKind::Identifier,
        });
    }
    coalesced.sort_by_key(|region| region.display.start);
    coalesced
}

fn split_copies_at_files(
    display: &str,
    fragments: &[SourceFragment],
    file_cuts: &[u32],
) -> Vec<SourceFragment> {
    let mut out = Vec::new();
    for fragment in fragments {
        let Some(written) = fragment.written else {
            out.push(fragment.clone());
            continue;
        };
        if fragment.kind != SourceFragmentKind::Copy {
            out.push(fragment.clone());
            continue;
        }
        let mut start_w = written.start;
        let mut start_d = fragment.display.start;
        let shown_len = (fragment.display.end - fragment.display.start) as usize;
        let written_len = (written.end - written.start) as usize;
        let aligned = shown_len == written_len;
        for &cut in file_cuts {
            if cut <= start_w || cut >= written.end || !aligned {
                continue;
            }
            let rel = (cut - start_w) as usize;
            let display_cut = start_d + rel as u32;
            if display.get(start_d as usize..display_cut as usize).is_none() {
                continue;
            }
            out.push(SourceFragment {
                display: Span {
                    start: start_d,
                    end: display_cut,
                },
                written: Some(Span {
                    start: start_w,
                    end: cut,
                }),
                kind: fragment.kind,
                macro_active: fragment.macro_active,
            });
            start_w = cut;
            start_d = display_cut;
        }
        if start_d < fragment.display.end {
            out.push(SourceFragment {
                display: Span {
                    start: start_d,
                    end: fragment.display.end,
                },
                written: Some(Span {
                    start: start_w,
                    end: written.end,
                }),
                kind: fragment.kind,
                macro_active: fragment.macro_active,
            });
        }
    }
    out
}

fn identifier_covers(
    display: &str,
    fragments: &[SourceFragment],
    file_cuts: &[u32],
) -> Vec<(Span, Span)> {
    let mut covers = Vec::new();
    for token in tokenize(display) {
        if token.kind != TokenKind::Ident || token.span.is_empty() {
            continue;
        }
        let mut parts = Vec::new();
        let mut has_sub = false;
        let mut has_copy = false;
        let mut ok = true;
        for fragment in fragments {
            let start = fragment.display.start.max(token.span.start);
            let end = fragment.display.end.min(token.span.end);
            if start >= end {
                continue;
            }
            let Some(written) = fragment.written else {
                ok = false;
                break;
            };
            if written.is_empty() {
                ok = false;
                break;
            }
            match fragment.kind {
                SourceFragmentKind::Substitution => {
                    has_sub = true;
                    parts.push(written);
                }
                SourceFragmentKind::Copy => {
                    has_copy = true;
                    let Some(projected) = project_fragment(fragment, display, start, end)
                    else {
                        ok = false;
                        break;
                    };
                    parts.push(projected);
                }
            }
        }
        if !ok || !has_sub || !has_copy || parts.is_empty() {
            continue;
        }
        parts.sort_by_key(|span| span.start);
        let mut cursor = parts[0].start;
        let mut contiguous = true;
        for part in &parts {
            if part.start != cursor {
                contiguous = false;
                break;
            }
            cursor = part.end;
        }
        let crosses_file = file_cuts
            .iter()
            .any(|&cut| parts[0].start < cut && cut < cursor);
        if contiguous && !crosses_file {
            covers.push((
                token.span,
                Span {
                    start: parts[0].start,
                    end: parts.last().map(|part| part.end).unwrap_or(parts[0].end),
                },
            ));
        }
    }
    covers.sort_by_key(|(display, _)| display.start);
    covers
}

fn subtract_spans(span: Span, covers: &[(Span, Span)]) -> Vec<Span> {
    let mut gaps = vec![span];
    for (cover, _) in covers {
        let mut next = Vec::new();
        for gap in gaps {
            if cover.end <= gap.start || cover.start >= gap.end {
                next.push(gap);
                continue;
            }
            if gap.start < cover.start {
                next.push(Span {
                    start: gap.start,
                    end: cover.start,
                });
            }
            if cover.end < gap.end {
                next.push(Span {
                    start: cover.end,
                    end: gap.end,
                });
            }
        }
        gaps = next;
    }
    gaps.into_iter().filter(|gap| !gap.is_empty()).collect()
}

fn coalesce_copies(regions: Vec<Region>, file_cuts: &[u32]) -> Vec<Region> {
    let mut out: Vec<Region> = Vec::new();
    for region in regions {
        if region.kind == RegionKind::Copy {
            if let Some(prev) = out.last_mut() {
                if prev.kind == RegionKind::Copy
                    && prev.display.end == region.display.start
                    && prev.written.end == region.written.start
                    && !file_cuts.contains(&region.written.start)
                {
                    prev.display.end = region.display.end;
                    prev.written.end = region.written.end;
                    continue;
                }
            }
        }
        out.push(region);
    }
    out
}

fn project_fragment(fragment: &SourceFragment, display: &str, d0: u32, d1: u32) -> Option<Span> {
    let written = fragment.written?;
    if d0 < fragment.display.start || d1 > fragment.display.end || d0 >= d1 {
        return None;
    }
    if fragment.kind == SourceFragmentKind::Substitution {
        // A substitution always jumps to the full interpolation site.
        return (d0 == fragment.display.start && d1 == fragment.display.end).then_some(written);
    }
    // The recorder already proved this display slice against the written span,
    // including newline normalization and omitted include-directive indent.
    let shown = display.get(fragment.display.start as usize..fragment.display.end as usize)?;
    let rel0 = (d0 - fragment.display.start) as usize;
    let rel1 = (d1 - fragment.display.start) as usize;
    if !shown.is_char_boundary(rel0) || !shown.is_char_boundary(rel1) {
        return None;
    }
    let written_len = (written.end - written.start) as usize;
    if shown.len() == written_len {
        return Some(Span {
            start: written.start + rel0 as u32,
            end: written.start + rel1 as u32,
        });
    }
    // CRLF in the written span is one `\n` in the preview. The display is
    // shorter by one byte per collapsed break, and the break sits at a
    // newline in the display.
    let (start, end) = map_display_newlines(shown, written_len, rel0, rel1)?;
    Some(Span {
        start: written.start + start as u32,
        end: written.start + end as u32,
    })
}

/// Map display offsets back through `\r\n` written as `\n`.
fn map_display_newlines(
    shown: &str,
    written_len: usize,
    rel0: usize,
    rel1: usize,
) -> Option<(usize, usize)> {
    let extra = written_len.checked_sub(shown.len())?;
    if extra == 0 || shown.bytes().filter(|&byte| byte == b'\n').count() < extra {
        return None;
    }
    let mut newlines = 0usize;
    let mut start = None;
    for (offset, ch) in shown.char_indices() {
        if offset == rel0 {
            start = Some(offset + newlines.min(extra));
        }
        if offset == rel1 {
            return Some((start?, offset + newlines.min(extra)));
        }
        if ch == '\n' && newlines < extra {
            newlines += 1;
        }
    }
    let start = start?;
    (rel1 == shown.len()).then_some((start, shown.len() + newlines.min(extra)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand::expand_report;
    use crate::parser::parse;
    use crate::preview_source::source;

    fn regions(text: &str) -> (String, Vec<Region>) {
        let report = expand_report(text);
        let model = parse(text);
        let layout = source(&report, &model, &[]).expect("source layout");
        let built = build_regions(&layout.text, &layout.fragments, &[]);
        (layout.text, built)
    }

    fn slice<'a>(text: &'a str, span: Span) -> &'a str {
        &text[span.start as usize..span.end as usize]
    }

    #[test]
    fn macro_built_beta_maps_to_complete_written_name() {
        let src = "@#for j in 1:2\nparameters beta_@{j};\n@#endfor\nvar y;\nmodel;\ny=1;\nend;\n";
        let (display, regions) = regions(src);
        let ids: Vec<_> = regions
            .iter()
            .filter(|region| region.kind == RegionKind::Identifier)
            .collect();
        assert_eq!(ids.len(), 2, "{regions:?}");
        assert_eq!(slice(&display, ids[0].display), "beta_1");
        assert_eq!(slice(src, ids[0].written), "beta_@{j}");
        assert_eq!(slice(&display, ids[1].display), "beta_2");
        assert_eq!(slice(src, ids[1].written), "beta_@{j}");
    }

    #[test]
    fn nested_interpolations_form_one_identifier() {
        let src = "@#for i in 1:1\n@#for j in 2:2\nparameters beta_@{i}_@{j};\n@#endfor\n@#endfor\nvar y;\nmodel;\ny=1;\nend;\n";
        let (display, regions) = regions(src);
        let id = regions
            .iter()
            .find(|region| region.kind == RegionKind::Identifier)
            .expect("identifier");
        assert_eq!(slice(&display, id.display), "beta_1_2");
        assert_eq!(slice(src, id.written), "beta_@{i}_@{j}");
    }
}
