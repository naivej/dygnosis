//! Exercise the MCP executable, default retention and explicit opt-out.
#![cfg(feature = "mcp-reuse")]

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

#[test]
fn mcp_process_defaults_to_completed_analysis_reuse() {
    // Guards direct MCP startup without the default store, changed responses
    // across retained tool calls, or a process that survives stdin closure.
    for enabled in [true, false] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dygnosis-mcp"));
        command
            .env_remove("DYGNOSIS_MCP_REUSE")
            .env("DYGNOSIS_MCP_REUSE_TRACE", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !enabled {
            command.env("DYGNOSIS_MCP_REUSE", "0");
        }
        let mut child = command.spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let mut errors = child.stderr.take().unwrap();
        let stderr = std::thread::spawn(move || {
            let mut text = String::new();
            errors.read_to_string(&mut text).unwrap();
            text
        });
        let writer = std::thread::spawn(move || {
            let source = json!({"kind":"text","text":"var y; model; y=0; end;"});
            let messages = [
                json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"mcp-process-test","version":"1"}}}),
                json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"dynare_model_info","arguments":{"source":source}}}),
                json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"dynare_equations","arguments":{"source":source,"equation":{"kind":"index","scope":"aggregate","index":0}}}}),
                json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"dynare_model_info","arguments":{"source":source}}}),
            ];
            let mut responses = Vec::new();
            for message in messages {
                writeln!(input, "{message}").unwrap();
                input.flush().unwrap();
                if message.get("id").is_some() {
                    let mut line = String::new();
                    assert!(
                        output.read_line(&mut line).unwrap() > 0,
                        "MCP server closed stdout before its response"
                    );
                    responses.push(serde_json::from_str::<Value>(&line).unwrap());
                }
            }
            drop(input);
            responses
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("MCP server did not stop after stdin closed");
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(status.success());
        let responses = writer.join().unwrap();
        assert_eq!(responses[0]["result"]["serverInfo"]["name"], "dygnosis");
        for response in &responses[1..] {
            assert_eq!(
                response["result"]["structuredContent"]["outcome"]["status"],
                "ok"
            );
        }
        assert_eq!(responses[1]["result"], responses[3]["result"]);
        let errors = stderr.join().unwrap();
        let events: Vec<Value> = errors
            .lines()
            .filter_map(|line| line.strip_prefix("MCP_RETENTION "))
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let completed: Vec<_> = events
            .iter()
            .filter(|event| event["event"] == "complete")
            .collect();
        assert_eq!(completed.len(), 3, "{errors}");
        for (index, event) in completed.iter().enumerate() {
            assert_eq!(event["cache_enabled"], enabled);
            assert_eq!(event["hit"], enabled && index > 0);
        }
        if enabled {
            assert!(
                events
                    .iter()
                    .any(|event| event["event"] == "close" && event["entries"] == 0)
            );
        }
    }
}
