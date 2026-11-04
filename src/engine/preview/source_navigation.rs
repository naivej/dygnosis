//! Source-layout navigation regions and macro tint ranges.

use serde_json::{Value, json};

use crate::engine::language::lexer::{TokenKind, tokenize};
use crate::engine::language::macro_expand::{SourceFragment, SourceFragmentKind};
use crate::engine::language::span::Span;

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

struct PreparedSource {
    fragments: Vec<SourceFragment>,
    identifiers: Vec<(Span, Span)>,
}

impl PreparedSource {
    fn new<E>(
        display: &str,
        fragments: &[SourceFragment],
        file_cuts: &[u32],
        check_cancellation: &impl Fn() -> Result<(), E>,
    ) -> Result<Self, E> {
        let fragments = split_copies_at_files(display, fragments, file_cuts, check_cancellation)?;
        let identifiers = identifier_covers(display, &fragments, file_cuts, check_cancellation)?;
        Ok(Self {
            fragments,
            identifiers,
        })
    }
}

#[cfg(test)]
#[derive(Default, Debug)]
pub(crate) struct ProjectionWork {
    pub(crate) split_passes: usize,
    pub(crate) identifier_passes: usize,
    pub(crate) fragments: usize,
    pub(crate) identifiers: usize,
    pub(crate) gap_cover_checks: usize,
}

#[cfg(test)]
thread_local! {
    static WORK: std::cell::RefCell<ProjectionWork> = std::cell::RefCell::default();
}

#[cfg(test)]
pub(crate) fn take_work() -> ProjectionWork {
    WORK.with(std::cell::RefCell::take)
}

/// Build navigation regions and tint ranges from the same prepared fragments.
///
/// `location` maps a spliced written span to one verified written_location JSON
/// value, or `None` when the span is empty, crosses files, or cannot be proven.
pub(crate) fn source_projection_json<E>(
    display: &str,
    fragments: &[SourceFragment],
    file_cuts: &[u32],
    include_spans: &[Span],
    check_cancellation: impl Fn() -> Result<(), E>,
    mut location: impl FnMut(Span) -> Option<Value>,
    mut effective_range: impl FnMut(Span) -> Value,
) -> Result<(Value, Value), E> {
    let prepared = PreparedSource::new(display, fragments, file_cuts, &check_cancellation)?;
    let ranges = macro_display_ranges(&prepared, include_spans, &check_cancellation)?;
    let regions = build_regions(display, prepared, file_cuts, &check_cancellation)?;
    let mut rows = Vec::new();
    for (index, region) in regions.into_iter().enumerate() {
        check_cancellation()?;
        if let Some(written_location) = location(region.written) {
            rows.push(json!({
                "id": format!("r{index}"),
                "effective_range": effective_range(region.display),
                "written_location": written_location,
                "kind": region.kind.as_str(),
            }));
        }
    }
    check_cancellation()?;
    let mut tint = Vec::with_capacity(ranges.len());
    for span in ranges {
        check_cancellation()?;
        tint.push(effective_range(span));
    }
    Ok((Value::Array(rows), Value::Array(tint)))
}

/// Sorted, non-overlapping display spans for macro-expanded text tinting.
///
/// A fragment is shaded when it has a written span and either `macro_active` is
/// set, its kind is `Substitution`, or its written span lies inside an included
/// splice segment. Synthetic fragments (`written: None`) are never shaded. A
/// macro-built identifier is shaded across its whole display span because it
/// contains a whole, nonempty substitution with a written span.
fn macro_display_ranges<E>(
    prepared: &PreparedSource,
    include_spans: &[Span],
    check_cancellation: &impl Fn() -> Result<(), E>,
) -> Result<Vec<Span>, E> {
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
    for (id_display, _) in &prepared.identifiers {
        check_cancellation()?;
        shaded.push(*id_display);
    }
    for fragment in &prepared.fragments {
        check_cancellation()?;
        if shaded_fragment(fragment) {
            shaded.push(fragment.display);
        }
    }
    Ok(merge_shaded_ranges(shaded))
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

fn build_regions<E>(
    display: &str,
    prepared: PreparedSource,
    file_cuts: &[u32],
    check_cancellation: &impl Fn() -> Result<(), E>,
) -> Result<Vec<Region>, E> {
    let PreparedSource {
        fragments,
        identifiers,
    } = prepared;
    let mut leftovers = Vec::new();
    // The recorder appends non-overlapping fragments in display order. Keep
    // a cover that extends past this fragment for the next one.
    let mut covers = identifiers.iter().peekable();
    for fragment in &fragments {
        check_cancellation()?;
        let Some(written) = fragment.written else {
            continue;
        };
        if written.is_empty() || fragment.display.is_empty() {
            continue;
        }
        let kind = match fragment.kind {
            SourceFragmentKind::Copy => RegionKind::Copy,
            SourceFragmentKind::Substitution => RegionKind::Substitution,
        };
        let mut push_gap = |start, end| {
            if let Some(written) = project_fragment(fragment, display, start, end) {
                leftovers.push(Region {
                    display: Span { start, end },
                    written,
                    kind,
                });
            }
        };
        let mut start = fragment.display.start;
        let end = fragment.display.end;
        while let Some((cover, _)) = covers.peek() {
            check_cancellation()?;
            #[cfg(test)]
            WORK.with(|work| work.borrow_mut().gap_cover_checks += 1);
            if cover.start >= end {
                break;
            }
            if cover.end > start {
                if cover.start > start {
                    push_gap(start, cover.start);
                }
                start = cover.end;
                if start > end {
                    break;
                }
            }
            covers.next();
        }
        if start < end {
            push_gap(start, end);
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
    Ok(coalesced)
}

fn split_copies_at_files<E>(
    display: &str,
    fragments: &[SourceFragment],
    file_cuts: &[u32],
    check_cancellation: &impl Fn() -> Result<(), E>,
) -> Result<Vec<SourceFragment>, E> {
    let mut out = Vec::new();
    for fragment in fragments {
        check_cancellation()?;
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
    #[cfg(test)]
    WORK.with(|work| {
        let mut work = work.borrow_mut();
        work.split_passes += 1;
        work.fragments += out.len();
    });
    Ok(out)
}

fn identifier_covers<E>(
    display: &str,
    fragments: &[SourceFragment],
    file_cuts: &[u32],
    check_cancellation: &impl Fn() -> Result<(), E>,
) -> Result<Vec<(Span, Span)>, E> {
    let mut covers = Vec::new();
    for token in tokenize(display) {
        check_cancellation()?;
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
    #[cfg(test)]
    WORK.with(|work| {
        let mut work = work.borrow_mut();
        work.identifier_passes += 1;
        work.identifiers += covers.len();
    });
    Ok(covers)
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
    use crate::engine::language::expand::expand_report;
    use crate::engine::language::parser::parse;
    use crate::engine::preview::preview_source::source;
    use crate::engine::workspace::Workspace;

    fn active() -> Result<(), ()> {
        Ok(())
    }

    fn regions(text: &str) -> (String, Vec<Region>) {
        let report = expand_report(text);
        let model = parse(text);
        let layout = source(&report, &model, &[], &[]).expect("source layout");
        let prepared = PreparedSource::new(&layout.text, &layout.fragments, &[], &active).unwrap();
        let built = build_regions(&layout.text, prepared, &[], &active).unwrap();
        (layout.text, built)
    }

    fn tint(text: &str) -> (String, Vec<Span>) {
        let report = expand_report(text);
        let model = parse(text);
        let layout = source(&report, &model, &[], &[]).expect("source layout");
        let prepared = PreparedSource::new(&layout.text, &layout.fragments, &[], &active).unwrap();
        let ranges = macro_display_ranges(&prepared, &[], &active).unwrap();
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
        let prepared =
            PreparedSource::new(&layout.text, &layout.fragments, &cuts, &active).unwrap();
        let ranges = macro_display_ranges(&prepared, &includes, &active).unwrap();
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
    fn cancelled_source_preparation_stops_before_projecting_locations_or_tint() {
        let display = "beta_1";
        let fragments = [SourceFragment {
            display: Span::new(0, display.len()),
            written: Some(Span::new(0, display.len())),
            kind: SourceFragmentKind::Copy,
            macro_active: true,
        }];
        let checks = std::cell::Cell::new(0);
        let cancel_during_identifiers = || {
            let count = checks.get() + 1;
            checks.set(count);
            (count < 2).then_some(()).ok_or("cancelled")
        };
        let result = source_projection_json(
            display,
            &fragments,
            &[],
            &[],
            cancel_during_identifiers,
            |_| panic!("cancelled source region reached written projection"),
            |_| panic!("cancelled source preview reached display projection"),
        );
        assert_eq!(result, Err("cancelled"));
    }

    #[test]
    fn source_projection_keeps_file_cuts_and_skipped_region_ids() {
        // The written text is `p a@{n} z`. The generated name is `a1`.
        let fragments = [
            SourceFragment {
                display: Span::new(0, 3),
                written: Some(Span::new(0, 3)),
                kind: SourceFragmentKind::Copy,
                macro_active: false,
            },
            SourceFragment {
                display: Span::new(3, 4),
                written: Some(Span::new(3, 7)),
                kind: SourceFragmentKind::Substitution,
                macro_active: false,
            },
            SourceFragment {
                display: Span::new(4, 6),
                written: Some(Span::new(7, 9)),
                kind: SourceFragmentKind::Copy,
                macro_active: false,
            },
        ];
        let coordinates = |span: Span| json!([span.start, span.end]);
        let project = |cuts: &[u32], rejected: Option<Span>| {
            source_projection_json(
                "p a1 z",
                &fragments,
                cuts,
                &[],
                active,
                |span| (Some(span) != rejected).then(|| coordinates(span)),
                coordinates,
            )
            .unwrap()
        };

        // Cuts at the name's ends allow the cover. An unverified location
        // omits its region but keeps the ID of the following copy.
        let (regions, tint) = project(&[2, 7], Some(Span::new(2, 7)));
        assert_eq!(
            regions,
            json!([
                {"id":"r0", "effective_range":[0,2], "written_location":[0,2], "kind":"copy"},
                {"id":"r2", "effective_range":[4,6], "written_location":[7,9], "kind":"copy"},
            ])
        );
        assert_eq!(tint, json!([[2, 4]]));

        // The extra cut between copy and substitution prevents the cover.
        let (regions, tint) = project(&[2, 3, 7], None);
        assert_eq!(
            regions,
            json!([
                {"id":"r0", "effective_range":[0,2], "written_location":[0,2], "kind":"copy"},
                {"id":"r1", "effective_range":[2,3], "written_location":[2,3], "kind":"copy"},
                {"id":"r2", "effective_range":[3,4], "written_location":[3,7], "kind":"substitution"},
                {"id":"r3", "effective_range":[4,6], "written_location":[7,9], "kind":"copy"},
            ])
        );
        assert_eq!(tint, json!([[3, 4]]));
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
        assert!(
            ranges.iter().any(|span| span.start == beta_at as u32
                && span.end == (beta_at + "beta_2".len()) as u32)
        );
        assert!(
            !ranges
                .iter()
                .any(|span| span.start <= params as u32 && (params + 10) as u32 <= span.end)
        );
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
