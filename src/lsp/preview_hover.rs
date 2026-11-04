//! Hover reads an already proven Source preview and its selected model.

use super::*;
use crate::engine::analysis::source_inventory::SourceInventory;
use crate::engine::analysis::symbols::SemanticModel;
use crate::engine::control;
use std::collections::VecDeque;
use std::sync::Weak;

const MAX_SNAPSHOTS: usize = 16;
const MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(serde::Deserialize)]
pub(super) struct SnapshotId {
    root_uri: Url,
    snapshot_id: String,
}

#[derive(serde::Deserialize)]
pub(super) struct HoverParams {
    #[serde(flatten)]
    snapshot: SnapshotId,
    position: Position,
    #[serde(default)]
    definition_link: bool,
}

#[derive(Debug, serde::Serialize)]
pub(super) struct PreviewHover {
    #[serde(flatten)]
    hover: Hover,
    #[serde(skip_serializing_if = "Option::is_none")]
    definition: Option<PreviewDefinition>,
}

impl From<Hover> for PreviewHover {
    fn from(hover: Hover) -> Self {
        Self {
            hover,
            definition: None,
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct PreviewDefinition {
    #[serde(flatten)]
    location: Location,
    document_version: Option<i32>,
}

struct Snapshot {
    root: Url,
    id: String,
    text: String,
    index: LineIndex,
    spans: Vec<Option<Span>>,
    inventory: SourceInventory,
    view: Weak<SemanticModel>,
    bytes: usize,
}

impl Snapshot {
    fn matches(&self, id: &SnapshotId) -> bool {
        self.root == id.root_uri && self.id == id.snapshot_id
    }

    fn hover(
        &self,
        inner: &Inner,
        view: Arc<SemanticModel>,
        params: &HoverParams,
    ) -> Option<PreviewHover> {
        let byte = self
            .index
            .offset_utf16(&self.text, span_pos(params.position));
        let order = self
            .inventory
            .lexical_tokens
            .partition_point(|token| token.span.end <= byte);
        let token = self.inventory.lexical_tokens.get(order)?;
        let span = self.spans.get(order).copied().flatten()?;
        if token.span != span || span.start > byte || span.end <= byte {
            return None;
        }
        if let Some(hover) =
            written_requests::hover_at(&self.inventory, &self.text, byte, &self.index)
        {
            return hover.map(PreviewHover::from);
        }

        let preferences = inner.presentation_for(&self.root);
        let model_hover = |markdown, span| {
            let range = span_range(&self.index, &self.text, span);
            let mut hover = markdown_hover(markdown, Some(range));
            source_intelligence::label_root(inner, &self.root, &mut hover);
            PreviewHover::from(hover)
        };
        if token.kind == TokenKind::Ident
            && let Some(description) =
                locals::preview_hover(inner, &self.root, &view, order, &preferences)
        {
            if !params.definition_link {
                return Some(model_hover(description.into_markdown(), span));
            }
            let mut hover = model_hover(description.markdown, span);
            hover.definition = description.definition.map(|location| PreviewDefinition {
                document_version: inner.document(&location.uri).map(|doc| doc.version),
                location,
            });
            return Some(hover);
        }
        let spelling = token.text(&self.text);
        let (name, name_span) = if token.kind == TokenKind::String && spelling.len() >= 2 {
            (
                &spelling[1..spelling.len() - 1],
                Span {
                    start: span.start + 1,
                    end: span.end - 1,
                },
            )
        } else {
            (spelling, span)
        };
        if matches!(token.kind, TokenKind::Ident | TokenKind::String)
            && name_span.start <= byte
            && byte < name_span.end
            && let Some(binding) = view.typed_at_order(order, name)
        {
            return binding
                .map(|binding| model_hover(typed_names::description(&binding, false), name_span));
        }
        if token.kind == TokenKind::String {
            let (name, within) = view.semantic_tag_at(order, byte - span.start)?;
            let markdown = decl_hover_markdown_in_view(&view, name, &preferences)?;
            let name_span = Span {
                start: span.start + within.start,
                end: span.start + within.end,
            };
            return Some(model_hover(markdown, name_span));
        }
        if token.kind != TokenKind::Ident || !self.inventory.permits_display_name(byte) {
            return None;
        }
        let markdown = decl_hover_markdown_in_view(&view, spelling, &preferences)?;
        Some(model_hover(markdown, span))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::analysis::model_info::ASSIGNED_NUMBER_PASSES;
    use crate::engine::workspace::INPUT_REVISION_CHECKS;

    const SOURCE: &str = concat!(
        "@#for j in 1:2\n",
        "var y_@{j}; parameters beta_@{j}; beta_@{j} = @{j} / 10;\n",
        "@#endfor\n",
        "model_local_variable first_local, second_local;\n",
        "model;\n",
        "# unused_local = 1;\n",
        "# first_local = 3;\n",
        "# second_local = 4;\n",
        "@#for j in 1:2\n",
        "# local_@{j} = @{j} + beta_@{j};\n",
        "/* 🧪 */ [mcp='y_@{j} > 0'] y_@{j} = local_@{j} + beta_@{j} * y_@{j}(-1);\n",
        "@#endfor\n",
        "end; steady(solve_algo=1); stoch_simul(order=2, pruning);\n",
    );

    async fn open(backend: &Backend, root: &Url, text: &str) {
        backend
            .did_open(DidOpenTextDocumentParams {
                text_document: TextDocumentItem {
                    uri: root.clone(),
                    language_id: "dynare".into(),
                    version: 1,
                    text: text.into(),
                },
            })
            .await;
    }

    async fn preview(backend: &Backend, root: &Url) -> Value {
        backend
            .execute_command(ExecuteCommandParams {
                command: "dynare/showEffectiveModel".into(),
                arguments: vec![json!({"root_uri":root, "layout":"source", "hover":true})],
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap()
    }

    async fn watched_change(backend: &Backend, uri: &Url) {
        backend
            .did_change_watched_files(DidChangeWatchedFilesParams {
                changes: vec![FileEvent {
                    uri: uri.clone(),
                    typ: FileChangeType::CHANGED,
                }],
            })
            .await;
    }

    fn params(root: &Url, preview: &Value, needle: &str) -> HoverParams {
        let text = preview["effective_text"].as_str().unwrap();
        let at = text.rfind(needle).unwrap() as u32;
        let point = LineIndex::new(text).position_utf16(text, at);
        HoverParams {
            snapshot: SnapshotId {
                root_uri: root.clone(),
                snapshot_id: preview["hover_snapshot_id"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{preview}"))
                    .into(),
            },
            position: Position::new(point.line, point.character),
            definition_link: false,
        }
    }

    fn markdown(hover: &PreviewHover) -> &str {
        match &hover.hover.contents {
            HoverContents::Markup(markup) => &markup.value,
            _ => panic!("expected Markdown hover"),
        }
    }

    #[tokio::test]
    async fn preview_hover_uses_generated_occurrences_and_cached_values() {
        // Guards collapsing loop copies to one written position or repeating the
        // value walk and root revision acquisition for each pointer movement.
        let (service, _) = new_service();
        let backend = service.inner();
        let root = Url::parse("untitled:/hover-loops.mod").unwrap();
        open(backend, &root, SOURCE).await;
        let result = preview(backend, &root).await;
        assert_eq!(result["complete"], true, "{result}");
        ASSIGNED_NUMBER_PASSES.with(|count| count.set(0));
        INPUT_REVISION_CHECKS.with(|count| count.set(0));
        for (name, value) in [("beta_1", "0.1"), ("beta_2", "0.2"), ("beta_1", "0.1")] {
            let query = params(&root, &result, name);
            let start = query.position;
            let hover = backend
                .effective_preview_hover(query)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                markdown(&hover),
                format!("**Parameter**\n\nValue: `{value}`")
            );
            assert_eq!(
                hover.hover.range,
                Some(Range::new(
                    start,
                    Position::new(start.line, start.character + 6)
                ))
            );
        }
        assert_eq!(ASSIGNED_NUMBER_PASSES.with(std::cell::Cell::get), 1);
        assert_eq!(INPUT_REVISION_CHECKS.with(std::cell::Cell::get), 0);

        let local = backend
            .effective_preview_hover(params(&root, &result, "local_2"))
            .await
            .unwrap()
            .unwrap();
        assert!(
            markdown(&local).contains("Model-local variable"),
            "{local:?}"
        );
        assert!(markdown(&local).contains("`2+beta_2`"), "{local:?}");
        assert!(markdown(&local).contains("[Definition](untitled:/hover-loops.mod#L10,3)"));
        // The definition name follows the equation's # token. The hover must
        // select that name, even when there is no corresponding use.
        for (needle, name, expression) in [
            ("local_1 =", "local_1", "`1+beta_1`"),
            ("local_2 =", "local_2", "`2+beta_2`"),
            ("unused_local", "unused_local", "`1`"),
        ] {
            let mut query = params(&root, &result, needle);
            query.definition_link = true;
            let start = query.position;
            let definition = backend
                .effective_preview_hover(query)
                .await
                .unwrap()
                .unwrap_or_else(|| panic!("missing model-local hover for {name}"));
            assert!(markdown(&definition).starts_with("**Model-local variable**"));
            assert!(markdown(&definition).contains(expression), "{definition:?}");
            assert!(!markdown(&definition).contains("[Definition]"));
            let written = definition.definition.as_ref().unwrap();
            assert_eq!(written.location.uri, root);
            assert_eq!(written.document_version, Some(1));
            assert_eq!(
                definition.hover.range,
                Some(Range::new(
                    start,
                    Position::new(start.line, start.character + name.len() as u32)
                ))
            );
        }
        for (needle, expression) in [("first_local,", "`3`"), ("second_local;", "`4`")] {
            let declaration = backend
                .effective_preview_hover(params(&root, &result, needle))
                .await
                .unwrap()
                .unwrap();
            assert!(markdown(&declaration).contains(expression));
        }
        let option = backend
            .effective_preview_hover(params(&root, &result, "order"))
            .await
            .unwrap()
            .unwrap();
        assert!(markdown(&option).contains("stoch_simul"), "{option:?}");
        let value = backend
            .effective_preview_hover(params(&root, &result, "1)"))
            .await
            .unwrap()
            .unwrap();
        assert!(markdown(&value).contains("solve_algo"), "{value:?}");
        let tag_query = params(&root, &result, "y_2 >");
        let start = tag_query.position;
        let tag = backend
            .effective_preview_hover(tag_query)
            .await
            .unwrap()
            .unwrap();
        assert!(markdown(&tag).contains("Endogenous variable"), "{tag:?}");
        assert_eq!(
            tag.hover.range,
            Some(Range::new(
                start,
                Position::new(start.line, start.character + 3)
            ))
        );
        assert!(
            backend
                .effective_preview_hover(params(&root, &result, "🧪"))
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn preview_hover_keeps_open_root_after_unchanged_disk_notification() {
        // Guards a late filesystem event discarding the live overlay's model.
        let file = std::env::temp_dir().join(format!(
            "dygnosis-preview-hover-watched-{}.mod",
            std::process::id()
        ));
        std::fs::write(&file, SOURCE).unwrap();
        let root = Url::from_file_path(&file).unwrap();
        let (service, _) = new_service();
        let backend = service.inner();
        open(backend, &root, SOURCE).await;
        backend.wait_document_analysis(Some(&root)).await;
        let result = preview(backend, &root).await;
        let query = || params(&root, &result, "beta_2");
        let before = backend.effective_preview_hover(query()).await.unwrap();
        watched_change(backend, &root).await;
        let after = backend.effective_preview_hover(query()).await.unwrap();
        std::fs::remove_file(file).unwrap();
        assert_eq!(markdown(&before.unwrap()), "**Parameter**\n\nValue: `0.2`");
        assert_eq!(
            markdown(&after.expect("an unchanged editor overlay keeps its preview hover")),
            "**Parameter**\n\nValue: `0.2`"
        );
    }

    #[tokio::test]
    async fn preview_hover_refreshes_after_an_unchanged_disk_include_is_reloaded() {
        // Guards matching a fresh display to a retired model of the same revision.
        let folder = std::env::temp_dir().join(format!(
            "dygnosis-preview-hover-include-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("calibration.inc");
        std::fs::write(&file, "parameters weight; weight=0.2;\n").unwrap();
        let include = Url::from_file_path(&file).unwrap();
        let root = Url::from_file_path(folder.join("root.mod")).unwrap();
        let (service, _) = new_service();
        let backend = service.inner();
        open(
            backend,
            &root,
            "var y;\n@#include \"calibration.inc\"\nmodel; y=weight; end;\n",
        )
        .await;
        backend.wait_document_analysis(Some(&root)).await;
        let initial = preview(backend, &root).await;
        watched_change(backend, &include).await;
        let refreshed = preview(backend, &root).await;
        let same_value = if refreshed["hover_snapshot_id"].is_string() {
            backend
                .effective_preview_hover(params(&root, &refreshed, "weight"))
                .await
                .unwrap()
        } else {
            None
        };
        std::fs::write(&file, "parameters weight; weight=0.3;\n").unwrap();
        watched_change(backend, &include).await;
        let prior = if same_value.is_some() {
            &refreshed
        } else {
            &initial
        };
        let stale = backend
            .effective_preview_hover(params(&root, prior, "weight"))
            .await
            .unwrap();
        let changed = preview(backend, &root).await;
        let new_value = backend
            .effective_preview_hover(params(&root, &changed, "weight"))
            .await
            .unwrap();
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(folder).unwrap();
        assert_eq!(initial["revision"], refreshed["revision"]);
        assert_eq!(
            markdown(&same_value.expect("Refresh should capture the current model")),
            "**Parameter**\n\nValue: `0.2`"
        );
        assert!(stale.is_none());
        assert_eq!(
            markdown(&new_value.unwrap()),
            "**Parameter**\n\nValue: `0.3`"
        );
    }

    #[tokio::test]
    async fn preview_hover_withholds_changed_and_released_snapshots() {
        // Guards answering an old display with the current model's facts.
        let (service, _) = new_service();
        let backend = service.inner();
        let root = Url::parse("untitled:/hover-revision.mod").unwrap();
        open(backend, &root, SOURCE).await;
        let old = preview(backend, &root).await;
        backend
            .did_change(DidChangeTextDocumentParams {
                text_document: VersionedTextDocumentIdentifier::new(root.clone(), 2),
                content_changes: vec![TextDocumentContentChangeEvent {
                    range: None,
                    range_length: None,
                    text: SOURCE.replace(" / 10", " / 100"),
                }],
            })
            .await;
        assert!(
            backend
                .effective_preview_hover(params(&root, &old, "beta_2"))
                .await
                .unwrap()
                .is_none()
        );
        let new = preview(backend, &root).await;
        let query = params(&root, &new, "beta_2");
        let hover = backend
            .effective_preview_hover(query)
            .await
            .unwrap()
            .unwrap();
        assert!(markdown(&hover).contains("`0.02`"));
        backend
            .effective_preview_closed(params(&root, &new, "beta_2").snapshot)
            .await;
        assert!(
            backend
                .effective_preview_hover(params(&root, &new, "beta_2"))
                .await
                .unwrap()
                .is_none()
        );
        backend
            .effective_preview_closed(params(&root, &old, "beta_2").snapshot)
            .await;
        assert_eq!(backend.lock_inner().preview_hovers.bytes, 0);
    }

    #[tokio::test]
    async fn preview_hover_distinguishes_generated_typed_names_from_strings() {
        // Guards treating every equal string as a typed name or requiring a
        // generated declaration to have a literal written name.
        let (service, _) = new_service();
        let backend = service.inner();
        let root = Url::parse("untitled:/hover-typed.mod").unwrap();
        let source = "@#define suffix = 2\ndatabase db_@{suffix};\nvar y(long_name='db_2');\nmodel; y=0; end;\n";
        open(backend, &root, source).await;
        let result = preview(backend, &root).await;
        let name = backend
            .effective_preview_hover(params(&root, &result, "db_2;"))
            .await
            .unwrap()
            .unwrap();
        assert!(markdown(&name).contains("Database"), "{name:?}");
        assert!(
            backend
                .effective_preview_hover(params(&root, &result, "db_2'"))
                .await
                .unwrap()
                .is_none()
        );
    }
}

#[derive(Default)]
pub(super) struct Snapshots {
    entries: VecDeque<Snapshot>,
    bytes: usize,
    next: u64,
}

impl Snapshots {
    pub(super) fn insert(
        &mut self,
        root: Url,
        view: &Arc<SemanticModel>,
        text: String,
        index: LineIndex,
        spans: Vec<Option<Span>>,
        inventory: SourceInventory,
    ) -> Option<String> {
        let bytes = text.capacity()
            + index.allocation_bytes()
            + spans.capacity() * size_of::<Option<Span>>()
            + inventory.retained_bytes();
        if bytes > MAX_BYTES {
            return None;
        }
        while self.entries.len() >= MAX_SNAPSHOTS || self.bytes > MAX_BYTES - bytes {
            self.bytes -= self.entries.pop_front()?.bytes;
        }
        self.next = self.next.checked_add(1)?;
        let id = self.next.to_string();
        self.entries.push_back(Snapshot {
            root,
            id: id.clone(),
            text,
            index,
            spans,
            inventory,
            view: Arc::downgrade(view),
            bytes,
        });
        self.bytes += bytes;
        Some(id)
    }

    fn close(&mut self, id: &SnapshotId) {
        if let Some(index) = self.entries.iter().position(|entry| entry.matches(id)) {
            self.bytes -= self.entries.remove(index).expect("snapshot index").bytes;
        }
    }
}

impl Backend {
    pub(super) async fn effective_preview_hover(
        &self,
        params: HoverParams,
    ) -> Result<Option<PreviewHover>> {
        ordering::ready().await;
        control::run(ordering::preview_cancellation(), || {
            let inner = self.lock_inner();
            let snapshot = inner
                .preview_hovers
                .entries
                .iter()
                .find(|entry| entry.matches(&params.snapshot))?;
            let view = snapshot.view.upgrade()?;
            let root = &snapshot.root;
            if !inner
                .semantic_views
                .get(root)
                .is_some_and(|current| Arc::ptr_eq(current, &view))
                || !inner
                    .workspace
                    .effective_model_is_cached(root.as_str(), &view.model)
            {
                return None;
            }
            let hover = snapshot.hover(&inner, view, &params)?;
            if !inner.workspace.input_snapshot_is_current(root.as_str()) {
                return None;
            }
            Some(hover)
        })
        .map_err(|_| tower_lsp::jsonrpc::Error::request_cancelled())
    }

    pub(super) async fn effective_preview_closed(&self, id: SnapshotId) {
        ordering::ready().await;
        self.lock_inner().preview_hovers.close(&id);
    }
}
