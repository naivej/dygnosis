//! Written-source presentation over the shared inventory and availability trace.

use super::*;
use crate::engine::analysis::source_inventory::SourceItemKind;
use crate::engine::analysis_configuration::{
    SourceAvailability, SourceFileSelection, SourceFileStructure, SourceStructureQuery,
};
use crate::engine::workspace::include_resolver::normalize_uri;

pub(super) fn file(inner: &mut Inner, document: &Url) -> Option<SourceFileStructure> {
    let roots = inner.context_roots(document);
    if roots.len() != 1 {
        return None;
    }
    let root = &roots[0];
    inner.prepare_root(root);
    let key = normalize_uri(document.as_str());
    inner
        .workspace
        .source_structure(
            root.as_str(),
            &SourceStructureQuery {
                files: vec![if root == document {
                    SourceFileSelection::Root
                } else {
                    SourceFileSelection::File(key.clone())
                }],
            },
        )
        .ok()?
        .files
        .into_iter()
        .find(|file| {
            file.file.as_deref() == Some(key.as_str())
                || (file.file.is_none() && document.scheme() == "untitled" && root == document)
        })
}

pub(super) fn availability_label(state: SourceAvailability) -> &'static str {
    match state {
        SourceAvailability::Active => "active",
        SourceAvailability::Inactive => "inactive",
        SourceAvailability::Mixed => "mixed",
        SourceAvailability::Unknown => "unknown",
    }
}

/// Semantic facts use the root's selected configuration; written source remains unchanged.
pub(super) fn label(inner: &Inner, document: &Url, hover: &mut Hover) {
    let roots = inner.context_roots(document);
    if roots.len() != 1 {
        return;
    }
    let choices = inner
        .workspace
        .root_analysis_configuration(roots[0].as_str());
    let Some(name) = choices.name.as_deref().filter(|name| {
        !matches!(
            *name,
            "Analysis preview"
                | "Analysis preview (modified)"
                | "Macro inputs"
                | "Macro inputs (modified)"
        )
    }) else {
        return;
    };
    let detail = format!(
        "\n\nAnalysis: {name}{}",
        if choices.is_preview() {
            " (preview)"
        } else {
            ""
        }
    );
    match &mut hover.contents {
        HoverContents::Markup(markup) => markup.value.push_str(&detail),
        HoverContents::Scalar(MarkedString::String(value)) => value.push_str(&detail),
        HoverContents::Array(values) => values.push(MarkedString::String(detail)),
        HoverContents::Scalar(value) => {
            hover.contents = HoverContents::Array(vec![value.clone(), MarkedString::String(detail)])
        }
    }
}

/// Resolve literal written include links without acquiring any dormant body.
pub(super) fn literal_links(
    inner: &mut Inner,
    document: &Url,
    text: &str,
    index: &LineIndex,
) -> Vec<DocumentLink> {
    let roots = inner.context_roots(document);
    if roots.len() != 1 {
        return Vec::new();
    }
    let root = &roots[0];
    inner.prepare_root(root);
    let Some(inventory) = inner.workspace.source_inventory(document.as_str()) else {
        return Vec::new();
    };
    let including = normalize_uri(document.as_str());
    let Ok(mut lookup) = inner.workspace.source_include_lookup(root.as_str()) else {
        return Vec::new();
    };
    let mut links = Vec::new();
    for token in inventory
        .macro_tokens
        .iter()
        .filter(|token| token.kind == TokenKind::MacroDir && !token.comment_context)
    {
        let Some(literal) =
            crate::engine::language::parser::literal_include_filename(token.text(text))
        else {
            continue;
        };
        let target = match inner
            .workspace
            .source_include_target(&mut lookup, &including, &literal)
        {
            Ok(Some(target)) => target,
            Ok(None) => continue,
            Err(_) => break,
        };
        let Some(uri) = file_url_from_path_key(&target).or_else(|| Url::parse(&target).ok()) else {
            continue;
        };
        links.push(DocumentLink {
            range: span_range(index, text, token.span),
            target: Some(uri),
            tooltip: Some(format!("Open Source: {literal}")),
            data: Some(json!({"dygnosis_source_include":true,"root_uri":root})),
        });
    }
    links
}

pub(super) fn hover(inner: &mut Inner, document: &Url, byte: u32) -> Option<Hover> {
    let file = file(inner, document)?;
    let item = file
        .inventory
        .items
        .iter()
        .filter(|item| item.span.start <= byte && byte < item.span.end)
        .min_by_key(|item| item.span.end - item.span.start)?;
    let availability = file
        .availability
        .iter()
        .find(|row| row.source_id == item.source_id);
    let control = matches!(
        item.kind,
        SourceItemKind::Conditional | SourceItemKind::Branch
    ) && item.selection.start <= byte
        && byte < item.selection.end;
    let state = availability.map_or(SourceAvailability::Unknown, |row| {
        if control {
            row.control_availability
        } else {
            row.availability
        }
    });
    let root = inner.context_roots(document).into_iter().next()?;
    let choices = inner.workspace.root_analysis_configuration(root.as_str());
    let overridden = choices.overrides.iter().find_map(|choice| {
        file.sites
            .iter()
            .find(|site| {
                &site.target == choice.target()
                    && site.directive_span.start <= byte
                    && byte < site.directive_span.end
            })
            .map(|site| (choice, site))
    });
    if state == SourceAvailability::Active && overridden.is_none() && !item.template && !control {
        return None;
    }
    // A containing statement supplies availability, not the hovered name.
    // Reuse its retained lexical tokens for a precise inner token and range.
    let (name, selection, option) = if item.selection.start <= byte && byte < item.selection.end {
        (
            item.name.as_deref().unwrap_or("Written source"),
            item.selection,
            None,
        )
    } else if item.kind == SourceItemKind::MacroDefinition {
        // A preview receipt describes the whole written directive, including
        // its keyword and replacement expression outside the defined name.
        (
            item.name.as_deref().unwrap_or("Written source"),
            item.span,
            None,
        )
    } else {
        let tokens = &file.inventory.lexical_tokens;
        let at = tokens.partition_point(|token| token.span.end <= byte);
        let token = tokens.get(at).filter(|token| {
            token.kind == TokenKind::Ident
                && token.span.start <= byte
                && byte < token.span.end
                && item.span.start <= token.span.start
                && token.span.end <= item.span.end
        })?;
        let name = token.text(&file.text);
        let option = if item.kind == SourceItemKind::Command
            && tokens
                .get(at + 1)
                .is_some_and(|next| next.kind == TokenKind::Eq)
        {
            item.name.as_deref().and_then(|command| {
                syntax_intel::options(command)
                    .find(|option| option.name.eq_ignore_ascii_case(name))
                    .map(|option| (command, option))
            })
        } else {
            None
        };
        (name, token.span, option)
    };
    let mut message = format!(
        "**{name}** · {}",
        if state == SourceAvailability::Mixed {
            "active in some expansions"
        } else {
            availability_label(state)
        }
    );
    if state == SourceAvailability::Unknown
        && let Some(cause) = availability.and_then(|row| {
            if control {
                row.control_cause.as_ref()
            } else {
                row.cause.as_ref()
            }
        })
    {
        message.push_str("\n\n");
        message.push_str(&cause.message);
    }
    if let Some((command, option)) = option {
        message.push_str(&format!(
            "\n\n**`{command}` option**: `{name}`\n\n{}",
            option.summary
        ));
    }
    if let Some((choice, site)) = overridden {
        use crate::engine::analysis_configuration::AnalysisOverride;
        let value = match choice {
            AnalysisOverride::Definition { expression, .. } => {
                format!("Replacement expression: `{expression}`")
            }
            AnalysisOverride::SkipDefinition { .. } => {
                "Skip this definition; keep any earlier value.".into()
            }
            AnalysisOverride::Branch { branch_id, .. } => {
                if let Some(branch_id) = branch_id {
                    site.branches
                        .iter()
                        .find(|branch| &branch.branch_id == branch_id)
                        .map_or_else(
                            || "The selected branch is unavailable in this source.".into(),
                            |branch| format!("Branch preview: `{}`", branch.expected_text),
                        )
                } else {
                    "Skip this conditional for analysis.".into()
                }
            }
        };
        message.push_str(&format!("\n\n{value}"));
        if let Ok(metadata) = inner.workspace.analysis_metadata(root.as_str())
            && let Some(receipt) = metadata
                .overrides
                .iter()
                .find(|receipt| receipt.operation.target() == choice.target())
            && (receipt.application
                != crate::engine::analysis_configuration::OverrideApplication::Applied
                || receipt.cause.is_some())
        {
            let application = match receipt.application {
                crate::engine::analysis_configuration::OverrideApplication::Applied => "applied",
                crate::engine::analysis_configuration::OverrideApplication::NotReached => {
                    "not reached"
                }
                crate::engine::analysis_configuration::OverrideApplication::Unknown => "unknown",
            };
            message.push_str(&format!("\n\nApplication: {application}."));
            if let Some(cause) = &receipt.cause {
                message.push(' ');
                message.push_str(cause);
            }
        }
    }
    let index = inner
        .workspace
        .source_document(root.as_str(), &normalize_uri(document.as_str()))
        .map(|source| source.source_index())
        .unwrap_or_else(|| Arc::new(LineIndex::new(&file.text)));
    let mut hover = markdown_hover(message, Some(span_range(&index, &file.text, selection)));
    label(inner, document, &mut hover);
    Some(hover)
}

pub(super) fn candidates(inner: &mut Inner, arguments: &[Value]) -> Value {
    let Some(params) = arguments
        .first()
        .and_then(|value| serde_json::from_value::<TextDocumentPositionParams>(value.clone()).ok())
    else {
        return json!({"code":"INVALID_ARGUMENTS","error":"Source candidates require a document URI and cursor position."});
    };
    let document = &params.text_document.uri;
    let Some(doc) = inner.document(document) else {
        return json!({"code":"DOCUMENT_NOT_OPEN","error":"Open the written source before finding source candidates."});
    };
    let text = doc.text.clone();
    let version = doc.version;
    let index = LineIndex::new(&text);
    let byte = index.offset_utf16(&text, span_pos(params.position));
    let Some((name, _)) = syntax_intel::word_at(&text, byte) else {
        return json!({"code":"SOURCE_CANDIDATE_UNAVAILABLE","error":"Place the cursor on a written name to find source candidates."});
    };
    let Some(inventory) = inner.workspace.source_inventory(document.as_str()) else {
        return json!({"code":"SOURCE_CANDIDATE_UNAVAILABLE","error":"Written source information is unavailable for this document."});
    };
    let projected = file(inner, document);
    let rows: Vec<_> = inventory.compatible_candidates(byte).filter(|item| item.name.as_deref() == Some(name.as_str()))
        .map(|item| {
            let state = projected.as_ref().and_then(|file| file.availability.iter().find(|row| row.source_id == item.source_id))
                .map_or(SourceAvailability::Unknown, |row| row.availability);
            json!({"source_id":item.source_id,"name":name,"label":"source candidate · unverified","kind":item.written_kind,"availability":availability_label(state),
                "location":{"uri":document,"range":span_range(&index,&text,item.selection)}})
        }).collect();
    json!({"schema_version":1,"document_uri":document,"document_version":version,"source_revision":inventory.source_revision,
        "candidates":rows,"message":if rows.is_empty() {format!("No declaration of {name} was found in {}.{}", document.path().rsplit('/').next().unwrap_or("this file"), inventory.coverage_cause.as_ref().map(|cause|format!(" {cause}")).unwrap_or_default())} else {"Choose a source declaration; this does not establish references.".into()}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn literal_browsing_batches_lookup_and_reuses_cached_macro_source() {
        // Guards one full document-key lookup and a fresh budget per literal,
        // or a second macro/written parse merely to prove a browsing relation.
        let root = Url::parse("file:///C:/dygnosis-source-batch/main.mod").unwrap();
        let child = Url::parse("file:///C:/dygnosis-source-batch/dormant.inc").unwrap();
        let text = format!(
            "var y;model;y=0;end;\n@#if 0\n{}@#endif\n",
            "@#include \"dormant.inc\"\n".repeat(100)
        );
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.clone(), 1);
        let mut inner = server.lock_inner();
        inner
            .workspace
            .update_document(child.as_str(), "@#error \"dormant\"\n");
        for id in 0..100 {
            inner.workspace.update_document(
                &format!("file:///C:/dygnosis-source-batch/file{id}.inc"),
                "// supplied sibling\n",
            );
        }
        inner.workspace.get_effective_model(root.as_str()).unwrap();
        let inventory = inner.workspace.source_inventory(root.as_str()).unwrap();
        let parses =
            crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|builds| builds.get());
        crate::engine::workspace::SOURCE_LOOKUP_BUILDS.with(|builds| builds.set(0));
        assert_eq!(
            literal_links(&mut inner, &root, &text, &LineIndex::new(&text)).len(),
            100
        );
        assert_eq!(
            crate::engine::workspace::SOURCE_LOOKUP_BUILDS.with(|builds| builds.get()),
            1
        );
        assert!(Arc::ptr_eq(
            &inventory,
            &inner.workspace.source_inventory(root.as_str()).unwrap()
        ));
        crate::engine::workspace::SOURCE_LOOKUP_BUILDS.with(|builds| builds.set(0));
        assert!(inner.source_browsing_relation(&root, &child));
        assert_eq!(
            crate::engine::workspace::SOURCE_LOOKUP_BUILDS.with(|builds| builds.get()),
            1
        );
        assert_eq!(
            crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|builds| builds.get()),
            parses
        );
        assert!(
            inner
                .workspace
                .cached_source_inventory(child.as_str())
                .is_none()
        );
    }

    fn selected_preview(server: &Backend, root: &Url) {
        let mut inner = server.lock_inner();
        let source = inner
            .workspace
            .source_structure(root.as_str(), &SourceStructureQuery::default())
            .unwrap();
        let target = source
            .files
            .iter()
            .flat_map(|file| file.sites.iter())
            .find(|site| {
                site.kind == crate::engine::analysis_configuration::DirectiveKind::Definition
            })
            .unwrap()
            .target
            .clone();
        inner.workspace.set_root_analysis_configuration(root.as_str(),serde_json::from_value(json!({"name":"Alternative","overrides":[{"kind":"definition","target":target,"expression":"1"}]})).unwrap()).unwrap();
        inner.analysis_configuration_committed(root);
        inner.refresh_diagnostics(Some(root));
    }
    fn action_params(root: &Url) -> CodeActionParams {
        CodeActionParams {
            text_document: TextDocumentIdentifier { uri: root.clone() },
            range: Range::new(Position::new(0, 0), Position::new(1000, 0)),
            context: CodeActionContext {
                diagnostics: Vec::new(),
                only: Some(vec![CodeActionKind::QUICKFIX]),
                trigger_kind: None,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        }
    }

    #[tokio::test]
    async fn preview_disables_stored_fixes_and_metadata_actions_without_edits() {
        // Both stored E001 repairs and I208/I209 metadata actions must stay read-only in previews.
        let root = Url::parse("file:///C:/dygnosis-analysis-actions/main.mod").unwrap();
        for body in [
            "var y;\nmodel;\ny=0;\nend;\n",
            "var y;\nmodel;\ny=0\nend;\n",
        ] {
            let (service, _socket) = new_service();
            let server = service.inner();
            server.upsert(root.clone(), format!("@#define FLAG=0\n{body}"), 1);
            selected_preview(server, &root);
            let actions = server.quick_fixes(&action_params(&root)).unwrap();
            assert!(!actions.is_empty());
            for action in actions {
                let CodeActionOrCommand::CodeAction(action) = action else {
                    panic!("no executable command in preview")
                };
                assert!(action.edit.is_none());
                assert!(action.command.is_none());
                assert!(
                    action
                        .disabled
                        .unwrap()
                        .reason
                        .contains("ANALYSIS_PREVIEW_EDIT_UNSUPPORTED")
                );
            }
        }
    }

    #[tokio::test]
    async fn semantic_hover_names_the_selected_configuration_and_preview_rename_has_code() {
        // A semantic hover must not display facts under Source after a configured selection.
        let root = Url::parse("file:///C:/dygnosis-analysis-hover/main.mod").unwrap();
        let text =
            "@#define FLAG=0\nvar y;\nparameters rho;\nrho=0.5;\nmodel;\ny=rho*y(-1);\nend;\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        selected_preview(server, &root);
        let pos = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: root.clone() },
            position: Position::new(5, 3),
        };
        let hover = server.hover_at(&pos).unwrap();
        let value = serde_json::to_value(hover).unwrap().to_string();
        assert!(value.contains("Analysis: Alternative (preview)"), "{value}");
        let response=server.execute("dynare/captureRename",&[json!({"textDocument":{"uri":root},"position":{"line":5,"character":3},"newName":"persistence"})]);
        assert_eq!(response["code"], "ANALYSIS_PREVIEW_EDIT_UNSUPPORTED");
        assert!(response.get("edit").is_none());
    }

    #[tokio::test]
    async fn semantic_hover_omits_automatic_analysis_names() {
        // Guards generic preview labels hiding the useful symbol facts in every hover.
        let root = Url::parse("file:///C:/dygnosis-analysis-hover/main.mod").unwrap();
        let text =
            "@#define FLAG=0\nvar y;\nparameters rho;\nrho=0.5;\nmodel;\ny=rho*y(-1);\nend;\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        selected_preview(server, &root);
        for name in [
            None,
            Some("Analysis preview"),
            Some("Analysis preview (modified)"),
            Some("Macro inputs"),
            Some("Macro inputs (modified)"),
        ] {
            {
                let mut inner = server.lock_inner();
                let mut choices =
                    (*inner.workspace.root_analysis_configuration(root.as_str())).clone();
                choices.name = name.map(str::to_owned);
                inner
                    .workspace
                    .set_root_analysis_configuration(root.as_str(), choices)
                    .unwrap();
                inner.analysis_configuration_committed(&root);
            }
            let hover = server
                .hover_at(&TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: root.clone() },
                    position: Position::new(5, 3),
                })
                .unwrap();
            let value = serde_json::to_value(hover).unwrap().to_string();
            assert!(value.contains("rho"), "{value}");
            assert!(!value.contains("Analysis:"), "{value}");
        }
    }

    #[tokio::test]
    async fn conditional_hover_reports_each_arm_under_an_else_preview() {
        // Guards the opener reporting directive execution and the selected else having no activity.
        let root = Url::parse("file:///C:/dygnosis-branch-hover/main.mod").unwrap();
        let text = "@#define FLAG=1\n@#if FLAG\nvar a;\n@#else\nvar b;\n@#endif\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        {
            let mut inner = server.lock_inner();
            let source = inner
                .workspace
                .source_structure(root.as_str(), &SourceStructureQuery::default())
                .unwrap();
            let site = source.files[0]
                .sites
                .iter()
                .find(|site| {
                    site.kind == crate::engine::analysis_configuration::DirectiveKind::Conditional
                })
                .unwrap();
            let choices = serde_json::from_value(json!({"overrides":[{
                "kind":"branch","target":site.target,"branch_id":site.branches[1].branch_id
            }]}))
            .unwrap();
            inner
                .workspace
                .set_root_analysis_configuration(root.as_str(), choices)
                .unwrap();
            inner.analysis_configuration_committed(&root);
            let projected = file(&mut inner, &root).unwrap();
            let states: Vec<_> = projected
                .inventory
                .items
                .iter()
                .filter(|item| {
                    matches!(
                        item.kind,
                        SourceItemKind::Conditional | SourceItemKind::Branch
                    )
                })
                .map(|item| {
                    projected
                        .availability
                        .iter()
                        .find(|row| row.source_id == item.source_id)
                        .unwrap()
                        .control_availability
                })
                .collect();
            assert_eq!(
                states,
                [SourceAvailability::Inactive, SourceAvailability::Active]
            );
        }
        let mut actual = Vec::new();
        for (line, character) in [(1, 3), (1, 6), (3, 3)] {
            let hover = server.hover_at(&TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: root.clone() },
                position: Position::new(line, character),
            });
            actual.push(hover.map(|hover| match hover.contents {
                HoverContents::Markup(markup) => {
                    markup.value.lines().next().unwrap_or_default().to_owned()
                }
                _ => panic!("markdown hover"),
            }));
        }
        assert!(
            actual[0]
                .as_ref()
                .is_some_and(|text| text.ends_with(" · inactive"))
                && actual[1]
                    .as_ref()
                    .is_some_and(|text| text.ends_with(" · inactive"))
                && actual[2]
                    .as_ref()
                    .is_some_and(|text| text.ends_with(" · active")),
            "if / else hovers: {actual:?}"
        );
    }

    #[tokio::test]
    async fn conditional_hover_tracks_source_and_each_preview_without_reanalysis() {
        // Guards stale arm labels after forcing, skipping or clearing a branch, and repeated parsing for hover.
        use crate::engine::analysis_configuration::{AnalysisConfiguration, AnalysisOverride};
        use crate::engine::language::parser::PARSE_EXPANDED_BUILDS;
        for (opener, newline) in ["@#if FLAG", "@#ifdef FLAG", "@#ifndef FLAG"]
            .into_iter()
            .flat_map(|opener| ["\n", "\r\n"].map(move |newline| (opener, newline)))
        {
            let root = Url::parse("file:///C:/dygnosis-branch-hover/main.mod").unwrap();
            let text = format!(
                "@#define FLAG=1\n{opener}\nvar a;\n@#elseif 0\nvar b;\n@#else\nvar c;\n@#endif\n"
            )
            .replace('\n', newline);
            let (service, _socket) = new_service();
            let server = service.inner();
            server.upsert(root.clone(), text, 1);
            let (target, branches) = {
                let mut inner = server.lock_inner();
                let source = inner
                    .workspace
                    .source_structure(root.as_str(), &SourceStructureQuery::default())
                    .unwrap();
                let site = source.files[0]
                    .sites
                    .iter()
                    .find(|site| {
                        site.kind
                            == crate::engine::analysis_configuration::DirectiveKind::Conditional
                    })
                    .unwrap();
                (
                    site.target.clone(),
                    site.branches
                        .iter()
                        .map(|branch| branch.branch_id.clone())
                        .collect::<Vec<_>>(),
                )
            };
            let positions = [
                (1, 3, 0),
                (1, opener.find("FLAG").unwrap() as u32, 0),
                (3, 3, 1),
                (3, 9, 1),
                (5, 3, 2),
            ];
            let hover_at = |line, character| {
                let hover = server
                    .hover_at(&TextDocumentPositionParams {
                        text_document: TextDocumentIdentifier { uri: root.clone() },
                        position: Position::new(line, character),
                    })
                    .unwrap();
                let HoverContents::Markup(markup) = hover.contents else {
                    panic!("markdown hover")
                };
                markup.value.lines().next().unwrap_or_default().to_owned()
            };
            for selected in [
                None,
                Some(Some(0)),
                Some(Some(1)),
                Some(Some(2)),
                Some(None),
                None,
            ] {
                let mut choices = AnalysisConfiguration::default();
                if let Some(index) = selected {
                    choices.overrides.push(AnalysisOverride::Branch {
                        target: target.clone(),
                        branch_id: index.map(|index| branches[index].clone()),
                    });
                }
                {
                    let mut inner = server.lock_inner();
                    inner
                        .workspace
                        .set_root_analysis_configuration(root.as_str(), choices)
                        .unwrap();
                    inner.analysis_configuration_committed(&root);
                }
                hover_at(1, 3);
                let before = PARSE_EXPANDED_BUILDS.with(|builds| builds.get());
                let active =
                    selected.unwrap_or(Some(if opener == "@#ifndef FLAG" { 2 } else { 0 }));
                for (line, character, arm) in positions {
                    let actual = hover_at(line, character);
                    let expected = if active == Some(arm) {
                        "active"
                    } else {
                        "inactive"
                    };
                    assert!(
                        actual.ends_with(&format!(" · {expected}")),
                        "{opener} / {selected:?} / line {line}: {actual}"
                    );
                }
                assert_eq!(
                    PARSE_EXPANDED_BUILDS.with(|builds| builds.get()),
                    before,
                    "current branch hovers must reuse the selected analysis"
                );
            }
        }
    }

    #[tokio::test]
    async fn activity_hover_omits_source_copies_and_keeps_unknown_causes() {
        // Guards copying a visible statement into its hover or hiding why activity is unknown.
        for (condition, state) in [("0", "inactive"), ("MISSING", "unknown")] {
            let root = Url::parse("file:///C:/dygnosis-activity-hover/main.mod").unwrap();
            let text = format!("@#if {condition}\nvar hidden;\n@#endif\nvar y;model;y=0;end;\n");
            let (service, _socket) = new_service();
            let server = service.inner();
            server.upsert(root.clone(), text.clone(), 1);
            let mut inner = server.lock_inner();
            let hover = hover(&mut inner, &root, text.find("hidden").unwrap() as u32).unwrap();
            let HoverContents::Markup(markup) = hover.contents else {
                panic!("markdown hover")
            };
            assert!(
                markup.value.starts_with(&format!("**hidden** · {state}")),
                "{}",
                markup.value
            );
            assert!(!markup.value.contains("Written:"));
            assert!(!markup.value.contains("var hidden;"));
            assert!(
                !markup
                    .value
                    .contains("Model values, timing and equation numbers")
            );
            if state == "inactive" {
                assert_eq!(markup.value, "**hidden** · inactive");
            } else {
                assert!(
                    markup.value.contains("\n\n"),
                    "unknown activity retains its cause"
                );
            }
        }
    }

    #[tokio::test]
    async fn dormant_custom_extension_include_link_resolves_without_loading_its_body() {
        // Literal Open Source must use selected-root lookup and typed metadata, without dormant execution.
        struct Temporary(std::path::PathBuf);
        impl Drop for Temporary {
            fn drop(&mut self) {
                if let (Ok(parent), Ok(target)) =
                    (std::env::temp_dir().canonicalize(), self.0.canonicalize())
                    && target.parent() == Some(parent.as_path())
                    && target
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.starts_with("dygnosis-source-link-"))
                {
                    let _ = std::fs::remove_dir_all(target);
                }
            }
        }
        let directory = Temporary(std::env::temp_dir().join(format!(
                "dygnosis-source-link-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            )));
        std::fs::create_dir_all(&directory.0).unwrap();
        let child_path = directory.0.join("dormant.txt");
        std::fs::write(&child_path, "@#error \"This body must not execute\"\n").unwrap();
        let root = Url::from_file_path(directory.0.join("main.mod")).unwrap();
        // `temp_dir()` may use a symlinked path (for example /var on macOS),
        // while the document link uses the canonical filesystem path.
        let child = Url::from_file_path(child_path.canonicalize().unwrap()).unwrap();
        let text =
            "var y;model;y=0;end;\n@#if 0\n@#include \"dormant.txt\"\n@#include UNKNOWN\n@#endif\n";
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(root.clone(), text.into(), 1);
        assert!(
            server
                .lock_inner()
                .workspace
                .get_source(child.as_str())
                .is_none()
        );
        let links = server.document_links(&root).unwrap();
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target.as_ref(), Some(&child));
        assert_eq!(
            links[0].data.as_ref().unwrap()["dygnosis_source_include"],
            true
        );
        assert_eq!(links[0].data.as_ref().unwrap()["root_uri"], root.as_str());
        let mut inner = server.lock_inner();
        assert!(inner.workspace.get_source(child.as_str()).is_none());
        assert!(inner.workspace.owner_roots(child.as_str()).is_empty());
        assert!(inner.workspace.includes_complete(root.as_str()));
    }
}
