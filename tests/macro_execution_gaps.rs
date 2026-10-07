//! Slice 11 execution-gap controls: emitted lexical context, varying includes,
//! fatal stop, debug grammar, Unicode defines, and shared expansion budgets.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use dygnosis::diagnostic::{analyze, check_file};
use dygnosis::expand::expand_report;
use dygnosis::parse;
use dygnosis::{dynare_expand, JsonStage, Workspace};

fn codes(source: &str) -> Vec<String> {
    analyze(&parse(source))
        .into_iter()
        .map(|row| format!("{} {}", row.code, row.message))
        .collect()
}

fn has_code(source: &str, code: &str, needle: &str) -> bool {
    analyze(&parse(source))
        .iter()
        .any(|row| row.code == code && row.message.contains(needle))
}

fn virtual_uri(name: &str) -> String {
    if cfg!(windows) {
        format!(r"C:\dygnosis-exec-gap\{name}")
    } else {
        format!("/tmp/dygnosis-exec-gap/{name}")
    }
}

fn temp_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "dyg-exec-gap-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn param_names(model: &dygnosis::Model) -> Vec<String> {
    model
        .param_assignments
        .iter()
        .map(|assignment| model.name(assignment.name).to_string())
        .collect()
}

#[test]
fn substitutions_join_the_surrounding_lexical_context() {
    let commented = "\
@#define open = \"/*\"
@#define close = \"*/\"
var y;
model;
@{open}
y = 1;
@{close}
y = 0;
end;
";
    let report = expand_report(commented);
    assert!(report.complete, "{}", report.effective_text);
    assert_eq!(report.n_equations, 1, "{}", report.effective_text);
    assert!(
        report.effective_text.contains("y = 0"),
        "{}",
        report.effective_text
    );
    assert!(!analyze(&parse(commented))
        .iter()
        .any(|row| row.code == "E020"));

    let closed = "\
@#define close = \"*/\"
var y;
model;
/* @{close}
y = 0;
end;
";
    let report = expand_report(closed);
    assert!(report.complete, "{}", report.effective_text);
    assert_eq!(report.n_equations, 1, "{}", report.effective_text);

    let joined = "\
@#define empty = \"\"
var y@{empty}z;
model;
yz = 0;
end;
";
    let model = parse(joined);
    assert!(
        model
            .endogenous
            .iter()
            .any(|decl| model.name(decl.name) == "yz"),
        "names {:?}",
        model
            .endogenous
            .iter()
            .map(|decl| model.name(decl.name))
            .collect::<Vec<_>>()
    );
    assert!(!analyze(&model).iter().any(|row| row.code == "E020"));

    let in_line = "// @{missing}\nvar y; model; y=0; end;\n";
    assert!(
        has_code(in_line, "E063", "Unknown variable missing"),
        "{:?}",
        codes(in_line)
    );
    assert_eq!(expand_report(in_line).n_equations, 0);
    let in_percent = "% @{missing}\nvar y; model; y=0; end;\n";
    assert!(
        has_code(in_percent, "E063", "Unknown variable missing"),
        "{:?}",
        codes(in_percent)
    );
    let in_line_ok = "@#define x = 1\n// @{x}\nvar y; model; y=0; end;\n";
    let report = expand_report(in_line_ok);
    assert!(report.complete, "{}", report.effective_text);
    assert_eq!(report.n_equations, 1, "{}", report.effective_text);

    let hidden = "\
@#define x = 1
/*
@#define x = 2
*/
var y; model; y=@{x}; end;
";
    let text = expand_report(hidden).effective_text;
    assert!(text.contains("y = 2") || text.contains("y=2"), "{text}");

    let generated = "@#define s = \"@#define z = 1 @{z}\"\n@{s}\n";
    let text = expand_report(generated).effective_text;
    assert!(text.contains("@#define"), "{text}");
    assert!(text.contains("@{z}"), "{text}");
    assert!(!has_code(generated, "E063", "Unknown variable z"));
}

#[test]
fn two_backslashes_continue_a_directive_and_one_does_not() {
    let joined = "\
@#define x = 1 + \\\\
2
var y; model; y=@{x}; end;
";
    let text = expand_report(joined).effective_text;
    assert!(text.contains("y = 3") || text.contains("y=3"), "{text}");

    let noted = "\
@#define x = 1 + \\\\ // kept out of the expression
2
var y; model; y=@{x}; end;
";
    let text = expand_report(noted).effective_text;
    assert!(text.contains("y = 3") || text.contains("y=3"), "{text}");

    let single = "\
@#define x = 1 + \\
2
var y; model; y=@{x}; end;
";
    let text = expand_report(single).effective_text;
    assert!(
        !text.contains("y = 3") && !text.contains("y=3"),
        "one backslash must not continue: {text}"
    );
}

#[test]
fn loop_includes_each_evaluated_target() {
    let root = "\
var y;
model;
@#for i in [\"a\", \"b\"]
@#include i + \".inc\"
@#endfor
end;
";
    let mut files = HashMap::new();
    files.insert(virtual_uri("vary.mod"), root.to_string());
    files.insert(virtual_uri("a.inc"), "y = 1;\n".to_string());
    files.insert(virtual_uri("b.inc"), "y = 2;\n".to_string());
    files.insert(
        virtual_uri("dormant.inc"),
        "parameters dormant_hit; dormant_hit = 1;\n".to_string(),
    );
    let mut ws = Workspace::new();
    for (uri, text) in &files {
        ws.update_document(uri, text.clone());
    }
    let uri = virtual_uri("vary.mod");
    let model = ws.get_effective_model(&uri).unwrap();
    assert_eq!(model.equations.len(), 2, "{}", model.source);
    let records = ws.include_records(&uri).unwrap();
    assert!(records.unresolved.is_empty(), "{:?}", records.unresolved);
    assert!(
        records
            .resolved
            .iter()
            .any(|record| record.filename == "a.inc"),
        "{:?}",
        records.resolved
    );
    assert!(
        records
            .resolved
            .iter()
            .any(|record| record.filename == "b.inc"),
        "{:?}",
        records.resolved
    );

    let report = ws.expand_report(&uri).unwrap();
    assert!(report.complete, "{}", report.effective_text);
    let frames: Vec<_> = report
        .origins
        .iter()
        .flat_map(|origin| origin.origin_frames.iter())
        .filter(|frame| frame.kind == "for")
        .map(|frame| frame.value.clone().unwrap_or_default())
        .collect();
    assert!(
        frames.iter().any(|value| value.contains('a'))
            && frames.iter().any(|value| value.contains('b')),
        "{frames:?}"
    );
    let uris: Vec<_> = report
        .origins
        .iter()
        .filter_map(|origin| origin.origin_uri.clone())
        .collect();
    assert!(uris.iter().any(|file| file.ends_with("a.inc")), "{uris:?}");
    assert!(uris.iter().any(|file| file.ends_with("b.inc")), "{uris:?}");
}

#[test]
fn function_target_and_nested_include_keep_every_dependency() {
    let root = "\
@#define f(name) = name + \".inc\"
@#for i in [\"a\", \"b\"]
@#include f(i)
@#endfor
";
    let mut ws = Workspace::new();
    let root_uri = virtual_uri("fn.mod");
    ws.update_document(&root_uri, root);
    ws.update_document(
        &virtual_uri("a.inc"),
        "@#include \"nested.inc\"\nparameters a_hit; a_hit = 1;\n",
    );
    ws.update_document(&virtual_uri("b.inc"), "parameters b_hit; b_hit = 1;\n");
    ws.update_document(
        &virtual_uri("nested.inc"),
        "parameters nested_hit; nested_hit = 1;\n",
    );
    let model = ws.get_effective_model(&root_uri).unwrap();
    let names = param_names(model);
    assert!(names.contains(&"a_hit".to_string()), "{names:?}");
    assert!(names.contains(&"b_hit".to_string()), "{names:?}");
    assert!(names.contains(&"nested_hit".to_string()), "{names:?}");
    let included = ws.resolve_all_includes(&root_uri);
    assert!(
        included.keys().any(|key| key.ends_with("a.inc")),
        "{included:?}"
    );
    assert!(
        included.keys().any(|key| key.ends_with("b.inc")),
        "{included:?}"
    );
    assert!(
        included.keys().any(|key| key.ends_with("nested.inc")),
        "{included:?}"
    );
}

#[test]
fn disk_search_path_include_reads_only_executed_files() {
    let dir = temp_dir();
    let sub = dir.join("sub");
    fs::create_dir_all(&sub).unwrap();
    let root = dir.join("root.mod");
    fs::write(
        &root,
        "\
@#includepath \"sub\"
@#for i in [\"a\", \"b\"]
@#include i + \".inc\"
@#endfor
@#if 0
@#include \"dormant.inc\"
@#endif
",
    )
    .unwrap();
    fs::write(sub.join("a.inc"), "parameters a_hit; a_hit = 1;\n").unwrap();
    fs::write(sub.join("b.inc"), "parameters b_hit; b_hit = 1;\n").unwrap();
    fs::write(
        sub.join("dormant.inc"),
        "parameters dormant_hit; dormant_hit = 1;\n",
    )
    .unwrap();
    let uri = root.to_string_lossy().to_string();
    let mut ws = Workspace::new();
    ws.update_document(&uri, fs::read_to_string(&root).unwrap());
    let model = ws.get_effective_model(&uri).unwrap();
    let names = param_names(model);
    assert!(names.contains(&"a_hit".to_string()), "{names:?}");
    assert!(names.contains(&"b_hit".to_string()), "{names:?}");
    assert!(!names.iter().any(|name| name == "dormant_hit"), "{names:?}");
    let records = ws.include_records(&uri).unwrap();
    assert!(records.unresolved.is_empty(), "{:?}", records.unresolved);
    assert!(!records
        .resolved
        .iter()
        .any(|record| record.filename.contains("dormant")));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn fatal_macro_failure_drops_later_body_and_keeps_earlier_messages() {
    let missing = "\
@#echo \"before\"
@#include \"absent.inc\"
@#echo \"after missing\"
@#include \"late.inc\"
var y; model; y=0; end;
";
    let mut workspace = Workspace::new();
    let uri = virtual_uri("fatal-stop.mod");
    workspace.update_document(&uri, missing);
    let report = workspace.expand_report(&uri).unwrap();
    assert!(
        report
            .macro_messages
            .iter()
            .any(|message| message.message.contains("before")),
        "{:?}",
        report.macro_messages
    );
    assert!(
        !report
            .macro_messages
            .iter()
            .any(|message| message.message.contains("after")),
        "{:?}",
        report.macro_messages
    );
    assert!(
        !report.effective_text.contains("y = 0"),
        "{}",
        report.effective_text
    );
    assert!(
        !report.effective_text.contains("late.inc"),
        "{}",
        report.effective_text
    );
    assert_eq!(report.n_equations, 0);
    let diags = check_file(missing, "absent_root.mod");
    assert!(diags.iter().any(|row| row.code == "E061"), "{diags:?}");

    let halted = "\
@#echo \"before\"
@#error \"halt\"
var y; model; y=1; end;
";
    let report = expand_report(halted);
    assert!(has_code(halted, "E064", "halt"), "{:?}", codes(halted));
    assert!(
        report
            .macro_messages
            .iter()
            .any(|message| message.message.contains("before")),
        "{:?}",
        report.macro_messages
    );
    assert!(
        !report.effective_text.contains("y = 1"),
        "{}",
        report.effective_text
    );
    assert_eq!(report.n_equations, 0);

    let payload = dynare_expand(missing, None, None);
    assert_eq!(payload["complete"], false);
    assert_eq!(payload["status"], "incomplete");
    assert_eq!(payload["n_equations"], 0);
    let messages = payload["macro_messages"].as_array().unwrap();
    assert!(messages
        .iter()
        .any(|row| row["kind"] == "echo" && row["message"].as_str().unwrap().contains("before")));
    assert!(!messages
        .iter()
        .any(|row| row["message"].as_str().unwrap_or("").contains("after")));
    assert!(messages[0].get("location").is_some());
    let quiet = dynare_expand("var y; model; y=0; end;", None, None);
    assert!(quiet.get("status").is_none(), "{quiet}");
    assert_eq!(quiet["macro_messages"].as_array().unwrap().len(), 0);
}

#[test]
fn debug_directives_use_pinned_grammar_and_the_written_save_line() {
    assert!(
        has_code(
            "@#echomacrovars 1\n",
            "E062",
            "syntax error, unexpected NUMBER, expecting EOL"
        ),
        "{:?}",
        codes("@#echomacrovars 1\n")
    );
    assert!(has_code(
        "@#echomacrovars (save\n",
        "E062",
        "syntax error, unexpected EOL, expecting RPAREN"
    ));
    assert!(has_code(
        "@#echomacrovars (save) 1\n",
        "E062",
        "syntax error, unexpected NUMBER, expecting EOL"
    ));
    assert!(has_code(
        "@#line 99\n",
        "E062",
        "syntax error, unexpected NUMBER, expecting QUOTED_STRING"
    ));
    assert!(has_code(
        "@#line \"f\"\n",
        "E062",
        "syntax error, unexpected EOL, expecting NUMBER"
    ));
    let accepted = "@#define foo = 1\n@#echomacrovars foo\n";
    assert!(
        !analyze(&parse(accepted))
            .iter()
            .any(|row| row.code == "E062"),
        "{:?}",
        codes(accepted)
    );

    let named = "@#line \"other.mod\" 9\nvar y; model; y=@{missing}; end;\n";
    let set = dygnosis::check_file_with_origins(named, "real_root.mod");
    let index = set
        .diagnostics
        .iter()
        .position(|row| row.code == "E063")
        .expect("E063");
    assert!(
        set.origins[index]
            .as_ref()
            .map(|origin| origin.file.as_str())
            != Some("other.mod"),
        "{:?}",
        set.origins[index]
    );
    assert!(!set.diagnostics[index].message.contains("other.mod"));

    let mut ws = Workspace::new();
    let root = virtual_uri("save_root.mod");
    let child = virtual_uri("save.inc");
    ws.update_document(&root, "@#define x = 3\n@#include \"save.inc\"\n");
    ws.update_document(&child, "@#echomacrovars(save) x\n");
    let report = ws.expand_report(&root).unwrap();
    let text = &report.effective_text;
    assert!(text.contains("options_.macrovars_line_1.x = 3;"), "{text}");
    assert!(!text.contains("options_.macrovars_line_2"), "{text}");
    assert!(!text.contains(" ;"), "{text}");
}

#[test]
fn unicode_defines_do_not_panic() {
    for source in [
        "@#define s=\"€x\"\nvar y; model; y=@{s}; end;\n",
        "@#define s = \"€x\"\nvar y; model; y=@{s}; end;\n",
    ] {
        let report = expand_report(source);
        assert!(
            report.effective_text.contains("€x"),
            "{source} -> {}",
            report.effective_text
        );
    }
}

#[test]
fn nested_loops_and_repeated_text_stop_at_named_limits() {
    let loops = "\
@#for i in 1:100
@#for j in 1:100
@#for k in 1:100
// x
@#endfor
@#endfor
@#endfor
var y; model; y=0; end;
";
    let diags = analyze(&parse(loops));
    assert!(
        diags
            .iter()
            .any(|row| row.code == "I211" && row.message.contains("iteration work")),
        "{diags:?}"
    );
    assert!(
        !diags.iter().any(|row| row.code.starts_with('E')),
        "{diags:?}"
    );
    let report = expand_report(loops);
    assert!(!report.complete);
    assert_eq!(report.n_equations, 0);
    assert!(
        !report.effective_text.contains("y = 0"),
        "{}",
        report.effective_text
    );

    // Source text, not `@{s}`: copying a macro string also spends its byte
    // length as iteration work, so that shape stops before the output cap.
    let line = "x".repeat(1000);
    let emit = format!("@#for i in 1:9000\n{line}\n@#endfor\nvar y; model; y=0; end;\n");
    let diags = analyze(&parse(&emit));
    assert!(
        diags
            .iter()
            .any(|row| row.code == "I211" && row.message.contains("output size")),
        "{diags:?}"
    );
    let report = expand_report(&emit);
    assert!(!report.complete);
    assert_eq!(report.n_equations, 0);
    assert!(
        !report.effective_text.contains("y = 0"),
        "{}",
        report.effective_text
    );
}

#[test]
fn interpolation_does_not_quote_nested_strings() {
    let source = "\
@#echo [\"a\",\"b\"]
@#echo (\"a\",\"b\")
parameters p;
p=@{[\"1\",\"2\"]};
var y;
model;
y=0;
end;
";
    let report = expand_report(source);
    assert!(
        report.effective_text.contains("[1, 2]"),
        "{}",
        report.effective_text
    );
    assert!(
        !report.effective_text.contains("[\"1\""),
        "{}",
        report.effective_text
    );
    let echoes: Vec<_> = report
        .macro_messages
        .iter()
        .map(|message| message.message.clone())
        .collect();
    assert_eq!(echoes, ["[a, b]", "(a, b)"], "{echoes:?}");
    assert!(!has_code(source, "E001", "unexpected"));
}

#[test]
fn define_string_keeps_a_raw_newline() {
    let source = "@#define s = \"0\n+1\"\nvar y;\nmodel;\ny=@{s};\nend;\n";
    assert!(
        !has_code(source, "E062", "syntax error"),
        "{:?}",
        codes(source)
    );
    let report = expand_report(source);
    assert!(
        report.effective_text.contains('0') && report.effective_text.contains("+1"),
        "{}",
        report.effective_text
    );
    assert_eq!(report.n_equations, 1, "{}", report.effective_text);
}

#[test]
fn missing_assignment_separators_are_reported() {
    let fire = [
        "parameters beta alpha;\nbeta=1 alpha=2;\nvar y;\nmodel;\ny=0;\nend;\n",
        "@#define gen = \"alpha = 2\"\nparameters beta alpha;\nbeta=1 @{gen};\nvar y;\nmodel;\ny=0;\nend;\n",
        "var y z;\nmodel;\ny=0;\nz=0;\nend;\ninitval;\ny=1 z=2;\nend;\n",
        "@#define gen = \"z = 2\"\nvar y z;\nmodel;\ny=0;\nz=0;\nend;\ninitval;\ny = 1 @{gen};\nend;\n",
    ];
    for source in fire {
        assert!(
            has_code(source, "E001", "missing semicolon"),
            "{source}\n{:?}",
            codes(source)
        );
    }
    let quiet = [
        "parameters beta alpha;\nbeta=1; alpha=2;\nvar y;\nmodel;\ny=0;\nend;\n",
        "@#define gen = \"alpha = 2\"\nparameters beta alpha;\nbeta=1; @{gen};\nvar y;\nmodel;\ny=0;\nend;\n",
        "var y z;\nmodel;\ny=0;\nz=0;\nend;\ninitval;\ny=1; z=2;\nend;\n",
        "@#define gen = \"z = 2\"\nvar y z;\nmodel;\ny=0;\nz=0;\nend;\ninitval;\ny = 1; @{gen};\nend;\n",
    ];
    for source in quiet {
        assert!(
            !has_code(source, "E001", "missing semicolon"),
            "{source}\n{:?}",
            codes(source)
        );
    }
}

#[test]
fn two_include_sites_in_one_loop_keep_every_hit() {
    let root = "\
var a b c d e;
model;
@#for i in [\"a\",\"b\",\"c\"]
@#include i+\".inc\"
@#if i != \"c\"
@#include i+\"2.inc\"
@#endif
@#endfor
end;
";
    let mut files = HashMap::new();
    let root_uri = virtual_uri("two.mod");
    files.insert(root_uri.clone(), root.to_string());
    for (name, body) in [
        ("a.inc", "a=0;\n"),
        ("b.inc", "b=0;\n"),
        ("c.inc", "c=0;\n"),
        ("a2.inc", "d=0;\n"),
        ("b2.inc", "e=0;\n"),
    ] {
        files.insert(virtual_uri(name), body.to_string());
    }
    let mut ws = Workspace::new();
    for (uri, text) in &files {
        ws.update_document(uri, text.clone());
    }
    let model = ws.get_effective_model(&root_uri).unwrap();
    let names: Vec<_> = model
        .equations
        .iter()
        .map(|equation| equation.lhs.clone())
        .collect();
    assert_eq!(
        names,
        ["a", "d", "b", "e", "c"],
        "{}\n{names:?}",
        model.source
    );
}

#[test]
fn echo_without_final_newline_stays_in_the_child() {
    let root = "// one\n// two\n@#include \"child.inc\"\n";
    for child in ["@#echo \"child\"", "@#echo \"child\"\n"] {
        let mut ws = Workspace::new();
        let root_uri = virtual_uri("echo-root.mod");
        let child_uri = virtual_uri("child.inc");
        ws.update_document(&root_uri, root);
        ws.update_document(&child_uri, child);
        let report = ws.expand_report(&root_uri).unwrap();
        let message = report
            .macro_messages
            .iter()
            .find(|message| message.message == "child")
            .unwrap_or_else(|| panic!("{child:?} {:?}", report.macro_messages));
        let file = message.file.as_deref().unwrap_or("");
        assert!(file.ends_with("child.inc"), "{child:?} file={file}");
        assert_eq!(message.span.start, 0, "{child:?} {:?}", message.span);
    }
}

#[test]
fn pinned_debug_grammar_matches_dynare_7_2_when_present() {
    let binary = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("SKIP pinned debug grammar: Dynare 7.2 preprocessor is absent");
        return;
    }
    for (source, needle) in [
        (
            "@#echomacrovars 1\n",
            "syntax error, unexpected NUMBER, expecting EOL",
        ),
        (
            "@#echomacrovars (save\n",
            "syntax error, unexpected EOL, expecting RPAREN",
        ),
        (
            "@#line 99\n",
            "syntax error, unexpected NUMBER, expecting QUOTED_STRING",
        ),
        (
            "@#line \"f\"\n",
            "syntax error, unexpected EOL, expecting NUMBER",
        ),
    ] {
        let result = dygnosis::run_preprocessor(
            source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        let text = result.raw_stdout.clone() + &result.raw_stderr;
        assert!(!result.success, "{source}: {text}");
        assert!(text.contains(needle), "{source}: {text}");
        assert!(has_code(source, "E062", needle), "{:?}", codes(source));
    }
}
