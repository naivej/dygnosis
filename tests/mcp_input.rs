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
