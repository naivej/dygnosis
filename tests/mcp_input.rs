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

fn arguments(name: &str) -> Value {
    match name {
        "dynare_find_references" => json!({"symbol": "y"}),
        "dynare_rename" => json!({"old_name": "y", "new_name": "output"}),
        _ => json!({}),
    }
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
