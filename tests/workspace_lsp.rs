//! Real-process LSP checks: folder-only activation and report/input lifetime.
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tower_lsp::lsp_types::Url;

static NEXT: AtomicU64 = AtomicU64::new(0);
const MODEL: &str = "var y; model; y=0; end; initval; y=0; end;\n";

struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dygnosis-workspace-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
    fn write(&self, name: &str, text: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    fn uri(&self, name: &str) -> String {
        Url::from_file_path(self.0.join(name)).unwrap().to_string()
    }
    fn folder(&self, name: &str) -> Value {
        json!({"uri":self.uri(name),"name":name})
    }
}
impl Drop for Files {
    fn drop(&mut self) {
        if let Ok(path) = self.0.canonicalize() {
            let temp = std::env::temp_dir().canonicalize().unwrap();
            assert!(path.starts_with(&temp) && path != temp);
            let _ = fs::remove_dir_all(path);
        }
    }
}

struct Wire {
    child: Child,
    input: ChildStdin,
    output: tokio::sync::mpsc::UnboundedReceiver<Value>,
    seen: Vec<Value>,
    id: u64,
}
impl Wire {
    fn new() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_dygnosis"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                let mut length = None;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    if let Some(raw) = line.strip_prefix("Content-Length:") {
                        length = raw.trim().parse::<usize>().ok();
                    }
                }
                let mut body = vec![0; length.expect("Content-Length")];
                if reader.read_exact(&mut body).is_err() {
                    return;
                }
                if sender
                    .send(serde_json::from_slice(&body).expect("JSON-RPC"))
                    .is_err()
                {
                    return;
                }
            }
        });
        Self {
            child,
            input,
            output: receiver,
            seen: Vec::new(),
            id: 0,
        }
    }
    fn send(&mut self, message: Value) {
        let body = message.to_string();
        write!(self.input, "Content-Length: {}\r\n\r\n{}", body.len(), body).unwrap();
        self.input.flush().unwrap();
    }
    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc":"2.0","method":method,"params":params}));
    }
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let message = self.output.recv().await.expect("server message");
                if message["id"] == id && message.get("method").is_none() {
                    assert!(message.get("error").is_none(), "{message}");
                    return message["result"].clone();
                }
                if message.get("id").is_some() && message.get("method").is_some() {
                    self.send(json!({"jsonrpc":"2.0","id":message["id"],"result":null}));
                }
                self.seen.push(message);
            }
        })
        .await
        .expect("request within 15 seconds")
    }
    async fn start(&mut self, folders: Vec<Value>, loose: Value, scoped: Vec<Value>, opt_in: bool) {
        let response = self.request("initialize", json!({"workspaceFolders":folders,"capabilities":{"experimental":{"dygnosis":{"workspaceStatusChanged":opt_in}}},
            "initializationOptions":{"dynare":{"configuration":{"schemaVersion":1,"loose":loose,"folders":scoped}}}})).await;
        assert_eq!(
            response["capabilities"]["experimental"]["dygnosis"]["workspaceDiagnostics"]["schema_version"],
            1
        );
        self.notify("initialized", json!({}));
    }
    async fn command(&mut self, command: &str) -> Value {
        self.request(
            "workspace/executeCommand",
            json!({"command":command,"arguments":[]}),
        )
        .await
    }
    async fn command_args(&mut self, command: &str, arguments: Value) -> Value {
        self.request(
            "workspace/executeCommand",
            json!({"command":command,"arguments":[arguments]}),
        )
        .await
    }
    async fn start_context(&mut self, files: &Files, workspace: bool) {
        let init = self
            .request(
                "initialize",
                json!({"workspaceFolders":[files.folder("")],
            "capabilities":{"experimental":{"dygnosis":{"documentContext":{"schema_version":1}}}},
            "initializationOptions":{"dynare":{"workspaceDiagnostics":workspace}}}),
            )
            .await;
        assert_eq!(
            init["capabilities"]["experimental"]["dygnosis"]["documentContext"]["schema_version"],
            1
        );
        self.notify("initialized", json!({}));
    }
    async fn start_analysis(&mut self, files: &Files, workspace: bool) {
        self.request("initialize", json!({"workspaceFolders":[files.folder("")],
            "capabilities":{"experimental":{"dygnosis":{"documentContext":{"schema_version":1},"analysisConfiguration":{"schema_version":1}}}},
            "initializationOptions":{"dynare":{"workspaceDiagnostics":workspace}}})).await;
        self.notify("initialized", json!({}));
    }
    async fn analysis_context(&mut self, root: &str) -> Value {
        self.command_args("dynare/analysisContext", json!({"root_uri":root}))
            .await
    }
    async fn select_analysis(&mut self, root: &str, proof: &Value, analysis: Value) -> Value {
        self.command_args("dynare/setAnalysisConfiguration",json!({"root_uri":root,
            "expected_revision":proof["revision"],"expected_selection_generation":proof["analysis"]["selection_generation"],
            "analysis":analysis,"include_paths":null})).await
    }
    async fn choose_context(&mut self, document: &str, version: i32, root: &str) -> Value {
        let info = self
            .command_args(
                "dynare/modelInfo",
                json!({"root_uri":root,"document_uri":document}),
            )
            .await;
        assert!(info.get("error").is_none(), "{info}");
        self.command_args("dynare/setDocumentContext", json!({"document_uri":document,"document_version":version,"root_uri":root,"expected_revision":info["revision"]})).await
    }
    async fn complete(&mut self) -> Value {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let status = self.command("dynare/workspaceStatus").await;
                if status["complete"] == true {
                    return status;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("workspace completed")
    }
    async fn pull(&mut self, uri: &str) -> Vec<Value> {
        self.request(
            "textDocument/diagnostic",
            json!({"textDocument":{"uri":uri}}),
        )
        .await["items"]
            .as_array()
            .unwrap()
            .clone()
    }
    fn open(&mut self, uri: &str, text: &str, version: i32) {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument":{"uri":uri,"languageId":"dynare","text":text,"version":version}}),
        );
    }
    fn change(&mut self, uri: &str, text: &str, version: i32) {
        self.notify(
            "textDocument/didChange",
            json!({"textDocument":{"uri":uri,"version":version},"contentChanges":[{"text":text}]}),
        );
    }
    fn close(&mut self, uri: &str) {
        self.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}));
    }
    fn changed(&mut self, uri: &str, kind: u8) {
        self.notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes":[{"uri":uri,"type":kind}]}),
        );
    }
}
impl Drop for Wire {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn root<'a>(status: &'a Value, uri: &str) -> &'a Value {
    status["roots"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["root_uri"] == uri)
        .unwrap_or_else(|| panic!("root absent {uri}: {status}"))
}

fn outline_named<'a>(rows: &'a Value, name: &str) -> Option<&'a Value> {
    rows.as_array()?.iter().find_map(|row| {
        if row["name"] == name {
            Some(row)
        } else {
            outline_named(&row["children"], name)
        }
    })
}

// Guards one selected context for counts/expansion/Outline, refused late choices,
// and stale overrides retaining their label while source browsing still works.
#[tokio::test]
async fn captured_configuration_guards_ignore_live_choices_and_watch_both_sides() {
    // Model Diff must keep frozen choices after a live switch, but reject a
    // changed B-only overlay or a newly present higher-priority include.
    let files = Files::new();
    let text = "@#ifndef MODE\n@#define MODE=0\n@#endif\nvar y;\n@#if MODE\n@#include \"values.data\"\nmodel;y=p;end;\n@#else\nmodel;y=0;end;\n@#endif\n";
    files.write("root.mod", text);
    files.write("lib/values.data", "parameters p; p=1;\n");
    let root = files.uri("root.mod");
    let mut wire = Wire::new();
    wire.start_analysis(&files, false).await;
    wire.open(&root, text, 1);
    let context = wire.analysis_context(&root).await;
    let model = wire
        .command_args("dynare/modelInfo", json!({"root_uri":root}))
        .await;
    let side = |id: &str, analysis: Value| {
        json!({"kind":"working","input_id":id,"root_uri":root,
        "expected_revision":model["revision"],"analysis":analysis,"search_paths":[files.0.join("lib").to_string_lossy()]})
    };
    let comparison = wire.command_args("dynare/compareModelSnapshots", json!({"schema_version":1,
        "before":side("A",json!({})),"after":side("B",json!({"defines":[{"name":"MODE","expression":"1"}]}))})).await;
    assert!(
        comparison["inputs"]["after"]["guards"].is_array(),
        "{comparison}"
    );
    let guards = json!({"inputs":[comparison["inputs"]["before"],comparison["inputs"]["after"]]});
    assert_eq!(
        wire.command_args("dynare/validateComparisonInputs", guards.clone())
            .await["current"],
        true
    );
    let selected = wire
        .select_analysis(
            &root,
            &context,
            json!({"name":"Live choice","defines":[{"name":"MODE","expression":"0"}]}),
        )
        .await;
    assert_eq!(selected["accepted"], true, "{selected}");
    assert_eq!(
        wire.command_args("dynare/validateComparisonInputs", guards.clone())
            .await["current"],
        true
    );
    wire.open(&files.uri("lib/values.data"), "parameters p; p=2;\n", 1);
    assert_eq!(
        wire.command_args("dynare/validateComparisonInputs", guards.clone())
            .await["current"],
        false
    );
    wire.notify(
        "textDocument/didClose",
        json!({"textDocument":{"uri":files.uri("lib/values.data")}}),
    );
    assert_eq!(
        wire.command_args("dynare/validateComparisonInputs", guards.clone())
            .await["current"],
        true
    );
    files.write("values.data", "parameters p; p=9;\n");
    assert_eq!(
        wire.command_args("dynare/validateComparisonInputs", guards)
            .await["current"],
        false
    );
}

#[tokio::test]
async fn failed_configuration_comparison_keeps_both_side_summaries_over_stdio() {
    // A missing include on B must not erase A's captured choice or turn B's
    // unavailable counts into zero or expose navigation from a failed pair.
    let files = Files::new();
    let text = "@#ifndef ALT\n@#define ALT=0\n@#endif\nvar y;\n@#if ALT\n@#include \"missing.inc\"\n@#endif\nmodel; y=0; end;\n";
    files.write("root.mod", text);
    let root = files.uri("root.mod");
    let mut wire = Wire::new();
    wire.start_analysis(&files, false).await;
    wire.open(&root, text, 1);
    let model = wire
        .command_args("dynare/modelInfo", json!({"root_uri":root}))
        .await;
    let side = |id: &str, analysis: Value| {
        json!({"kind":"working","input_id":id,
        "root_uri":root,"expected_revision":model["revision"],"analysis":analysis})
    };
    let failed = wire.command_args("dynare/compareModelSnapshots", json!({"schema_version":1,
        "before":side("A",json!({})),
        "after":side("B",json!({"name":"Missing include choice","defines":[{"name":"ALT","expression":"1"}]}))})).await;
    assert_eq!(failed["state"], "failure", "{failed}");
    assert_eq!(failed["code"], "INCOMPLETE_INPUT", "{failed}");
    let before = &failed["analysis_summaries"]["before"];
    let after = &failed["analysis_summaries"]["after"];
    assert_eq!(before["complete"], true, "{failed}");
    assert_eq!(after["complete"], false, "{failed}");
    assert_eq!(
        after["analysis"]["selection"]["name"],
        "Missing include choice"
    );
    assert_ne!(
        before["analysis"]["selection"]["configuration_id"],
        after["analysis"]["selection"]["configuration_id"]
    );
    for summary in [before, after] {
        assert!(summary["counts"].is_null(), "{summary}");
        assert!(
            summary["counts_cause"]
                .as_str()
                .is_some_and(|cause| !cause.is_empty()),
            "{summary}"
        );
    }
    assert!(
        after["counts_cause"]
            .as_str()
            .unwrap()
            .contains("missing.inc"),
        "{after}"
    );
    for field in ["changes", "sources", "navigation"] {
        assert!(
            failed.get(field).is_none(),
            "Failed comparison exposed {field}: {failed}"
        );
    }
}

#[tokio::test]
async fn analysis_choices_refresh_both_views_and_stale_source_browsing_over_stdio() {
    let files = Files::new();
    let text = "@#ifndef MODE\n@#define MODE=0\n@#endif\nvar y;\n@#if MODE==0\nmodel; y=0; end;\n@#else\nparameters p;\nmodel; y=p; end;\n@#endif\n";
    files.write("root.mod", text);
    let uri = files.uri("root.mod");
    let mut wire = Wire::new();
    wire.start_analysis(&files, false).await;
    wire.open(&uri, text, 1);
    let symbols = wire
        .request(
            "textDocument/documentSymbol",
            json!({"textDocument":{"uri":uri}}),
        )
        .await;
    assert!(outline_named(&symbols, "p").is_some(), "{symbols}");
    assert_eq!(
        outline_named(&symbols, "@#else").unwrap()["detail"],
        "inactive",
        "{symbols}"
    );
    let proof = wire.analysis_context(&uri).await;
    let selected = wire
        .select_analysis(
            &uri,
            &proof,
            json!({"name":"Alternative","defines":[{"name":"MODE","expression":"1"}]}),
        )
        .await;
    assert_eq!(selected["accepted"], true, "{selected}");
    let info = wire
        .command_args("dynare/modelInfo", json!({"root_uri":uri}))
        .await;
    assert_eq!(info["n_parameters"], 1, "{info}");
    assert_eq!(info["analysis"]["selection"]["name"], "Alternative");
    let expanded = wire
        .command_args(
            "dynare/showEffectiveModel",
            json!({"root_uri":uri,"layout":"source"}),
        )
        .await;
    assert_eq!(
        expanded["analysis"]["selection"]["configuration_id"],
        info["analysis"]["selection"]["configuration_id"]
    );
    assert!(
        expanded["effective_text"].as_str().unwrap().contains("y=p"),
        "{expanded}"
    );
    let refused = wire.select_analysis(&uri, &proof, json!({})).await;
    assert_eq!(refused["code"], "ANALYSIS_CONTEXT_STALE", "{refused}");
    let target = selected["sites"]
        .as_array()
        .unwrap()
        .iter()
        .find(|site| site["name"] == "MODE")
        .unwrap()["target"]
        .clone();
    let preview=wire.select_analysis(&uri,&selected,json!({"name":"Policy preview","overrides":[{"kind":"definition","target":target,"expression":"1"}]})).await;
    assert_eq!(
        preview["analysis"]["selection"]["preview"], true,
        "{preview}"
    );
    let changed = text.replace("MODE=0", "MODE=2");
    wire.change(&uri, &changed, 2);
    let stale = wire
        .command_args("dynare/modelInfo", json!({"root_uri":uri}))
        .await;
    assert_eq!(stale["analysis"]["state"], "stale", "{stale}");
    assert_eq!(stale["analysis"]["selection"]["name"], "Policy preview");
    assert!(stale.get("n_equations").is_none(), "{stale}");
    let symbols = wire
        .request(
            "textDocument/documentSymbol",
            json!({"textDocument":{"uri":uri}}),
        )
        .await;
    assert!(outline_named(&symbols, "p").is_some(), "{symbols}");
    let current = wire.analysis_context(&uri).await;
    assert_eq!(
        wire.select_analysis(&uri, &current, json!({})).await["accepted"],
        true
    );
    assert_eq!(fs::read_to_string(files.0.join("root.mod")).unwrap(), text);
}

// Guards excluded open helpers acquiring standalone errors, configured saved
// reports reusing Source diagnostics, and workspace totals counting profiles.
#[tokio::test]
async fn configured_saved_checks_and_dormant_helpers_share_root_context_over_stdio() {
    let files = Files::new();
    let text = "@#ifndef ON\n@#define ON=1\n@#endif\n@#if ON\n@#include \"helper.inc\"\n@#else\nvar y; model; y=0; end;\n@#endif\n";
    let helper = "@#define supplied=ON\nvar x; model; x=unknown; end;\n";
    files.write("root.mod", text);
    files.write("helper.inc", helper);
    let uri = files.uri("root.mod");
    let h = files.uri("helper.inc");
    let mut wire = Wire::new();
    wire.start_analysis(&files, true).await;
    wire.complete().await;
    wire.open(&uri, text, 1);
    wire.open(&h, helper, 1);
    assert!(has_code(&wire.pull(&h).await, "E020"));
    let proof = wire.analysis_context(&uri).await;
    let selected = wire
        .select_analysis(
            &uri,
            &proof,
            json!({"name":"Without helper","defines":[{"name":"ON","expression":"0"}]}),
        )
        .await;
    assert_eq!(selected["accepted"], true, "{selected}");
    let info = wire
        .command_args("dynare/modelInfo", json!({"root_uri":uri,"document_uri":h}))
        .await;
    assert_eq!(info["source_browsing"], true, "{info}");
    let notes = wire.pull(&h).await;
    assert!(
        !has_code(&notes, "E020") && !has_code(&notes, "E063"),
        "{notes:?}"
    );
    wire.close(&h);
    wire.close(&uri);
    let status = wire.complete().await;
    assert_eq!(status["non_default_roots"], 1, "{status}");
    assert_eq!(root(&status, &uri)["errors"], 0, "{status}");
    let proof = wire.analysis_context(&uri).await;
    let selected = wire
        .select_analysis(
            &uri,
            &proof,
            json!({"name":"With helper","defines":[{"name":"ON","expression":"1"}]}),
        )
        .await;
    assert_eq!(selected["accepted"], true, "{selected}");
    let status = wire.complete().await;
    assert!(
        root(&status, &uri)["errors"].as_u64().unwrap() > 0,
        "{status}"
    );
    assert!(
        wire.pull(&h)
            .await
            .iter()
            .filter(|row| row["code"] == "E020")
            .all(|row| row["message"].as_str().unwrap().contains("With helper"))
    );
}

// Guards phantom standalone macro errors and workspace contributions leaking into
// a helper's selected editor context, including unsaved and reopened inputs.
#[tokio::test]
async fn mod_helper_context_routes_real_macro_cause_and_keeps_workspace_inventory() {
    for workspace in [false, true] {
        let files = Files::new();
        let parent = "@#define supplied = 1\n@#include \"helper.mod\"\n";
        let helper = "@#define rejected = supplied + \"text\"\n";
        files.write("parent.mod", parent);
        files.write("helper.mod", helper);
        let (p, h) = (files.uri("parent.mod"), files.uri("helper.mod"));
        let mut wire = Wire::new();
        wire.start_context(&files, workspace).await;
        if workspace {
            wire.complete().await;
        }
        wire.open(&p, parent, 1);
        wire.open(&h, helper, 1);
        let notes = wire.pull(&h).await;
        assert!(has_code(&notes, "E285"), "{notes:?}");
        assert!(
            !has_code(&notes, "E063"),
            "standalone error leaked: {notes:?}"
        );
        assert_eq!(
            notes.iter().find(|note| note["code"] == "E285").unwrap()["range"]["start"]["line"],
            0
        );
        let before = if workspace {
            Some(wire.complete().await)
        } else {
            None
        };
        let standalone = wire.choose_context(&h, 1, &h).await;
        assert!(standalone.get("error").is_none(), "{standalone}");
        let notes = wire.pull(&h).await;
        assert!(has_code(&notes, "E063"), "{notes:?}");
        assert!(
            !has_code(&notes, "E285"),
            "owner error leaked into standalone: {notes:?}"
        );
        if let Some(before) = before {
            let after = wire.complete().await;
            assert_eq!(
                after["roots"], before["roots"],
                "editor choice changed saved-model reports"
            );
            assert_eq!(after["roots"].as_array().unwrap().len(), 2);
        }
        let stale = wire
            .command_args(
                "dynare/setDocumentContext",
                json!({"document_uri":h,"document_version":0,"root_uri":null}),
            )
            .await;
        assert_eq!(stale["code"], "STALE_DOCUMENT");
        assert!(has_code(&wire.pull(&h).await, "E063"));
        let selected = wire.choose_context(&h, 1, &p).await;
        assert!(selected.get("error").is_none(), "{selected}");
        assert!(!has_code(&wire.pull(&h).await, "E063"));
        let old = wire
            .command_args("dynare/modelInfo", json!({"root_uri":p,"document_uri":h}))
            .await;
        wire.change(&p, "@#define supplied = 2\n@#include \"helper.mod\"\n", 2);
        let stale = wire.command_args("dynare/setDocumentContext", json!({"document_uri":h,"document_version":1,"root_uri":p,"expected_revision":old["revision"]})).await;
        assert_eq!(stale["code"], "STALE_MODEL");
        wire.change(&h, "@#define accepted = supplied + 1\n", 2);
        let notes = wire.pull(&h).await;
        assert!(
            !has_code(&notes, "E063") && !has_code(&notes, "E285"),
            "{notes:?}"
        );
        wire.change(&p, "@#define supplied = 2\n", 3);
        assert!(
            has_code(&wire.pull(&h).await, "E063"),
            "removed owner must restore ordinary resolution (workspace={workspace})"
        );
        wire.change(&p, parent, 4);
        wire.choose_context(&h, 2, &h).await;
        assert!(has_code(&wire.pull(&h).await, "E063"));
        wire.close(&h);
        wire.open(&h, helper, 3);
        let notes = wire.pull(&h).await;
        assert!(
            has_code(&notes, "E285") && !has_code(&notes, "E063"),
            "reopen retained stale standalone mode: {notes:?}"
        );
    }
}

// Guards one selected semantic context while Rename still validates all owners
// of the written bytes it would change.
#[tokio::test]
async fn ambiguous_context_waits_for_choice_and_navigation_follows_it_without_weakening_rename() {
    let files = Files::new();
    let parent = "var y; parameters p;\n@#include \"helper.mod\"\n";
    let helper = "model;\ny = p;\nend;\n";
    for name in ["a.mod", "b.mod", "unrelated.mod"] {
        files.write(
            name,
            parent
                .replace(
                    "helper.mod",
                    if name == "unrelated.mod" {
                        "absent.inc"
                    } else {
                        "helper.mod"
                    },
                )
                .as_str(),
        );
    }
    files.write("helper.mod", helper);
    let (a, b, h) = (
        files.uri("a.mod"),
        files.uri("b.mod"),
        files.uri("helper.mod"),
    );
    let mut wire = Wire::new();
    wire.start_context(&files, false).await;
    wire.open(&a, parent, 1);
    wire.open(&b, parent, 1);
    wire.open(&h, helper, 1);
    let position = json!({"textDocument":{"uri":h},"position":{"line":1,"character":4}});
    assert!(
        wire.pull(&h).await.is_empty(),
        "ambiguous context must not choose standalone or a parent"
    );
    assert!(
        wire.request("textDocument/definition", position.clone())
            .await
            .is_null()
    );
    for owner in [&a, &b] {
        let selected = wire.choose_context(&h, 1, owner).await;
        assert!(selected.get("error").is_none(), "{selected}");
        let definition = wire
            .request("textDocument/definition", position.clone())
            .await;
        assert_eq!(definition["uri"], *owner, "{definition}");
        let references = wire.request("textDocument/references", json!({"textDocument":{"uri":h},"position":{"line":1,"character":4},"context":{"includeDeclaration":true}})).await;
        assert!(
            references
                .as_array()
                .unwrap()
                .iter()
                .any(|site| site["uri"] == *owner)
        );
        assert!(
            !references
                .as_array()
                .unwrap()
                .iter()
                .any(|site| site["uri"] == *if owner == &a { &b } else { &a })
        );
    }
    let rename = wire.command_args("dynare/captureRename", json!({"textDocument":{"uri":h},"position":{"line":1,"character":4},"newName":"changed"})).await;
    assert_eq!(rename["code"], "RENAME_REFUSED", "{rename}");
    let unrelated = files.uri("unrelated.mod");
    let proof = wire
        .command_args("dynare/modelInfo", json!({"root_uri":unrelated}))
        .await;
    let unproven = wire.command_args("dynare/setDocumentContext", json!({"document_uri":h,"document_version":1,"root_uri":unrelated,"expected_revision":proof["revision"]})).await;
    assert_eq!(unproven["code"], "DOCUMENT_NOT_OWNED");
    assert_eq!(
        wire.request("textDocument/definition", position.clone())
            .await["uri"],
        b
    );
    let cleared = wire
        .command_args(
            "dynare/setDocumentContext",
            json!({"document_uri":h,"document_version":1,"root_uri":null}),
        )
        .await;
    assert!(cleared.get("error").is_none());
    assert!(
        wire.request("textDocument/definition", position)
            .await
            .is_null()
    );
}

// Guards the .mod helper fix for ordinary LSP clients which have no owner picker,
// and rejects an explicit context command unless its capability was negotiated.
#[tokio::test]
async fn legacy_client_uses_unique_owner_without_enabling_explicit_context_commands() {
    let files = Files::new();
    let parent = "@#define supplied = 1\n@#include \"helper.mod\"\n";
    let helper = "@#define rejected = supplied + \"text\"\n";
    files.write("parent.mod", parent);
    files.write("helper.mod", helper);
    let (p, h) = (files.uri("parent.mod"), files.uri("helper.mod"));
    let mut wire = Wire::new();
    wire.start(
        vec![files.folder("")],
        json!({"workspaceDiagnostics":false}),
        vec![],
        false,
    )
    .await;
    wire.open(&p, parent, 1);
    wire.open(&h, helper, 1);
    let notes = wire.pull(&h).await;
    assert!(
        has_code(&notes, "E285") && !has_code(&notes, "E063"),
        "{notes:?}"
    );
    let result = wire
        .command_args(
            "dynare/setDocumentContext",
            json!({"document_uri":h,"document_version":1,"root_uri":null}),
        )
        .await;
    assert_eq!(result["code"], "UNSUPPORTED_CAPABILITY");
    assert!(has_code(&wire.pull(&h).await, "E285"));
}

// Guards collision checking in the displayed standalone helper as well as every
// parent which would consume the same edited declaration.
#[tokio::test]
async fn standalone_helper_rename_checks_its_own_macro_branch_before_all_other_owners() {
    let files = Files::new();
    let parent = "@#define inside = 1\n@#include \"helper.mod\"\n";
    let helper = "@#ifndef inside\nparameters clash;\n@#endif\nparameters p;\n";
    files.write("parent.mod", parent);
    files.write("helper.mod", helper);
    let (p, h) = (files.uri("parent.mod"), files.uri("helper.mod"));
    let mut wire = Wire::new();
    wire.start_context(&files, false).await;
    wire.open(&p, parent, 1);
    wire.open(&h, helper, 1);
    let selected = wire.choose_context(&h, 1, &h).await;
    assert!(selected.get("error").is_none(), "{selected}");
    let rename = wire.command_args("dynare/captureRename", json!({"textDocument":{"uri":h},"position":{"line":3,"character":11},"newName":"clash"})).await;
    assert_eq!(rename["code"], "RENAME_REFUSED", "{rename}");
    assert!(
        rename["error"]
            .as_str()
            .unwrap()
            .contains("already declared"),
        "{rename}"
    );
    let safe = wire.command_args("dynare/captureRename", json!({"textDocument":{"uri":h},"position":{"line":3,"character":11},"newName":"renamed"})).await;
    assert!(safe.get("error").is_none(), "{safe}");
    assert_eq!(
        safe["owners"].as_array().unwrap().len(),
        2,
        "the standalone view must retain the parent safety audit"
    );
}

// Guards same-refresh fallback when a closed saved owner removes its include.
#[tokio::test]
async fn a_changed_closed_owner_rechecks_the_open_helpers_standalone_fallback_immediately() {
    let files = Files::new();
    let parent = "@#define supplied = 1\n@#include \"helper.mod\"\n";
    let helper = "@#define accepted = supplied + 1\n";
    files.write("parent.mod", parent);
    files.write("helper.mod", helper);
    let (p, h) = (files.uri("parent.mod"), files.uri("helper.mod"));
    let mut wire = Wire::new();
    wire.start_context(&files, false).await;
    wire.open(&p, parent, 1);
    wire.open(&h, helper, 1);
    assert!(!has_code(&wire.pull(&h).await, "E063"));
    wire.close(&p);
    assert!(!has_code(&wire.pull(&h).await, "E063"));
    files.write("parent.mod", "@#define supplied = 1\n");
    wire.changed(&p, 2);
    let notes = wire.pull(&h).await;
    assert!(
        has_code(&notes, "E063"),
        "the helper fallback was not checked in the watched-file refresh: {notes:?}"
    );
    let info = wire
        .command_args("dynare/modelInfo", json!({"root_uri":h}))
        .await;
    assert_eq!(info["owner_roots"], json!([]));
}

// Guards selected helper context and standalone diagnostics after a background
// saved-root check removes an include, without a watched-file refresh.
#[tokio::test]
async fn workspace_recheck_removes_closed_owner_and_restores_open_helper_fallback() {
    let files = Files::new();
    let parent = "@#define supplied = 1\n@#include \"helper.inc\"\n";
    let helper = "@#define accepted = supplied + 1\n";
    files.write("parent.mod", parent);
    files.write("helper.inc", helper);
    let (p, h) = (files.uri("parent.mod"), files.uri("helper.inc"));
    let mut wire = Wire::new();
    wire.start_context(&files, true).await;
    wire.complete().await;
    wire.open(&h, helper, 1);
    let selected = wire.choose_context(&h, 1, &p).await;
    assert!(selected.get("error").is_none(), "{selected}");
    assert_eq!(selected["root_uris"], json!([p]));
    assert!(!has_code(&wire.pull(&h).await, "E063"));
    wire.complete().await;

    // The owner stays closed. Recheck must observe disk and repair the editor
    // context through background adoption, without a didChangeWatchedFiles.
    files.write("parent.mod", "@#define supplied = 1\n");
    wire.command("dynare/recheckWorkspace").await;
    wire.complete().await;
    let notes = wire.pull(&h).await;
    assert!(
        has_code(&notes, "E063"),
        "background owner removal did not restore standalone helper checks: {notes:?}"
    );
    let info = wire
        .command_args("dynare/modelInfo", json!({"root_uri":h}))
        .await;
    assert_eq!(info["owner_roots"], json!([]));
}

fn has_code(items: &[Value], code: &str) -> bool {
    items.iter().any(|diag| diag["code"] == code)
}

fn first_checking(statuses: &[Value]) -> Option<&str> {
    statuses
        .iter()
        .filter(|message| message["method"] == "dynare/workspaceStatusChanged")
        .flat_map(|message| message["params"]["roots"].as_array().into_iter().flatten())
        .find(|entry| entry["state"] == "checking")
        .and_then(|entry| entry["root_uri"].as_str())
}

#[tokio::test]
async fn configured_work_reaches_inactive_checks_and_formatting_over_stdio() {
    // Guards editor diagnostics and both format workers replaying macros with
    // the default after the selected setting already completed the model.
    let files = Files::new();
    let text = format!(
        "@#define big=\"{}\"\n@#for i in 1:400\n@#define scratch=big\n@#endfor\n@#if 0\nmodel;\n    y=2;\nend;\nvar é;\n@#endif\nvar y;\nmodel;\ny=0;\nend;\n",
        "a".repeat(5_000)
    );
    let uri = files.uri("root.mod");
    let mut wire = Wire::new();
    wire.start(vec![], json!({"macroWorkLimit":10000000}), vec![], false)
        .await;
    wire.open(&uri, &text, 1);
    let diagnostics = wire.pull(&uri).await;
    assert!(
        !has_code(&diagnostics, "E001") && !has_code(&diagnostics, "I211"),
        "{diagnostics:?}"
    );
    let formatted = wire
        .request(
            "textDocument/formatting",
            json!({
                "textDocument":{"uri":uri},"options":{"tabSize":4,"insertSpaces":false}
            }),
        )
        .await;
    assert!(
        formatted[0]["newText"]
            .as_str()
            .unwrap()
            .contains("model;\n    y=2;\nend;")
    );
    let range = wire
        .request(
            "textDocument/rangeFormatting",
            json!({
                "textDocument":{"uri":uri},"options":{"tabSize":4,"insertSpaces":false},
                "range":{"start":{"line":5,"character":0},"end":{"line":9,"character":0}}
            }),
        )
        .await;
    assert!(
        range.is_null(),
        "inactive source must stay unchanged: {range}"
    );
}

#[tokio::test]
async fn scoped_macro_work_limits_recheck_open_and_saved_models_over_stdio() {
    // Guards workspace workers using a default or another root's setting, cached
    // facts surviving a work-limit change, and invalid editor values going quiet.
    // The complete case includes retained availability for 200 interpolations;
    // leave room for the absolute temporary path in each recorded occurrence.
    const HIGH_WORK_LIMIT: u64 = 250_000;
    let files = Files::new();
    let text = "@#for i in 1:100\nvar y@{i};\n@#endfor\nmodel;\n@#for i in 1:100\ny@{i}=0;\n@#endfor\nend;\n";
    files.write("a/root.mod", text);
    files.write("b/root.mod", text);
    let a = files.uri("a/root.mod");
    let b = files.uri("b/root.mod");
    let scoped = |a_limit: Value, b_limit: Value| {
        vec![
            json!({"uri":files.uri("a"),"settings":{"macroWorkLimit":a_limit}}),
            json!({"uri":files.uri("b"),"settings":{"macroWorkLimit":b_limit}}),
        ]
    };
    let mut wire = Wire::new();
    wire.start(
        vec![files.folder("a"), files.folder("b")],
        json!({}),
        scoped(json!(1000), json!(HIGH_WORK_LIMIT)),
        true,
    )
    .await;
    let status = wire.complete().await;
    assert_eq!(root(&status, &a)["state"], "incomplete", "{status}");
    assert_eq!(root(&status, &b)["state"], "checked", "{status}");
    wire.open(&a, text, 1);
    assert!(has_code(&wire.pull(&a).await, "I211"));
    let info_args =
        |uri: &str| json!({"command":"dynare/modelInfo","arguments":[{"root_uri":uri}]});
    let before = wire
        .request("workspace/executeCommand", info_args(&a))
        .await;
    assert_eq!(before["complete"], false, "{before}");
    let source_args = json!({"command":"dynare/showEffectiveModel","arguments":[{"root_uri":a,"layout":"source"}]});
    let source_stopped = wire
        .request("workspace/executeCommand", source_args.clone())
        .await;
    assert_eq!(source_stopped["complete"], false);
    assert!(
        source_stopped["incomplete_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason["limit"]["limit"] == "iteration work"
                && reason["limit"]["ceiling"] == 1000),
        "{source_stopped}"
    );
    let snapshot = |folders: Vec<Value>| json!({"settings":{"dynare":{"configuration":{"schemaVersion":1,"loose":{},"folders":folders}}}});
    wire.notify(
        "workspace/didChangeConfiguration",
        snapshot(scoped(json!(HIGH_WORK_LIMIT), json!(1000))),
    );
    let after = wire
        .request("workspace/executeCommand", info_args(&a))
        .await;
    assert_eq!(after["complete"], true, "{after}");
    assert_eq!(after["n_equations"], 100, "{after}");
    let source_complete = wire.request("workspace/executeCommand", source_args).await;
    assert_eq!(source_complete["complete"], true, "{source_complete}");
    assert_ne!(before["revision"], after["revision"]);
    assert!(!has_code(&wire.pull(&a).await, "I211"));
    let status = wire.complete().await;
    assert_eq!(root(&status, &a)["state"], "checked", "{status}");
    assert_eq!(root(&status, &b)["state"], "incomplete", "{status}");
    wire.notify(
        "workspace/didChangeConfiguration",
        snapshot(scoped(json!(0), json!(HIGH_WORK_LIMIT))),
    );
    let fallback = wire
        .request("workspace/executeCommand", info_args(&a))
        .await;
    assert_eq!(fallback["complete"], true, "{fallback}");
    wire.complete().await;
    assert!(
        wire.seen
            .iter()
            .any(|message| message["method"] == "window/logMessage"
                && message["params"]["message"]
                    .as_str()
                    .is_some_and(|text| text.contains("Invalid dynare.macroWorkLimit")
                        && text.contains("default"))),
        "invalid setting must explain its fallback"
    );
}

#[tokio::test]
async fn folder_only_cli_discovery_checked_errors_and_identical_push_pull() {
    let files = Files::new();
    files.write("good.mod", MODEL);
    files.write("sub/bad.mod", "var y; model; y=unknown; end;\n");
    files.write("+generated/skip.mod", "broken");
    files.write("ordinary.dyn", MODEL);
    files.write("includes/unused.inc", "parameters unused;\n");
    files.write("+generated/skip.inc", "broken");
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], true)
        .await;
    let status = wire.complete().await;
    assert_eq!(status["counts"]["checked"], 3);
    assert_eq!(status["coverage_complete"], true);
    assert!(
        root(&status, &files.uri("sub/bad.mod"))["errors"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(
        status["include_files"],
        json!([files.uri("includes/unused.inc")])
    );
    assert_eq!(
        root(&status, &files.uri("ordinary.dyn"))["state"],
        "checked"
    );
    let cli = dygnosis::check_walk::collect_model_files(&files.0).unwrap();
    assert_eq!(status["roots"].as_array().unwrap().len(), cli.len());
    for path in cli {
        let uri = Url::from_file_path(&path).unwrap().to_string();
        let text = fs::read_to_string(&path).unwrap();
        let expected = dygnosis::check_file_with_origins(&text, &path);
        let pulled = wire.pull(&uri).await;
        let expected_codes: Vec<_> = expected
            .diagnostics
            .iter()
            .map(|diag| diag.code.as_str())
            .collect();
        let actual_codes: Vec<_> = pulled
            .iter()
            .map(|diag| diag["code"].as_str().unwrap())
            .collect();
        assert_eq!(actual_codes, expected_codes);
        let pushed = wire
            .seen
            .iter()
            .rev()
            .find(|item| {
                item["method"] == "textDocument/publishDiagnostics" && item["params"]["uri"] == uri
            })
            .unwrap();
        assert_eq!(pushed["params"]["diagnostics"], json!(pulled));
        let workspace = wire
            .request("workspace/diagnostic", json!({"previousResultIds":[]}))
            .await;
        let report = workspace["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["uri"] == uri)
            .unwrap();
        assert_eq!(report["items"], json!(pulled));
    }
    assert!(
        wire.seen
            .iter()
            .any(|message| message["method"] == "dynare/workspaceStatusChanged")
    );
    wire.open(&files.uri("ordinary.dyn"), "var y; model; y=x; end;", 1);
    assert!(has_code(
        &wire.pull(&files.uri("ordinary.dyn")).await,
        "E020"
    ));
}

#[tokio::test]
async fn include_inventory_and_dyn_roots_follow_saved_file_lifetimes() {
    let files = Files::new();
    files.write("root.mod", MODEL);
    files.write("nested/variant.DYN", MODEL);
    files.write("includes/shared.INC", "parameters p;\n");
    let mut wire = Wire::new();
    wire.start(
        vec![files.folder(""), files.folder("includes")],
        json!({}),
        vec![json!({"uri":files.uri(""),"settings":{"workspaceExcludePaths":["excluded/**"]}})],
        true,
    )
    .await;
    let status = wire.complete().await;
    assert_eq!(status["counts"]["checked"], 2);
    assert_eq!(
        status["include_files"],
        json!([files.uri("includes/shared.INC")])
    );

    files.write("excluded/new.dyn", MODEL);
    files.write("excluded/unreferenced.inc", "parameters q;\n");
    wire.changed(&files.uri("excluded/new.dyn"), 1);
    wire.changed(&files.uri("excluded/unreferenced.inc"), 1);
    let status = wire.complete().await;
    assert_eq!(
        root(&status, &files.uri("excluded/new.dyn"))["state"],
        "excluded"
    );
    assert_eq!(status["counts"]["checked"], 2);
    assert_eq!(status["include_files"].as_array().unwrap().len(), 2);
    assert_eq!(status["roots"].as_array().unwrap().len(), 3);

    fs::rename(
        files.0.join("includes/shared.INC"),
        files.0.join("includes/renamed.inc"),
    )
    .unwrap();
    wire.changed(&files.uri("includes/shared.INC"), 3);
    wire.changed(&files.uri("includes/renamed.inc"), 1);
    let status = wire.complete().await;
    assert_eq!(
        status["include_files"],
        json!([
            files.uri("excluded/unreferenced.inc"),
            files.uri("includes/renamed.inc")
        ])
    );

    fs::remove_file(files.0.join("nested/variant.DYN")).unwrap();
    fs::remove_file(files.0.join("includes/renamed.inc")).unwrap();
    wire.changed(&files.uri("nested/variant.DYN"), 3);
    wire.changed(&files.uri("includes/renamed.inc"), 3);
    let status = wire.complete().await;
    assert_eq!(status["counts"]["checked"], 1);
    assert_eq!(status["roots"].as_array().unwrap().len(), 2);
    assert_eq!(
        status["include_files"],
        json!([files.uri("excluded/unreferenced.inc")])
    );
}

#[tokio::test]
async fn scoped_exclusions_open_roots_and_removal_clear_only_one_owner() {
    let files = Files::new();
    files.write("a/skip.mod", "var y; model; y=x; end;");
    files.write("b/skip.mod", "var z; model; z=x; end;");
    files.write("common.data", "parameters p;\n");
    for (folder, name) in [("a", "y"), ("b", "z")] {
        files.write(
            &format!("{folder}/root.mod"),
            &format!("@#include \"../common.data\"\nvar {name}; model; {name}=0; end;\n"),
        );
    }
    let mut wire = Wire::new();
    wire.start(
        vec![files.folder("a"), files.folder("b")],
        json!({}),
        vec![json!({"uri":files.uri("a"),"settings":{"workspaceExcludePaths":["skip.mod"]}})],
        false,
    )
    .await;
    let status = wire.complete().await;
    assert_eq!(root(&status, &files.uri("a/skip.mod"))["state"], "excluded");
    assert_eq!(root(&status, &files.uri("b/skip.mod"))["state"], "checked");
    assert!(wire.pull(&files.uri("a/skip.mod")).await.is_empty());
    assert!(has_code(
        &wire.pull(&files.uri("common.data")).await,
        "W022"
    ));
    wire.open(&files.uri("a/skip.mod"), "var y; model; y=x; end;", 1);
    assert!(has_code(&wire.pull(&files.uri("a/skip.mod")).await, "E020"));
    wire.notify(
        "workspace/didChangeWorkspaceFolders",
        json!({"event":{"added":[],"removed":[files.folder("a")]}}),
    );
    wire.complete().await;
    assert!(has_code(
        &wire.pull(&files.uri("common.data")).await,
        "W022"
    ));
    assert!(has_code(&wire.pull(&files.uri("a/skip.mod")).await, "E020"));
    wire.notify(
        "workspace/didChangeConfiguration",
        json!({"settings":{"dynare":{"workspaceDiagnostics":false}}}),
    );
    let status = wire.command("dynare/workspaceStatus").await;
    assert_eq!(status["enabled"], false);
    assert!(wire.pull(&files.uri("common.data")).await.is_empty());
    assert!(has_code(&wire.pull(&files.uri("a/skip.mod")).await, "E020"));
}

#[tokio::test]
async fn shared_arbitrary_include_overlays_close_to_disk_and_single_report_owner() {
    let files = Files::new();
    files.write("equations.data", "y=0;\n");
    for name in ["a.mod", "b.mod"] {
        files.write(
            name,
            "var y; model;\n@#include \"equations.data\"\nend; initval; y=0; end;\n",
        );
    }
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    let before = wire.complete().await;
    let a_before = root(&before, &files.uri("a.mod"))["revision"].clone();
    let b_before = root(&before, &files.uri("b.mod"))["revision"].clone();
    wire.open(&files.uri("equations.data"), "y=missing;\n", 1);
    let changed = wire.complete().await;
    assert_ne!(root(&changed, &files.uri("a.mod"))["revision"], a_before);
    assert_ne!(root(&changed, &files.uri("b.mod"))["revision"], b_before);
    let items = wire.pull(&files.uri("equations.data")).await;
    assert_eq!(
        items.iter().filter(|diag| diag["code"] == "E020").count(),
        1,
        "shared roots merge identical diagnostics"
    );
    wire.close(&files.uri("equations.data"));
    let after = wire.complete().await;
    assert_eq!(root(&after, &files.uri("a.mod"))["revision"], a_before);
    assert!(!has_code(
        &wire.pull(&files.uri("equations.data")).await,
        "E020"
    ));
    wire.open(&files.uri("a.mod"), "var y; model; y=missing; end;", 1);
    wire.complete().await;
    assert!(has_code(&wire.pull(&files.uri("a.mod")).await, "E020"));
    wire.close(&files.uri("a.mod"));
    wire.complete().await;
    assert!(!has_code(&wire.pull(&files.uri("a.mod")).await, "E020"));
}

#[tokio::test]
async fn missing_search_candidates_companion_creation_and_incomplete_coverage() {
    let files = Files::new();
    files.write("search/.keep", "");
    files.write(
        "root.mod",
        "@#include \"values.anything\"\nvar y; model; y=p; end; steady;\n",
    );
    let mut wire = Wire::new();
    wire.start(
        vec![files.folder("")],
        json!({"searchPaths":["search"]}),
        vec![],
        false,
    )
    .await;
    let missing = wire.complete().await;
    assert_eq!(
        root(&missing, &files.uri("root.mod"))["state"],
        "incomplete"
    );
    assert_eq!(missing["coverage_complete"], false);
    let candidates = root(&missing, &files.uri("root.mod"))["dependency_candidates"]
        .as_array()
        .unwrap();
    let candidates: Vec<_> = candidates
        .iter()
        .map(|uri| dygnosis::include_resolver::normalize_uri(uri.as_str().unwrap()))
        .collect();
    assert!(
        candidates.contains(&dygnosis::include_resolver::normalize_uri(
            &files.uri("search/values.anything")
        ))
    );
    assert!(
        candidates.contains(&dygnosis::include_resolver::normalize_uri(
            &files.uri("root_steadystate.m")
        ))
    );
    files.write("search/values.anything", "parameters p; p=0;\n");
    wire.changed(&files.uri("search/values.anything"), 1);
    let found = wire.complete().await;
    assert_eq!(root(&found, &files.uri("root.mod"))["state"], "checked");
    let revision = root(&found, &files.uri("root.mod"))["revision"].clone();
    files.write(
        "root_steadystate.m",
        "function [ys,params,check]=root_steadystate(ys,exo,M_,options_)\ncheck=0;\nend\n",
    );
    wire.changed(&files.uri("root_steadystate.m"), 1);
    let companion = wire.complete().await;
    assert_ne!(
        root(&companion, &files.uri("root.mod"))["revision"],
        revision
    );
    fs::remove_file(files.0.join("root_steadystate.m")).unwrap();
    wire.changed(&files.uri("root_steadystate.m"), 3);
    assert_eq!(
        root(&wire.complete().await, &files.uri("root.mod"))["revision"],
        revision
    );
}

#[tokio::test]
async fn cancel_recheck_edit_resume_and_off_reject_queued_reports() {
    let files = Files::new();
    files.write("root.mod", MODEL);
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    wire.complete().await;
    let cancelled = wire.command("dynare/cancelWorkspace").await;
    assert_eq!(cancelled["cancelled"], true);
    files.write("new.mod", "var y; model; y=x; end;");
    assert_eq!(
        wire.command("dynare/workspaceStatus").await["roots"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    wire.command("dynare/recheckWorkspace").await;
    let rescanned = wire.complete().await;
    assert_eq!(rescanned["roots"].as_array().unwrap().len(), 2);
    wire.command("dynare/cancelWorkspace").await;
    files.write("root.mod", "var y; model; y=x; end;");
    wire.changed(&files.uri("root.mod"), 2);
    assert_eq!(wire.complete().await["cancelled"], false);
    assert!(has_code(&wire.pull(&files.uri("root.mod")).await, "E020"));
    wire.changed(&files.uri("root.mod"), 2);
    wire.notify(
        "workspace/didChangeConfiguration",
        json!({"settings":{"dynare":{"workspaceDiagnostics":false}}}),
    );
    wire.command("dynare/workspaceStatus").await;
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert!(wire.pull(&files.uri("root.mod")).await.is_empty());
    assert_eq!(
        wire.command("dynare/recheckWorkspace").await["enabled"],
        false
    );
}

#[tokio::test]
async fn monotonic_document_versions_coalescing_root_deletion_and_rename() {
    let files = Files::new();
    files.write("root.mod", MODEL);
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    wire.complete().await;
    wire.open(&files.uri("root.mod"), MODEL, 1);
    wire.change(&files.uri("root.mod"), "var y; model; y=x; end;", 3);
    wire.change(&files.uri("root.mod"), MODEL, 2);
    assert!(has_code(&wire.pull(&files.uri("root.mod")).await, "E020"));
    wire.change(&files.uri("root.mod"), MODEL, 4);
    wire.change(&files.uri("root.mod"), MODEL, 5);
    let coalesced = wire.complete().await;
    assert_eq!(coalesced["metrics"]["completed_jobs"], 1);
    assert!(!has_code(&wire.pull(&files.uri("root.mod")).await, "E020"));
    wire.close(&files.uri("root.mod"));
    wire.complete().await;
    fs::rename(files.0.join("root.mod"), files.0.join("renamed.mod")).unwrap();
    wire.changed(&files.uri("root.mod"), 3);
    wire.changed(&files.uri("renamed.mod"), 1);
    let renamed = wire.complete().await;
    assert_eq!(renamed["roots"].as_array().unwrap().len(), 1);
    assert_eq!(
        root(&renamed, &files.uri("renamed.mod"))["state"],
        "checked"
    );
    assert!(wire.pull(&files.uri("root.mod")).await.is_empty());
}

#[tokio::test]
async fn failed_folder_walk_retains_include_inventory_until_folder_removal() {
    let files = Files::new();
    files.write("a/retained.inc", "// retained\n");
    files.write("b/current.inc", "// current\n");
    let mut wire = Wire::new();
    wire.start(
        vec![files.folder("a"), files.folder("b")],
        json!({}),
        vec![],
        true,
    )
    .await;
    let before = wire.complete().await;
    fs::rename(files.0.join("a"), files.0.join("unavailable-a")).unwrap();
    wire.command("dynare/recheckWorkspace").await;
    let failed = wire.complete().await;
    assert_eq!(failed["include_files"], before["include_files"]);
    assert_eq!(failed["coverage_complete"], false);
    assert_eq!(
        failed["discovery_failures"][0]["folder_uri"],
        files.uri("a")
    );
    assert!(
        failed["discovery_failures"][0]["failure"]
            .as_str()
            .unwrap()
            .contains("include files")
    );
    assert!(failed["roots"].as_array().unwrap().is_empty());

    wire.notify(
        "workspace/didChangeWorkspaceFolders",
        json!({"event":{"added":[],"removed":[files.folder("a")]}}),
    );
    let removed = wire.complete().await;
    assert_eq!(
        removed["include_files"],
        json!([files.uri("b/current.inc")])
    );
    assert_eq!(removed["coverage_complete"], true);
}

#[tokio::test]
async fn failed_discovery_is_not_complete_coverage_and_notifications_are_opt_in() {
    let files = Files::new();
    let mut wire = Wire::new();
    wire.start(vec![files.folder("absent")], json!({}), vec![], false)
        .await;
    let status = wire.complete().await;
    assert_eq!(status["coverage_complete"], false);
    assert_eq!(status["discovery_failures"].as_array().unwrap().len(), 1);
    assert!(
        !wire
            .seen
            .iter()
            .any(|message| message["method"] == "dynare/workspaceStatusChanged")
    );
}

#[tokio::test]
async fn chosen_active_root_is_first_and_checks_are_serial() {
    let files = Files::new();
    for name in ["a.mod", "b.mod", "chosen.mod"] {
        files.write(name, MODEL);
    }
    let mut wire = Wire::new();
    wire.request("initialize", json!({"workspaceFolders":[files.folder("")],"capabilities":{"experimental":{"dygnosis":{"workspaceStatusChanged":true}}}})).await;
    wire.notify(
        "dynare/activeModelChanged",
        json!({"root_uri":files.uri("chosen.mod")}),
    );
    wire.notify("initialized", json!({}));
    wire.complete().await;
    let statuses: Vec<_> = wire
        .seen
        .iter()
        .filter(|message| message["method"] == "dynare/workspaceStatusChanged")
        .map(|message| &message["params"])
        .collect();
    let checking: Vec<_> = statuses
        .iter()
        .filter(|status| status["counts"]["checking"] == 1)
        .collect();
    assert!(!checking.is_empty());
    assert_eq!(
        root(checking[0], &files.uri("chosen.mod"))["state"],
        "checking"
    );
    assert!(
        statuses
            .iter()
            .all(|status| status["counts"]["checking"].as_u64().unwrap() <= 1)
    );
}

#[tokio::test]
async fn unopened_root_fixes_retain_context_and_open_document_versions() {
    let files = Files::new();
    files.write("body.inc", "y=0;\n");
    files.write(
        "root.mod",
        "var y; model;\n@#include \"body.inc\"\n[name='root'] y=1; end;\n",
    );
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    wire.complete().await;
    let notes = wire.pull(&files.uri("root.mod")).await;
    let note = notes
        .iter()
        .find(|diag| diag["code"] == "I208")
        .unwrap()
        .clone();
    assert_eq!(note["data"]["root"], files.uri("root.mod"));
    let actions = wire.request("textDocument/codeAction", json!({"textDocument":{"uri":files.uri("root.mod")},"range":note["range"],"context":{"diagnostics":[note]}})).await;
    let action = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action.get("edit").is_some())
        .unwrap();
    assert!(
        action["edit"]["documentChanges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edit| edit["textDocument"]["uri"] == files.uri("body.inc")
                && edit["textDocument"]["version"].is_null())
    );
    wire.open(&files.uri("body.inc"), "y=0;\n", 9);
    wire.complete().await;
    let note = wire
        .pull(&files.uri("root.mod"))
        .await
        .into_iter()
        .find(|diag| diag["code"] == "I208")
        .unwrap();
    let actions = wire.request("textDocument/codeAction", json!({"textDocument":{"uri":files.uri("root.mod")},"range":note["range"],"context":{"diagnostics":[note]}})).await;
    let action = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action.get("edit").is_some())
        .unwrap();
    assert!(
        action["edit"]["documentChanges"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edit| edit["textDocument"]["uri"] == files.uri("body.inc")
                && edit["textDocument"]["version"] == 9)
    );
}

#[tokio::test]
async fn workspace_pull_clears_removed_previous_reports_and_server_restart_rediscovers() {
    let files = Files::new();
    files.write("root.mod", "var y; model; y=x; end;");
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    wire.complete().await;
    wire.notify(
        "workspace/didChangeConfiguration",
        json!({"settings":{"dynare":{"workspaceDiagnostics":false}}}),
    );
    let reports = wire
        .request(
            "workspace/diagnostic",
            json!({"previousResultIds":[{"uri":files.uri("root.mod"),"value":"previous"}]}),
        )
        .await;
    assert_eq!(reports["items"][0]["items"], json!([]));
    drop(wire);
    let mut restarted = Wire::new();
    restarted
        .start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    assert_eq!(restarted.complete().await["counts"]["checked"], 1);
    assert!(has_code(
        &restarted.pull(&files.uri("root.mod")).await,
        "E020"
    ));
}

#[tokio::test]
async fn turning_off_during_a_running_check_cannot_republish_workspace_diagnostics() {
    let files = Files::new();
    let mut large = String::from("var y; model;\n");
    for _ in 0..18000 {
        large.push_str("y=unknown;\n");
    }
    large.push_str("end;\n");
    files.write("large.mod", &large);
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], true)
        .await;
    loop {
        let status = wire.command("dynare/workspaceStatus").await;
        if status["counts"]["checking"] == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    wire.notify(
        "workspace/didChangeConfiguration",
        json!({"settings":{"dynare":{"workspaceDiagnostics":false}}}),
    );
    assert_eq!(
        wire.command("dynare/workspaceStatus").await["enabled"],
        false
    );
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(wire.pull(&files.uri("large.mod")).await.is_empty());
    assert!(!wire.seen.iter().any(|message| {
        message["method"] == "textDocument/publishDiagnostics"
            && message["params"]["uri"] == files.uri("large.mod")
            && message["params"]["diagnostics"]
                .as_array()
                .is_some_and(|items| !items.is_empty())
    }));
}

#[tokio::test]
async fn directory_and_loader_reads_are_revision_inputs() {
    let files = Files::new();
    files.write("root.mod", "@#includepath \"missing\"\nvar y; model; y=0; end;\nload_params_and_steady_state('params.data');\n");
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    let before = wire.complete().await;
    let previous_revision = root(&before, &files.uri("root.mod"))["revision"].clone();
    let items = wire.pull(&files.uri("root.mod")).await;
    assert!(has_code(&items, "E304"));
    assert!(!has_code(&items, "E306"));
    fs::create_dir(files.0.join("missing")).unwrap();
    wire.changed(&files.uri("missing"), 1);
    let directory_ready = wire.complete().await;
    let directory_revision = root(&directory_ready, &files.uri("root.mod"))["revision"].clone();
    assert_ne!(directory_revision, previous_revision);
    let items = wire.pull(&files.uri("root.mod")).await;
    assert!(!has_code(&items, "E304"));
    assert!(has_code(&items, "E306"));
    files.write("params.data", "y 0\n");
    wire.changed(&files.uri("params.data"), 1);
    let after = wire.complete().await;
    assert_ne!(
        root(&after, &files.uri("root.mod"))["revision"],
        directory_revision
    );
    let items = wire.pull(&files.uri("root.mod")).await;
    assert!(!has_code(&items, "E304"));
    assert!(!has_code(&items, "E306"));
}

#[tokio::test]
async fn complete_presentation_snapshots_keep_checked_workspace_roots() {
    // Guards native Outline filters and other view settings starting another
    // workspace pass when the client sends a complete configuration snapshot.
    let files = Files::new();
    files.write("root.mod", MODEL);
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    let before = wire.complete().await;
    for settings in [
        json!({"outline":{"activity":"active"}}),
        json!({"outline":{"activity":"inactive"},"nameDetails":{"longName":false},"parameterValueHints":false,"formatIndent":2}),
    ] {
        wire.notify("workspace/didChangeConfiguration", json!({"settings":{"dynare":{"configuration":{"schemaVersion":1,"loose":settings,"folders":[]}}}}));
        let after = wire.complete().await;
        assert_eq!(after["pass_revision"], before["pass_revision"], "{after}");
        assert_eq!(after["roots"], before["roots"], "{after}");
    }
}

#[tokio::test]
async fn adding_workspace_folder_keeps_existing_checked_roots() {
    // Guards folder discovery forcing another check of unchanged old folders.
    let files = Files::new();
    files.write("a/root.mod", MODEL);
    files.write("b/root.mod", MODEL);
    let mut wire = Wire::new();
    wire.start(vec![files.folder("a")], json!({}), vec![], false)
        .await;
    let before = wire.complete().await;
    wire.notify(
        "workspace/didChangeWorkspaceFolders",
        json!({"event":{"added":[files.folder("b")],"removed":[]}}),
    );
    let after = wire.complete().await;
    assert_eq!(after["metrics"]["completed_jobs"], 1, "{after}");
    assert_eq!(
        root(&after, &files.uri("a/root.mod")),
        root(&before, &files.uri("a/root.mod"))
    );
    assert_eq!(root(&after, &files.uri("b/root.mod"))["state"], "checked");
}

#[tokio::test]
async fn scoped_analysis_settings_recheck_only_changed_roots() {
    // Guards full settings snapshots discarding unrelated folder provenance.
    let files = Files::new();
    files.write("a/root.mod", MODEL);
    files.write("b/root.mod", MODEL);
    let mut wire = Wire::new();
    wire.start(
        vec![files.folder("a"), files.folder("b")],
        json!({}),
        vec![],
        false,
    )
    .await;
    let before = wire.complete().await;
    wire.notify("workspace/didChangeConfiguration", json!({"settings":{"dynare":{"configuration":{"schemaVersion":1,"loose":{},"folders":[{"uri":files.uri("a"),"settings":{"macroWorkLimit":1000000}}]}}}}));
    let after = wire.complete().await;
    assert_eq!(after["metrics"]["completed_jobs"], 1, "{after}");
    assert_eq!(after["metrics"]["reused_jobs"], 0, "{after}");
    assert_ne!(
        root(&after, &files.uri("a/root.mod"))["revision"],
        root(&before, &files.uri("a/root.mod"))["revision"]
    );
    assert_eq!(
        root(&after, &files.uri("b/root.mod")),
        root(&before, &files.uri("b/root.mod"))
    );
}

#[tokio::test]
async fn unchanged_inputs_reuse_reports_but_explicit_recheck_computes_again() {
    let files = Files::new();
    files.write("root.mod", MODEL);
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    wire.complete().await;
    wire.changed(&files.uri("root.mod"), 2);
    let reused = wire.complete().await;
    assert_eq!(reused["metrics"]["completed_jobs"], 1);
    assert_eq!(reused["metrics"]["reused_jobs"], 1);
    wire.command("dynare/recheckWorkspace").await;
    let recomputed = wire.complete().await;
    assert_eq!(recomputed["metrics"]["completed_jobs"], 1);
    assert_eq!(recomputed["metrics"]["reused_jobs"], 0);
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn committed_dependency_candidates_wait_for_the_next_report() {
    let files = Files::new();
    files.write(
        "root.mod",
        "@#include \"observed/params.inc\"\nvar y; model; y=beta; end; initval; y=0; end;\n",
    );
    files.write("observed/params.inc", "parameters beta; beta=0.5;\n");
    files.write("replacement/params.inc", "parameters beta; beta=0.9;\n");
    let root_uri = files.uri("root.mod");
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    let before = wire.complete().await;
    let committed = root(&before, &root_uri).clone();
    assert!(
        committed["dependency_candidates"]
            .as_array()
            .unwrap()
            .contains(&json!(files.uri("observed/params.inc")))
    );

    // A status read must describe the committed report, even if a path now
    // resolves elsewhere. A new analysis must then observe that new identity.
    let observed = files.0.join("observed");
    let replacement = files.0.join("replacement");
    fs::rename(&observed, files.0.join("previous")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&replacement, &observed).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Directory junctions do not require symbolic-link privilege. Both
        // ends are inside this test's verified temporary directory.
        let result = Command::new("cmd.exe")
            .creation_flags(0x08000000)
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&observed)
            .arg(&replacement)
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
    }
    let unchanged = wire.command("dynare/workspaceStatus").await;
    assert_eq!(root(&unchanged, &root_uri), &committed);

    wire.command("dynare/recheckWorkspace").await;
    let after = wire.complete().await;
    let updated = root(&after, &root_uri);
    assert_ne!(updated["revision"], committed["revision"]);
    assert!(
        updated["dependency_candidates"]
            .as_array()
            .unwrap()
            .contains(&json!(files.uri("replacement/params.inc")))
    );
    assert!(
        !updated["dependency_candidates"]
            .as_array()
            .unwrap()
            .contains(&json!(files.uri("observed/params.inc")))
    );
}

#[cfg(any(unix, windows))]
#[tokio::test]
async fn retargeted_include_routes_to_current_open_alias_version() {
    let files = Files::new();
    files.write(
        "root.mod",
        "@#include \"observed/body.data\"\nvar y; model; y=0; end;\n",
    );
    files.write("observed/body.data", "parameters first_unused;\n");
    files.write("replacement/body.data", "parameters second_unused;\n");
    let alias = files.uri("observed/body.data");
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    wire.complete().await;
    wire.open(&alias, "parameters first_unused;\n", 7);
    wire.complete().await;
    assert!(
        wire.pull(&alias)
            .await
            .iter()
            .any(|item| item["code"] == "W022"
                && item["message"].as_str().unwrap().contains("first_unused"))
    );

    let observed = files.0.join("observed");
    let replacement = files.0.join("replacement");
    fs::rename(&observed, files.0.join("previous")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&replacement, &observed).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Both the junction and its target stay inside the owned fixture.
        let result = Command::new("cmd.exe")
            .creation_flags(0x08000000)
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&observed)
            .arg(&replacement)
            .output()
            .unwrap();
        assert!(result.status.success(), "{result:?}");
    }

    wire.seen.clear();
    wire.change(&alias, "parameters second_unused;\n", 8);
    wire.command("dynare/recheckWorkspace").await;
    wire.complete().await;
    let current = wire.pull(&alias).await;
    assert!(current.iter().any(|item| {
        item["code"] == "W022" && item["message"].as_str().unwrap().contains("second_unused")
    }));
    assert!(
        !current
            .iter()
            .any(|item| item["message"].as_str().unwrap().contains("first_unused"))
    );
    let physical = files.uri("replacement/body.data");
    assert_eq!(wire.pull(&physical).await, current);
    assert!(wire.seen.iter().any(|message| {
        message["method"] == "textDocument/publishDiagnostics"
            && message["params"]["uri"] == alias
            && message["params"]["version"] == 8
            && message["params"]["diagnostics"] == json!(current)
    }));
    let workspace = wire
        .request("workspace/diagnostic", json!({"previousResultIds":[]}))
        .await;
    assert!(
        workspace["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| { item["uri"] == alias && item["items"] == json!(current) })
    );
    assert!(
        !workspace["items"].as_array().unwrap().iter().any(|item| {
            item["uri"] == physical && !item["items"].as_array().unwrap().is_empty()
        })
    );
}

#[cfg(windows)]
#[tokio::test]
async fn a_case_alias_open_is_the_same_workspace_report_owner() {
    let files = Files::new();
    files.write("root.mod", MODEL);
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    wire.complete().await;
    let alias = files.uri("root.mod").replace("root.mod", "ROOT.mod");
    wire.open(&alias, "var y; model; y=unknown; end;", 1);
    let opened = wire.complete().await;
    assert_eq!(opened["roots"].as_array().unwrap().len(), 1);
    assert!(has_code(&wire.pull(&alias).await, "E020"));
    assert!(has_code(&wire.pull(&files.uri("root.mod")).await, "E020"));
    wire.command("dynare/recheckWorkspace").await;
    assert_eq!(wire.complete().await["roots"].as_array().unwrap().len(), 1);
    wire.close(&alias);
    wire.complete().await;
    assert!(!has_code(&wire.pull(&files.uri("root.mod")).await, "E020"));
}

#[cfg(windows)]
#[tokio::test]
async fn simultaneous_case_alias_opens_replace_one_overlay_and_version() {
    let files = Files::new();
    files.write("root.mod", MODEL);
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    wire.complete().await;
    let original = files.uri("root.mod");
    let alias = original.replace("root.mod", "ROOT.mod");
    wire.open(&original, MODEL, 1);
    wire.complete().await;
    wire.seen.clear();
    wire.open(&alias, "var y; model; y=unknown; end;", 2);
    let completed = wire.complete().await;
    assert_eq!(completed["roots"].as_array().unwrap().len(), 1);
    let original_items = wire.pull(&original).await;
    let alias_items = wire.pull(&alias).await;
    assert_eq!(original_items, alias_items);
    assert!(has_code(&original_items, "E020"));
    let clears = wire
        .seen
        .iter()
        .position(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == original
                && message["params"]["diagnostics"] == json!([])
        })
        .unwrap();
    let current = wire
        .seen
        .iter()
        .position(|message| {
            message["method"] == "textDocument/publishDiagnostics"
                && message["params"]["uri"] == alias
                && message["params"]["version"] == 2
        })
        .unwrap();
    assert!(
        clears < current,
        "a native-equivalent URI clear precedes the current report"
    );
    let workspace = wire
        .request(
            "workspace/diagnostic",
            json!({"previousResultIds":[{"uri":original,"value":"previous"}]}),
        )
        .await;
    let reports = workspace["items"].as_array().unwrap();
    let old = reports
        .iter()
        .position(|report| report["uri"] == original && report["items"] == json!([]))
        .unwrap();
    let current = reports
        .iter()
        .position(|report| report["uri"] == alias && report["items"] == json!(alias_items))
        .unwrap();
    assert!(old < current);
    for code in ["I208", "I209"] {
        assert!(
            original_items
                .iter()
                .filter(|item| item["code"] == code)
                .count()
                <= 1
        );
    }
    let info = wire
        .request(
            "workspace/executeCommand",
            json!({"command":"dynare/modelInfo","arguments":[{"root_uri":original}]}),
        )
        .await;
    assert_eq!(info["document_version"], 2);
    wire.change(&original, MODEL, 1);
    assert!(
        has_code(&wire.pull(&alias).await, "E020"),
        "old native-alias version is rejected"
    );
    wire.close(&original);
    wire.complete().await;
    assert!(!has_code(&wire.pull(&alias).await, "E020"));
}

#[tokio::test]
async fn changed_include_withdraws_old_ranges_immediately_and_keeps_other_owners() {
    let files = Files::new();
    files.write("body.data", "parameters unused;\n");
    files.write("unrelated.data", "parameters separate;\n");
    files.write(
        "a.mod",
        "@#include \"body.data\"\nvar y; model; y=0; end;\n",
    );
    files.write(
        "b.mod",
        "@#include \"unrelated.data\"\nvar z; model; z=0; end;\n",
    );
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], false)
        .await;
    wire.complete().await;
    let previous = wire.pull(&files.uri("body.data")).await;
    assert!(has_code(&previous, "W022"));
    let unaffected = wire.pull(&files.uri("unrelated.data")).await;
    wire.seen.clear();
    wire.open(&files.uri("body.data"), "", 1);
    assert!(
        wire.pull(&files.uri("body.data")).await.is_empty(),
        "pending reports cannot supply old ranges"
    );
    assert_eq!(wire.pull(&files.uri("unrelated.data")).await, unaffected);
    for message in &wire.seen {
        if message["method"] == "textDocument/publishDiagnostics"
            && message["params"]["uri"] == files.uri("body.data")
            && message["params"]["version"] == 1
        {
            assert!(
                message["params"]["diagnostics"]
                    .as_array()
                    .unwrap()
                    .is_empty(),
                "old report was tagged with the new document version: {message}"
            );
        }
    }
    wire.complete().await;
    assert!(wire.pull(&files.uri("body.data")).await.is_empty());
}

#[tokio::test]
async fn switching_off_alone_preserves_open_roots_folder_search_paths() {
    let files = Files::new();
    files.write("lib/values.data", "parameters p; p=0;\n");
    let text = "@#include \"values.data\"\nvar y; model; y=p; end;\n";
    files.write("root.mod", text);
    let mut wire = Wire::new();
    wire.start(
        vec![files.folder("")],
        json!({}),
        vec![json!({"uri":files.uri(""),"settings":{"searchPaths":["lib"]}})],
        false,
    )
    .await;
    wire.complete().await;
    wire.open(&files.uri("root.mod"), text, 1);
    assert!(!has_code(&wire.pull(&files.uri("root.mod")).await, "E061"));
    wire.notify(
        "workspace/didChangeConfiguration",
        json!({"settings":{"dynare":{"workspaceDiagnostics":false}}}),
    );
    assert!(!has_code(&wire.pull(&files.uri("root.mod")).await, "E061"));
}

#[tokio::test]
async fn explicit_owner_priority_survives_hidden_inputs_until_reroot_or_null_and_resets_on_restart()
{
    let files = Files::new();
    for name in ["chosen.mod", "other.mod", "third.mod"] {
        files.write(name, MODEL);
    }
    files.write("fragment.mod", "parameters p; p=0;\n");
    let mut wire = Wire::new();
    wire.request("initialize", json!({"workspaceFolders":[files.folder("")],"capabilities":{"experimental":{"dygnosis":{"workspaceStatusChanged":true}}}})).await;
    wire.notify(
        "dynare/activeModelChanged",
        json!({"root_uri":files.uri("chosen.mod")}),
    );
    wire.open(&files.uri("other.mod"), MODEL, 1);
    wire.open(&files.uri("fragment.mod"), "parameters p; p=0;\n", 1);
    wire.request(
        "textDocument/hover",
        json!({"textDocument":{"uri":files.uri("other.mod")},"position":{"line":0,"character":4}}),
    )
    .await;
    wire.request(
        "workspace/executeCommand",
        json!({"command":"dynare/modelInfo","arguments":[{"root_uri":files.uri("fragment.mod")}]}),
    )
    .await;
    wire.notify(
        "textDocument/didSave",
        json!({"textDocument":{"uri":files.uri("other.mod")},"text":MODEL}),
    );
    assert_eq!(
        wire.command("dynare/workspaceStatus").await["counts"]["checking"],
        0
    );
    assert!(
        first_checking(&wire.seen).is_none(),
        "workspace work waits for initialized"
    );
    wire.notify("initialized", json!({}));
    wire.complete().await;
    assert_eq!(
        first_checking(&wire.seen),
        Some(files.uri("chosen.mod").as_str())
    );
    for choice in [Some(files.uri("third.mod")), None] {
        wire.seen.clear();
        wire.notify("dynare/activeModelChanged", json!({"root_uri":choice}));
        wire.request("textDocument/hover", json!({"textDocument":{"uri":files.uri("other.mod")},"position":{"line":0,"character":4}})).await;
        wire.command("dynare/recheckWorkspace").await;
        wire.complete().await;
        let expected = choice.unwrap_or_else(|| files.uri("other.mod"));
        assert_eq!(first_checking(&wire.seen), Some(expected.as_str()));
    }
    drop(wire);
    let mut restarted = Wire::new();
    restarted.request("initialize", json!({"workspaceFolders":[files.folder("")],"capabilities":{"experimental":{"dygnosis":{"workspaceStatusChanged":true}}}})).await;
    restarted.open(&files.uri("other.mod"), MODEL, 1);
    restarted.notify("initialized", json!({}));
    let restarted_status = restarted.complete().await;
    assert_eq!(
        root(&restarted_status, &files.uri("other.mod"))["state"],
        "checked"
    );
    // The editor worker can finish the active model before saved-root discovery.
    // An explicit pass tests restart priority after that compatible work settles.
    restarted.seen.clear();
    restarted.command("dynare/recheckWorkspace").await;
    restarted.complete().await;
    assert_eq!(
        first_checking(&restarted.seen),
        Some(files.uri("other.mod").as_str())
    );
}
