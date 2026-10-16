use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

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

fn diagnostic_rows(reply: &Value) -> Vec<Value> {
    assert!(reply.get("error").is_none(), "{reply}");
    let result: Value =
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(result["schema_version"], 3);
    result["data"]["roots"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|root| root["diagnostics"].as_array().unwrap().iter().cloned())
        .collect()
}

#[test]
fn partial_model_tools_keep_the_actual_macro_cause_and_written_source_over_stdio() {
    // Guards generic I211 replacements, missing source ownership and invented
    // limit receipts on ordinary missing-file or official type failures.
    let mut wire = McpWire::new();
    let root = "@#include \"child.inc\"\nvar y; model; y=0; end;\n";
    for (child, code, message, limit) in [
        (
            "/* 🚀 */\n@#include \"child.inc\"\n",
            "I211",
            "execution depth",
            true,
        ),
        (
            "/* 🚀 */\n@#include \"absent.inc\"\n",
            "E061",
            "Could not open absent.inc",
            false,
        ),
        (
            "/* 🚀 */\n@#define bad=1 in 2\n",
            "E285",
            "Second argument",
            false,
        ),
    ] {
        for tool in [
            "dynare_diagnose",
            "dynare_model_info",
            "dynare_expand",
            "dynare_equations",
        ] {
            let reply = wire.call(tool, json!({"source":{"kind":"map","root":"root.mod","files":{"root.mod":root,"child.inc":child}},"detail":"compact"}));
            assert!(reply.get("error").is_none(), "{reply}");
            let result: Value = serde_json::from_str(
                reply["result"]["content"]
                    .as_array()
                    .unwrap()
                    .last()
                    .unwrap()["text"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap();
            let status = if tool == "dynare_diagnose" && code == "E061" {
                "refused"
            } else {
                "partial"
            };
            assert_eq!(result["outcome"]["status"], status, "{tool}: {result}");
            let reason = if tool == "dynare_diagnose" {
                &result["coverage"]["roots"][0]["reasons"][0]
            } else {
                &result["coverage"][0]["reasons"][0]
            };
            assert_eq!(reason["code"], code, "{tool}: {result}");
            assert!(
                reason["message"].as_str().unwrap().contains(message),
                "{reason}"
            );
            assert_eq!(reason["source"]["file"], "child.inc", "{reason}");
            assert_eq!(reason["source"]["range"]["line"], 2, "{reason}");
            assert_eq!(reason["include_chain"][0]["file"], "root.mod", "{reason}");
            assert!(!reason["omitted"].as_str().unwrap().is_empty(), "{reason}");
            assert_eq!(
                result["limits"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|row| row["code"] == "MACRO_LIMIT"),
                limit,
                "{result}"
            );
        }
    }
}

#[test]
fn split_source_syntax_cause_keeps_only_its_verified_written_anchor_over_stdio() {
    // Guards an effective equation span being presented as if its included
    // bytes occupied the root's removed include directive.
    let mut wire = McpWire::new();
    let root = "var y; model;\ny=\n@#include \"term.inc\"\nend;";
    let reply = wire.call("dynare_model_info", json!({"source":{"kind":"map","root":"root.mod","files":{"root.mod":root,"term.inc":"0"}},"detail":"compact"}));
    let result: Value = serde_json::from_str(
        reply["result"]["content"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let reason = result["coverage"][0]["reasons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|reason| reason["code"] == "E001")
        .unwrap();
    assert_eq!(
        reason["source"],
        json!({"file":"root.mod","range":{"line":2,"column":1,"end_line":2,"end_column":2}}),
        "{reason}"
    );
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
        "two model snapshots",
        "server host",
        "saved Working",
        "no fetch",
        "editor-buffer",
    ] {
        assert!(
            description.contains(phrase),
            "Missing discovery policy {phrase}: {tool}"
        );
    }
    assert!(tool["inputSchema"]["properties"]["comparison"].is_object());
    let decode = |reply: Value| {
        assert!(reply.get("error").is_none(), "{reply}");
        let result =
            serde_json::from_str::<Value>(reply["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(result["schema_version"], 3);
        if result["data"].is_null() {
            result
        } else {
            result["data"].clone()
        }
    };
    let mut arguments = json!({"comparison":{"kind":"repository","repository_path":directory.to_str().unwrap(),
        "before":{"kind":"git", "root_file":"main.mod", "ref":before},
        "after":{"kind":"git", "root_file":"main.mod", "ref":after}},"macro_work_limit":100000,"detail":"full"});
    let historical = decode(wire.call("dynare_compare_models", arguments.clone()));
    assert_eq!(historical["state"], "result", "{historical}");
    assert_eq!(historical["inputs"]["before"]["commit"], before);
    assert_eq!(historical["inputs"]["after"]["commit"], after);
    assert_eq!(historical["inputs"]["before"]["macro_work_limit"], 100000);
    assert_eq!(historical["inputs"]["after"]["macro_work_limit"], 100000);
    let mut changed_allowance = arguments.clone();
    changed_allowance["macro_work_limit"] = json!(200000);
    changed_allowance["expected_revision_a"] = historical["inputs"]["before"]["revision"].clone();
    changed_allowance["expected_revision_b"] = historical["inputs"]["after"]["revision"].clone();
    let stale_allowance = decode(wire.call("dynare_compare_models", changed_allowance));
    assert_eq!(
        stale_allowance["outcome"]["reasons"][0]["code"],
        "INPUT_CHANGED"
    );
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
    arguments["comparison"]["before"]["ref"] = json!("HEAD");
    arguments["comparison"]["after"] = json!({"kind":"working", "root_file":"main.mod"});
    let saved = decode(wire.call("dynare_compare_models", arguments.clone()));
    assert_eq!(saved["state"], "result", "{saved}");
    assert_eq!(saved["inputs"]["after"]["source_policy"], "saved_files");
    assert_eq!(saved["inputs"]["after"]["macro_work_limit"], 100000);
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
    missing["comparison"]["before"]["root_file"] = json!("absent.mod");
    let failure = decode(wire.call("dynare_compare_models", missing));
    assert_eq!(failure["code"], "ROOT_NOT_FOUND");
    assert_eq!(failure["side"], "before");
    assert!(failure.get("changed_equations").is_none());
    let text = decode(wire.call("dynare_compare_models", json!({"comparison":{"kind":"sources","before":{"kind":"text","text":"var y; model; y=1; end;"},"after":{"kind":"text","text":"var y; model; y=2; end;"}},"detail":"full"})));
    assert!(text.get("changed_equations").is_some());
    assert!(text.get("inputs").is_none());
    let supplied_root = "@#include \"body\"\n@#include \"empty\"\n";
    let supplied_old = "parameters p; estimated_params; p,normal_pdf,.5,.1; end;";
    let supplied_new = supplied_old.replace("normal_pdf,.5", "normal_pdf,.6");
    let supplied = decode(wire.call("dynare_compare_models", json!({"comparison":{"kind":"sources",
        "before":{"kind":"map","root":"root.mod","files":{"root.mod":supplied_root,"body":supplied_old,"empty":"","unused":"not executed"}},
        "after":{"kind":"map","root":"root.mod","files":{"root.mod":supplied_root,"body":supplied_new,"empty":"","unused":"not executed"}}},"detail":"full"})));
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
        assert!(
            result["coverage"]["limits"]
                .as_array()
                .unwrap()
                .iter()
                .any(|limit| limit["code"] == "sources_not_captured")
        );
    }
    // Exercise relative sibling folders through the published JSON arguments,
    // not only through the repository adapter's internal entry point.
    let workspace = directory.join("workspace");
    let common = directory.join("common");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&common).unwrap();
    std::fs::write(workspace.join("main.mod"), root).unwrap();
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
            .arg(&workspace)
            .args(command_args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut sibling_arguments = json!({"comparison":{"kind":"repository","repository_path":workspace.to_str().unwrap(),
        "before":{"kind":"working", "root_file":"main.mod"},
        "after":{"kind":"working", "root_file":"main.mod"}, "search_paths":["../common"]},"detail":"full"});
    let sibling_saved = decode(wire.call("dynare_compare_models", sibling_arguments.clone()));
    assert_eq!(sibling_saved["state"], "result", "{sibling_saved}");
    assert_eq!(
        sibling_saved["inputs"]["before"]["search_paths"],
        json!(["../common"])
    );
    sibling_arguments["comparison"]["before"] =
        json!({"kind":"git", "root_file":"main.mod", "ref":"HEAD"});
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
    let mut args = json!({"source":{"kind":"map","root":"main.mod","root_text":root,"files":{"main.mod":"","body.inc":"y=e+0*(1/(p^0-1));"}}});
    let decode = |reply: Value| -> Vec<Value> {
        assert!(reply.get("error").is_none(), "{reply}");
        diagnostic_rows(&reply)
    };
    let rows = decode(wire.call("dynare_diagnose", args.clone()));
    let errors: Vec<_> = rows.iter().filter(|row| row["code"] == "E278").collect();
    assert_eq!(errors.len(), 1, "{rows:?}");
    assert_eq!(errors[0]["file"], "body.inc");
    assert_eq!(errors[0]["severity"], "ERROR");
    assert_eq!(
        errors[0]["message"],
        "Division by zero when forming (1)/(0); denominator simplified to 0 (possibly after substituting a variable set to 0)."
    );
    assert_eq!(errors[0]["line"], 1);
    assert_eq!(errors[0]["column"], 8);
    assert_eq!(errors[0]["end_column"], 17);
    assert!(
        rows.iter()
            .all(|row| !matches!(row["code"].as_str(), Some("E021" | "W022" | "W042"))),
        "{rows:?}"
    );
    args["source"]["files"]["body.inc"] = json!("y=e+1/0.0;");
    let rows = decode(wire.call("dynare_diagnose", args));
    assert!(rows.iter().all(|row| row["code"] != "E278"), "{rows:?}");
    assert!(rows.iter().any(|row| row["code"] == "W022"), "{rows:?}");
}

fn arguments(name: &str) -> Value {
    match name {
        "dynare_symbol_info" => json!({"symbol":{"kind":"name","name":"y"}}),
        "dynare_rename" => json!({"symbol":{"kind":"name","name":"y"},"new_name":"output"}),
        _ => json!({}),
    }
}

#[test]
fn empty_steady_state_and_verbatim_boundaries_keep_unsaved_owners_on_the_wire() {
    let mut wire = McpWire::new();
    let root = "var y; varexo e; model; y=e; end;\n@#include \"body.inc\"\n";
    let mut args = json!({"source":{"kind":"map","root":"main.mod","root_text":root,"files":{"main.mod":"old disk contents","body.inc":"steady_state_model; end;"}}});
    let decode = |reply: Value| -> Vec<Value> {
        assert!(reply.get("error").is_none(), "{reply}");
        diagnostic_rows(&reply)
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
    args["source"]["files"]["body.inc"] = json!("steady_state_model; y=0; end;");
    let rows = decode(wire.call("dynare_diagnose", args.clone()));
    assert!(
        rows.iter().all(|row| row["severity"] != "ERROR"),
        "{rows:?}"
    );

    args["source"]["files"]["body.inc"] =
        json!("verbatim;\nparameters phantom;\nmodel;\ny={1};\nend\nend;\n");
    args["source"]["root_text"] = json!(format!("{root}shocks; var e=1; end;"));
    let rows = decode(wire.call("dynare_diagnose", args.clone()));
    assert!(
        rows.iter().all(|row| row["severity"] != "ERROR"),
        "{rows:?}"
    );
    let mut info_args = args.clone();
    info_args["sections"] = json!(["structure"]);
    let reply = wire.call("dynare_model_info", info_args);
    assert!(reply.get("error").is_none(), "{reply}");
    let info: Value =
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(
        info["data"]["structure"]["n_parameters"], 0,
        "raw text must not declare phantom: {info}"
    );
    assert_eq!(
        info["data"]["structure"]["n_equations"], 1,
        "raw text must not create an equation: {info}"
    );
    args["source"]["root_text"] = json!(format!("{root}shocks; var e; end;"));
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
fn model_tools_validate_exact_map_roots_and_preserve_empty_overlays() {
    // Guards implicit root selection and empty overlays falling back to stale file text.
    let mut wire = McpWire::new();
    let text = "var y; model; y=1; end;";
    for name in [
        "dynare_diagnose",
        "dynare_model_info",
        "dynare_equations",
        "dynare_expand",
        "dynare_actions",
        "dynare_symbol_info",
        "dynare_rename",
    ] {
        for key in [None, Some("missing.mod"), Some("MAIN.mod")] {
            for overlay in [None, Some(text)] {
                let mut args = arguments(name);
                args["source"] = json!({"kind":"map","files":{"main.mod":text}});
                if let Some(key) = key {
                    args["source"]["root"] = json!(key);
                }
                if let Some(overlay) = overlay {
                    args["source"]["root_text"] = json!(overlay);
                }
                let reply = wire.call(name, args);
                assert_eq!(reply["error"]["code"], -32602, "{name}: {reply}");
                let message = reply["error"]["message"].as_str().unwrap();
                assert!(!message.is_empty());
                assert!(message.contains("root"), "{message}");
            }
        }
        let mut mapped = arguments(name);
        mapped["source"] = json!({"kind":"map","root":"main.mod","files":{"main.mod":text}});
        let omitted = wire.call(name, mapped.clone());
        assert!(omitted.get("error").is_none(), "{name}: {omitted}");
        mapped["source"]["root_text"] = json!(text);
        let explicit = wire.call(name, mapped.clone());
        assert_eq!(omitted["result"], explicit["result"], "{name}");
        mapped["source"]["root_text"] = json!("");
        let empty_overlay = wire.call(name, mapped.clone());
        assert!(
            empty_overlay.get("error").is_none(),
            "{name}: {empty_overlay}"
        );
        mapped["source"]["files"] = json!({"main.mod":""});
        mapped["source"]
            .as_object_mut()
            .unwrap()
            .remove("root_text");
        let empty_text = wire.call(name, mapped.clone());
        assert_eq!(empty_overlay["result"], empty_text["result"], "{name}");
        mapped["source"]["files"] = json!({});
        assert_eq!(wire.call(name, mapped)["error"]["code"], -32602);
        let mut single = arguments(name);
        single["source"] = json!({"kind":"text","text":text});
        let result = wire.call(name, single);
        assert!(result.get("error").is_none(), "{name}: {result}");
    }
}

#[test]
fn slice21_checks_use_unsaved_root_and_include_text_on_the_wire() {
    let mut wire = McpWire::new();
    let root = "var y r; varexo e; parameters p; p=.5;\n@#include \"body.inc\"\nsteady_state_model; y=0; end;\nplanner_objective y^2; ramsey_model(instruments=(r));\n";
    let mut args = json!({"source":{"kind":"map","root":"main.mod","root_text":root,"files":{"main.mod":"","body.inc":"model; y=0*p+e; end;"}}});
    let reply = wire.call("dynare_diagnose", args.clone());
    let decode = |reply: &Value| -> Vec<Value> {
        assert!(reply.get("error").is_none(), "{reply}");
        diagnostic_rows(reply)
    };
    let diagnostics = decode(&reply);
    let p = diagnostics
        .iter()
        .find(|d| d["code"] == "W022")
        .expect("unused p");
    assert_eq!(
        p["message"],
        "Parameter(s) p not used in model equations or steady_state_model"
    );
    assert!(
        p["file"].is_null(),
        "root-owned diagnostics omit the file key"
    );
    assert!(
        !diagnostics.iter().any(|d| d["code"] == "W042"),
        "{diagnostics:?}"
    );

    args["source"]["files"]["body.inc"] = json!("model; y=p+0*e; end;");
    let diagnostics = decode(&wire.call("dynare_diagnose", args.clone()));
    assert!(
        diagnostics
            .iter()
            .any(|d| d["code"] == "E021" && d["file"].is_null()),
        "{diagnostics:?}"
    );

    args["source"]["files"]["body.inc"] = json!("model; y=p+e; end;");
    args["source"]["root_text"] = json!(format!("{root}resid(1);"));
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

    args["source"]["root_text"] = json!(format!("{root}resid(non_zero);"));
    let diagnostics = decode(&wire.call("dynare_diagnose", args));
    assert!(
        !diagnostics
            .iter()
            .any(|d| matches!(d["code"].as_str(), Some("E001" | "E021" | "W022" | "W042"))),
        "{diagnostics:?}"
    );
}

#[test]
fn macro_work_configuration_is_isolated_and_revision_bound_over_stdio() {
    // Guards lost request settings, fabricated default ceilings, and stale
    // selections after changing only the macro work allowance.
    let source = "@#for i in 1:100\nvar y@{i};\n@#endfor\nmodel;\n@#for i in 1:100\ny@{i}=0;\n@#endfor\nend;\n";
    let canonical = |reply: Value| -> Value {
        assert!(reply.get("error").is_none(), "{reply}");
        serde_json::from_str(
            reply["result"]["content"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap()
    };
    let mut wire = McpWire::new();
    let small = canonical(wire.call(
        "dynare_model_info",
        json!({"source":{"kind":"text","text":source},"macro_work_limit":1000,"detail":"compact"}),
    ));
    assert_eq!(small["coverage"][0]["complete"], false, "{small}");
    assert!(
        small["limits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["limit"] == "iteration work" && row["ceiling"] == 1000),
        "{small}"
    );
    let large = canonical(wire.call(
        "dynare_model_info",
        json!({"source":{"kind":"text","text":source},"macro_work_limit":100000,"detail":"compact"}),
    ));
    assert_eq!(large["coverage"][0]["complete"], true, "{large}");
    assert_eq!(large["data"]["summary"]["equation_count"], 100, "{large}");
    assert_ne!(
        large["inputs"][0]["revision"],
        small["inputs"][0]["revision"]
    );
    let stale = canonical(wire.call("dynare_model_info", json!({"source":{"kind":"text","text":source},"macro_work_limit":1000,"expected_revision":large["inputs"][0]["revision"],"detail":"compact"})));
    assert_eq!(stale["outcome"]["reasons"][0]["code"], "INPUT_CHANGED");
    let ordinary = canonical(wire.call(
        "dynare_model_info",
        json!({"source":{"kind":"text","text":source},"detail":"compact"}),
    ));
    assert_eq!(ordinary["coverage"][0]["complete"], true);
    assert_eq!(
        ordinary["inputs"][0]["macro_work_limit"],
        dygnosis::MacroWorkLimit::DEFAULT
    );
    for invalid in [
        json!(0),
        json!(-1),
        json!(1.5),
        json!(null),
        json!("1000"),
        json!(100000001),
    ] {
        let reply = wire.call(
            "dynare_model_info",
            json!({"source":{"kind":"text","text":source},"macro_work_limit":invalid}),
        );
        assert_eq!(reply["error"]["code"], -32602, "{reply}");
    }
}

#[test]
fn batch_and_comparison_apply_work_per_root_and_per_side_over_stdio() {
    // Guards spending one allowance across roots/sides and transport routes
    // silently retaining the default or accepting invalid work values.
    let source = "@#for i in 1:100\nvar y@{i};\n@#endfor\nmodel;\n@#for i in 1:100\ny@{i}=0;\n@#endfor\nend;\n";
    let canonical = |reply: Value| -> Value {
        assert!(reply.get("error").is_none(), "{reply}");
        serde_json::from_str(
            reply["result"]["content"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap()
    };
    let mut wire = McpWire::new();
    let batch_args = json!({"batch":{"kind":"map","files":{"a.mod":source,"b.mod":source},"roots":["a.mod","b.mod"]},"macro_work_limit":50000,"detail":"compact"});
    let large = canonical(wire.call("dynare_diagnose", batch_args.clone()));
    assert_eq!(large["data"]["summary"]["checked"], 2, "{large}");
    assert!(
        large["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|input| input["macro_work_limit"] == 50000)
    );
    let mut small_args = batch_args.clone();
    small_args["macro_work_limit"] = json!(1000);
    let small = canonical(wire.call("dynare_diagnose", small_args.clone()));
    assert!(
        small["limits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["limit"] == "iteration work" && row["ceiling"] == 1000),
        "{small}"
    );
    small_args["expected_revisions"] = large["data"]["root_revisions"].clone();
    let stale = canonical(wire.call("dynare_diagnose", small_args));
    assert!(
        stale["data"]["roots"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["code"] == "INPUT_CHANGED"),
        "{stale}"
    );
    let compare_args = json!({"comparison":{"kind":"sources","before":{"kind":"text","text":source},"after":{"kind":"text","text":source}},"macro_work_limit":50000,"detail":"compact"});
    let large = canonical(wire.call("dynare_compare_models", compare_args.clone()));
    assert_eq!(large["coverage"]["inputs"]["before"]["complete"], true);
    assert_eq!(large["coverage"]["inputs"]["after"]["complete"], true);
    assert!(
        !large["limits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|limit| limit["code"] == "MACRO_LIMIT")
    );
    assert!(
        large["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|input| input["macro_work_limit"] == 50000)
    );
    let mut small_args = compare_args.clone();
    small_args["macro_work_limit"] = json!(1000);
    let small = canonical(wire.call("dynare_compare_models", small_args.clone()));
    assert_eq!(small["outcome"]["status"], "refused");
    assert_eq!(small["coverage"]["inputs"]["before"]["complete"], false);
    assert_eq!(small["coverage"]["inputs"]["after"]["complete"], false);
    for side in ["before", "after"] {
        assert!(
            small["limits"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["side"] == side
                    && row["limit"] == "iteration work"
                    && row["ceiling"] == 1000),
            "{small}"
        );
    }
    small_args["expected_revision_a"] = large["inputs"][0]["revision"].clone();
    small_args["expected_revision_b"] = large["inputs"][1]["revision"].clone();
    let stale = canonical(wire.call("dynare_compare_models", small_args));
    assert_eq!(
        stale["outcome"]["reasons"][0]["code"], "INPUT_CHANGED",
        "{stale}"
    );
    for (tool, mut args) in [
        ("dynare_diagnose", batch_args),
        ("dynare_compare_models", compare_args),
    ] {
        args["macro_work_limit"] = json!(0);
        let reply = wire.call(tool, args);
        assert_eq!(reply["error"]["code"], -32602, "{reply}");
    }
}

#[test]
fn configured_work_reaches_auxiliary_checks_and_edit_reparses_over_stdio() {
    // Guards a complete configured model gaining a false E001 from a default
    // replay, losing a late E304, or dropping a verified fix for unnamed text.
    let prefix = format!(
        "@#define big=\"{}\"\n@#for i in 1:400\n@#define scratch=big\n@#endfor\n",
        "a".repeat(5_000)
    );
    let canonical = |reply: &Value| -> Value {
        assert!(reply.get("error").is_none(), "{reply}");
        serde_json::from_str(
            reply["result"]["content"]
                .as_array()
                .unwrap()
                .last()
                .unwrap()["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap()
    };
    let mut wire = McpWire::new();
    let source = format!("{prefix}@#if 0\nvar é;\n@#endif\nvar y; model; y=0; end;\n");
    let reply = wire.call(
        "dynare_diagnose",
        json!({
            "source":{"kind":"map","root":"main.mod","files":{"main.mod":source}},"macro_work_limit":10000000
        }),
    );
    let diagnostics = diagnostic_rows(&reply);
    assert!(
        !diagnostics.iter().any(|row| row["code"] == "E001"),
        "{reply}"
    );
    let result = canonical(&reply);
    assert_eq!(result["coverage"]["roots"][0]["complete"], true, "{result}");
    assert!(result["limits"].as_array().unwrap().is_empty(), "{result}");

    let source = format!("{prefix}@#includepath \"missing-directory\"\nvar y; model; y=0; end;\n");
    let result = canonical(&wire.call(
        "dynare_diagnose",
        json!({
            "source":{"kind":"map","root":"main.mod","files":{"main.mod":source}},"macro_work_limit":10000000
        }),
    ));
    assert!(
        result["data"]["roots"][0]["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["code"] == "E304"),
        "{result}"
    );
    assert!(result["limits"].as_array().unwrap().is_empty(), "{result}");

    let source =
        format!("{prefix}@#if 0\nmodel;\n    y=2;\nend;\n@#endif\nvar y;\nmodel;\ny=0;\nend;\n");
    let formatted = canonical(&wire.call(
        "dynare_format",
        json!({
            "source":{"kind":"text","text":source},"macro_work_limit":10000000
        }),
    ));
    assert!(
        formatted["proposal"]["edits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edit| edit["new_text"]
                .as_str()
                .unwrap()
                .contains("model;\n    y=2;\nend;")),
        "{formatted}"
    );

    let source = format!("{prefix}var y; parameters p;\np=.5\nmodel; y=p; end;\n");
    let fixed = canonical(&wire.call(
        "dynare_actions",
        json!({
            "fixes":true,"source":{"kind":"text","text":source},"macro_work_limit":10000000
        }),
    ));
    assert!(!fixed["proposal"].is_null(), "{fixed}");
    assert!(
        fixed["proposal"]["edits"]
            .as_array()
            .unwrap()
            .iter()
            .any(|edit| edit["new_text"].as_str().unwrap().contains("p=.5;")),
        "{}",
        fixed["outcome"]
    );
}

#[test]
fn repeated_partial_splices_keep_the_actual_first_written_cause_over_stdio() {
    // Guards identical requests spending shared splice work in hash order and
    // attaching a different included source/caller to the same actual stop.
    let table = (0..4097)
        .map(|n| format!("\"{n}\""))
        .collect::<Vec<_>>()
        .join(",");
    let args = json!({
        "source":{"kind":"map","root":"/root.mod","files":{
            "/root.mod":"@#include \"table0.inc\"\n@#include \"table1.inc\"\nvar y; model; y=0; end;\n",
            "/table0.inc":format!("@#define a0=[{table}]\n// @{{a0[1]}}\n"),
            "/table1.inc":format!("@#define a1=[{table}]\n// @{{a1[1]}}\n")}},
        "macro_work_limit":250000,"detail":"compact"
    });
    let canonical = |reply: Value| -> Value {
        assert!(reply.get("error").is_none(), "{reply}");
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    };
    let mut wire = McpWire::new();
    let first = canonical(wire.call("dynare_model_info", args.clone()));
    assert_eq!(first["coverage"][0]["complete"], false);
    let reason = &first["coverage"][0]["reasons"][0];
    assert_eq!(reason["code"], "I211");
    assert_eq!(reason["source"]["file"], "/table0.inc");
    assert_eq!(reason["include_chain"][0]["range"]["line"], 1);
    assert_eq!(reason["limit"]["ceiling"], 250000);
    for _ in 0..15 {
        let current = canonical(wire.call("dynare_model_info", args.clone()));
        assert_eq!(current, first, "same inputs must retain the actual stop");
    }
}
