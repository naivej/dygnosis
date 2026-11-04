//! Written locations for one effective model preview and its input snapshot.

use std::cell::OnceCell;
use std::collections::HashMap;

use serde_json::{Value, json};
use tower_lsp::lsp_types::{Range, Url};

use crate::engine::language::expand::{EquationOrigin, OriginFrame};
use crate::engine::language::parser::normalize_newlines;
use crate::engine::language::span::{LineIndex, Span};
use crate::engine::workspace::include_resolver::normalize_uri;
use crate::engine::workspace::model_map::WrittenSegment;

use super::{Inner, file_url_from_path_key, span_range};

const INDEX_BYTES: usize = 4 * 1024 * 1024;

pub(super) struct PreviewLocations<'a> {
    inner: &'a Inner,
    root: &'a Url,
    sources: HashMap<String, WrittenSource<'a>>,
    remaining_index_bytes: usize,
}

struct WrittenSource<'a> {
    coordinates: Option<Coordinates<'a>>,
    canonical_uri: OnceCell<Option<Url>>,
    navigation_target: OnceCell<Option<(Url, Option<i32>)>>,
}

struct Coordinates<'a> {
    text: &'a str,
    bare_cr: bool,
    workspace_index: Option<&'a LineIndex>,
    normalized_index: Option<LineIndex>,
    raw_index: Option<LineIndex>,
}

#[cfg(test)]
thread_local! {
    // Source entries, canonical URIs, navigation targets, new indexes.
    static WORK: std::cell::Cell<[usize; 4]> = const { std::cell::Cell::new([0; 4]) };
}

#[cfg(test)]
fn record_work(index: usize) {
    WORK.with(|counter| {
        let mut counts = counter.get();
        counts[index] += 1;
        counter.set(counts);
    });
}

fn written_source<'a, 'map>(
    sources: &'map mut HashMap<String, WrittenSource<'a>>,
    inner: &'a Inner,
    key: &str,
) -> &'map mut WrittenSource<'a> {
    if !sources.contains_key(key) {
        #[cfg(test)]
        record_work(0);
        let coordinates = inner
            .workspace
            .source_with_cached_index(key)
            .or_else(|| inner.workspace.get_source(key).map(|text| (text, None)))
            .map(|(text, workspace_index)| {
                let bytes = text.as_bytes();
                let bare_cr = bytes
                    .iter()
                    .enumerate()
                    .any(|(i, byte)| *byte == b'\r' && bytes.get(i + 1) != Some(&b'\n'));
                Coordinates {
                    text,
                    bare_cr,
                    workspace_index,
                    normalized_index: None,
                    raw_index: None,
                }
            });
        sources.insert(
            key.to_owned(),
            WrittenSource {
                coordinates,
                canonical_uri: OnceCell::new(),
                navigation_target: OnceCell::new(),
            },
        );
    }
    sources.get_mut(key).expect("source entry inserted")
}

impl Coordinates<'_> {
    fn range(&mut self, span: Span, navigation: bool, remaining: &mut usize) -> Range {
        // A bare CR starts a line for navigation. Legacy origins count only LF.
        let raw_only = self.bare_cr && !navigation;
        if !raw_only && let Some(index) = self.workspace_index {
            return span_range(index, self.text, span);
        }
        let slot = if raw_only {
            &mut self.raw_index
        } else {
            &mut self.normalized_index
        };
        if let Some(index) = slot {
            return span_range(index, self.text, span);
        }
        #[cfg(test)]
        record_work(3);
        let index = if navigation && self.bare_cr {
            LineIndex::new(&normalize_newlines(self.text))
        } else {
            LineIndex::new(self.text)
        };
        let range = span_range(&index, self.text, span);
        if let Some(remaining_after) = remaining.checked_sub(index.allocation_bytes()) {
            *remaining = remaining_after;
            *slot = Some(index);
        }
        range
    }
}

impl WrittenSource<'_> {
    fn canonical_uri(&self, key: &str) -> Option<&Url> {
        self.canonical_uri
            .get_or_init(|| {
                #[cfg(test)]
                record_work(1);
                file_url_from_path_key(key)
            })
            .as_ref()
    }
}

impl<'a> PreviewLocations<'a> {
    pub(super) fn new(inner: &'a Inner, root: &'a Url) -> Self {
        Self {
            inner,
            root,
            sources: HashMap::new(),
            remaining_index_bytes: INDEX_BYTES,
        }
    }

    pub(super) fn navigation(&mut self, segment: &WrittenSegment) -> Option<Value> {
        let key = segment.file.as_deref()?;
        if segment.span.is_empty() {
            return None;
        }
        let source = written_source(&mut self.sources, self.inner, key);
        let coordinates = source.coordinates.as_mut()?;
        coordinates
            .text
            .get(segment.span.start as usize..segment.span.end as usize)?;
        let range = coordinates.range(segment.span, true, &mut self.remaining_index_bytes);
        let (uri, version) = source
            .navigation_target
            .get_or_init(|| {
                #[cfg(test)]
                record_work(2);
                let uri = self
                    .inner
                    .docs
                    .keys()
                    .chain(std::iter::once(self.root))
                    .find(|uri| normalize_uri(uri.as_str()) == key)
                    .or_else(|| source.canonical_uri(key))?;
                if !["file", "untitled"].contains(&uri.scheme()) {
                    return None;
                }
                Some((uri.clone(), self.inner.document(uri).map(|doc| doc.version)))
            })
            .as_ref()?;
        Some(json!({"uri":uri, "range":range, "document_version":version}))
    }

    fn insert_origin(
        &mut self,
        object: &mut serde_json::Map<String, Value>,
        file: Option<&str>,
        span: Span,
    ) {
        let mut range = Range::default();
        if let Some(key) = file {
            let source = written_source(&mut self.sources, self.inner, key);
            if let Some(uri) = source.canonical_uri(key) {
                object.insert("origin_uri".into(), json!(uri));
            }
            if let Some(coordinates) = &mut source.coordinates {
                range = coordinates.range(span, false, &mut self.remaining_index_bytes);
            }
        }
        object.insert("range".into(), json!(range));
    }

    pub(super) fn equation_origin(
        &mut self,
        origin: &EquationOrigin,
        has_heterogeneous: bool,
    ) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("index".into(), json!(origin.index));
        if has_heterogeneous {
            object.insert("scope_index".into(), json!(origin.scope_index));
            if let Some(dimension) = &origin.dimension {
                object.insert("scope".into(), json!("heterogeneous"));
                object.insert("dimension".into(), json!(dimension));
                object.insert("block_index".into(), json!(origin.block_index));
            } else {
                object.insert("scope".into(), json!("aggregate"));
            }
        }
        self.insert_origin(
            &mut object,
            origin.origin_uri.as_deref(),
            origin.origin_span,
        );
        if !origin.origin_frames.is_empty() {
            let frames = origin
                .origin_frames
                .iter()
                .map(|frame| self.origin_frame(frame))
                .collect();
            object.insert("origin_frames".into(), Value::Array(frames));
        }
        Value::Object(object)
    }

    fn origin_frame(&mut self, frame: &OriginFrame) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("kind".into(), json!(frame.kind));
        if let Some(variable) = &frame.variable {
            object.insert("variable".into(), json!(variable));
        }
        if let Some(value) = &frame.value {
            object.insert("value".into(), json!(value));
        }
        self.insert_origin(&mut object, frame.origin_uri.as_deref(), frame.origin_span);
        Value::Object(object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::preview::source_navigation::take_work;
    use crate::lsp::{model_map::written_range, new_service};

    #[tokio::test]
    async fn source_preview_shares_preparation_and_skips_unrelated_covers() {
        let (service, _socket) = new_service();
        let server = service.inner();
        let root = Url::parse("untitled:preview-work.mod").unwrap();
        let text = "@#for i in 1:64\nvar y_@{i};\n@#endfor\nmodel;\n@#for i in 1:64\ny_@{i} = 0;\n@#endfor\nend;\n";
        server.upsert(root.clone(), text.into(), 1);
        let args = [json!({"root_uri": root, "layout": "source"})];

        take_work();
        let result = server.execute("dynare/showEffectiveModel", &args).unwrap();
        let work = take_work();
        assert_eq!(result["complete"], true, "{result}");
        let regions = result["source_navigation"].as_array().unwrap();
        assert_eq!(
            regions
                .iter()
                .filter(|row| row["kind"] == "identifier")
                .count(),
            128
        );
        assert!(!result["macro_ranges"].as_array().unwrap().is_empty());
        assert_eq!((work.split_passes, work.identifier_passes), (1, 1));
        assert_eq!(work.identifiers, 128);
        // Each fragment can check one cover without advancing. All other
        // checks advance the cursor, so a full scan per fragment fails here.
        assert!(work.gap_cover_checks > 0);
        assert!(
            work.gap_cover_checks <= work.fragments + work.identifiers,
            "{work:?}"
        );
    }

    #[tokio::test]
    async fn preview_reuses_written_work_and_uses_the_edited_source() {
        // Repeated occurrences need distinct rows and one lookup per written file.
        let (service, _socket) = new_service();
        let server = service.inner();
        let root = Url::parse("untitled:preview-locations.mod").unwrap();
        let source = "var y;\rmodel;\r@#for i in 1:64\r/* 😀 */ y = @{i};\r@#endfor\rend;\r";
        let args = [json!({"root_uri": root})];

        for (version, text) in [(1, source.to_owned()), (2, format!("// added\r\n{source}"))] {
            server.upsert(root.clone(), text, version);
            WORK.with(|counter| counter.set([0; 4]));
            let result = server.execute("dynare/showEffectiveModel", &args).unwrap();
            let [sources, canonical_uris, targets, indexes] = WORK.with(|counter| counter.get());
            assert_eq!([sources, canonical_uris, targets], [1, 1, 1]);
            assert!((1..=2).contains(&indexes), "{indexes} new indexes");
            assert_eq!(result["complete"], true, "{result}");
            let rows = result["navigation"].as_array().unwrap();
            assert_eq!(rows.len(), 64);
            assert_ne!(rows[0]["id"], rows[63]["id"]);
            assert_ne!(rows[0]["effective_range"], rows[63]["effective_range"]);
            for row in rows {
                let target = &row["written_locations"][0];
                assert_eq!(target["uri"], root.as_str());
                assert_eq!(target["document_version"], version);
                assert_eq!(
                    target["range"]["start"],
                    json!({"line": 2 + version, "character": 9})
                );
            }
            assert_eq!(result["origins"][0]["range"]["start"]["line"], version - 1);
        }
    }

    #[tokio::test]
    async fn index_cap_and_missing_sources_keep_existing_location_rules() {
        let (service, _socket) = new_service();
        let server = service.inner();
        let root = Url::parse("untitled:preview-cap.mod").unwrap();
        let unsupported = Url::parse("custom://preview/part.inc").unwrap();
        let text = "// text\r/* 😀 */ y = 0;\r";
        server.upsert(root.clone(), text.into(), 3);
        server.store_document(unsupported.clone(), text.into(), 4);
        let inner = server.lock_inner();
        let mut locations = PreviewLocations::new(&inner, &root);
        locations.remaining_index_bytes = 0;
        let y = text.find('y').unwrap();
        let mut segment = WrittenSegment {
            file: Some(root.to_string()),
            span: Span::new(y, y + 1),
        };
        let expected = written_range(text, segment.span).unwrap();
        assert_eq!(
            locations.navigation(&segment).unwrap()["range"],
            json!(expected)
        );
        let cached = locations.sources[root.as_str()]
            .coordinates
            .as_ref()
            .unwrap();
        assert!(cached.raw_index.is_none() && cached.normalized_index.is_none());

        let emoji = text.find('😀').unwrap();
        for span in [
            Span::new(y, y),
            Span::new(emoji + 1, y),
            Span::new(y, text.len() + 1),
        ] {
            segment.span = span;
            assert!(locations.navigation(&segment).is_none());
        }
        let missing = "untitled:missing-preview-source.inc";
        segment.file = Some(missing.into());
        segment.span = Span::new(y, y + 1);
        assert!(locations.navigation(&segment).is_none());
        let mut origin = serde_json::Map::new();
        locations.insert_origin(&mut origin, Some(missing), segment.span);
        assert_eq!(origin["origin_uri"], missing);
        assert_eq!(origin["range"], json!(Range::default()));

        segment.file = Some(unsupported.to_string());
        assert!(locations.navigation(&segment).is_none());
        locations.insert_origin(&mut origin, Some(unsupported.as_str()), segment.span);
        assert_eq!(origin["origin_uri"], unsupported.as_str());
        assert_ne!(origin["range"], json!(Range::default()));
    }
}
