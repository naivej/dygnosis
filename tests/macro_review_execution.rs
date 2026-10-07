//! Independent slice 11 review regressions: parse before execution, literal
//! inline markers, exact loop order, and fatal failures in included files.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use dygnosis::diagnostic::{analyze, Severity};
use dygnosis::expand::expand_report;
use dygnosis::{dynare_diagnose, dynare_expand, parse, JsonStage};

const MODEL: &str = "var y;\nmodel; y=0; end;\n";

fn official_macro_output(source: &str) -> Option<String> {
    official_macro_run(source, &[], false).map(|(success, text)| {
        assert!(success, "{text}");
        text
    })
}

fn official_macro_run(source: &str, files: &[(&str, &str)], check: bool) -> Option<(bool, String)> {
    let binary = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        return None;
    }
    let temporary = std::env::temp_dir()
        .canonicalize()
        .expect("temporary directory");
    let directory = temporary.join(format!(
        "dygnosis-macro-review-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    let file = directory.join("probe.mod");
    std::fs::write(&file, source).unwrap();
    for (name, text) in files {
        std::fs::write(directory.join(name), text).unwrap();
    }
    let mut command = std::process::Command::new(binary);
    command.arg(&file).current_dir(&directory);
    if check {
        command.args(["json=check", "onlyjson"]);
    } else {
        command.arg("onlymacro");
    }
    let output = command.output().expect("pinned macro processor");
    let text = (String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr))
        .replace("\r\n", "\n");
    assert!(directory.canonicalize().unwrap().starts_with(&temporary));
    std::fs::remove_dir_all(&directory).unwrap();
    Some((output.status.success(), text))
}

fn errors(source: &str) -> Vec<(String, String)> {
    analyze(&parse(source))
        .into_iter()
        .filter(|row| row.severity == Severity::Error)
        .map(|row| (row.code.to_string(), row.message))
        .collect()
}

fn syntax_cases() -> Vec<(&'static str, String, &'static str)> {
    [
        (
            "inactive define",
            "@#if false\n@#define x=1+\n@#endif\n",
            "syntax error, unexpected EOL",
        ),
        (
            "inactive include",
            "@#if false\n@#include 1+\n@#endif\n",
            "syntax error, unexpected EOL",
        ),
        (
            "inactive if",
            "@#if false\n@#if 1+\n@#endif\n@#endif\n",
            "syntax error, unexpected EOL",
        ),
        (
            "inactive interpolation",
            "@#if false\nparameters p_@{1+};\n@#endif\n",
            "syntax error, unexpected END_EVAL",
        ),
        (
            "inactive unknown directive",
            "@#if false\n@#nosuch 1\n@#endif\n",
            "character unrecognized by lexer",
        ),
        (
            "empty loop",
            "@#for i in []\n@#define x=1+\n@#endfor\n",
            "syntax error, unexpected EOL",
        ),
        (
            "taken elseif",
            "@#if 1\n@#elseif 1+\n@#endif\n",
            "syntax error, unexpected EOL",
        ),
        (
            "error before syntax",
            "@#error \"halt\"\n@#define x=1+\n",
            "syntax error, unexpected EOL",
        ),
        (
            "unknown before syntax",
            "@#echo missing\n@#define x=1+\n",
            "syntax error, unexpected EOL",
        ),
        (
            "else arguments",
            "@#if 0\n@#else 1\n@#endif\n",
            "syntax error, unexpected TEXT, expecting EOL",
        ),
        (
            "endif arguments",
            "@#if 1\n@#endif 1\n",
            "syntax error, unexpected TEXT, expecting EOL",
        ),
        (
            "endfor arguments",
            "@#for i in [1]\n\n@#endfor 1\n",
            "syntax error, unexpected TEXT, expecting EOL",
        ),
        (
            "duplicate else",
            "@#if 0\n@#else\n@#else\n@#endif\n",
            "syntax error, unexpected ELSE, expecting ENDIF",
        ),
        (
            "elseif after else",
            "@#if 0\n@#else\n@#elseif 1\n@#endif\n",
            "syntax error, unexpected ELSEIF, expecting ENDIF",
        ),
        (
            "nonvariable binder",
            "@#for 1 in [1]\n@#echo 1\n@#endfor\n",
            "For loop indices must be a variable or a tuple",
        ),
        (
            "nonvariable tuple binder",
            "@#for (i,2) in [(1,2)]\n@#echo 1\n@#endfor\n",
            "For loop indices must be variables",
        ),
        (
            "body syntax before binder",
            "@#for 1 in [1]\n@#define x=1+\n@#endfor\n",
            "syntax error, unexpected EOL",
        ),
        (
            "debug save name",
            "@#echomacrovars(notsave)\n",
            "syntax error, unexpected IDENTIFIER, expecting SAVE",
        ),
        (
            "debug reserved name",
            "@#echomacrovars true\n",
            "syntax error, unexpected TRUE, expecting EOL",
        ),
        (
            "line trailing power",
            "@#line \"f\" 1^2\n",
            "syntax error, unexpected POWER, expecting EOL",
        ),
        (
            "line trailing comparison",
            "@#line \"f\" 1>=2\n",
            "syntax error, unexpected GREATER_EQUAL, expecting EOL",
        ),
    ]
    .into_iter()
    .map(|(name, prefix, needle)| (name, format!("{prefix}{MODEL}"), needle))
    .collect()
}

#[test]
fn the_whole_macro_file_is_parsed_before_execution() {
    for (name, source, needle) in syntax_cases() {
        assert_eq!(
            errors(&source),
            vec![("E062".to_string(), needle.to_string())],
            "{name}: {source}"
        );
        let report = expand_report(&source);
        assert!(!report.complete, "{name}");
        assert_eq!(report.n_equations, 0, "{name}");
        assert!(
            report.macro_messages.is_empty(),
            "{name}: {:?}",
            report.macro_messages
        );
    }
    for prefix in [
        "@#if false\n@#echo missing\n@#endif\n",
        "@#for i in []\n@#echo missing\n@#endfor\n",
    ] {
        let source = format!("{prefix}{MODEL}");
        assert!(errors(&source).is_empty(), "{source}");
        assert!(expand_report(&source).complete, "{source}");
    }
}

#[test]
fn directive_fallback_text_uses_an_explicit_source_context_bound() {
    let source = format!("@#xdefine x=3\n\n@#echo x\n{MODEL}");
    let rows = analyze(&parse(&source));
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].code, "I211");
    assert!(rows[0].message.contains("source context"));
    let report = expand_report(&source);
    assert!(!report.complete);
    assert_eq!(report.n_equations, 0);
    assert!(report.navigation.is_empty());
    let canonical = source.replace("@#xdefine", "@#define");
    assert!(expand_report(&canonical).complete);
    assert_eq!(expand_report(&canonical).macro_messages[0].message, "3");
    if let Some((success, text)) = official_macro_run(&source, &[], true) {
        assert!(success, "{text}");
        assert!(text.contains("): 3\n"), "{text}");
    }
}

#[test]
fn inline_markers_remain_text_and_compact_keywords_are_directives() {
    for prefix in [
        "parameters p; @#define x=1\np=@{x};\n",
        "/* @#define x=1\n*/\nparameters p; p=@{x};\n",
    ] {
        let source = format!("{prefix}{MODEL}");
        assert_eq!(
            errors(&source),
            vec![("E063".to_string(), "Unknown variable x".to_string())]
        );
    }
    let quoted = format!("disp(\"@#define x=1\");\n{MODEL}");
    assert!(
        errors(&quoted).is_empty(),
        "{quoted}: {:?}",
        errors(&quoted)
    );
    let report = expand_report(&quoted);
    assert!(
        report.complete,
        "{report:?}; model={:?}",
        parse(&quoted).macro_type_errors
    );
    let compact = format!("@#definex=2\n@#if1\n@#echo x\n@#endif\n{MODEL}");
    assert!(errors(&compact).is_empty(), "{compact}");
    let report = expand_report(&compact);
    assert!(report.complete);
    assert_eq!(report.macro_messages[0].message, "2");
}

#[test]
fn defined_conditions_require_the_entire_variable_expression() {
    for command in ["ifdef", "ifndef"] {
        for argument in ["1", "x+1", "\"x\""] {
            let source = format!("@#define x=1\n@#{command} {argument}\n@#endif\n{MODEL}");
            assert_eq!(
                errors(&source),
                vec![(
                    "E283".to_string(),
                    "The condition must be a variable name".to_string()
                )],
                "{source}"
            );
        }
        let dormant = format!("@#if false\n@#{command} 1\n@#endif\n@#endif\n{MODEL}");
        assert!(errors(&dormant).is_empty(), "{dormant}");
    }
}

#[test]
fn line_numbers_accept_the_pinned_number_tokens() {
    for number in ["inf", "nan", "INF", "1.2", "2D2", ".5", "1."] {
        let source = format!("@#line \"fake.mod\" {number}\n{MODEL}");
        assert!(
            errors(&source).is_empty(),
            "{source}: {:?}",
            errors(&source)
        );
        assert!(expand_report(&source).complete);
    }
    let source = format!("@#line \"fake.mod\" -1\n{MODEL}");
    assert_eq!(
        errors(&source),
        vec![(
            "E062".to_string(),
            "syntax error, unexpected MINUS, expecting NUMBER".to_string()
        )]
    );
}

#[test]
fn a_filtered_singleton_tuple_keeps_tuple_binding() {
    let source = format!("@#for (i,) in [(1,2)] when true\n@#echo i\n@#endfor\n{MODEL}");
    let needle = "The number of elements in the input  set tuple are not the same as the number of elements in the output expression tuple";
    assert_eq!(
        errors(&source),
        vec![("E284".to_string(), needle.to_string())]
    );
    assert!(!expand_report(&source).complete);
    if let Some((success, text)) = official_macro_run(&source, &[], true) {
        assert!(!success, "{text}");
        assert!(text.contains(needle), "{text}");
    }
    let unfiltered = source.replace(" when true", "");
    let report = expand_report(&unfiltered);
    assert!(report.complete);
    assert_eq!(report.macro_messages[0].message, "(1, 2)");
}

#[test]
fn loop_bodies_execute_before_a_later_tuple_refusal() {
    let echo = format!("@#for (i,j) in [(1,2),(3,)]\n@#echo i\n@#endfor\n{MODEL}");
    let report = expand_report(&echo);
    assert!(!report.complete);
    assert_eq!(
        report
            .macro_messages
            .iter()
            .map(|message| message.message.as_str())
            .collect::<Vec<_>>(),
        vec!["1"]
    );
    assert_eq!(
        errors(&echo),
        vec![(
            "E284".to_string(),
            "Encountered tuple of size 1 but only have 2 index variables".to_string()
        )]
    );
    let abort = echo.replace("@#echo i", "@#error \"halt first\"");
    assert_eq!(
        errors(&abort),
        vec![(
            "E064".to_string(),
            "Macro-processing error: halt first".to_string()
        )]
    );
    let empty = format!("@#for () in [()]\n@#echo 1\n@#endfor\n@#echo defined(_i)\n{MODEL}");
    let report = expand_report(&empty);
    assert!(report.complete, "{empty}: {:?}", errors(&empty));
    assert_eq!(
        report
            .macro_messages
            .iter()
            .map(|message| message.message.as_str())
            .collect::<Vec<_>>(),
        vec!["1", "false"]
    );
}

#[test]
fn include_hits_distinguish_iterations_with_the_same_index_value() {
    let root = "@#define n=0\n@#for i in [1,1]\n@#define n=n+1\n@#include (string)n+\".inc\"\n@#endfor\nvar y; model; y=0; end;\n";
    let files = HashMap::from([
        ("/probe.mod".to_string(), root.to_string()),
        ("/1.inc".to_string(), "@#echo \"1.inc\"\n".to_string()),
        ("/2.inc".to_string(), "@#echo \"2.inc\"\n".to_string()),
    ]);
    let report = dynare_expand(root, Some("/probe.mod"), Some(&files));
    assert_eq!(report["complete"], true, "{report}");
    assert_eq!(
        report["macro_messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["message"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["1.inc", "2.inc"]
    );
}

#[test]
fn a_failed_interpolation_is_not_part_of_the_emitted_prefix() {
    let source = "@#echo \"before\"\nvar y; model; y=@{missing}; end;\n@#echo \"after\"\n";
    let report = expand_report(source);
    assert!(!report.complete);
    assert_eq!(report.n_equations, 0);
    assert!(
        !report.effective_text.contains("@{missing}"),
        "{}",
        report.effective_text
    );
    assert!(
        !report.effective_text.contains("end"),
        "{}",
        report.effective_text
    );
    assert_eq!(
        report
            .macro_messages
            .iter()
            .map(|message| message.message.as_str())
            .collect::<Vec<_>>(),
        vec!["before"]
    );
    assert_eq!(
        errors(source),
        vec![("E063".to_string(), "Unknown variable missing".to_string())]
    );
}

#[test]
fn child_macro_failures_stop_later_file_reads_and_preserve_the_refusal() {
    let root = "@#include \"child.inc\"\n@#include \"late.inc\"\nvar y; model; y=0; end;\n";
    for (child, code, needle) in [
        ("@#error \"halt\"\n", "E064", "halt"),
        ("@#echo missing\n", "E063", "Unknown variable missing"),
    ] {
        let files = HashMap::from([
            ("/probe.mod".to_string(), root.to_string()),
            ("/child.inc".to_string(), child.to_string()),
        ]);
        let diagnostics = dynare_diagnose(root, Some("/probe.mod"), Some(&files));
        assert_eq!(diagnostics.len(), 1, "{child}: {diagnostics:?}");
        assert_eq!(diagnostics[0].code, code);
        assert!(diagnostics[0].message.contains(needle));
        let report = dynare_expand(root, Some("/probe.mod"), Some(&files));
        assert_eq!(report["complete"], false);
        assert_eq!(report["n_equations"], 0);
    }
}

#[test]
fn nested_header_effects_at_one_point_remain_in_order() {
    let root =
        "@#include (string)[\"child.inc\" for p in [1]]\n@#echo p\nvar y; model; y=0; end;\n";
    let child = "@#for j in [i for i in [2,3]]\n@#include (string)j+\".inc\"\n@#endfor\n@#echo i\n";
    let files = HashMap::from([
        ("/probe.mod".to_string(), root.to_string()),
        ("/[child.inc]".to_string(), child.to_string()),
        ("/2.inc".to_string(), "@#echo j\n".to_string()),
        ("/3.inc".to_string(), "@#echo j\n".to_string()),
    ]);
    let report = dynare_expand(root, Some("/probe.mod"), Some(&files));
    assert_eq!(report["complete"], true, "{report}");
    assert_eq!(
        report["macro_messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["message"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["2", "3", "3", "1"]
    );
}

#[test]
fn child_syntax_refusals_keep_only_messages_already_executed() {
    let root = "@#echo \"before\"\nvar y;\n@#if1\n@#include \"child.inc\"\n@#endif\n@#echo \"after\"\nmodel; y=0; end;\n";
    let child = "@#else\n";
    let files = HashMap::from([
        ("/probe.mod".to_string(), root.to_string()),
        ("/child.inc".to_string(), child.to_string()),
    ]);
    let report = dynare_expand(root, Some("/probe.mod"), Some(&files));
    assert_eq!(report["complete"], false);
    assert_eq!(report["n_equations"], 0);
    assert_eq!(
        report["macro_messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["message"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["before"]
    );
    assert!(
        report["effective_text"].as_str().unwrap().contains("var y"),
        "{report}"
    );
    assert!(
        !report["effective_text"].as_str().unwrap().contains("model"),
        "{report}"
    );
    if let Some((success, text)) = official_macro_run(root, &[("child.inc", child)], true) {
        assert!(!success, "{text}");
        assert!(text.contains("): before\n"), "{text}");
        assert!(!text.contains("): after\n"), "{text}");
    }
}

#[test]
fn a_refused_include_directory_stops_counts_and_later_messages() {
    let root = "@#echo \"before\"\n@#includepath \"absent-review-directory\"\n@#echo \"after\"\nvar y; model; y=0; end;\n";
    let files = HashMap::from([("/probe.mod".to_string(), root.to_string())]);
    let rows = dynare_diagnose(root, Some("/probe.mod"), Some(&files));
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].code, "E304");
    assert_eq!(
        rows[0].message,
        "absent-review-directory does not evaluate to a valid directory"
    );
    let report = dynare_expand(root, Some("/probe.mod"), Some(&files));
    assert_eq!(report["complete"], false, "{report}");
    assert_eq!(report["n_equations"], 0);
    assert_eq!(
        report["macro_messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["message"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["before"]
    );
    if let Some((success, text)) = official_macro_run(root, &[], true) {
        assert!(!success, "{text}");
        assert!(
            text.contains("absent-review-directory does not evaluate to a valid directory"),
            "{text}"
        );
        assert!(!text.contains("): after\n"), "{text}");
    }
}

#[test]
fn included_files_keep_their_own_macro_grammar_boundary() {
    for (root, child, needle) in [
        (
            "var y;\n@#if1\n@#include \"child.inc\"\n@#endif\nmodel; y=0; end;\n",
            "@#else\n",
            "syntax error, unexpected ELSE, expecting end of file",
        ),
        (
            "var y;\n@#for i in [1]\n@#include \"child.inc\"\n@#endfor\nmodel; y=0; end;\n",
            "@#endfor\n",
            "syntax error, unexpected ENDFOR, expecting end of file",
        ),
        (
            "var y;\n@#if1\n@#include \"child.inc\"\n@#endif\nmodel; y=0; end;\n",
            "@#if1\n",
            "syntax error, unexpected end of file, expecting ENDIF",
        ),
    ] {
        let files = HashMap::from([
            ("/probe.mod".to_string(), root.to_string()),
            ("/child.inc".to_string(), child.to_string()),
        ]);
        let rows = dynare_diagnose(root, Some("/probe.mod"), Some(&files));
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].code, "E062");
        assert_eq!(rows[0].message, needle);
        let report = dynare_expand(root, Some("/probe.mod"), Some(&files));
        assert_eq!(report["complete"], false, "{report}");
        assert_eq!(report["n_equations"], 0);
        assert!(report["navigation"].as_array().unwrap().is_empty());
        if let Some((success, text)) = official_macro_run(root, &[("child.inc", child)], true) {
            assert!(!success, "{text}");
            assert!(text.contains(needle), "{text}");
        }
    }
}

#[test]
fn a_late_missing_target_keeps_earlier_include_messages() {
    let root = "@#for i in [\"ok\",\"missing\"]\n@#include i+\".inc\"\n@#endfor\nvar y; model; y=0; end;\n";
    let files = HashMap::from([
        ("/probe.mod".to_string(), root.to_string()),
        ("/ok.inc".to_string(), "@#echo \"before\"\n".to_string()),
    ]);
    let rows = dynare_diagnose(root, Some("/probe.mod"), Some(&files));
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].code, "E061");
    assert!(rows[0].message.contains("missing.inc"));
    let report = dynare_expand(root, Some("/probe.mod"), Some(&files));
    assert_eq!(report["complete"], false);
    assert_eq!(report["n_equations"], 0);
    assert_eq!(
        report["macro_messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["message"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["before"]
    );
    if let Some((success, text)) =
        official_macro_run(root, &[("ok.inc", "@#echo \"before\"\n")], true)
    {
        assert!(!success, "{text}");
        assert!(text.contains("): before\n"), "{text}");
        assert!(text.contains("Could not open missing.inc"), "{text}");
    }
}

#[test]
fn crlf_macro_messages_exclude_the_line_terminator() {
    let root = "@#define a=1\r\n@#include \"child.inc\"\r\nvar y; model; y=0; end;\r\n";
    for (child, expected_value, expected_column) in [
        ("@#echo a\r\n", "1", 9),
        ("@#echo length(\"a\r\nb\")\r\n", "4", 4),
    ] {
        let files = HashMap::from([
            ("/probe.mod".to_string(), root.to_string()),
            ("/child.inc".to_string(), child.to_string()),
        ]);
        let report = dynare_expand(root, Some("/probe.mod"), Some(&files));
        assert_eq!(report["complete"], true, "{report}");
        let message = &report["macro_messages"][0];
        assert_eq!(message["message"], expected_value);
        assert_eq!(
            message["location"]["end_column"], expected_column,
            "{message}"
        );
        assert_eq!(
            message["location"]["end_line"],
            if child.contains("a\r\nb") { 2 } else { 1 }
        );
    }
}

#[test]
fn comments_do_not_supply_continuation_or_when_tokens() {
    for prefix in [
        "@#define x=1 // \\\\\n@#echo x\n",
        "@#define x=1 // first // \\\\\n@#echo x\n",
        "@#for i in [1] // when missing\n@#echo i\n@#endfor\n",
    ] {
        let source = format!("{prefix}{MODEL}");
        let report = expand_report(&source);
        assert!(report.complete, "{source}: {:?}", errors(&source));
        assert_eq!(
            report
                .macro_messages
                .iter()
                .map(|message| message.message.as_str())
                .collect::<Vec<_>>(),
            vec!["1"]
        );
        if let Some(text) = official_macro_output(&source) {
            assert!(text.contains("): 1\n"), "{text}");
        }
    }
    let source = format!("@#define s=\"a\nb\" \\\\\n+\"c\"\n@#echo s\n{MODEL}");
    let report = expand_report(&source);
    assert!(report.complete, "{source}: {:?}", errors(&source));
    assert_eq!(report.macro_messages[0].message, "a\nbc");
    if let Some(text) = official_macro_output(&source) {
        assert!(text.contains("): a\nbc\n"), "{text}");
    }
}

#[test]
fn multiline_macro_strings_preserve_written_backslashes() {
    let source = format!("@#define s=\"a\\\\\nb\"\n@#echo s\n{MODEL}");
    let report = expand_report(&source);
    assert!(report.complete, "{:?}", errors(&source));
    assert_eq!(report.macro_messages[0].message, "a\\\\\nb");
}

#[test]
fn thousands_of_adjacent_interpolations_use_one_final_lexer_join() {
    let adjacent = "@{\"x\"}".repeat(3000);
    let source = format!("var {adjacent};\nmodel; {adjacent}=0; end;\n");
    let report = expand_report(&source);
    assert!(report.complete, "{:?}", analyze(&parse(&source)));
    assert_eq!(report.n_equations, 1);
    let model = parse(&source);
    assert_eq!(model.name(model.endogenous[0].name), "x".repeat(3000));
}

#[test]
fn nested_if_origins_spend_the_root_work_budget_before_cloning() {
    let within = format!(
        "{}{}{MODEL}",
        "@#if1\n \n".repeat(200),
        "@#endif\n".repeat(200)
    );
    assert!(expand_report(&within).complete);
    let bounded = format!(
        "{}{}{MODEL}",
        "@#if1\n \n".repeat(1500),
        "@#endif\n".repeat(1500)
    );
    let report = expand_report(&bounded);
    assert!(!report.complete);
    assert_eq!(report.n_equations, 0);
    assert!(analyze(&parse(&bounded))
        .iter()
        .any(|row| row.code == "I211" && row.message.contains("iteration work")));
}

#[test]
fn non_utf8_path_values_name_the_mapping_limit() {
    for directive in ["include", "includepath"] {
        let source = format!("@#define s=\"€x\"\n@#{directive} s[1]\n{MODEL}");
        let rows = analyze(&parse(&source));
        assert_eq!(rows.len(), 1, "{source}: {rows:?}");
        assert_eq!(rows[0].code, "I211");
        assert!(rows[0].message.contains("non-UTF-8 byte slice"));
        assert!(!expand_report(&source).complete);
    }
}

#[test]
fn macrovars_selection_keeps_case_sensitive_names() {
    let source = format!("@#define x=1\n@#define X=2\n@#echomacrovars x\n{MODEL}");
    let report = expand_report(&source);
    assert!(report.complete);
    assert!(report.macro_messages[0].message.contains("  x = 1"));
    assert!(!report.macro_messages[0].message.contains("  X = 2"));
    if let Some(text) = official_macro_output(&source) {
        assert!(text.contains("  x = 1"));
        assert!(!text.contains("  X = 2"));
    }
    let function = format!("@#define x=1\n@#define X()=2\n@#echomacrovars X\n{MODEL}");
    let report = expand_report(&function);
    assert!(!report.macro_messages[0].message.contains("  X = "));
    assert!(report.macro_messages[0].message.contains("  X() = 2"));
    if let Some(text) = official_macro_output(&function) {
        assert!(!text.contains("  X = "));
        assert!(text.contains("  X() = 2"));
    }
}

#[test]
fn syntax_refusals_agree_with_dynare_7_2_when_present() {
    let binary = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("SKIP macro execution honesty: Dynare 7.2 is absent");
        return;
    }
    for (name, source, needle) in syntax_cases() {
        let result = dygnosis::run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        let text = result.raw_stdout + &result.raw_stderr;
        assert!(!result.success, "{name}: {text}");
        assert!(text.contains(needle), "{name}: {text}");
        assert_eq!(
            errors(&source),
            vec![("E062".to_string(), needle.to_string())],
            "{name}"
        );
    }
}
