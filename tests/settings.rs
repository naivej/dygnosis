use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use dygnosis::include_resolver::path_key;
use dygnosis::server::{new_service, Backend};
use dygnosis::Workspace;
use serde_json::{json, Value};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct Files(PathBuf);

impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dygnosis-settings-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        path
    }

    fn uri(&self, name: &str) -> Url {
        file_uri(&self.0.join(name))
    }

    fn folder(&self, name: &str) -> WorkspaceFolder {
        WorkspaceFolder {
            uri: self.uri(name),
            name: name.to_owned(),
        }
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn file_uri(path: &Path) -> Url {
    Url::from_file_path(path).unwrap()
}

fn initialize(folders: Vec<WorkspaceFolder>, options: Value) -> InitializeParams {
    InitializeParams {
        workspace_folders: Some(folders),
        initialization_options: Some(options),
        ..InitializeParams::default()
    }
}

async fn open(backend: &Backend, uri: Url, text: &str) {
    backend
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri,
                language_id: "dynare".to_owned(),
                version: 1,
                text: text.to_owned(),
            },
        })
        .await;
}

async fn codes(backend: &Backend, uri: Url) -> Vec<String> {
    let result = backend
        .diagnostic(DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier { uri },
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .unwrap();
    let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) = result
    else {
        panic!("full report")
    };
    full.full_document_diagnostic_report
        .items
        .iter()
        .filter_map(|diag| {
            if let Some(NumberOrString::String(code)) = &diag.code {
                Some(code.clone())
            } else {
                None
            }
        })
        .collect()
}

fn variables(workspace: &mut Workspace, root: &Url) -> Vec<String> {
    let model = workspace.get_effective_model(root.as_str()).unwrap();
    model
        .endogenous
        .iter()
        .map(|decl| model.name(decl.name).to_owned())
        .collect()
}

#[test]
fn scoped_workspace_caches_and_overlays_do_not_borrow_another_roots_paths() {
    let files = Files::new();
    let a = files.uri("a/root.mod");
    let b = files.uri("b/root.mod");
    let c = files.uri("c/root.mod");
    files.write("a/lib/shared.inc", "var from_a;\n");
    files.write("b/lib/shared.inc", "var from_b;\n");
    files.write("a/other/shared.inc", "var changed_a;\n");
    let mut workspace = Workspace::new();
    for root in [&a, &b, &c] {
        workspace.update_document(root.as_str(), "@#include \"shared.inc\"\n");
    }
    workspace.set_root_search_paths(a.as_str(), vec![files.0.join("a/lib")]);
    workspace.set_root_search_paths(b.as_str(), vec![files.0.join("b/lib")]);
    workspace.set_root_search_paths(c.as_str(), Vec::new());
    assert_eq!(variables(&mut workspace, &a), ["from_a"]);
    assert_eq!(variables(&mut workspace, &b), ["from_b"]);
    let b_revision = workspace.input_revision(b.as_str()).unwrap();
    workspace.update_document(files.uri("b/lib/overlay.inc").as_str(), "var overlay_b;\n");
    workspace.update_document(c.as_str(), "@#include \"overlay.inc\"\n");
    assert!(variables(&mut workspace, &c).is_empty());
    assert_eq!(workspace.find_unresolved_includes(c.as_str()).len(), 1);
    workspace.set_root_search_paths(a.as_str(), vec![files.0.join("a/other")]);
    assert_eq!(variables(&mut workspace, &a), ["changed_a"]);
    assert_eq!(variables(&mut workspace, &b), ["from_b"]);
    assert_eq!(workspace.input_revision(b.as_str()).unwrap(), b_revision);
}

#[test]
fn revisions_observe_disk_edits_missing_candidates_companions_and_overlay_precedence() {
    let files = Files::new();
    let source = "@#include \"shared.inc\"\n";
    let root_path = files.write("model/root.mod", source);
    let root = file_uri(&root_path);
    let shared = files.write("lib/shared.inc", "var from_lib;\n");
    let mut workspace = Workspace::new();
    workspace.set_root_search_paths(root.as_str(), vec![files.0.join("lib")]);
    workspace.load_from_disk(&root_path).unwrap();
    let first = workspace.input_revision(root.as_str()).unwrap();
    assert_eq!(variables(&mut workspace, &root), ["from_lib"]);
    fs::write(&shared, "var changed_lib;\n").unwrap();
    let second = workspace.input_revision(root.as_str()).unwrap();
    assert_ne!(first, second);
    assert_eq!(variables(&mut workspace, &root), ["changed_lib"]);
    let local = files.write("model/shared.inc", "var from_local;\n");
    let third = workspace.input_revision(root.as_str()).unwrap();
    assert_ne!(second, third);
    assert_eq!(variables(&mut workspace, &root), ["from_local"]);
    workspace.update_document(file_uri(&local).as_str(), "var from_overlay;\n");
    let overlay = workspace.input_revision(root.as_str()).unwrap();
    fs::write(&local, "var changed_disk;\n").unwrap();
    assert_eq!(workspace.input_revision(root.as_str()).unwrap(), overlay);
    assert_eq!(variables(&mut workspace, &root), ["from_overlay"]);
    workspace.remove_document(file_uri(&local).as_str());
    assert_ne!(workspace.input_revision(root.as_str()).unwrap(), overlay);
    assert_eq!(variables(&mut workspace, &root), ["changed_disk"]);
    let before_companion = workspace.input_revision(root.as_str()).unwrap();
    let companion = files.write(
        "model/root_steadystate.m",
        "function ys = root_steadystate(ys)\nend\n",
    );
    let with_companion = workspace.input_revision(root.as_str()).unwrap();
    assert_ne!(before_companion, with_companion);
    workspace.update_document(files.uri("unrelated.inc").as_str(), "var unrelated;\n");
    assert_eq!(
        workspace.input_revision(root.as_str()).unwrap(),
        with_companion
    );
    fs::write(companion, "% changed companion\n").unwrap();
    assert_ne!(
        workspace.input_revision(root.as_str()).unwrap(),
        with_companion
    );
    fs::remove_file(root_path).unwrap();
    assert!(workspace.input_revision(root.as_str()).is_none());
}

#[tokio::test]
async fn folder_snapshot_uses_deepest_folder_and_loose_defaults() {
    let files = Files::new();
    files.write("outer/lib/values.inc", "parameters p; p = 1;\n");
    files.write("outer/nested/lib/values.inc", "parameters p; p = 2;\n");
    files.write("loose/lib/values.inc", "parameters p; p = 3;\n");
    let outer = files.folder("outer");
    let nested = files.folder("outer/nested");
    let options = json!({"dynare":{"configuration":{
        "schemaVersion":1,
        "loose":{"searchPaths":["lib"],"formatIndent":4,"nameDetails":{"longName":false}},
        "folders":[
            {"uri":outer.uri,"settings":{"searchPaths":["lib"],"formatIndent":2,"nameDetails":{"tex":false}}},
            {"uri":nested.uri,"settings":{"searchPaths":["lib"],"formatIndent":3,"parameterValueHints":false}}
        ]
    }}});
    let (service, _socket) = new_service();
    let backend = service.inner();
    backend
        .initialize(initialize(vec![outer, nested], options))
        .await
        .unwrap();
    let source = "@#include \"values.inc\"\nvar y;\nmodel;\ny=p;\nend;\n";
    for name in ["outer/root.mod", "outer/nested/root.mod", "loose/root.mod"] {
        let uri = files.uri(name);
        open(backend, uri.clone(), source).await;
        assert!(!codes(backend, uri)
            .await
            .iter()
            .any(|code| code == "E020" || code == "E061"));
    }
    assert!(
        !backend
            .presentation_settings(&files.uri("outer/root.mod"))
            .name_details
            .tex
    );
    assert!(
        !backend
            .presentation_settings(&files.uri("outer/nested/root.mod"))
            .parameter_value_hints
    );
    assert!(
        !backend
            .presentation_settings(&files.uri("loose/root.mod"))
            .name_details
            .long_name
    );
    for (name, count) in [
        ("outer/root.mod", 2),
        ("outer/nested/root.mod", 3),
        ("loose/root.mod", 4),
    ] {
        let uri = files.uri(name);
        let edits = backend
            .formatting(DocumentFormattingParams {
                text_document: TextDocumentIdentifier { uri },
                options: FormattingOptions {
                    tab_size: 8,
                    insert_spaces: true,
                    ..FormattingOptions::default()
                },
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap()
            .unwrap();
        assert!(edits[0]
            .new_text
            .contains(&format!("\n{}y", " ".repeat(count))));
    }
    let virtual_uri = Url::parse("untitled:Untitled-1.mod").unwrap();
    assert!(
        !backend
            .presentation_settings(&virtual_uri)
            .name_details
            .long_name
    );
}

#[test]
fn revisions_keep_equal_disk_overlay_text_but_observe_bytes_and_root_paths() {
    let files = Files::new();
    let source = "@#include \"values.inc\"\n";
    let root_path = files.write("model/root.mod", source);
    let include_path = files.write("model/values.inc", "var initial_y;\n");
    let root = file_uri(&root_path);
    let include = file_uri(&include_path);
    let mut workspace = Workspace::new();
    workspace.load_from_disk(&root_path).unwrap();
    let disk = workspace.input_revision(root.as_str()).unwrap();

    workspace.update_document(root.as_str(), source);
    workspace.update_document(include.as_str(), "var initial_y;\n");
    assert_eq!(workspace.input_revision(root.as_str()).unwrap(), disk);
    workspace.remove_document(include.as_str());
    workspace.remove_document(root.as_str());
    assert_eq!(workspace.input_revision(root.as_str()).unwrap(), disk);

    workspace.update_document(include.as_str(), "var edited_overlay_y;\n");
    let edited = workspace.input_revision(root.as_str()).unwrap();
    assert_ne!(edited, disk);
    fs::write(&include_path, "var changed_disk_y;\n").unwrap();
    assert_eq!(workspace.input_revision(root.as_str()).unwrap(), edited);
    assert_eq!(variables(&mut workspace, &root), ["edited_overlay_y"]);
    workspace.remove_document(include.as_str());
    let closed = workspace.input_revision(root.as_str()).unwrap();
    assert_ne!(closed, edited);
    assert_ne!(closed, disk);
    assert_eq!(variables(&mut workspace, &root), ["changed_disk_y"]);

    workspace.update_document(include.as_str(), "var changed_disk_y;\n");
    assert_eq!(workspace.input_revision(root.as_str()).unwrap(), closed);
    workspace.set_root_search_paths(root.as_str(), vec![files.0.join("extra")]);
    assert_ne!(workspace.input_revision(root.as_str()).unwrap(), closed);
    assert_eq!(variables(&mut workspace, &root), ["changed_disk_y"]);
}

#[tokio::test]
async fn model_info_revision_stays_current_when_opening_unchanged_included_text() {
    async fn facts(backend: &Backend, root: &Url) -> Value {
        backend
            .execute_command(ExecuteCommandParams {
                command: "dynare/modelInfo".to_owned(),
                arguments: vec![json!({"root_uri":root})],
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap()
    }
    let files = Files::new();
    let source = "var output_y;\nmodel;\n@#include \"row.inc\"\nend;\n";
    files.write("root.mod", source);
    let include_path = files.write("row.inc", "output_y=1;\n");
    let root = files.uri("root.mod");
    let include = files.uri("row.inc");
    let (service, _socket) = new_service();
    let backend = service.inner();
    open(backend, root.clone(), source).await;
    let disk = facts(backend, &root).await;
    assert_eq!(disk["complete"], true);
    open(backend, include.clone(), "output_y=1;\n").await;
    let opened = facts(backend, &root).await;
    assert_eq!(opened["revision"], disk["revision"]);
    // Opening a file preserves its client URI spelling; Windows disk paths may
    // have been normalized to lower case before an editor URI was available.
    let normalize_locations = |value: &Value| {
        fn normalize(value: &mut Value) {
            match value {
                Value::Object(fields) => {
                    if let Some(Value::String(uri)) = fields.get_mut("uri") {
                        *uri = dygnosis::include_resolver::normalize_uri(uri);
                    }
                    for field in fields.values_mut() {
                        normalize(field);
                    }
                }
                Value::Array(rows) => rows.iter_mut().for_each(normalize),
                _ => {}
            }
        }
        let mut value = value.clone();
        normalize(&mut value);
        value
    };
    assert_eq!(
        normalize_locations(&opened["equations"]),
        normalize_locations(&disk["equations"])
    );

    backend
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: include.clone(),
                version: 2,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: "output_y=2;\n".to_owned(),
            }],
        })
        .await;
    let edited = facts(backend, &root).await;
    assert_ne!(edited["revision"], opened["revision"]);
    fs::write(include_path, "output_y=3;\n").unwrap();
    assert_eq!(facts(backend, &root).await["revision"], edited["revision"]);
    backend
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: include },
        })
        .await;
    let closed = facts(backend, &root).await;
    assert_ne!(closed["revision"], edited["revision"]);
    assert_ne!(closed["revision"], disk["revision"]);

    backend
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"dynare":{"nameDetails":{"tex":false}}}),
        })
        .await;
    assert_ne!(facts(backend, &root).await["revision"], closed["revision"]);
}

#[tokio::test]
async fn snapshots_clear_removed_values_and_folder_events_reselect_settings() {
    let files = Files::new();
    let outer = files.folder("outer");
    let nested = files.folder("outer/nested");
    let uri = files.uri("outer/nested/root.mod");
    let (service, _socket) = new_service();
    let backend = service.inner();
    let snapshot = json!({"configuration":{"schemaVersion":1,"loose":{},"folders":[
        {"uri":outer.uri,"settings":{"nameDetails":{"tex":false}}},
        {"uri":nested.uri,"settings":{"nameDetails":{"longName":false},"outline":{"sections":[],"equationNumbers":false}}}
    ]}});
    backend
        .initialize(initialize(vec![outer.clone(), nested.clone()], snapshot))
        .await
        .unwrap();
    open(backend, uri.clone(), "var y; model; y=0; end;\n").await;
    let before = backend.model_input_revision(&uri).unwrap();
    assert!(backend
        .presentation_settings(&uri)
        .outline
        .sections
        .is_empty());
    backend
        .did_change_workspace_folders(DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent {
                added: Vec::new(),
                removed: vec![nested.clone()],
            },
        })
        .await;
    let preferences = backend.presentation_settings(&uri);
    assert!(preferences.name_details.long_name);
    assert!(!preferences.name_details.tex);
    assert_ne!(backend.model_input_revision(&uri).unwrap(), before);
    backend
        .did_change_workspace_folders(DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent {
                added: vec![nested],
                removed: Vec::new(),
            },
        })
        .await;
    assert!(backend.presentation_settings(&uri).name_details.tex);
    backend.did_change_configuration(DidChangeConfigurationParams {
        settings: json!({"dynare":{"configuration":{"schemaVersion":1,"loose":{},"folders":[]}}})
    }).await;
    assert_eq!(backend.presentation_settings(&uri), Default::default());
}

#[tokio::test]
async fn legacy_single_folder_and_by_root_settings_remain_usable_and_isolated() {
    let files = Files::new();
    files.write("folder/lib/values.inc", "parameters p; p=1;\n");
    files.write("other/lib/values.inc", "parameters p; p=2;\n");
    let a = files.uri("folder/root.mod");
    let b = files.uri("other/root.mod");
    let (service, _socket) = new_service();
    let backend = service.inner();
    let mut params = initialize(
        Vec::new(),
        json!({"dynare":{"searchPaths":["lib"],"formatIndent":2}}),
    );
    params.workspace_folders = None;
    #[allow(deprecated)]
    {
        params.root_uri = Some(files.uri("folder"));
    }
    backend.initialize(params).await.unwrap();
    let source = "@#include \"values.inc\"\nvar y; model; y=p; end;\n";
    open(backend, a.clone(), source).await;
    assert!(!codes(backend, a.clone()).await.contains(&"E061".to_owned()));
    backend
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"dynare":{"searchPaths":[],"searchPathsByRoot":{
                a.as_str():[files.0.join("folder/lib")], b.as_str():[files.0.join("other/lib")]
            }}}),
        })
        .await;
    open(backend, b.clone(), source).await;
    assert!(!codes(backend, a.clone()).await.contains(&"E061".to_owned()));
    assert!(!codes(backend, b.clone()).await.contains(&"E061".to_owned()));
    let revision = backend.model_input_revision(&a).unwrap();
    backend
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"dynare":{"formatIndent":4}}),
        })
        .await;
    assert_ne!(backend.model_input_revision(&a).unwrap(), revision);
    assert!(!codes(backend, a).await.contains(&"E061".to_owned()));
}

#[tokio::test]
async fn virtual_roots_never_resolve_native_includes_or_companions() {
    let files = Files::new();
    let include = files.write("exists.inc", "var from_disk;\n");
    let (service, _socket) = new_service();
    let backend = service.inner();
    backend
        .initialize(initialize(Vec::new(), json!({"searchPaths":[files.0]})))
        .await
        .unwrap();
    for raw in ["untitled:Untitled-1.mod", "vscode-test://test/root.mod"] {
        let uri = Url::parse(raw).unwrap();
        let text = format!(
            "@#include \"{}\"\nvar y; model; y=0; end;\n",
            include.display()
        );
        open(backend, uri.clone(), &text).await;
        assert!(!codes(backend, uri.clone())
            .await
            .contains(&"E061".to_owned()));
        let effective = backend
            .execute_command(ExecuteCommandParams {
                command: "dynare/showEffectiveModel".to_owned(),
                arguments: vec![json!({"uri":uri})],
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap()
            .unwrap();
        assert!(!effective["effective_text"]
            .as_str()
            .unwrap()
            .contains("from_disk"));
        assert_eq!(effective["status"], "incomplete");
        assert!(backend.model_input_revision(&uri).is_some());
        backend
            .did_close(DidCloseTextDocumentParams {
                text_document: TextDocumentIdentifier { uri: uri.clone() },
            })
            .await;
        assert!(backend.model_input_revision(&uri).is_none());
    }
}

#[tokio::test]
async fn close_and_delete_revisions_preserve_live_overlays_and_clear_missing_roots() {
    let files = Files::new();
    let path = files.write("root.mod", "var y; model; y=0; end;\n");
    let uri = file_uri(&path);
    let (service, _socket) = new_service();
    let backend = service.inner();
    open(backend, uri.clone(), "var y; model; y=1; end;\n").await;
    let overlay = backend.model_input_revision(&uri).unwrap();
    fs::write(&path, "var y; model; y=2; end;\n").unwrap();
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent::new(uri.clone(), FileChangeType::CHANGED)],
        })
        .await;
    assert_eq!(backend.model_input_revision(&uri).unwrap(), overlay);
    backend
        .did_close(DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
        })
        .await;
    let disk = backend.model_input_revision(&uri).unwrap();
    assert_ne!(disk, overlay);
    fs::remove_file(path).unwrap();
    backend
        .did_change_watched_files(DidChangeWatchedFilesParams {
            changes: vec![FileEvent::new(uri.clone(), FileChangeType::DELETED)],
        })
        .await;
    assert!(backend.model_input_revision(&uri).is_none());
}

#[tokio::test]
async fn failed_includes_withhold_facts_and_keep_only_earlier_text() {
    let files = Files::new();
    files.write("cycle.inc", "@#include \"cycle.mod\"\n");
    let (service, _socket) = new_service();
    let backend = service.inner();
    for (name, target, code) in [
        ("missing.mod", "missing.inc", "E061"),
        ("cycle.mod", "cycle.inc", "W062"),
    ] {
        let uri = files.uri(name);
        let text = format!("var y;\nmodel;\ny=0;\nend;\n@#include \"{target}\"\n");
        open(backend, uri.clone(), &text).await;
        let effective = backend
            .execute_command(ExecuteCommandParams {
                command: "dynare/showEffectiveModel".to_owned(),
                arguments: vec![json!({"uri":uri})],
                work_done_progress_params: WorkDoneProgressParams::default(),
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(effective["status"], "incomplete", "{name}");
        let origins = effective["origins"].as_array().unwrap();
        assert!(origins.is_empty(), "{name}");
        assert_eq!(effective["navigation"], json!([]), "{name}");
        assert!(
            effective["effective_text"]
                .as_str()
                .unwrap()
                .contains("y = 0"),
            "{effective}"
        );
        assert!(
            codes(backend, uri).await.contains(&code.to_owned()),
            "{name}"
        );
    }
}

#[test]
fn absolute_loose_search_paths_are_normalized_independently() {
    let files = Files::new();
    let root = files.uri("loose/root.mod");
    let mut workspace = Workspace::new();
    let path = files.write("library/exists.inc", "var z;\n");
    workspace.update_document(root.as_str(), "@#include \"exists.inc\"\n");
    workspace.set_root_search_paths(
        root.as_str(),
        vec![PathBuf::from(path_key(path.parent().unwrap()))],
    );
    assert_eq!(variables(&mut workspace, &root), ["z"]);
}

#[tokio::test]
async fn invalid_settings_normalize_duplicates_and_fall_back_to_defaults() {
    let files = Files::new();
    let uri = files.uri("root.mod");
    let (service, socket) = new_service();
    drop(socket);
    let backend = service.inner();
    backend.initialize(initialize(Vec::new(), json!({"dynare":{"configuration":{
        "schemaVersion":1,"folders":[],"loose":{
            "formatIndent":-1,"searchPaths":[null,"lib","lib"],
            "nameDetails":{"longName":"false","tex":false},
            "outline":{"sections":["equations","bad","equations","blocks"],"equationNumbers":null},
            "parameterValueHints":"false"
        }
    }}}))).await.unwrap();
    let preferences = backend.presentation_settings(&uri);
    assert!(preferences.name_details.long_name);
    assert!(!preferences.name_details.tex);
    assert_eq!(preferences.outline.sections, ["equations", "blocks"]);
    assert!(preferences.outline.equation_numbers);
    assert!(preferences.parameter_value_hints);
    backend
        .did_change_configuration(DidChangeConfigurationParams {
            settings: json!({"configuration":{
                "schemaVersion":99,"folders":[],"loose":{}
            }}),
        })
        .await;
    assert_eq!(backend.presentation_settings(&uri), preferences);
    backend.did_change_configuration(DidChangeConfigurationParams { settings: json!({"configuration":{
        "schemaVersion":1,"folders":[],"loose":{"nameDetails":0,"outline":false,"parameterValueHints":false}
    }}) }).await;
    let preferences = backend.presentation_settings(&uri);
    assert!(preferences.name_details.tex);
    assert_eq!(preferences.outline.sections.len(), 5);
    assert!(!preferences.parameter_value_hints);
}

#[tokio::test]
async fn include_owners_survive_cache_invalidation_and_remain_separate_from_presentation() {
    let files = Files::new();
    let root = files.uri("a/root.mod");
    let second = files.uri("a/second.mod");
    let include = files.uri("b/part.mod");
    files.write("b/part.mod", "parameters p; p=1;\n");
    let (service, _socket) = new_service();
    let backend = service.inner();
    let a = files.folder("a");
    let b = files.folder("b");
    backend.initialize(initialize(vec![a.clone(), b.clone()], json!({"configuration":{
        "schemaVersion":1,"loose":{},"folders":[
            {"uri":a.uri,"settings":{"searchPaths":["../b"],"nameDetails":{"longName":false}}},
            {"uri":b.uri,"settings":{"nameDetails":{"tex":false}}}
        ]
    }}))).await.unwrap();
    for uri in [&root, &second] {
        open(
            backend,
            uri.clone(),
            "@#include \"part.mod\"\nvar y; model; y=p; end;\n",
        )
        .await;
    }
    let owners = backend.known_model_roots(&include);
    assert_eq!(owners, [root.clone(), second.clone()]);
    assert!(!backend.presentation_settings(&root).name_details.long_name);
    assert!(!backend.presentation_settings(&include).name_details.tex);
    open(backend, include.clone(), "parameters p; p=2;\n").await;
    assert_eq!(backend.known_model_roots(&include), owners);
    assert!(!backend.known_model_roots(&include).contains(&include));
}

struct Wire {
    read: tokio::io::BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
    write: tokio::io::WriteHalf<tokio::io::DuplexStream>,
    server: tokio::task::JoinHandle<()>,
}

impl Wire {
    fn new() -> Self {
        let (client, server_io) = tokio::io::duplex(65536);
        let (read, write) = tokio::io::split(client);
        let (server_read, server_write) = tokio::io::split(server_io);
        let (service, socket) = new_service();
        let server = tokio::spawn(async move {
            tower_lsp::Server::new(server_read, server_write, socket)
                .serve(service)
                .await;
        });
        Self {
            read: tokio::io::BufReader::new(read),
            write,
            server,
        }
    }

    async fn send(&mut self, value: Value) {
        use tokio::io::AsyncWriteExt;
        let body = value.to_string();
        self.write
            .write_all(format!("Content-Length: {}\r\n\r\n{}", body.len(), body).as_bytes())
            .await
            .unwrap();
    }

    async fn read(&mut self) -> Value {
        use tokio::io::{AsyncBufReadExt, AsyncReadExt};
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let mut length = None;
            loop {
                let mut line = String::new();
                assert_ne!(
                    self.read.read_line(&mut line).await.unwrap(),
                    0,
                    "server ended"
                );
                if line == "\r\n" {
                    break;
                }
                if let Some(raw) = line.strip_prefix("Content-Length:") {
                    length = Some(raw.trim().parse::<usize>().unwrap());
                }
            }
            let mut body = vec![0; length.expect("Content-Length")];
            self.read.read_exact(&mut body).await.unwrap();
            serde_json::from_slice(&body).unwrap()
        })
        .await
        .expect("LSP message within 10 seconds")
    }

    async fn until_response(&mut self, id: i32) -> Vec<Value> {
        let mut messages = Vec::new();
        loop {
            let message = self.read().await;
            let done = message.get("id").and_then(Value::as_i64) == Some(i64::from(id))
                && message.get("method").is_none();
            if let Some(request_id) = message
                .get("id")
                .filter(|_| message.get("method").is_some())
            {
                self.send(json!({"jsonrpc":"2.0","id":request_id,"result":null}))
                    .await;
            }
            messages.push(message);
            if done {
                return messages;
            }
        }
    }

    async fn initialize(&mut self, folder: WorkspaceFolder, opt_in: bool) {
        self.send(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "workspaceFolders":[folder],
            "capabilities":{
                "experimental":{"dygnosis":{"modelInfoChanged":opt_in}},
                "workspace":{"semanticTokens":{"refreshSupport":true},"inlayHint":{"refreshSupport":true}}
            }
        }})).await;
        let messages = self.until_response(1).await;
        let capabilities = &messages.last().unwrap()["result"]["capabilities"];
        assert_eq!(
            capabilities["experimental"]["dygnosis"]["modelInfo"]["schema_version"],
            1
        );
        assert_eq!(
            capabilities["workspace"]["workspaceFolders"]["supported"],
            true
        );
        assert!(capabilities["experimental"]["dygnosis"]
            .get("projectStatus")
            .is_none());
        self.send(json!({"jsonrpc":"2.0","method":"initialized","params":{}}))
            .await;
    }

    async fn open(&mut self, uri: &Url, source: &str) {
        self.send(
            json!({"jsonrpc":"2.0","method":"textDocument/didOpen","params":{
                "textDocument":{"uri":uri,"languageId":"dynare","version":1,"text":source}
            }}),
        )
        .await;
        loop {
            let message = self.read().await;
            if message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == uri.as_str()
            {
                break;
            }
        }
    }

    async fn settings_change(&mut self, uri: &Url) -> Vec<Value> {
        self.send(
            json!({"jsonrpc":"2.0","method":"workspace/didChangeConfiguration","params":{
                "settings":{"dynare":{"nameDetails":{"tex":false}}}
            }}),
        )
        .await;
        let mut messages = Vec::new();
        loop {
            let message = self.read().await;
            let done = message["method"] == "workspace/inlayHint/refresh";
            if let Some(id) = message
                .get("id")
                .filter(|_| message.get("method").is_some())
            {
                self.send(json!({"jsonrpc":"2.0","id":id,"result":null}))
                    .await;
            }
            messages.push(message);
            if done {
                break;
            }
        }
        self.send(json!({"jsonrpc":"2.0","id":2,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":uri}}})).await;
        messages.extend(self.until_response(2).await);
        messages
    }
}

impl Drop for Wire {
    fn drop(&mut self) {
        self.server.abort();
    }
}

#[tokio::test]
async fn model_info_invalidation_is_opt_in_and_requests_native_refresh() {
    let files = Files::new();
    let root = files.uri("root.mod");
    for opt_in in [false, true] {
        let mut wire = Wire::new();
        wire.initialize(files.folder(""), opt_in).await;
        wire.open(&root, "var y; model; y=0; end;\n").await;
        let messages = wire.settings_change(&root).await;
        let changes: Vec<_> = messages
            .iter()
            .filter(|message| message["method"] == "dynare/modelInfoChanged")
            .collect();
        assert_eq!(changes.len(), usize::from(opt_in));
        if opt_in {
            assert_eq!(changes[0]["params"]["root_uri"], root.as_str());
            assert_eq!(changes[0]["params"]["schema_version"], 1);
            assert!(changes[0]["params"]["revision"].as_str().is_some());
        }
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["method"] == "workspace/semanticTokens/refresh")
                .count(),
            1
        );
        assert_eq!(
            messages
                .iter()
                .filter(|message| message["method"] == "workspace/inlayHint/refresh")
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn include_invalidation_keeps_owner_provenance_and_clears_deleted_closed_root() {
    let files = Files::new();
    let included = files.write("values.inc", "parameters p; p=1;\n");
    let root_path = files.write(
        "root.mod",
        "@#include \"values.inc\"\nvar y; model; y=p; end;\n",
    );
    let root = file_uri(&root_path);
    let unrelated = files.uri("unrelated.mod");
    let mut wire = Wire::new();
    wire.initialize(files.folder(""), true).await;
    wire.open(&root, &fs::read_to_string(&root_path).unwrap())
        .await;
    wire.open(&unrelated, "var z; model; z=0; end;\n").await;
    fs::write(included, "parameters p; p=2;\n").unwrap();
    wire.send(
        json!({"jsonrpc":"2.0","method":"workspace/didChangeWatchedFiles","params":{
            "changes":[{"uri":files.uri("values.inc"),"type":2}]
        }}),
    )
    .await;
    let mut changes = Vec::new();
    loop {
        let message = wire.read().await;
        if message["method"] == "dynare/modelInfoChanged" {
            changes.push(message.clone());
        }
        if let Some(id) = message
            .get("id")
            .filter(|_| message.get("method").is_some())
        {
            wire.send(json!({"jsonrpc":"2.0","id":id,"result":null}))
                .await;
        }
        if message["method"] == "workspace/inlayHint/refresh" {
            break;
        }
    }
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0]["params"]["root_uri"], root.as_str());
    wire.send(json!({"jsonrpc":"2.0","method":"textDocument/didClose","params":{"textDocument":{"uri":root}}})).await;
    // Closing equal disk/overlay text changes source ownership, not analysis
    // inputs. An ordinary request provides a wire barrier without waiting for
    // a model invalidation which the equal content deliberately does not emit.
    wire.send(json!({"jsonrpc":"2.0","id":3,"method":"textDocument/diagnostic","params":{"textDocument":{"uri":root}}})).await;
    let closed = wire.until_response(3).await;
    assert!(!closed
        .iter()
        .any(|message| message["method"] == "dynare/modelInfoChanged"));
    fs::remove_file(root_path).unwrap();
    wire.send(
        json!({"jsonrpc":"2.0","method":"workspace/didChangeWatchedFiles","params":{
            "changes":[{"uri":root,"type":3}]
        }}),
    )
    .await;
    loop {
        let message = wire.read().await;
        if message["method"] == "dynare/modelInfoChanged" {
            assert_eq!(message["params"]["root_uri"], root.as_str());
            assert!(message["params"]["revision"].is_null());
            break;
        }
    }
}
