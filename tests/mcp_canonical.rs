//! Exercise canonical discovery and agent workflows over real stdio.
#![cfg(feature = "mcp-reuse")]
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Wire {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    id: u64,
}
impl Wire {
    fn new(edition: &str) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dygnosis-mcp"));
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
fn current_contract_rejects_retired_tools_and_arguments_over_stdio() {
    let mut current = Wire::new("2025-06-18");
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
    for retired in [
        "dynare_find_references",
        "dynare_auto_fix",
        "dynare_explain",
        "dynare_list_diagnostic_codes",
        "dynare_list_options",
        "dynare_related_files",
        "dynare_workspace_diagnose",
        "dynare_code_actions",
    ] {
        let reply = current.call(retired, json!({}));
        assert_eq!(reply["error"]["code"], -32602);
        assert!(
            reply["error"]["message"]
                .as_str()
                .unwrap()
                .contains("tools/list")
        );
    }
}

// Guards the actual discovery/call adapter dropping useful merged facts or
// making the caller obtain unrelated identities before a known query or edit.
#[test]
fn direct_agent_reference_inspection_and_edit_workflows_run_over_stdio() {
    let mut wire = Wire::new("2025-06-18");
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
    assert!(
        !options["data"]["forms"][0]["signatures"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let explanation = wire.data("dynare_diagnostic_info", json!({"code":"W010"}));
    assert!(
        explanation["data"]["diagnostics"][0]["explanation"]
            .as_str()
            .unwrap()
            .contains("W010")
    );
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
    let mut wire = Wire::new("2024-11-05");
    let listed = wire.request("tools/list", json!({}));
    assert!(
        listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool.get("outputSchema").is_none())
    );
    let result = wire.result("dynare_diagnostic_info", json!({"code":"W010"}));
    assert!(result.get("structuredContent").is_none());
    assert_eq!(result["content"].as_array().unwrap().len(), 1);
    let text: Value = serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(text["schema_version"], 3);
    assert_eq!(text["data"]["diagnostics"][0]["code"], "W010");
}

// Guards a selected named binding inheriting declaration, timing or assignment
// facts from an ordinary symbol with the same spelling through the real adapter.
#[test]
fn typed_bindings_keep_metadata_in_their_own_namespace_over_stdio() {
    let text = "heterogeneity_dimension h;parameters h $H$;h=.5;var y;model;[name='Y']y=h*y(-1);end;var_model(model_name=y,eqtags=['Y']);";
    let source = json!({"kind":"text","text":text});
    let mut wire = Wire::new("2025-06-18");
    for (name, named_kind, ordinary_kind) in [
        ("h", "heterogeneity_dimension", "parameters"),
        ("y", "var_model", "var"),
    ] {
        let ambiguous = wire.data(
            "dynare_symbol_info",
            json!({"source":source,"symbol":{"kind":"name","name":name}}),
        );
        assert_eq!(
            ambiguous["outcome"]["reasons"][0]["code"],
            "AMBIGUOUS_SYMBOL"
        );
        let candidates = ambiguous["data"]["candidates"].as_array().unwrap();
        for kind in [named_kind, ordinary_kind] {
            let candidate = candidates.iter().find(|row| row["kind"] == kind).unwrap();
            let result = wire.data("dynare_symbol_info", json!({"source":source,"symbol":{"kind":"binding","binding_id":candidate["binding_id"]},"expected_revision":ambiguous["inputs"][0]["revision"],"sections":["metadata","assignments","references"]}));
            let binding = &result["data"]["binding"];
            assert_eq!(binding["kind"], kind);
            assert!(binding.get("namespace").is_some());
            if kind == named_kind {
                assert!(binding["timing"].is_null());
                assert!(binding["tex_name"].is_null());
                assert!(binding["written_kind"].is_null());
                assert!(binding["assignment_values"].as_array().unwrap().is_empty());
            } else if name == "h" {
                assert_eq!(binding["tex_name"], "H");
                assert_eq!(binding["assignment_values"][0]["value"], 0.5);
            } else {
                assert!(binding["timing"].is_object());
            }
        }
    }
}

// Guards head-specific labels losing their namespace in ambiguous candidates
// or binding reuse, and a copied label receiving only a partial Rename.
#[test]
fn copied_subsamples_keep_namespaces_and_complete_rename_over_stdio() {
    let text = "parameters a b;a=.5;b=.5;a.subsamples(short=2000Q1:2001Q1);a.short.options(init=.4);b.subsamples=a.subsamples;b.short.options(init=.6);";
    let source = json!({"kind":"text","text":text});
    let mut wire = Wire::new("2025-06-18");
    let ambiguous = wire.data(
        "dynare_symbol_info",
        json!({"source":source,"symbol":{"kind":"name","name":"short"}}),
    );
    let candidates = ambiguous["data"]["candidates"].as_array().unwrap();
    let copied = candidates
        .iter()
        .find(|row| row["namespace"] == "subsamples(b)")
        .unwrap();
    assert!(
        candidates
            .iter()
            .any(|row| row["namespace"] == "subsamples(a)")
    );
    let selector = json!({"kind":"binding","binding_id":copied["binding_id"]});
    let revision = &ambiguous["inputs"][0]["revision"];
    let selected = wire.data(
        "dynare_symbol_info",
        json!({"source":source,"symbol":selector,"expected_revision":revision}),
    );
    assert_eq!(selected["data"]["binding"]["namespace"], "subsamples(b)");
    assert_eq!(selected["data"]["references"].as_array().unwrap().len(), 2);
    let renamed = wire.data(
        "dynare_rename",
        json!({"source":source,"symbol":selector,"expected_revision":revision,"new_name":"early"}),
    );
    assert_eq!(renamed["outcome"]["status"], "changed");
    assert_eq!(renamed["proposal"]["edits"].as_array().unwrap().len(), 3);
}

// Guards known external function/database bindings being reported as absent,
// and a visible-only name edit being proposed without its external effect.
#[test]
fn external_names_expose_metadata_and_specific_refusals_over_stdio() {
    let mut wire = Wire::new("2025-06-18");
    let text = "external_function(name='pkg.helper',nargs=1,first_deriv_provided='pkg.deriv');var y;model;y=pkg.helper(y);end;database db;";
    let source = json!({"kind":"text","text":text});
    for (name, kind, count, refusal) in [
        (
            "pkg.helper",
            "external_function",
            2,
            "EXTERNAL_IMPLEMENTATION",
        ),
        (
            "pkg.deriv",
            "external_function",
            1,
            "EXTERNAL_IMPLEMENTATION",
        ),
        ("db", "database", 1, "EXTERNAL_NAMESPACE"),
    ] {
        let selector = json!({"kind":"name","name":name});
        let result = wire.data(
            "dynare_symbol_info",
            json!({"source":source,"symbol":selector}),
        );
        assert_eq!(result["data"]["binding"]["kind"], kind);
        assert_eq!(
            result["data"]["references"].as_array().unwrap().len(),
            count
        );
        if kind == "external_function" {
            assert_eq!(
                result["data"]["binding"]["written_kind"],
                "external_function"
            );
        }
        let renamed = wire.data(
            "dynare_rename",
            json!({"source":source,"symbol":selector,"new_name":"other"}),
        );
        assert_eq!(renamed["outcome"]["status"], "refused");
        assert_eq!(renamed["outcome"]["reasons"][0]["code"], refusal);
        assert!(renamed["proposal"].is_null());
    }
}

// Guards real MCP partial-source queries publishing a recovered name before
// its grammar callback, while completed earlier callbacks remain queryable.
#[test]
fn partial_named_grammar_does_not_publish_false_bindings_over_stdio() {
    let mut wire = Wire::new("2025-06-18");
    for text in [
        "heterogeneity_dimension ghost",
        "database ghost",
        "var_model(model_name=ghost,",
        "var y;model;y=pac_expectation(ghost,1);end;",
        "pac_target_info(ghost)",
    ] {
        let result = wire.data(
            "dynare_symbol_info",
            json!({"source":{"kind":"text","text":text},"symbol":{"kind":"name","name":"ghost"}}),
        );
        assert!(result["data"]["binding"].is_null(), "{text}: {result}");
        assert!(result["data"]["references"].as_array().unwrap().is_empty());
    }
    for text in [
        "heterogeneity_dimension ghost;",
        "database ghost;",
        "var_model(model_name=ghost,eqtags=['Y']);",
        "var y;model;y=pac_expectation(ghost)+;end;",
        "pac_target_info(ghost);target",
    ] {
        let result = wire.data(
            "dynare_symbol_info",
            json!({"source":{"kind":"text","text":text},"symbol":{"kind":"name","name":"ghost"}}),
        );
        assert_eq!(
            result["data"]["binding"]["name"], "ghost",
            "{text}: {result}"
        );
    }
    for (text, accepted) in [
        ("var y;model;y=pac_expectation(ghost);end;", true),
        ("var y;model;y=pac_expectation(ghost,1);end;", false),
        ("var y;epilogue;z=pac_expectation(ghost);end;", false),
    ] {
        let result = wire.data("dynare_symbol_info", json!({"source":{"kind":"text","text":text},"symbol":{"kind":"name","name":"pac_expectation"}}));
        assert_eq!(
            !result["data"]["binding"].is_null(),
            accepted,
            "{text}: {result}"
        );
        assert_eq!(
            result["data"]["references"].as_array().unwrap().len(),
            usize::from(accepted),
            "{text}: {result}"
        );
    }
}

// Guards a complete proposal changing an external database column string
// through an ordinary model variable. Refusal must return no partial edits.
#[test]
fn ordinary_symbol_with_explicit_database_field_refuses_rename_over_stdio() {
    let mut wire = Wire::new("2025-06-18");
    let base = "var y;varexo e;database db;model;y=.5*y(-1)+e;end;";
    let symbol = json!({"kind":"name","name":"y"});
    let safe = wire.data(
        "dynare_rename",
        json!({"source":{"kind":"text","text":base},"symbol":symbol,"new_name":"z"}),
    );
    assert_eq!(safe["outcome"]["status"], "changed");
    let text = format!("{base}shock_paths;var e;periods 1;values db.y;end;");
    let source = json!({"kind":"text","text":text});
    let info = wire.data(
        "dynare_symbol_info",
        json!({"source":source,"symbol":symbol}),
    );
    assert_eq!(info["data"]["binding"]["kind"], "var");
    assert_eq!(info["data"]["references"].as_array().unwrap().len(), 4);
    let refused = wire.data(
        "dynare_rename",
        json!({"source":source,"symbol":symbol,"new_name":"z"}),
    );
    assert_eq!(refused["outcome"]["status"], "refused");
    assert_eq!(
        refused["outcome"]["reasons"][0]["code"],
        "EXTERNAL_DATABASE_MEMBER"
    );
    assert!(
        refused["outcome"]["reasons"][0]["message"]
            .as_str()
            .unwrap()
            .contains("column")
    );
    assert!(refused["proposal"].is_null());
}

// A syntactically closed command can still refuse before adding its model.
// The canonical query must withhold both the false binding and full coverage.
#[test]
fn refused_semantic_callbacks_withhold_named_bindings_and_full_coverage_over_stdio() {
    let mut wire = Wire::new("2025-06-18");
    let base = "var y;parameters p;p=.9;model;[name='Y']y=0;end;";
    for command in [
        "var_model(model_name=ghost);",
        "trend_component_model(model_name=ghost,eqtags=['Y']);",
        "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=aux,horizon=1,time_shift=1);",
        "pac_model(model_name=ghost,discount=y);",
    ] {
        let text = format!("{base}{command}");
        let result = wire.data(
            "dynare_symbol_info",
            json!({"source":{"kind":"text","text":text},"symbol":{"kind":"name","name":"ghost"}}),
        );
        assert!(result["data"]["binding"].is_null(), "{command}: {result}");
        assert_eq!(
            result["coverage"][0]["complete"], false,
            "{command}: {result}"
        );
        let reasons = result["coverage"][0]["reasons"].as_array().unwrap();
        assert!(
            reasons.iter().any(|row| row["code"] == "PARSE_REFUSED"
                && row["message"]
                    .as_str()
                    .is_some_and(|message| !message.is_empty())),
            "{command}: {result}"
        );
    }
    for command in [
        "var_model(model_name=ghost,eqtags=['Y']);",
        "trend_component_model(model_name=ghost,eqtags=['Y'],targets=['Y']);",
        "var_expectation_model(model_name=ghost,variable=y,auxiliary_model_name=aux,horizon=1);",
        "pac_model(model_name=ghost,discount=p);",
    ] {
        let text = format!("{base}{command}");
        let result = wire.data(
            "dynare_symbol_info",
            json!({"source":{"kind":"text","text":text},"symbol":{"kind":"name","name":"ghost"}}),
        );
        assert_eq!(
            result["data"]["binding"]["name"], "ghost",
            "{command}: {result}"
        );
        assert_eq!(
            result["coverage"][0]["complete"], true,
            "{command}: {result}"
        );
    }
}

// The cause keeps the refused included command's source, even when its action
// boundary is immediately followed by a statement in the root file.
#[test]
fn semantic_parse_refusal_causes_keep_their_own_source_over_stdio() {
    let mut wire = Wire::new("2025-06-18");
    for (text, needle) in [
        ("var y;y=1;database later;", "not a parameter"),
        ("var y;model;y=absent;end;database later;", "absent"),
        (
            "var y;model;y=foo(y);end;database later;",
            "external function",
        ),
        (
            "parameters p;std(p).prior(shape=normal,mean=0,stdev=1);database later;",
            "neither endogenous or exogenous",
        ),
        (
            "parameters a;a.options(init=.4) trailing;database later;",
            "Parse callback",
        ),
    ] {
        let result = wire.data(
            "dynare_symbol_info",
            json!({"source":{"kind":"text","text":text},"symbol":{"kind":"name","name":"later"}}),
        );
        assert!(result["data"]["binding"].is_null(), "{result}");
        assert_eq!(result["coverage"][0]["complete"], false);
        assert!(
            result["coverage"][0]["reasons"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["message"]
                    .as_str()
                    .is_some_and(|message| message.contains(needle))),
            "{result}"
        );
    }
    let result = wire.data("dynare_symbol_info", json!({"source":{"kind":"map","root":"root.mod","files":{"root.mod":"@#include \"bad.inc\"\ndatabase later;","bad.inc":"var_model(model_name=ghost);"}},"symbol":{"kind":"name","name":"later"}}));
    assert!(result["data"]["binding"].is_null(), "{result}");
    let reasons = result["coverage"][0]["reasons"].as_array().unwrap();
    let cause = reasons
        .iter()
        .find(|row| row["code"] == "PARSE_REFUSED")
        .unwrap();
    assert!(
        cause["message"].as_str().unwrap().contains("eqtags"),
        "{result}"
    );
    assert!(cause.to_string().contains("bad.inc"), "{result}");
}

// Incomplete prior/options heads and copies retain their earlier declaration,
// but their label/head operands are not accepted References before callback.
#[test]
fn incomplete_subsample_operands_do_not_add_references_over_stdio() {
    let mut wire = Wire::new("2025-06-18");
    let base = "parameters a b;a.subsamples(short=2000Q1:2001Q1);";
    for tail in [
        "a.short.options(",
        "a.short.prior(",
        "a.short.options=b.short.options",
        "a.short.prior=b.short.prior",
    ] {
        let text = format!("{base}{tail}");
        let result = wire.data(
            "dynare_symbol_info",
            json!({"source":{"kind":"text","text":text},"symbol":{"kind":"name","name":"short"}}),
        );
        assert_eq!(
            result["data"]["references"].as_array().unwrap().len(),
            1,
            "{tail}: {result}"
        );
        assert_eq!(result["coverage"][0]["complete"], false);
    }
    let text = format!("{base}a.short.options(init=.4);");
    let result = wire.data(
        "dynare_symbol_info",
        json!({"source":{"kind":"text","text":text},"symbol":{"kind":"name","name":"short"}}),
    );
    assert_eq!(result["data"]["references"].as_array().unwrap().len(), 2);
    assert_eq!(result["coverage"][0]["complete"], true);
}
