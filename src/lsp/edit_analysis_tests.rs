//! First-request analysis reuse with current edited source locations.

use super::*;
use crate::engine::language::parser::PARSE_EXPANDED_BUILDS;

async fn open_source(server: &Backend, uri: &Url, text: String, version: i32) {
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version,
                text,
            },
        })
        .await;
}

async fn change_source(server: &Backend, uri: &Url, text: String, version: i32) {
    server
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text,
            }],
        })
        .await;
}

#[tokio::test]
async fn equation_label_collision_has_no_variable_hover() {
    // Guards a quoted equation label borrowing a declaration with equal text.
    let uri = Url::parse("untitled:label-collision.mod").unwrap();
    let source = "var y; model; [name='y'] y=0; end;";
    let (service, _socket) = new_service();
    let server = service.inner();
    server.upsert(uri.clone(), source.into(), 1);
    let hover = server.hover_at(&TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri },
        position: Position::new(0, source.find("'y'").unwrap() as u32 + 1),
    });
    assert!(
        hover.is_none(),
        "equation label acquired variable hover: {hover:?}"
    );
}

#[tokio::test]
async fn hover_providers_share_one_current_root_input_proof() {
    // Guards source, local and typed hover providers independently rereading
    // the same exact disk and missing-candidate inputs in one locked request.
    use crate::engine::workspace::{INPUT_FILE_READS, INPUT_REVISION_CHECKS};
    let uri = Url::parse("untitled:shared-hover-proof.mod").unwrap();
    let source = "var y; parameters p; p=.9; model; y=p*y(-1); end;";
    let (service, _socket) = new_service();
    let server = service.inner();
    server.upsert(uri.clone(), source.into(), 1);
    let pos = TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri },
        position: Position::new(0, source.find("y=p").unwrap() as u32),
    };
    server.hover_at(&pos).expect("warm the binding view");
    INPUT_REVISION_CHECKS.with(|checks| checks.set(0));
    INPUT_FILE_READS.with(|reads| reads.set(0));
    let before = Backend::model_hover_from_inputs(&mut server.lock_inner(), &pos)
        .expect("unshared provider chain");
    let before_checks = INPUT_REVISION_CHECKS.with(std::cell::Cell::get);
    let before_reads = INPUT_FILE_READS.with(std::cell::Cell::get);
    INPUT_REVISION_CHECKS.with(|checks| checks.set(0));
    INPUT_FILE_READS.with(|reads| reads.set(0));
    let hover = server.hover_at(&pos).expect("current endogenous hover");
    assert!(
        serde_json::to_string(&hover)
            .unwrap()
            .contains("Endogenous")
    );
    assert_eq!(
        INPUT_REVISION_CHECKS.with(std::cell::Cell::get),
        1,
        "hover providers repeated input validation"
    );
    let after_reads = INPUT_FILE_READS.with(std::cell::Cell::get);
    assert_eq!(hover, before);
    assert!(before_checks > 1);
    assert!(
        after_reads < before_reads,
        "shared proof repeated every source observation"
    );
    println!(
        "hover_input_proofs: captures={before_checks}->1 candidate_observations={before_reads}->{after_reads}; final exact-byte check retained"
    );
}

#[tokio::test]
async fn equation_label_does_not_wait_for_pending_model_analysis() {
    // Guards a position with no model hover waiting for a cold model check.
    let uri = Url::parse("untitled:pending-label.mod").unwrap();
    let source = "var y(long_name='y'); model; [name='y',custom='y'] y=0; end; // y\n/* y */\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open_source(server, &uri, source.into(), 1).await;
    let output = server.output_gate.lock().await;
    for byte in [
        source.find("'y'").unwrap() + 1,
        source.find("name='y',").unwrap() + 6,
        source.find("custom='y'").unwrap() + 8,
        source.find("// y").unwrap() + 3,
        source.find("/* y").unwrap() + 3,
        source.find("=0").unwrap() + 1,
    ] {
        let index = LineIndex::new(source);
        let hover = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            server.hover(HoverParams {
                text_document_position_params: TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: span_range(&index, source, Span::new(byte, byte)).start,
                },
                work_done_progress_params: Default::default(),
            }),
        )
        .await
        .expect("equation label waited for unrelated model facts")
        .unwrap();
        assert!(hover.is_none());
        assert!(server.lock_inner().semantic_views.is_empty());
    }
    drop(output);
    server.wait_document_analysis(Some(&uri)).await;
}

#[tokio::test]
async fn non_name_hover_reuses_written_tokens_without_semantic_work() {
    // Guards warm negative requests rebuilding executed models or bindings.
    let uri = Url::parse("untitled:negative-hover.mod").unwrap();
    let source = "var y(long_name='y'); model; [name='y'] y=0; end; // y\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    server.store_document(uri.clone(), source.into(), 1);
    let inventory = server
        .lock_inner()
        .workspace
        .source_inventory(uri.as_str())
        .unwrap();
    PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
    for byte in [
        source.find("'y'").unwrap() + 1,
        source.find("name='y']").unwrap() + 6,
        source.find("// y").unwrap() + 3,
    ] {
        assert!(
            server
                .hover_at(&TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(0, byte as u32),
                })
                .is_none()
        );
    }
    assert_eq!(PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get), 0);
    let inner = server.lock_inner();
    assert!(inner.semantic_views.is_empty());
    assert!(
        !inner.workspace.has_joined_source(uri.as_str()),
        "negative hover expanded the model"
    );
    assert!(Arc::ptr_eq(
        &inventory,
        &inner.workspace.source_inventory(uri.as_str()).unwrap()
    ));
}

#[tokio::test]
async fn hover_eligibility_keeps_quoted_names_semantic_tags_and_macros() {
    // Guards a literal-string shortcut hiding parser-owned names or macro help.
    let uri = Url::parse("untitled:eligible-hover.mod").unwrap();
    let source = "@#define FLAG=1\nexternal_function(name='ext',nargs=1);\nvar y; model; [endogenous='y',mcp='y>0'] y=ext(y(-1)); end;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    server.upsert(uri.clone(), source.into(), 1);
    for (needle, offset, expected) in [
        ("'ext'", 1, Some("External function")),
        ("endogenous='y'", 12, None),
        ("mcp='y>0'", 5, None),
        ("y=ext", 0, Some("Endogenous")),
        ("@#define", 3, Some("define")),
    ] {
        let byte = source.find(needle).unwrap() + offset;
        let index = LineIndex::new(source);
        let pos = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: span_range(&index, source, Span::new(byte, byte)).start,
        };
        assert!(server.hover_may_need_model(&pos), "lost eligible {needle}");
        if let Some(expected) = expected {
            let hover = server
                .hover_at(&pos)
                .unwrap_or_else(|| panic!("lost {needle} hover"));
            assert!(serde_json::to_string(&hover).unwrap().contains(expected));
        }
    }
}

#[tokio::test]
async fn hover_eligibility_preserves_dormant_source_and_uncertain_strings() {
    // Guards dropping source receipts or guessing a literal role in recovery
    // and interpolated strings. These positions keep their existing provider.
    let uri = Url::parse("untitled:uncertain-hover.mod").unwrap();
    for (source, needle, offset, expected) in [
        (
            "var y;\n@#if 0\nmodel; [name='y'] y=0; end;\n@#endif\nmodel; y=1; end;",
            "[name",
            0,
            Some("inactive"),
        ),
        (
            "var y;\n@#if 0\nmodel; [name='y'] y=0; end;\n@#endif\nmodel; y=1; end;",
            "y=0",
            0,
            Some("inactive"),
        ),
        (
            "@#define K=1\nvar y; model; [name='y_@{K}'] y=0; end;",
            "'y_",
            1,
            None,
        ),
        ("var y; model; [name='y", "'y", 1, None),
        ("external_function(name='ext", "'ext", 1, None),
        ("var y; disp('y'); model; y=0; end;", "'y'", 1, None),
    ] {
        let (service, _socket) = new_service();
        let server = service.inner();
        server.upsert(uri.clone(), source.into(), 1);
        let byte = source.find(needle).unwrap() + offset;
        let index = LineIndex::new(source);
        let pos = TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: span_range(&index, source, Span::new(byte, byte)).start,
        };
        assert!(
            server.hover_may_need_model(&pos),
            "lost source/uncertain site {source}"
        );
        if let Some(expected) = expected {
            let hover = server.hover_at(&pos).expect("dormant source receipt");
            assert!(serde_json::to_string(&hover).unwrap().contains(expected));
        }
    }
}

#[tokio::test]
async fn literal_command_hover_does_not_build_effective_semantics() {
    // Guards a catalog hover starting include expansion and symbol analysis
    // while the current edited model has no effective analysis yet.
    let uri = Url::parse("untitled:cheap-command.mod").unwrap();
    let source = "var y;\nmodel; y=0; end;\nsteady;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    {
        let mut inner = server.lock_inner();
        inner.workspace.update_document(uri.as_str(), source);
        inner.docs.insert(
            uri.clone(),
            OpenDoc {
                text: source.into(),
                version: 1,
                diagnostics: Vec::new(),
                library: Vec::new(),
            },
        );
    }
    let hover = server
        .hover_at(&TextDocumentPositionParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            position: Position::new(2, 2),
        })
        .unwrap();
    assert!(serde_json::to_string(&hover).unwrap().contains("Newton"));
    let inner = server.lock_inner();
    assert!(
        inner.semantic_views.is_empty(),
        "command help constructed semantic bindings"
    );
}

#[tokio::test]
async fn written_options_and_macro_directives_return_help_before_model_checks() {
    // Guards cheap help falling through to model checks and written native or
    // comment text acquiring Dynare syntax help through the early path.
    let uri = Url::parse("untitled:cheap-options.mod").unwrap();
    let source = "@#define FLAG=1\nvar y; model; y=0; end;\nstoch_simul(order=1);\nsteady(solve_algo=1);\nverbatim;\nsteady;\nend;\n// steady;\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open_source(server, &uri, source.into(), 1).await;
    for (line, character, expected) in [(0, 3, "define"), (2, 14, "order"), (3, 9, "solve_algo")] {
        let hover = server
            .syntax_hover_at(&TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
                position: Position::new(line, character),
            })
            .unwrap_or_else(|| panic!("written {expected} help needs no effective model"));
        assert!(serde_json::to_string(&hover).unwrap().contains(expected));
    }
    for line in [5, 7] {
        assert!(
            server
                .syntax_hover_at(&TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier { uri: uri.clone() },
                    position: Position::new(line, if line == 5 { 1 } else { 4 }),
                })
                .is_none()
        );
    }
    assert!(server.lock_inner().semantic_views.is_empty());
    server.wait_document_analysis(Some(&uri)).await;
}

#[tokio::test]
async fn input_commits_before_checks_and_first_semantic_request_uses_the_worker_result() {
    // Guards an input handler parsing under the request lock and a semantic
    // request starting a second analysis instead of joining the current job.
    let uri = Url::parse("untitled:deferred-input.mod").unwrap();
    let (service, _socket) = new_service();
    let server = service.inner();
    PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: "var y; parameters p; p=1; model; #local=p; y=local; end;\nsteady;".into(),
            },
        })
        .await;
    assert_eq!(
        PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get),
        0,
        "input commits must not parse or diagnose the model"
    );
    assert!(server.lock_inner().document_analysis.has_pending(&uri));
    let result = server
        .execute_command(ExecuteCommandParams {
            command: "dynare/modelInfo".into(),
            arguments: vec![json!({"root_uri":uri})],
            work_done_progress_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(result["n_equations"], 1);
    assert_eq!(
        PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get),
        0,
        "semantic request reparsed the completed worker snapshot"
    );
}

#[tokio::test]
async fn obsolete_document_check_cannot_replace_edited_model_facts() {
    // Guards an old background snapshot publishing model facts or diagnostics
    // after a newer input has committed while its output was blocked.
    let uri = Url::parse("untitled:deferred-stale.mod").unwrap();
    let (service, _socket) = new_service();
    let server = service.inner();
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: "var before; model; before=0; end;".into(),
            },
        })
        .await;
    let output = server.output_gate.lock().await;
    while server.lock_inner().document_analysis.cancellation.is_none() {
        tokio::task::yield_now().await;
    }
    server.store_document(uri.clone(), "var after; model; after=1; end;".into(), 2);
    server.lock_inner().queue_document_analysis(&uri);
    drop(output);
    server.wait_document_analysis(Some(&uri)).await;
    let result = server.model_info_command(&[json!({"root_uri":uri})]);
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(result["document_version"], 2);
    assert_eq!(result["endogenous"], json!(["after"]));
}

#[tokio::test]
async fn unrelated_input_preserves_inflight_work_and_valid_semantic_requests() {
    // Guards an independent model request waiting for another root's check,
    // and an edit cancelling a compatible job for that other root.
    let a = Url::parse("untitled:a-pending.mod").unwrap();
    let b = Url::parse("untitled:b-valid.mod").unwrap();
    let (service, _socket) = new_service();
    let server = service.inner();
    server.upsert(a.clone(), "var a; model; a=0; end;".into(), 1);
    server.upsert(b.clone(), "var b; model; b=0; end;".into(), 1);
    server.store_document(a.clone(), "var a; model; a=1; end;".into(), 2);
    server.lock_inner().queue_document_analysis(&a);
    let output = server.output_gate.lock().await;
    server.kick_document_analysis();
    let cancellation = loop {
        if let Some(flag) = server.lock_inner().document_analysis.cancellation.clone() {
            break flag;
        }
        tokio::task::yield_now().await;
    };
    let hover = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        server.hover(HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri: b.clone() },
                position: Position::new(0, 4),
            },
            work_done_progress_params: Default::default(),
        }),
    )
    .await
    .expect("valid B hover waited for A's blocked check")
    .unwrap();
    assert!(hover.is_some());
    server.store_document(b.clone(), "var b; model; b=2; end;".into(), 2);
    server.lock_inner().queue_document_analysis(&b);
    assert!(
        !cancellation.load(std::sync::atomic::Ordering::Relaxed),
        "B cancelled A's compatible job"
    );
    drop(output);
    server.wait_document_analysis(None).await;
    assert_eq!(
        server.model_info_command(&[json!({"root_uri":a})])["document_version"],
        2
    );
    assert_eq!(
        server.model_info_command(&[json!({"root_uri":b})])["document_version"],
        2
    );
}

#[tokio::test]
async fn unrelated_merge_does_not_publish_a_pending_documents_empty_report() {
    // Guards another workspace check publishing a temporary empty report at
    // the edited version before that document's own check has completed.
    let a = Url::parse("untitled:pending-publication.mod").unwrap();
    let b = Url::parse("untitled:unrelated-publication.mod").unwrap();
    let (service, _socket) = new_service();
    let server = service.inner();
    let source = "var y; model; y=missing; end;";
    server.upsert(a.clone(), source.into(), 1);
    server.upsert(b.clone(), "var x; model; x=0; end;".into(), 1);
    let checked = server.lock_inner().published[&a].clone();
    assert!(!checked.is_empty());
    server.store_document(a.clone(), format!("{source}\n// edit"), 2);
    server.lock_inner().queue_document_analysis(&a);
    {
        let mut inner = server.lock_inner();
        let publications = inner.merge_diagnostics(Some(&b));
        assert!(publications.iter().all(|(uri, _, _)| uri != &a));
        assert_eq!(inner.published[&a], checked);
        assert_eq!(inner.publication_versions[&a], Some(1));
    }
    server.wait_document_analysis(Some(&a)).await;
    let inner = server.lock_inner();
    assert_eq!(inner.publication_versions[&a], Some(2));
    assert!(!inner.published[&a].is_empty());
    assert_eq!(inner.docs[&a].diagnostics, inner.published[&a]);
}

#[tokio::test]
async fn newly_discovered_include_edit_rejects_the_completed_old_snapshot() {
    // Guards dependency edits omitted by the prior root's dependency set:
    // the completed snapshot must validate the dependencies it actually read.
    let folder =
        std::env::temp_dir().join(format!("dygnosis-new-dependency-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let root = Url::from_file_path(folder.join("root.mod")).unwrap();
    let helper = Url::from_file_path(folder.join("body.inc")).unwrap();
    let (service, _socket) = new_service();
    let server = service.inner();
    server.upsert(helper.clone(), "y=1;".into(), 1);
    server.upsert(root.clone(), "var y; model; y=0; end;".into(), 1);
    server.store_document(
        root.clone(),
        "var y; model;\n@#include \"body.inc\"\nend;".into(),
        2,
    );
    server.lock_inner().queue_document_analysis(&root);
    let output = server.output_gate.lock().await;
    server.kick_document_analysis();
    while server
        .lock_inner()
        .document_analysis
        .completed_snapshot
        .as_ref()
        != Some(&root)
    {
        tokio::task::yield_now().await;
    }
    server.store_document(helper.clone(), "y=2;".into(), 2);
    server.lock_inner().queue_document_analysis(&helper);
    drop(output);
    server.wait_document_analysis(Some(&root)).await;
    let result = server.model_info_command(&[json!({"root_uri":root})]);
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(result["equations"][0]["text"], "y = 2");
    std::fs::remove_dir(&folder).unwrap();
}

#[tokio::test]
async fn saving_unchanged_inputs_reuses_the_guarded_diagnostic_report() {
    // Guards checking the same model again when Save follows completed checks
    // and all source, include, lookup and configuration guards still match.
    let root = Url::parse("untitled:unchanged-save.mod").unwrap();
    let (service, _socket) = new_service();
    let server = service.inner();
    server.upsert(root.clone(), "var y; model; y=0; end;".into(), 1);
    let before = Arc::clone(&server.lock_inner().reports[&root]);
    server
        .did_save(DidSaveTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: root.clone() },
            text: None,
        })
        .await;
    server.wait_document_analysis(Some(&root)).await;
    assert!(
        Arc::ptr_eq(&before, &server.lock_inner().reports[&root]),
        "Save discarded a report whose exact inputs were still valid"
    );
}

#[test]
fn source_limit_explains_partial_results_once_and_clears_after_recovery() {
    // Guards silent empty source results, repeated notices from adjacent
    // providers, and reporting code that starts an unnecessary source parse.
    let uri = Url::parse("file:///C:/dygnosis-source-intelligence/limited.mod").unwrap();
    let mut inner = Inner::default();
    inner
        .workspace
        .set_root_macro_work_limit(uri.as_str(), crate::MacroWorkLimit::new(1).unwrap());
    inner
        .workspace
        .update_document(uri.as_str(), "var y;\nmodel;\ny=0;\nend;\n");
    assert!(inner.source_inspection_notice(&uri).is_none());
    assert!(
        inner
            .workspace
            .cached_source_inventory(uri.as_str())
            .is_none()
    );
    inner.workspace.source_inventory(uri.as_str()).unwrap();
    let message = inner.source_inspection_notice(&uri).unwrap();
    assert!(message.contains("limited.mod"));
    assert!(message.contains("work allowance"));
    assert!(message.contains("Available Outline entries and folds are retained"));
    assert!(inner.source_inspection_notice(&uri).is_none());
    inner
        .workspace
        .set_root_macro_work_limit(uri.as_str(), Default::default());
    inner.workspace.source_inventory(uri.as_str()).unwrap();
    assert!(inner.source_inspection_notice(&uri).is_none());
    assert!(!inner.source_inspection_notices.contains_key(&uri));
    inner
        .workspace
        .set_root_macro_work_limit(uri.as_str(), crate::MacroWorkLimit::new(1).unwrap());
    inner.workspace.source_inventory(uri.as_str()).unwrap();
    assert!(inner.source_inspection_notice(&uri).is_some());
}

#[tokio::test]
async fn written_intelligence_shares_inventory_without_accepting_dormant_model_facts() {
    // Guards a parse per source feature and dormant declarations/equations
    // acquiring semantic labels or changing the effective equation count.
    let uri = Url::parse("file:///C:/dygnosis-source-intelligence/model.mod").unwrap();
    let source = "@#define FLAG=0\r\nvar live;\r\n@#if FLAG\r\nvar dormant;\r\nmodel;\r\n[name='sleeping'] dormant=1;\r\nend;\r\n@#else\r\nmodel; live=1; end;\r\n@#endif\r\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    server.upsert(uri.clone(), source.into(), 1);
    let before = server.model_info_command(&[json!({"root_uri":uri})]);
    assert_eq!(before["n_equations"], 1);
    let inventory = server
        .lock_inner()
        .workspace
        .source_inventory(uri.as_str())
        .unwrap();
    PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
    assert!(server.folding_ranges(&uri).is_some());
    let rows = server.doc_symbols(&uri).unwrap();
    fn find<'a>(rows: &'a [DocumentSymbol], name: &str) -> Option<&'a DocumentSymbol> {
        rows.iter().find_map(|row| {
            if row.name == name {
                Some(row)
            } else {
                row.children
                    .as_deref()
                    .and_then(|children| find(children, name))
            }
        })
    }
    let candidate = find(&rows, "dormant").unwrap();
    assert!(
        !candidate
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("static"))
    );
    let equation = find(&rows, "sleeping").unwrap();
    assert!(equation.detail.is_none());
    assert_eq!(
        find(&rows, "@#if FLAG").unwrap().detail.as_deref(),
        Some("inactive")
    );
    assert_eq!(find(&rows, "FLAG").unwrap().detail, None);
    let byte = source.find("dormant=1").unwrap() as u32 + 8;
    let context = server
        .lock_inner()
        .syntax_context(&uri, source, byte, false)
        .unwrap();
    assert_eq!(context.language, "model_expression");
    assert_eq!(PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get), 0);
    assert!(Arc::ptr_eq(
        &inventory,
        &server
            .lock_inner()
            .workspace
            .source_inventory(uri.as_str())
            .unwrap()
    ));
    let after = server.model_info_command(&[json!({"root_uri":uri})]);
    assert_eq!(after["n_equations"], 1);
}

#[tokio::test]
async fn model_info_after_edits_reuses_the_checked_parse_and_keeps_current_origins() {
    // Guards a second core parse while preparing the first model-information
    // response after an edit, and stale semantic facts, include locations or
    // document versions.
    let root = Url::parse("file:///C:/dygnosis-edit-analysis/root.mod").unwrap();
    let child = Url::parse("file:///C:/dygnosis-edit-analysis/rows.inc").unwrap();
    let root_text = "var y z1 z2;\r\nparameters p;\r\np=1;\r\nmodel;\r\n#local=p;\r\n@#include \"rows.inc\"\r\nend;\r\n";
    let child_text = "/* 😀 */ [name='first'] y=local;\r\n@#for k in 1:2\r\n[name='copy'] z@{k}=y(-1)+@{k};\r\n@#endfor\r\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    open_source(server, &child, child_text.into(), 1).await;
    open_source(server, &root, root_text.into(), 1).await;
    let arguments = [json!({"root_uri":root,"document_uri":child})];
    let mut previous_revision = None;

    for phase in 0..4 {
        if phase == 1 {
            change_source(server, &root, format!("// root shift\r\n{root_text}"), 2).await;
        } else if phase == 2 {
            change_source(
                server,
                &child,
                format!("// include shift\r\n{child_text}"),
                2,
            )
            .await;
        } else if phase == 3 {
            let changed_root = root_text.replace("#local=p;", "#local=p+1;");
            change_source(server, &root, format!("// root shift\r\n{changed_root}"), 3).await;
        }
        server.wait_document_analysis(Some(&root)).await;
        PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
        let result = server.model_info_command(&arguments);
        let parse_builds = PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get);

        assert_eq!(result["complete"], true, "{result}");
        assert_eq!(result["n_equations"], 3);
        assert_eq!(result["document_version"], if phase >= 2 { 2 } else { 1 });
        assert_eq!(
            result["model_locals"]["definitions"][0]["expression"],
            if phase == 3 { "p+1" } else { "p" }
        );
        assert_eq!(
            result["model_locals"]["definitions"][0]["origin"]["uri"],
            root.as_str()
        );
        let equations = result["equations"].as_array().unwrap();
        let facts: Vec<_> = equations
            .iter()
            .map(|row| {
                (
                    row["name"].as_str().unwrap(),
                    row["number"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(facts, [("first", 1), ("copy", 2), ("copy", 3)]);
        assert_eq!(equations[0]["location"]["uri"], child.as_str());
        assert_eq!(
            equations[0]["location"]["range"]["start"],
            json!({"line":usize::from(phase >= 2),"character":9})
        );
        assert_eq!(equations[1]["location"], equations[2]["location"]);
        assert_eq!(
            equations[1]["location"]["range"]["start"]["line"],
            if phase >= 2 { 3 } else { 2 }
        );
        if let Some(previous_revision) = previous_revision {
            assert_ne!(result["revision"], previous_revision);
        }
        previous_revision = Some(result["revision"].clone());
        assert_eq!(
            parse_builds, 0,
            "first model-information request must reuse the checked core parse (phase {phase})"
        );

        PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
        assert_eq!(server.model_info_command(&arguments), result);
        assert_eq!(PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get), 0);
    }
}
