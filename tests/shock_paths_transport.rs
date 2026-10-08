//! Written shock-path diagnostics through live LSP overlays and stdio MCP.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use dygnosis::server::{new_service, Backend};
use dygnosis::span::LineIndex;
use serde_json::{json, Value};
use tower_lsp::{lsp_types::*, LanguageServer};

const HEAD: &str = "var y; varexo e u; parameters p q; database db; model; y=e+u+p+q; end;\n";

struct Pair {
    code: &'static str,
    fire: &'static str,
    quiet: &'static str,
    sentence: &'static str,
    written: &'static str,
    controlled: bool,
}

const PAIRS: &[Pair] = &[
    Pair { code: "E278", fire: "(q+p)/0", quiet: "(q+p)/0.0", sentence: "Division by zero when forming (q+p)/(0); denominator simplified to 0 (possibly after substituting a variable set to 0).", written: "(q+p)/0", controlled: false },
    Pair { code: "E278", fire: "(q+p)/0", quiet: "(q+p)/0e0", sentence: "Division by zero when forming (q+p)/(0); denominator simplified to 0 (possibly after substituting a variable set to 0).", written: "(q+p)/0", controlled: true },
    Pair { code: "E276", fire: "log(0)", quiet: "log(p)", sentence: "log(0) not defined!", written: "log(0)", controlled: false },
    Pair { code: "E277", fire: "log10(0)", quiet: "log10(p)", sentence: "log10(0) not defined!", written: "log10(0)", controlled: false },
    Pair { code: "E405", fire: "self.e(-1)", quiet: "0*self.e(-1)", sentence: "shock_paths: a lag of 1 is not allowed at period 1", written: "self.e(-1)", controlled: false },
    Pair { code: "E405", fire: "db.y(-1)", quiet: "db.p(-1)", sentence: "shock_paths: a lag of 1 is not allowed at period 1", written: "db.y(-1)", controlled: false },
    Pair { code: "E420", fire: "self.e", quiet: "0*self.e", sentence: "in the definition of 'e' in a 'shock_paths' block, the use of 'self.e' without a lag is not allowed, since it is a circular reference", written: "self.e", controlled: false },
    Pair { code: "E415", fire: "Self.u", quiet: "self.u", sentence: "Unknown database: Self. You may want to declare it via the 'database' command.", written: "Self.u", controlled: false },
    Pair { code: "E416", fire: "0*self.e", quiet: "init.y", sentence: "The syntax self.e is not accepted in an 'endogenize' stanza of a 'shock_paths' block", written: "self.e", controlled: true },
    Pair { code: "E001", fire: "p,", quiet: "p", sentence: "syntax error, unexpected ';'", written: ";", controlled: false },
    Pair { code: "E001", fire: "p; nonsense", quiet: "p", sentence: "syntax error, unexpected IDENTIFIER, expecting END", written: "nonsense", controlled: false },
    Pair { code: "E001", fire: "p; var e; values p", quiet: "p", sentence: "syntax error, unexpected VALUES, expecting PERIODS", written: "values", controlled: false },
    Pair { code: "E001", fire: "periods 1 nonsense; values p;", quiet: "periods 1; values p;", sentence: "syntax error, unexpected IDENTIFIER, expecting COMMA or ';'", written: "nonsense", controlled: false },
    Pair { code: "E001", fire: "periods 1,; values p;", quiet: "periods 1; values p;", sentence: "syntax error, unexpected ';', expecting END or DATE or INT_NUMBER", written: ";", controlled: false },
    Pair { code: "E395", fire: "periods 2:1,; values p;", quiet: "periods 1; values p;", sentence: "Can't have first period index greater than second index in range specification", written: "2:1", controlled: false },
];

fn body(pair: &Pair, value: &str) -> String {
    let target = if pair.controlled {
        "exogenize y"
    } else {
        "var e"
    };
    let endogenize = if pair.controlled {
        " endogenize e;"
    } else {
        ""
    };
    if value.starts_with("periods ") {
        format!("shock_paths;\n{target}; /*😀*/ {value}{endogenize}\nend;\n")
    } else {
        format!("shock_paths;\n{target}; periods 1; /*😀*/ values {value};{endogenize}\nend;\n")
    }
}

fn written_offsets(text: &str, written: &str) -> (u32, u32) {
    let anchor = text.find("/*😀*/").unwrap() + "/*😀*/".len();
    let anchor = if text[anchor..].trim_start().starts_with("periods ") {
        anchor
    } else {
        anchor + text[anchor..].find("values ").unwrap() + "values ".len()
    };
    let start = anchor + text[anchor..].find(written).unwrap();
    (start as u32, (start + written.len()) as u32)
}

async fn open(server: &Backend, uri: &Url, text: &str) {
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: text.into(),
            },
        })
        .await;
}

async fn replace(server: &Backend, uri: &Url, text: &str, version: i32) {
    server
        .did_change(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version,
            },
            content_changes: vec![TextDocumentContentChangeEvent {
                range: None,
                range_length: None,
                text: text.into(),
            }],
        })
        .await;
}

async fn pull(server: &Backend, uri: &Url) -> Vec<Diagnostic> {
    match server
        .diagnostic(DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier { uri: uri.clone() },
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
    {
        DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) => {
            full.full_document_diagnostic_report.items
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn path_pairs_keep_written_ranges_and_clear_after_unsaved_lsp_edits() {
    for newline in ["\n", "\r\n"] {
        for included in [false, true] {
            for pair in PAIRS {
                let (service, _socket) = new_service();
                let server = service.inner();
                let root = Url::parse("file:///C:/path-transport/main.mod").unwrap();
                let child = Url::parse("file:///C:/path-transport/body.inc").unwrap();
                let fire = body(pair, pair.fire).replace('\n', newline);
                let quiet = body(pair, pair.quiet).replace('\n', newline);
                let (owner, fire, quiet) = if included {
                    open(server, &child, &fire).await;
                    open(
                        server,
                        &root,
                        &format!("{HEAD}@#include \"body.inc\"\n").replace('\n', newline),
                    )
                    .await;
                    (&child, fire, quiet)
                } else {
                    let fire = format!("{}{fire}", HEAD.replace('\n', newline));
                    let quiet = format!("{}{quiet}", HEAD.replace('\n', newline));
                    open(server, &root, &fire).await;
                    (&root, fire, quiet)
                };
                let rows = pull(server, owner).await;
                let errors: Vec<_> = rows
                    .iter()
                    .filter(|row| row.code == Some(NumberOrString::String(pair.code.into())))
                    .collect();
                assert_eq!(
                    errors.len(),
                    1,
                    "{} included={included}: {rows:?}",
                    pair.code
                );
                assert_eq!(errors[0].message, pair.sentence);
                assert_eq!(errors[0].severity, Some(DiagnosticSeverity::ERROR));
                let (start, end) = written_offsets(&fire, pair.written);
                let index = LineIndex::new(&fire);
                let start = index.position_utf16(&fire, start);
                let end = index.position_utf16(&fire, end);
                assert_eq!(
                    errors[0].range,
                    Range::new(
                        Position::new(start.line, start.character),
                        Position::new(end.line, end.character)
                    ),
                    "{}: {fire}",
                    pair.code
                );
                replace(server, owner, &quiet, 2).await;
                let rows = pull(server, owner).await;
                assert!(
                    rows.iter()
                        .all(|row| row.severity != Some(DiagnosticSeverity::ERROR)),
                    "{} quiet={quiet}: {rows:?}",
                    pair.code
                );
            }
        }
    }
}

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
        let reply = wire.request("initialize", json!({ "protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": { "name": "path-transport-test", "version": "1" } }));
        assert!(reply.get("result").is_some(), "{reply}");
        writeln!(
            wire.input,
            "{}",
            json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })
        )
        .unwrap();
        wire.input.flush().unwrap();
        wire
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        writeln!(
            self.input,
            "{}",
            json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
        )
        .unwrap();
        self.input.flush().unwrap();
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

    fn diagnose(&mut self, args: Value) -> Vec<Value> {
        let reply = self.request(
            "tools/call",
            json!({ "name": "dynare_diagnose", "arguments": args }),
        );
        assert!(reply.get("error").is_none(), "{reply}");
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }
}

impl Drop for McpWire {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn path_pairs_keep_written_owners_and_scalar_columns_on_stdio_mcp() {
    let mut wire = McpWire::new();
    for newline in ["\n", "\r\n"] {
        for included in [false, true] {
            for pair in PAIRS {
                let fire = body(pair, pair.fire).replace('\n', newline);
                let quiet = body(pair, pair.quiet).replace('\n', newline);
                let root = if included {
                    format!("{HEAD}@#include \"body.inc\"\n").replace('\n', newline)
                } else {
                    format!("{}{fire}", HEAD.replace('\n', newline))
                };
                let mut args = json!({ "active_file": "main.mod", "file_content": root, "files": { "main.mod": "old disk text", "body.inc": fire } });
                let rows = wire.diagnose(args.clone());
                let errors: Vec<_> = rows.iter().filter(|row| row["code"] == pair.code).collect();
                assert_eq!(
                    errors.len(),
                    1,
                    "{} included={included}: {rows:?}",
                    pair.code
                );
                assert_eq!(errors[0]["message"], pair.sentence);
                assert_eq!(errors[0]["severity"], "ERROR");
                assert_eq!(
                    errors[0]["file"],
                    if included {
                        json!("body.inc")
                    } else {
                        Value::Null
                    }
                );
                let written = if included { fire } else { root };
                let (start, end) = written_offsets(&written, pair.written);
                let index = LineIndex::new(&written);
                let start = index.position(&written, start);
                let end = index.position(&written, end);
                assert_eq!(errors[0]["line"], start.line + 1);
                assert_eq!(errors[0]["column"], start.character + 1);
                assert_eq!(errors[0]["end_line"], end.line + 1);
                assert_eq!(errors[0]["end_column"], end.character + 1);
                if included {
                    args["files"]["body.inc"] = json!(quiet);
                } else {
                    args["file_content"] = json!(format!("{}{quiet}", HEAD.replace('\n', newline)));
                }
                let rows = wire.diagnose(args);
                assert!(
                    rows.iter().all(|row| row["severity"] != "ERROR"),
                    "{}: {rows:?}",
                    pair.code
                );
            }
        }
    }
}

#[tokio::test]
async fn path_macro_copies_keep_per_root_multiplicity_and_source_navigation() {
    let mut wire = McpWire::new();
    for newline in ["\n", "\r\n"] {
        let body = "@#if 0\nshock_paths; var e; periods 1; values log10(0); end;\n@#endif\n@#for j in 1:2\nshock_paths; var e; periods 1; /*😀*/ values 1/@{denominator}; end;\n@#endfor\n".replace('\n', newline);
        let root_text = format!(
            "@#define denominator = \"0\"\n{HEAD}@#include \"body.inc\"\n@#include \"body.inc\"\n"
        )
        .replace('\n', newline);
        let quiet = root_text.replace("\"0\"", "\"0.0\"");
        let root = Url::parse("file:///C:/path-macro/main.mod").unwrap();
        let other = Url::parse("file:///C:/path-macro/other.mod").unwrap();
        let child = Url::parse("file:///C:/path-macro/body.inc").unwrap();
        let (service, _socket) = new_service();
        let server = service.inner();
        open(server, &child, &body).await;
        open(server, &root, &root_text).await;
        open(server, &other, &root_text).await;
        let rows = pull(server, &child).await;
        let errors: Vec<_> = rows
            .iter()
            .filter(|row| row.code == Some(NumberOrString::String("E278".into())))
            .collect();
        assert_eq!(
            errors.len(),
            4,
            "two roots retain the maximum per-root count: {rows:?}"
        );
        let start = body.find("1/@{denominator}").unwrap() as u32;
        let end = start + "1/@{denominator}".len() as u32;
        let index = LineIndex::new(&body);
        let from = index.position_utf16(&body, start);
        let to = index.position_utf16(&body, end);
        for row in errors {
            assert_eq!(row.message, "Division by zero when forming (1)/(0); denominator simplified to 0 (possibly after substituting a variable set to 0).");
            assert_eq!(row.severity, Some(DiagnosticSeverity::ERROR));
            assert_eq!(
                row.range,
                Range::new(
                    Position::new(from.line, from.character),
                    Position::new(to.line, to.character)
                )
            );
        }
        assert!(
            rows.iter()
                .all(|row| row.code != Some(NumberOrString::String("E277".into()))),
            "inactive branch: {rows:?}"
        );
        let mut args = json!({ "active_file": "main.mod", "file_content": root_text, "files": { "main.mod": "old disk text", "body.inc": body } });
        let rows = wire.diagnose(args.clone());
        let errors: Vec<_> = rows.iter().filter(|row| row["code"] == "E278").collect();
        assert_eq!(errors.len(), 4, "{rows:?}");
        let from = index.position(&body, start);
        let to = index.position(&body, end);
        for row in errors {
            assert_eq!(row["file"], "body.inc");
            assert_eq!(row["severity"], "ERROR");
            assert_eq!(row["message"], "Division by zero when forming (1)/(0); denominator simplified to 0 (possibly after substituting a variable set to 0).");
            assert_eq!(row["line"], from.line + 1);
            assert_eq!(row["column"], from.character + 1);
            assert_eq!(row["end_line"], to.line + 1);
            assert_eq!(row["end_column"], to.character + 1);
        }
        assert!(rows.iter().all(|row| row["code"] != "E277"));
        args["file_content"] = json!(quiet);
        let rows = wire.diagnose(args);
        assert!(
            rows.iter().all(|row| row["severity"] != "ERROR"),
            "{rows:?}"
        );
        replace(server, &root, &quiet, 2).await;
        replace(server, &other, &quiet, 2).await;
        let rows = pull(server, &child).await;
        assert!(
            rows.iter()
                .all(|row| row.severity != Some(DiagnosticSeverity::ERROR)),
            "{rows:?}"
        );
        let preview = server
            .execute_command(ExecuteCommandParams {
                command: "dynare/showEffectiveModel".into(),
                arguments: vec![json!({ "root_uri": root, "layout": "source" })],
                work_done_progress_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(preview["complete"], true);
        let effective = preview["effective_text"].as_str().unwrap();
        assert_eq!(effective.matches("values 1/0.0;").count(), 4);
        assert!(!effective.contains("log10(0)"));
        let regions = preview["source_navigation"].as_array().unwrap();
        let written_targets: Vec<_> = regions
            .iter()
            .filter(|region| region["written_location"]["uri"] == child.as_str())
            .map(|region| {
                let range: Range =
                    serde_json::from_value(region["written_location"]["range"].clone()).unwrap();
                let start = index.offset_utf16(
                    &body,
                    dygnosis::span::Position {
                        line: range.start.line,
                        character: range.start.character,
                    },
                ) as usize;
                let end = index.offset_utf16(
                    &body,
                    dygnosis::span::Position {
                        line: range.end.line,
                        character: range.end.character,
                    },
                ) as usize;
                &body[start..end]
            })
            .collect();
        assert!(
            written_targets
                .iter()
                .any(|text| text.contains("values 1/")),
            "copied path text retains its source target: {regions:?}"
        );
        assert!(
            written_targets.contains(&"@{denominator}"),
            "interpolation retains its own source target: {regions:?}"
        );
    }
}
