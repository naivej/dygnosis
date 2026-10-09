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

/// Sorted, non-overlapping display spans for macro-expanded text tinting.
///
/// A fragment is shaded when it has a written span and either `macro_active` is
/// set, its kind is `Substitution`, or its written span lies inside an included
/// splice segment. Synthetic fragments (`written: None`) are never shaded. A
/// macro-built identifier is shaded across its whole display span when any of
/// its fragments is shaded.
pub(crate) fn macro_display_ranges(
    display: &str,
    fragments: &[SourceFragment],
    file_cuts: &[u32],
    include_spans: &[Span],
) -> Vec<Span> {
    let split = split_copies_at_files(display, fragments, file_cuts);
    let identifiers = identifier_covers(display, &split, file_cuts);
    let shaded_fragment = |fragment: &SourceFragment| -> bool {
        let Some(written) = fragment.written else {
            return false;
        };
        if written.is_empty() || fragment.display.is_empty() {
            return false;
        }
        fragment.macro_active
            || fragment.kind == SourceFragmentKind::Substitution
            || include_spans.iter().any(|segment| {
                !segment.is_empty() && written.start >= segment.start && written.end <= segment.end
            })
    };
    let mut shaded = Vec::new();
    for (id_display, _) in &identifiers {
        let any = split.iter().any(|fragment| {
            let start = fragment.display.start.max(id_display.start);
            let end = fragment.display.end.min(id_display.end);
            start < end && shaded_fragment(fragment)
        });
        if any {
            shaded.push(*id_display);
        }
    }
    for fragment in &split {
        if !shaded_fragment(fragment) {
            continue;
        }
        for gap in subtract_spans(fragment.display, &identifiers) {
            if !gap.is_empty() {
                shaded.push(gap);
            }
        }
    }
    merge_shaded_ranges(shaded)
}

pub(crate) fn macro_ranges_json(
    display: &str,
    fragments: &[SourceFragment],
    file_cuts: &[u32],
    include_spans: &[Span],
    effective_range: impl FnMut(Span) -> Value,
) -> Value {
    Value::Array(
        macro_display_ranges(display, fragments, file_cuts, include_spans)
            .into_iter()
            .map(effective_range)
            .collect(),
    )
}

fn merge_shaded_ranges(mut ranges: Vec<Span>) -> Vec<Span> {
    ranges.retain(|span| !span.is_empty());
    ranges.sort_by_key(|span| (span.start, span.end));
    let mut out: Vec<Span> = Vec::new();
    for span in ranges {
        if let Some(prev) = out.last_mut()
            && span.start <= prev.end
        {
            prev.end = prev.end.max(span.end);
            continue;
        }
        out.push(span);
    }
    out
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
            let Some(projected) = project_fragment(fragment, display, gap.start, gap.end) else {
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
            if display
                .get(start_d as usize..display_cut as usize)
                .is_none()
            {
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
                    // A replacement may produce several tokens. Only a whole
                    // replacement can be part of one written identifier; keep
                    // partial overlaps in the original substitution region.
                    if start != fragment.display.start || end != fragment.display.end {
                        ok = false;
                        break;
                    }
                    has_sub = true;
                    parts.push(written);
                }
                SourceFragmentKind::Copy => {
                    has_copy = true;
                    let Some(projected) = project_fragment(fragment, display, start, end) else {
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
        if region.kind == RegionKind::Copy
            && let Some(prev) = out.last_mut()
            && prev.kind == RegionKind::Copy
            && prev.display.end == region.display.start
            && prev.written.end == region.written.start
            && !file_cuts.contains(&region.written.start)
        {
            prev.display.end = region.display.end;
            prev.written.end = region.written.end;
            continue;
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
    // The recorder isolates every collapsed CRLF. No count-based guess is
    // needed about where mixed written line endings occurred.
    (shown == "\n" && written_len == 2 && rel0 == 0 && rel1 == 1).then_some(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::expand::expand_report;
    use crate::parser::parse;
    use crate::preview_source::source;
    use crate::workspace::Workspace;

    fn regions(text: &str) -> (String, Vec<Region>) {
        let report = expand_report(text);
        let model = parse(text);
        let layout = source(&report, &model, &[], &[]).expect("source layout");
        let built = build_regions(&layout.text, &layout.fragments, &[]);
        (layout.text, built)
    }

    fn tint(text: &str) -> (String, Vec<Span>) {
        let report = expand_report(text);
        let model = parse(text);
        let layout = source(&report, &model, &[], &[]).expect("source layout");
        let ranges = macro_display_ranges(&layout.text, &layout.fragments, &[], &[]);
        (layout.text, ranges)
    }

    fn tint_includes(
        root: &str,
        root_text: &str,
        files: &[(&str, &str)],
    ) -> (String, Vec<Span>, Vec<Span>) {
        let mut ws = Workspace::default();
        for (path, text) in files {
            ws.update_document(path, *text);
        }
        ws.update_document(root, root_text);
        let model = ws.get_effective_model(root).unwrap().clone();
        let report = ws.expand_report(root).unwrap().clone();
        let gaps = ws.source_layout_gaps(root);
        let cuts = ws.source_file_cuts(root);
        let includes = ws.source_include_spans(root);
        let layout = source(&report, &model, &gaps, &[]).expect("source layout");
        let ranges = macro_display_ranges(&layout.text, &layout.fragments, &cuts, &includes);
        (layout.text, ranges, includes)
    }

    fn slice(text: &str, span: Span) -> &str {
        &text[span.start as usize..span.end as usize]
    }

    fn shaded_text(display: &str, ranges: &[Span]) -> String {
        ranges
            .iter()
            .map(|span| slice(display, *span))
            .collect::<Vec<_>>()
            .join("|")
    }

    fn covers(display: &str, ranges: &[Span], needle: &str) -> bool {
        ranges
            .iter()
            .any(|span| slice(display, *span).contains(needle))
    }

    fn fully_shaded(display: &str, ranges: &[Span], needle: &str) -> bool {
        let start = display.find(needle).expect(needle);
        let span = Span::new(start, start + needle.len());
        ranges
            .iter()
            .any(|range| range.start <= span.start && span.end <= range.end)
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

    #[test]
    fn for_body_is_shaded_including_whitespace() {
        let (display, ranges) =
            tint("var y;\n@#for j in 1:2\nparameters beta_@{j};\n@#endfor\nmodel;\ny=1;\nend;\n");
        assert!(fully_shaded(&display, &ranges, "parameters beta_1;"));
        assert!(fully_shaded(&display, &ranges, "parameters beta_2;"));
        assert!(!covers(&display, &ranges, "var y"));
        assert!(!covers(&display, &ranges, "model;"));
        assert!(display.contains("parameters beta_1;\nparameters beta_2;\n"));
    }

    #[test]
    fn conditional_forms_shade_selected_bodies() {
        for src in [
            "var y;\n@#if 1\ny=c;\n@#endif\nmodel;\ny=1;\nend;\n",
            "var y;\n@#if 0\nbad=1;\n@#else\ny=c;\n@#endif\nmodel;\ny=1;\nend;\n",
            "var y;\n@#if 0\nbad=1;\n@#elseif 1\ny=c;\n@#endif\nmodel;\ny=1;\nend;\n",
            "var y;\n@#define FLAG = 1\n@#ifdef FLAG\ny=c;\n@#endif\nmodel;\ny=1;\nend;\n",
            "var y;\n@#ifndef FLAG\ny=c;\n@#endif\nmodel;\ny=1;\nend;\n",
        ] {
            let (display, ranges) = tint(src);
            assert!(
                fully_shaded(&display, &ranges, "y=c;"),
                "src={src}\ntext={display}\nshaded={}",
                shaded_text(&display, &ranges)
            );
            assert!(!covers(&display, &ranges, "bad"), "{display}");
            assert!(!covers(&display, &ranges, "var y"), "{display}");
        }
    }

    #[test]
    fn nested_ops_use_one_merged_shade() {
        let (display, ranges) = tint(
            "var y;\n@#for i in 1:1\n@#if 1\n  z=@{i};\n@#endif\n@#endfor\nmodel;\ny=1;\nend;\n",
        );
        assert!(fully_shaded(&display, &ranges, "  z=1;\n"), "{display}");
        assert_eq!(
            ranges
                .iter()
                .filter(|span| slice(&display, **span).contains("z=1"))
                .count(),
            1
        );
    }

    #[test]
    fn executed_include_is_shaded_and_dormant_is_not() {
        let (display, ranges, includes) = tint_includes(
            "C:/probe/root.mod",
            "model;\n@#include \"inner.mod\"\nend;\n",
            &[("C:/probe/inner.mod", "y=1;\n")],
        );
        assert!(!includes.is_empty(), "{includes:?}");
        assert!(fully_shaded(&display, &ranges, "y=1;\n"), "{display}");
        assert!(!covers(&display, &ranges, "model;"), "{display}");
        assert!(!covers(&display, &ranges, "end;"), "{display}");

        let (dormant, dormant_ranges, _) = tint_includes(
            "C:/probe/root.mod",
            "var y;\n@#if 0\n@#include \"inner.mod\"\n@#endif\nmodel;\ny=1;\nend;\n",
            &[("C:/probe/inner.mod", "leaked=1;\n")],
        );
        assert!(!dormant.contains("leaked"), "{dormant}");
        assert!(!covers(&dormant, &dormant_ranges, "leaked"));
        assert!(!covers(&dormant, &dormant_ranges, "y=1"), "{dormant}");
    }

    #[test]
    fn standalone_and_quoted_interpolation_are_shaded() {
        let (display, ranges) = tint("@#define x=99\nvar y;\nmodel;\ny=@{x};\nend;\n");
        assert!(fully_shaded(&display, &ranges, "99"), "{display}");
        assert!(!fully_shaded(&display, &ranges, "y="), "{display}");

        let (quoted, quoted_ranges) =
            tint("@#define j=2\nvar y;\nmodel;\n[name='eq@{j}'] y=1;\nend;\n");
        assert!(fully_shaded(&quoted, &quoted_ranges, "2"), "{quoted}");
        assert!(!covers(&quoted, &quoted_ranges, "eq"), "{quoted}");
    }

    #[test]
    fn beta_identifier_outside_body_shades_whole_name_not_neighbors() {
        let (display, ranges) =
            tint("@#define j=2\nparameters beta_@{j};\nvar y;\nmodel;\ny=1;\nend;\n");
        assert!(fully_shaded(&display, &ranges, "beta_2"), "{display}");
        assert!(!fully_shaded(&display, &ranges, "parameters "), "{display}");
        let beta_at = display.find("beta_2").unwrap();
        let params = display.find("parameters").unwrap();
        assert!(ranges
            .iter()
            .any(|span| span.start == beta_at as u32
                && span.end == (beta_at + "beta_2".len()) as u32));
        assert!(!ranges
            .iter()
            .any(|span| span.start <= params as u32 && (params + 10) as u32 <= span.end));
    }

    #[test]
    fn loop_body_shades_whole_parameters_line() {
        let (display, ranges) =
            tint("@#for j in 1:1\nparameters beta_@{j};\n@#endfor\nvar y;\nmodel;\ny=1;\nend;\n");
        assert!(
            fully_shaded(&display, &ranges, "parameters beta_1;"),
            "{display}"
        );
    }

    #[test]
    fn several_interpolations_unicode_and_crlf_shade() {
        let (display, ranges) = tint(
            "@#for i in 1:1\n@#for j in 2:2\nparameters beta_@{i}_@{j};\n@#endfor\n@#endfor\nvar y;\nmodel;\ny=1;\nend;\n",
        );
        assert!(
            fully_shaded(&display, &ranges, "parameters beta_1_2;"),
            "{display}"
        );

        let (unicode, unicode_ranges) =
            tint("@#define j=1\nvar y;\nmodel;\n[name='😀@{j}'] y=1;\nend;\n");
        assert!(fully_shaded(&unicode, &unicode_ranges, "1"), "{unicode}");
        assert!(!covers(&unicode, &unicode_ranges, "😀"), "{unicode}");

        let (crlf, crlf_ranges) = tint(
            "@#for j in 1:1\r\nparameters beta_@{j};\r\n@#endfor\r\nvar y;\r\nmodel;\r\ny=1;\r\nend;\r\n",
        );
        assert!(!crlf.contains('\r'), "{crlf:?}");
        assert!(
            fully_shaded(&crlf, &crlf_ranges, "parameters beta_1;"),
            "{crlf}"
        );
    }

    #[test]
    fn adjacent_shaded_ranges_merge_without_crossing_root() {
        let (display, ranges) =
            tint("@#define a=1\n@#define b=2\nvar y;\nmodel;\ny=@{a}@{b};\nend;\n");
        assert!(
            covers(&display, &ranges, "12") || fully_shaded(&display, &ranges, "1"),
            "{display}"
        );
        let ones: Vec<_> = ranges
            .iter()
            .filter(|span| {
                let text = slice(&display, **span);
                text.contains('1') || text.contains('2')
            })
            .collect();
        assert!(!ones.is_empty(), "{}", shaded_text(&display, &ranges));
        assert!(!fully_shaded(&display, &ranges, "y="), "{display}");
    }

    #[test]
    fn synthetic_include_separator_is_unshaded() {
        let (display, ranges, _) = tint_includes(
            "C:/probe/root.mod",
            "model;\n@#include \"inner.mod\"\nend;\n",
            &[("C:/probe/inner.mod", "y=1;")],
        );
        assert_eq!(display, "model;\ny=1;\nend;\n");
        assert!(fully_shaded(&display, &ranges, "y=1;"), "{display}");
        let sep = display.find("y=1;\n").unwrap() + "y=1;".len();
        assert!(
            !ranges
                .iter()
                .any(|span| span.start == sep as u32 && span.end == (sep + 1) as u32),
            "{}",
            shaded_text(&display, &ranges)
        );
    }
}
