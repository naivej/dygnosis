//! Exercise canonical discovery, migration and agent workflows over real stdio.
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

struct Wire {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
}
impl Wire {
    fn new(profile: Option<&str>, edition: &str) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dygnosis"));
        command.arg("mcp");
        if let Some(profile) = profile {
            command.args(["--profile", profile]);
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut wire = Self {
            input: child.stdin.take().unwrap(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            id: 0,
        };
        let initialized=wire.request("initialize",json!({"protocolVersion":edition,"capabilities":{},"clientInfo":{"name":"canonical-workflow-test","version":"1"}}));
        assert!(initialized["result"].is_object());
        wire.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        wire
    }
    fn send(&mut self, message: Value) {
        writeln!(self.input, "{message}").unwrap();
        self.input.flush().unwrap();
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        loop {
            let mut line = String::new();
            assert!(self.output.read_line(&mut line).unwrap() > 0, "MCP exited");
            let reply: Value = serde_json::from_str(&line).unwrap();
            if reply["id"] == id {
                return reply;
            }
        }
    }
    fn call(&mut self, name: &str, args: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":args}))
    }
    fn result(&mut self, name: &str, args: Value) -> Value {
        let response = self.call(name, args);
        assert!(response.get("error").is_none(), "{response}");
        response["result"].clone()
    }
    fn data(&mut self, name: &str, args: Value) -> Value {
        let result = self.result(name, args);
        let parsed: Value = serde_json::from_str(
            result["content"].as_array().unwrap().last().unwrap()["text"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result["structuredContent"], parsed);
        assert_eq!(parsed["schema_version"], 3);
        parsed
    }
}
impl Drop for Wire {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
const MODEL: &str = "var y; varexo e; parameters a; a=.5; model; [name='eq'] y=a*y(-1)+e; end;";
fn source() -> Value {
    json!({"kind":"text","text":MODEL})
}

// Guards a server advertising consolidation but still accepting old wrappers
// or silently returning old/new defaults under the same negotiated protocol.
#[test]
fn profiles_separate_discovery_defaults_and_results_over_stdio() {
    let mut current = Wire::new(None, "2025-06-18");
    let listed = current.request("tools/list", json!({}));
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        dygnosis::registered_tool_names()
    );
    for tool in tools {
        assert_eq!(tool["inputSchema"]["additionalProperties"], false);
        assert_eq!(
            tool["outputSchema"]["properties"]["schema_version"]["const"],
            3
        );
    }
    assert_eq!(
        current.call("dynare_list_options", json!({}))["error"]["code"],
        -32602
    );
    assert_eq!(
        current.call("dynare_model_info", json!({"file_content":MODEL}))["error"]["code"],
        -32602
    );
    let result = current.data("dynare_model_info", json!({"source":source()}));
    assert!(result["data"]["summary"].is_object());
    assert!(result["data"].get("endogenous").is_none());
    let mut legacy = Wire::new(Some("legacy-v2"), "2025-06-18");
    let legacy_list = legacy.request("tools/list", json!({}));
    assert!(legacy_list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool["name"] == "dynare_list_options"));
    assert_eq!(
        legacy.call("dynare_syntax", json!({}))["error"]["code"],
        -32602
    );
    let legacy_result = legacy.result("dynare_model_info", json!({"file_content":MODEL}));
    assert_eq!(legacy_result["structuredContent"]["schema_version"], 2);
    assert!(legacy_result["structuredContent"]["data"]["endogenous"].is_array());
    assert!(legacy_result["content"].as_array().unwrap().len() > 1);
    eprintln!(
        "Discovery serialized bytes: canonical={}, legacy={}",
        serde_json::to_vec(&listed["result"]).unwrap().len(),
        serde_json::to_vec(&legacy_list["result"]).unwrap().len()
    );
}

// Guards the actual discovery/call adapter dropping useful merged facts or
// making the caller obtain unrelated identities before a known query or edit.
#[test]
fn direct_agent_reference_inspection_and_edit_workflows_run_over_stdio() {
    let mut wire = Wire::new(None, "2025-06-18");
    for name in ["pac_model", "var_model", "diff", "log"] {
        let result = wire.data("dynare_syntax", json!({"name":name}));
        assert!(
            !result["data"]["forms"].as_array().unwrap().is_empty(),
            "{name}"
        );
    }
    let options = wire.data(
        "dynare_syntax",
        json!({"name":"irf","parent":"stoch_simul","kind":"option"}),
    );
    assert!(!options["data"]["forms"][0]["signatures"]
        .as_array()
        .unwrap()
        .is_empty());
    let explanation = wire.data("dynare_diagnostic_info", json!({"code":"W010"}));
    assert!(explanation["data"]["diagnostics"][0]["explanation"]
        .as_str()
        .unwrap()
        .contains("W010"));
    let binding = wire.data(
        "dynare_symbol_info",
        json!({"source":source(),"symbol":{"kind":"name","name":"a"}}),
    );
    assert_eq!(binding["data"]["binding"]["name"], "a");
    assert_eq!(binding["data"]["references"].as_array().unwrap().len(), 3);
    let equations = wire.data(
        "dynare_equations",
        json!({"source":source(),"symbol":{"kind":"name","name":"a"}}),
    );
    assert_eq!(equations["data"]["equations"].as_array().unwrap().len(), 1);
    let renamed = wire.data(
        "dynare_rename",
        json!({"source":source(),"symbol":{"kind":"name","name":"a"},"new_name":"rho"}),
    );
    assert_eq!(renamed["proposal"]["edits"].as_array().unwrap().len(), 3);
    assert!(renamed["data"].get("text").is_none());
    let listed = wire.data("dynare_actions", json!({"source":source()}));
    let action = listed["data"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == "long_names")
        .unwrap();
    assert!(action.get("proposal").is_none());
    let selected=wire.data("dynare_actions",json!({"source":source(),"action_id":action["action_id"],"expected_revision":listed["inputs"][0]["revision"]}));
    assert!(selected["proposal"]["edits"].is_array());
    let diagnosis = wire.data("dynare_diagnose", json!({"source":source()}));
    assert_eq!(diagnosis["data"]["roots"][0]["root"], "input:1");
    let batch=wire.data("dynare_diagnose",json!({"batch":{"kind":"map","roots":["root.mod","absent.mod"],"files":{"root.mod":MODEL}}}));
    assert_eq!(batch["data"]["summary"]["checked"], 1);
    assert_eq!(batch["data"]["summary"]["failed"], 1);
}

// Guards protocol edition being treated as the tool-contract selector.
#[test]
fn old_protocol_editions_still_receive_canonical_contract_json() {
    let mut wire = Wire::new(None, "2024-11-05");
    let listed = wire.request("tools/list", json!({}));
    assert!(listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .all(|tool| tool.get("outputSchema").is_none()));
    let result = wire.result("dynare_diagnostic_info", json!({"code":"W010"}));
    assert!(result.get("structuredContent").is_none());
    assert_eq!(result["content"].as_array().unwrap().len(), 1);
    let text: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["schema_version"], 3);
    assert_eq!(text["data"]["diagnostics"][0]["code"], "W010");
}
