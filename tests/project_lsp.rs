//! Real-process LSP checks: folder-only activation and report/input lifetime.
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{json, Value};
use tower_lsp::lsp_types::Url;

static NEXT: AtomicU64 = AtomicU64::new(0);
const MODEL: &str = "var y; model; y=0; end; initval; y=0; end;\n";

struct Files(PathBuf);
impl Files {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dygnosis-project-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
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
        let response = self.request("initialize", json!({"workspaceFolders":folders,"capabilities":{"experimental":{"dygnosis":{"projectStatusChanged":opt_in}}},
            "initializationOptions":{"dynare":{"configuration":{"schemaVersion":1,"loose":loose,"folders":scoped}}}})).await;
        assert_eq!(
            response["capabilities"]["experimental"]["dygnosis"]["projectDiagnostics"]
                ["schema_version"],
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
    async fn complete(&mut self) -> Value {
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let status = self.command("dynare/projectStatus").await;
                if status["complete"] == true {
                    return status;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("project completed")
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
fn has_code(items: &[Value], code: &str) -> bool {
    items.iter().any(|diag| diag["code"] == code)
}

fn first_checking(statuses: &[Value]) -> Option<&str> {
    statuses
        .iter()
        .filter(|message| message["method"] == "dynare/projectStatusChanged")
        .flat_map(|message| message["params"]["roots"].as_array().into_iter().flatten())
        .find(|entry| entry["state"] == "checking")
        .and_then(|entry| entry["root_uri"].as_str())
}

#[tokio::test]
async fn folder_only_cli_discovery_checked_errors_and_identical_push_pull() {
    let files = Files::new();
    files.write("good.mod", MODEL);
    files.write("sub/bad.mod", "var y; model; y=unknown; end;\n");
    files.write("+generated/skip.mod", "broken");
    files.write("ordinary.dyn", MODEL);
    let mut wire = Wire::new();
    wire.start(vec![files.folder("")], json!({}), vec![], true)
        .await;
    let status = wire.complete().await;
    assert_eq!(status["counts"]["checked"], 2);
    assert_eq!(status["coverage_complete"], true);
    assert!(
        root(&status, &files.uri("sub/bad.mod"))["errors"]
            .as_u64()
            .unwrap()
            > 0
    );
    let cli = dygnosis::check_walk::collect_mod_files(&files.0).unwrap();
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
    assert!(wire
        .seen
        .iter()
        .any(|message| message["method"] == "dynare/projectStatusChanged"));
    wire.open(&files.uri("ordinary.dyn"), "var y; model; y=x; end;", 1);
    assert!(has_code(
        &wire.pull(&files.uri("ordinary.dyn")).await,
        "E020"
    ));
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
        vec![json!({"uri":files.uri("a"),"settings":{"projectExcludePaths":["skip.mod"]}})],
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
        json!({"settings":{"dynare":{"projectDiagnostics":false}}}),
    );
    let status = wire.command("dynare/projectStatus").await;
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
    let cancelled = wire.command("dynare/cancelProject").await;
    assert_eq!(cancelled["cancelled"], true);
    files.write("new.mod", "var y; model; y=x; end;");
    assert_eq!(
        wire.command("dynare/projectStatus").await["roots"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    wire.command("dynare/recheckProject").await;
    let rescanned = wire.complete().await;
    assert_eq!(rescanned["roots"].as_array().unwrap().len(), 2);
    wire.command("dynare/cancelProject").await;
    files.write("root.mod", "var y; model; y=x; end;");
    wire.changed(&files.uri("root.mod"), 2);
    assert_eq!(wire.complete().await["cancelled"], false);
    assert!(has_code(&wire.pull(&files.uri("root.mod")).await, "E020"));
    wire.changed(&files.uri("root.mod"), 2);
    wire.notify(
        "workspace/didChangeConfiguration",
        json!({"settings":{"dynare":{"projectDiagnostics":false}}}),
    );
    wire.command("dynare/projectStatus").await;
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert!(wire.pull(&files.uri("root.mod")).await.is_empty());
    assert_eq!(
        wire.command("dynare/recheckProject").await["enabled"],
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
async fn failed_discovery_is_not_complete_coverage_and_notifications_are_opt_in() {
    let files = Files::new();
    let mut wire = Wire::new();
    wire.start(vec![files.folder("absent")], json!({}), vec![], false)
        .await;
    let status = wire.complete().await;
    assert_eq!(status["coverage_complete"], false);
    assert_eq!(status["discovery_failures"].as_array().unwrap().len(), 1);
    assert!(!wire
        .seen
        .iter()
        .any(|message| message["method"] == "dynare/projectStatusChanged"));
}

#[tokio::test]
async fn chosen_active_root_is_first_and_checks_are_serial() {
    let files = Files::new();
    for name in ["a.mod", "b.mod", "chosen.mod"] {
        files.write(name, MODEL);
    }
    let mut wire = Wire::new();
    wire.request("initialize", json!({"workspaceFolders":[files.folder("")],"capabilities":{"experimental":{"dygnosis":{"projectStatusChanged":true}}}})).await;
    wire.notify(
        "dynare/activeModelChanged",
        json!({"root_uri":files.uri("chosen.mod")}),
    );
    wire.notify("initialized", json!({}));
    wire.complete().await;
    let statuses: Vec<_> = wire
        .seen
        .iter()
        .filter(|message| message["method"] == "dynare/projectStatusChanged")
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
    assert!(statuses
        .iter()
        .all(|status| status["counts"]["checking"].as_u64().unwrap() <= 1));
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
    let notes = wire.pull(&files.uri("body.inc")).await;
    let note = notes
        .iter()
        .find(|diag| diag["code"] == "I208")
        .unwrap()
        .clone();
    assert_eq!(note["data"]["root"], files.uri("root.mod"));
    let actions = wire.request("textDocument/codeAction", json!({"textDocument":{"uri":files.uri("body.inc")},"range":note["range"],"context":{"diagnostics":[note]}})).await;
    let action = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action.get("edit").is_some())
        .unwrap();
    assert!(action["edit"]["documentChanges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edit| edit["textDocument"]["version"].is_null()));
    wire.open(&files.uri("body.inc"), "y=0;\n", 9);
    wire.complete().await;
    let note = wire
        .pull(&files.uri("body.inc"))
        .await
        .into_iter()
        .find(|diag| diag["code"] == "I208")
        .unwrap();
    let actions = wire.request("textDocument/codeAction", json!({"textDocument":{"uri":files.uri("body.inc")},"range":note["range"],"context":{"diagnostics":[note]}})).await;
    let action = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action.get("edit").is_some())
        .unwrap();
    assert!(action["edit"]["documentChanges"]
        .as_array()
        .unwrap()
        .iter()
        .any(|edit| edit["textDocument"]["version"] == 9));
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
        json!({"settings":{"dynare":{"projectDiagnostics":false}}}),
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
async fn turning_off_during_a_running_check_cannot_republish_project_diagnostics() {
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
        let status = wire.command("dynare/projectStatus").await;
        if status["counts"]["checking"] == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    wire.notify(
        "workspace/didChangeConfiguration",
        json!({"settings":{"dynare":{"projectDiagnostics":false}}}),
    );
    assert_eq!(wire.command("dynare/projectStatus").await["enabled"], false);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert!(wire.pull(&files.uri("large.mod")).await.is_empty());
    assert!(!wire.seen.iter().any(|message| message["method"]
        == "textDocument/publishDiagnostics"
        && message["params"]["uri"] == files.uri("large.mod")
        && message["params"]["diagnostics"]
            .as_array()
            .is_some_and(|items| !items.is_empty())));
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
    assert!(has_code(&items, "E306"));
    fs::create_dir(files.0.join("missing")).unwrap();
    files.write("params.data", "y 0\n");
    wire.changed(&files.uri("missing"), 1);
    wire.changed(&files.uri("params.data"), 1);
    let after = wire.complete().await;
    assert_ne!(
        root(&after, &files.uri("root.mod"))["revision"],
        previous_revision
    );
    let items = wire.pull(&files.uri("root.mod")).await;
    assert!(!has_code(&items, "E304"));
    assert!(!has_code(&items, "E306"));
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
    wire.command("dynare/recheckProject").await;
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
    assert!(committed["dependency_candidates"]
        .as_array()
        .unwrap()
        .contains(&json!(files.uri("observed/params.inc"))));

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
    let unchanged = wire.command("dynare/projectStatus").await;
    assert_eq!(root(&unchanged, &root_uri), &committed);

    wire.command("dynare/recheckProject").await;
    let after = wire.complete().await;
    let updated = root(&after, &root_uri);
    assert_ne!(updated["revision"], committed["revision"]);
    assert!(updated["dependency_candidates"]
        .as_array()
        .unwrap()
        .contains(&json!(files.uri("replacement/params.inc"))));
    assert!(!updated["dependency_candidates"]
        .as_array()
        .unwrap()
        .contains(&json!(files.uri("observed/params.inc"))));
}

#[cfg(windows)]
#[tokio::test]
async fn a_case_alias_open_is_the_same_project_report_owner() {
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
    wire.command("dynare/recheckProject").await;
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
        json!({"settings":{"dynare":{"projectDiagnostics":false}}}),
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
    wire.request("initialize", json!({"workspaceFolders":[files.folder("")],"capabilities":{"experimental":{"dygnosis":{"projectStatusChanged":true}}}})).await;
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
        wire.command("dynare/projectStatus").await["counts"]["checking"],
        0
    );
    assert!(
        first_checking(&wire.seen).is_none(),
        "project work waits for initialized"
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
        wire.command("dynare/recheckProject").await;
        wire.complete().await;
        let expected = choice.unwrap_or_else(|| files.uri("other.mod"));
        assert_eq!(first_checking(&wire.seen), Some(expected.as_str()));
    }
    drop(wire);
    let mut restarted = Wire::new();
    restarted.request("initialize", json!({"workspaceFolders":[files.folder("")],"capabilities":{"experimental":{"dygnosis":{"projectStatusChanged":true}}}})).await;
    restarted.open(&files.uri("other.mod"), MODEL, 1);
    restarted.notify("initialized", json!({}));
    restarted.complete().await;
    assert_eq!(
        first_checking(&restarted.seen),
        Some(files.uri("other.mod").as_str())
    );
}
