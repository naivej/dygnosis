use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

struct McpWire {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpWire {
    fn new() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_dygnosis"))
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut wire = Self {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            next_id: 0,
        };
        let reply = wire.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05", "capabilities": {},
                "clientInfo": {"name": "input-contract-test", "version": "1"}
            }),
        );
        assert!(reply.get("result").is_some(), "{reply}");
        wire.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
        wire
    }

    fn send(&mut self, message: Value) {
        writeln!(self.input, "{message}").unwrap();
        self.input.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let mut line = String::new();
            assert!(
                self.output.read_line(&mut line).unwrap() > 0,
                "MCP exited before reply"
            );
            let reply: Value = serde_json::from_str(&line).unwrap();
            if reply["id"] == id {
                return reply;
            }
        }
    }

    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))
    }
}

impl Drop for McpWire {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn repository_compare_is_discoverable_and_runs_real_git_and_saved_inputs_over_stdio() {
    let directory = std::env::temp_dir().join(format!(
        "dygnosis stdio history {} {}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .arg("-C")
            .arg(&directory)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "--quiet"]);
    git(&["config", "user.name", "Dygnosis MCP test"]);
    git(&["config", "user.email", "test@example.invalid"]);
    git(&["config", "core.autocrlf", "false"]);
    let root = "var y;\nmodel;\n@#include \"eq.inc\"\nend;\n";
    std::fs::write(directory.join("main.mod"), root).unwrap();
    std::fs::write(directory.join("eq.inc"), "[name='eq'] y=1;\n").unwrap();
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "Before"]);
    let before = git(&["rev-parse", "HEAD"]);
    std::fs::write(directory.join("eq.inc"), "[name='eq'] y=2;\n").unwrap();
    git(&["commit", "--quiet", "-am", "After"]);
    let after = git(&["rev-parse", "HEAD"]);
    std::fs::write(directory.join("eq.inc"), "[name='eq'] y=3;\n").unwrap();
    let mut wire = McpWire::new();
    let listed = wire.request("tools/list", json!({}));
    let tool = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "dynare_compare_models")
        .unwrap();
    let description = tool["description"].as_str().unwrap();
    for phrase in [
        "Before to After",
        "server host",
        "saved server-host files",
        "no fetch",
        "INPUT_CHANGED",
    ] {
        assert!(
            description.contains(phrase),
            "Missing discovery policy {phrase}: {tool}"
        );
    }
    assert_eq!(tool["inputSchema"]["oneOf"].as_array().unwrap().len(), 2);
    let decode = |reply: Value| {
        assert!(reply.get("error").is_none(), "{reply}");
        serde_json::from_str::<Value>(reply["result"]["content"][0]["text"].as_str().unwrap())
            .unwrap()
    };
    let mut arguments = json!({"repository_path":directory.to_str().unwrap(),
        "before":{"kind":"git", "root_file":"main.mod", "ref":before},
        "after":{"kind":"git", "root_file":"main.mod", "ref":after}});
    let historical = decode(wire.call("dynare_compare_models", arguments.clone()));
    assert_eq!(historical["state"], "result", "{historical}");
    assert_eq!(historical["inputs"]["before"]["commit"], before);
    assert_eq!(historical["inputs"]["after"]["commit"], after);
    assert_eq!(historical["changed_equations"].as_array().unwrap().len(), 1);
    assert_eq!(
        historical["sources"]["before"]["eq.inc"],
        "[name='eq'] y=1;\n"
    );
    assert_eq!(
        historical["sources"]["after"]["eq.inc"],
        "[name='eq'] y=2;\n"
    );
    assert!(!historical.to_string().contains("y = 3"));
    arguments["before"]["ref"] = json!("HEAD");
    arguments["after"] = json!({"kind":"working", "root_file":"main.mod"});
    let saved = decode(wire.call("dynare_compare_models", arguments.clone()));
    assert_eq!(saved["state"], "result", "{saved}");
    assert_eq!(saved["inputs"]["after"]["source_policy"], "saved_files");
    assert_eq!(saved["inputs"]["after"]["search_paths"], json!([]));
    assert!(saved.to_string().contains("y = 3"));
    let changed_source = &saved["source_changes"]["files"][0];
    let key = changed_source["after"]["file_key"].as_str().unwrap();
    assert_eq!(saved["sources"]["after"][key], "[name='eq'] y=3;\n");
    let mut mixed = arguments.clone();
    mixed["files"] = json!({});
    let invalid = wire.call("dynare_compare_models", mixed);
    assert_eq!(invalid["error"]["code"], -32602, "{invalid}");
    let mut missing = arguments.clone();
    missing["before"]["root_file"] = json!("absent.mod");
    let failure = decode(wire.call("dynare_compare_models", missing));
    assert_eq!(failure["code"], "ROOT_NOT_FOUND");
    assert_eq!(failure["side"], "before");
    assert!(failure.get("changed_equations").is_none());
    let text = decode(wire.call("dynare_compare_models", json!({"file_content_a":"var y; model; y=1; end;", "file_content_b":"var y; model; y=2; end;"})));
    assert!(text.get("changed_equations").is_some());
    assert!(text.get("inputs").is_none());
    let supplied_root = "@#include \"body\"\n@#include \"empty\"\n";
    let supplied_old = "parameters p; estimated_params; p,normal_pdf,.5,.1; end;";
    let supplied_new = supplied_old.replace("normal_pdf,.5", "normal_pdf,.6");
    let supplied = decode(wire.call("dynare_compare_models", json!({"file_content_a":supplied_root,"file_content_b":supplied_root,
        "active_file_a":"root.mod","active_file_b":"root.mod",
        "files_a":{"root.mod":supplied_root,"body":supplied_old,"empty":"","unused":"not executed"},
        "files_b":{"root.mod":supplied_root,"body":supplied_new,"empty":"","unused":"not executed"}})));
    assert_eq!(supplied["sources"]["before"]["empty"], "");
    assert!(supplied["sources"]["before"].get("unused").is_none());
    assert_eq!(
        supplied["source_changes"]["files"][0]["correspondence"],
        "proven_file_identity"
    );
    let prior = supplied["semantic"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["family"] == "priors")
        .unwrap();
    let target = supplied["navigation"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == prior["pointer"])
        .unwrap();
    assert_eq!(target["before"]["written_locations"][0]["file"], "body");
    for result in [&historical, &saved, &supplied] {
        assert_eq!(result["source_changes"]["schema_version"], 1);
        assert_eq!(result["source_changes"]["availability"], "complete");
        assert!(result["coverage"]["limits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|limit| limit["code"] == "sources_not_captured"));
    }
    // Exercise relative sibling folders through the published JSON arguments,
    // not only through the repository adapter's internal entry point.
    let project = directory.join("project");
    let common = directory.join("common");
    std::fs::create_dir(&project).unwrap();
    std::fs::create_dir(&common).unwrap();
    std::fs::write(project.join("main.mod"), root).unwrap();
    std::fs::write(common.join("eq.inc"), "[name='eq'] y=4;\n").unwrap();
    for command_args in [
        vec!["init", "--quiet"],
        vec!["config", "user.name", "Dygnosis MCP test"],
        vec!["config", "user.email", "test@example.invalid"],
        vec!["add", "main.mod"],
        vec!["commit", "--quiet", "-m", "External include"],
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(&project)
            .args(command_args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut sibling_arguments = json!({"repository_path":project.to_str().unwrap(),
        "before":{"kind":"working", "root_file":"main.mod"},
        "after":{"kind":"working", "root_file":"main.mod"}, "search_paths":["../common"]});
    let sibling_saved = decode(wire.call("dynare_compare_models", sibling_arguments.clone()));
    assert_eq!(sibling_saved["state"], "result", "{sibling_saved}");
    assert_eq!(
        sibling_saved["inputs"]["before"]["search_paths"],
        json!(["../common"])
    );
    sibling_arguments["before"] = json!({"kind":"git", "root_file":"main.mod", "ref":"HEAD"});
    let sibling_history = decode(wire.call("dynare_compare_models", sibling_arguments));
    assert_eq!(
        sibling_history["code"], "UNSUPPORTED_SOURCE",
        "{sibling_history}"
    );
    assert_eq!(sibling_history["side"], "before");
    assert!(sibling_history.get("changed_equations").is_none());
    // Protocol cancellation must leave the server able to handle the next call.
    wire.next_id += 1;
    let cancelled_id = wire.next_id;
    wire.send(json!({"jsonrpc":"2.0", "id":cancelled_id, "method":"tools/call", "params":{"name":"dynare_compare_models", "arguments":arguments}}));
    wire.send(json!({"jsonrpc":"2.0", "method":"notifications/cancelled", "params":{"requestId":cancelled_id, "reason":"test capture cancellation"}}));
    assert!(wire.request("ping", json!({})).get("result").is_some());
}

#[test]
fn constructor_refusals_use_unsaved_include_text_and_clear_on_decimal_denominators() {
    let mut wire = McpWire::new();
    let root = "var y; varexo e; parameters p; p=1; model;\n@#include \"body.inc\"\nend;";
    let mut args = json!({"active_file": "main.mod", "file_content": root,
        "files": {"main.mod": "", "body.inc": "y=e+0*(1/(p^0-1));"}});
    let decode = |reply: Value| -> Vec<Value> {
        assert!(reply.get("error").is_none(), "{reply}");
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let rows = decode(wire.call("dynare_diagnose", args.clone()));
    let errors: Vec<_> = rows.iter().filter(|row| row["code"] == "E278").collect();
    assert_eq!(errors.len(), 1, "{rows:?}");
    assert_eq!(errors[0]["file"], "body.inc");
    assert_eq!(errors[0]["severity"], "ERROR");
    assert_eq!(errors[0]["message"], "Division by zero when forming (1)/(0); denominator simplified to 0 (possibly after substituting a variable set to 0).");
    assert_eq!(errors[0]["line"], 1);
    assert_eq!(errors[0]["column"], 8);
    assert_eq!(errors[0]["end_column"], 17);
    assert!(
        rows.iter()
            .all(|row| !matches!(row["code"].as_str(), Some("E021" | "W022" | "W042"))),
        "{rows:?}"
    );
    args["files"]["body.inc"] = json!("y=e+1/0.0;");
    let rows = decode(wire.call("dynare_diagnose", args));
    assert!(rows.iter().all(|row| row["code"] != "E278"), "{rows:?}");
    assert!(rows.iter().any(|row| row["code"] == "W022"), "{rows:?}");
}

fn arguments(name: &str) -> Value {
    match name {
        "dynare_find_references" => json!({"symbol": "y"}),
        "dynare_rename" => json!({"old_name": "y", "new_name": "output"}),
        _ => json!({}),
    }
}

#[test]
fn empty_steady_state_and_verbatim_boundaries_keep_unsaved_owners_on_the_wire() {
    let mut wire = McpWire::new();
    let root = "var y; varexo e; model; y=e; end;\n@#include \"body.inc\"\n";
    let mut args = json!({"active_file": "main.mod", "file_content": root,
        "files": {"main.mod": "old disk contents", "body.inc": "steady_state_model; end;"}});
    let decode = |reply: Value| -> Vec<Value> {
        assert!(reply.get("error").is_none(), "{reply}");
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let rows = decode(wire.call("dynare_diagnose", args.clone()));
    let errors: Vec<_> = rows.iter().filter(|row| row["code"] == "E001").collect();
    assert_eq!(errors.len(), 1, "{rows:?}");
    assert_eq!(errors[0]["file"], "body.inc");
    assert_eq!(errors[0]["message"], "syntax error, unexpected END");
    assert_eq!(errors[0]["severity"], "ERROR");
    assert_eq!(
        (
            errors[0]["line"].as_u64(),
            errors[0]["column"].as_u64(),
            errors[0]["end_column"].as_u64()
        ),
        (Some(1), Some(21), Some(24))
    );
    args["files"]["body.inc"] = json!("steady_state_model; y=0; end;");
    let rows = decode(wire.call("dynare_diagnose", args.clone()));
    assert!(
        rows.iter().all(|row| row["severity"] != "ERROR"),
        "{rows:?}"
    );

    args["files"]["body.inc"] =
        json!("verbatim;\nparameters phantom;\nmodel;\ny={1};\nend\nend;\n");
    args["file_content"] = json!(format!("{root}shocks; var e=1; end;"));
    let rows = decode(wire.call("dynare_diagnose", args.clone()));
    assert!(
        rows.iter().all(|row| row["severity"] != "ERROR"),
        "{rows:?}"
    );
    let reply = wire.call("dynare_model_info", args.clone());
    assert!(reply.get("error").is_none(), "{reply}");
    let info: Value =
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        info["n_parameters"], 0,
        "raw text must not declare phantom: {info}"
    );
    assert_eq!(
        info["n_equations"], 1,
        "raw text must not create an equation: {info}"
    );
    args["file_content"] = json!(format!("{root}shocks; var e; end;"));
    let rows = decode(wire.call("dynare_diagnose", args));
    let error = rows.iter().find(|row| row["code"] == "E001").unwrap();
    assert_eq!(
        error["message"],
        "syntax error, unexpected END, expecting PERIODS"
    );
    assert!(
        error["file"].is_null(),
        "the real refused row belongs to root: {error}"
    );
}

#[test]
fn seven_tools_validate_nonempty_maps_on_the_wire() {
    let mut wire = McpWire::new();
    let text = "var y; model; y=1; end;";
    for name in [
        "dynare_diagnose",
        "dynare_model_info",
        "dynare_equations",
        "dynare_expand",
        "dynare_related_files",
        "dynare_find_references",
        "dynare_rename",
    ] {
        for key in [None, Some("missing.mod"), Some("MAIN.mod")] {
            for content in [None, Some(text)] {
                let mut args = arguments(name);
                args["files"] = json!({"main.mod": text});
                if let Some(key) = key {
                    args["active_file"] = json!(key);
                }
                if let Some(content) = content {
                    args["file_content"] = json!(content);
                }
                let reply = wire.call(name, args);
                assert_eq!(reply["error"]["code"], -32602, "{name}: {reply}");
                let message = key.map_or_else(
                    || "active_file is required with a nonempty files map".to_string(),
                    |key| format!("\"{key}\" is not in the file map"),
                );
                assert_eq!(reply["error"]["message"], message, "{name}: {reply}");
                assert!(reply.get("result").is_none(), "{name}: {reply}");
            }
        }

        let mut mapped = arguments(name);
        mapped["files"] = json!({"main.mod": text});
        mapped["active_file"] = json!("main.mod");
        let omitted = wire.call(name, mapped.clone());
        assert!(omitted.get("error").is_none(), "{name}: {omitted}");
        mapped["file_content"] = json!(text);
        let explicit = wire.call(name, mapped.clone());
        assert_eq!(omitted["result"], explicit["result"], "{name}");
        mapped["file_content"] = json!("");
        let empty_overlay = wire.call(name, mapped.clone());
        assert!(
            empty_overlay.get("error").is_none(),
            "{name}: {empty_overlay}"
        );
        mapped["files"] = json!({"main.mod": ""});
        mapped.as_object_mut().unwrap().remove("file_content");
        let empty_map_text = wire.call(name, mapped);
        assert_eq!(empty_overlay["result"], empty_map_text["result"], "{name}");

        let mut single = arguments(name);
        single["file_content"] = json!(text);
        let without_map = wire.call(name, single.clone());
        single["files"] = json!({});
        single["active_file"] = json!("ignored.mod");
        let empty_map = wire.call(name, single);
        assert_eq!(without_map["result"], empty_map["result"], "{name}");
    }
}

#[test]
fn slice21_checks_use_unsaved_root_and_include_text_on_the_wire() {
    let mut wire = McpWire::new();
    let root = "var y r; varexo e; parameters p; p=.5;\n@#include \"body.inc\"\nsteady_state_model; y=0; end;\nplanner_objective y^2; ramsey_model(instruments=(r));\n";
    let mut args = json!({
        "active_file": "main.mod",
        "file_content": root,
        "files": {"main.mod": "", "body.inc": "model; y=0*p+e; end;"}
    });
    let reply = wire.call("dynare_diagnose", args.clone());
    let decode = |reply: &Value| -> Vec<Value> {
        assert!(reply.get("error").is_none(), "{reply}");
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let diagnostics = decode(&reply);
    let p = diagnostics
        .iter()
        .find(|d| d["code"] == "W022")
        .expect("unused p");
    assert_eq!(p["message"], "Parameter(s) p not used in the model");
    assert!(
        p["file"].is_null(),
        "root-owned diagnostics omit the file key"
    );
    assert!(
        !diagnostics.iter().any(|d| d["code"] == "W042"),
        "{diagnostics:?}"
    );

    args["files"]["body.inc"] = json!("model; y=p+0*e; end;");
    let diagnostics = decode(&wire.call("dynare_diagnose", args.clone()));
    assert!(
        diagnostics
            .iter()
            .any(|d| d["code"] == "E021" && d["file"].is_null()),
        "{diagnostics:?}"
    );

    args["files"]["body.inc"] = json!("model; y=p+e; end;");
    args["file_content"] = json!(format!("{root}resid(1);"));
    let diagnostics = decode(&wire.call("dynare_diagnose", args.clone()));
    assert!(
        diagnostics.iter().any(|d| d["code"] == "E001"
            && d["message"] == "syntax error, unexpected INT_NUMBER, expecting NON_ZERO"),
        "{diagnostics:?}"
    );
    assert!(
        !diagnostics.iter().any(|d| d["code"] == "E271"),
        "{diagnostics:?}"
    );

    args["file_content"] = json!(format!("{root}resid(non_zero);"));
    let diagnostics = decode(&wire.call("dynare_diagnose", args));
    assert!(
        !diagnostics
            .iter()
            .any(|d| matches!(d["code"].as_str(), Some("E001" | "E021" | "W022" | "W042"))),
        "{diagnostics:?}"
    );
}
