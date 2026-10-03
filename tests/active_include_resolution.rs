use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dygnosis::{dynare_diagnose, dynare_model_info, Workspace};

const ROOT: &str = "C:/dygnosis-active-includes/root.mod";
const MODEL: &str = "var y; model; y=0; end;\n";

#[test]
fn dormant_includes_are_quiet_and_do_not_change_expansion() {
    for prefix in [
        "@#if 0\n@#include \"absent.inc\"\n@#endif\n",
        "@#if 0\n@#include \"bad.inc\"\n@#endif\n",
        "@#if 0\n@#include \"leak.inc\"\n@#endif\n",
        "@#if 0\n@#include \"unknown.inc\"\n@#include \"duplicate.inc\"\n@#endif\n",
        "@#for k in []\n@#include \"absent.inc\"\n@#endfor\n",
    ] {
        let text = format!("{prefix}{MODEL}");
        let files = HashMap::from([
            (ROOT.to_string(), text.clone()),
            (
                "C:/dygnosis-active-includes/bad.inc".to_string(),
                "@#if 1\n".to_string(),
            ),
            (
                "C:/dygnosis-active-includes/leak.inc".to_string(),
                "@#endif\nvar z; model; z=missing; end;\n@#if 0\n".to_string(),
            ),
            (
                "C:/dygnosis-active-includes/unknown.inc".to_string(),
                "var y; model; y=missing; end;\n".to_string(),
            ),
            (
                "C:/dygnosis-active-includes/duplicate.inc".to_string(),
                "var y y; model; y=0; end;\n".to_string(),
            ),
        ]);
        let diagnostics = dynare_diagnose(&text, Some(ROOT), Some(&files));
        assert!(
            !diagnostics
                .iter()
                .any(|d| matches!(d.code.as_str(), "E061" | "E062" | "E020" | "W031")),
            "{prefix}: {diagnostics:?}"
        );
        let mut workspace = Workspace::new();
        for (key, source) in files {
            workspace.update_document(&key, source);
        }
        let expanded = workspace.expand_report(ROOT).unwrap();
        assert!(
            !expanded.effective_text.contains("missing"),
            "{prefix}: {expanded:?}"
        );
        assert_eq!(expanded.n_equations, 1, "{prefix}: {expanded:?}");
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dygnosis-active-includes-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let path = self.0.canonicalize().unwrap();
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        assert!(path.starts_with(&temporary) && path != temporary);
        std::fs::remove_dir_all(path).unwrap();
    }
}

const FILES: &[(&str, &str)] = &[
    ("bad.inc", "@#if 1\n"),
    ("leak.inc", "@#else\nvar z; model; z=missing; end;\n"),
    ("flags.inc", "@#define ENABLED=0\n"),
    ("outer.inc", "@#include \"flags.inc\"\n"),
    ("loop.inc", "@#define ENABLED=k\n"),
    ("equation.inc", "y_@{k}=0;\n"),
    ("unknown.inc", "var y; model; y=missing; end;\n"),
    ("duplicate.inc", "var y y; model; y=0; end;\n"),
    ("missing.inc", "@#include \"absent.inc\"\n"),
    ("cycle.inc", "@#include \"root.mod\"\n"),
];

fn supplied(source: &str) -> HashMap<String, String> {
    std::iter::once((ROOT.to_string(), source.to_string()))
        .chain(FILES.iter().map(|(name, text)| {
            (
                format!("C:/dygnosis-active-includes/{name}"),
                text.to_string(),
            )
        }))
        .collect()
}

fn quiet_cases() -> Vec<String> {
    [
        "@#if 0\n@#include \"absent.inc\"\n@#include \"bad.inc\"\n@#include \"leak.inc\"\n@#include \"cycle.inc\"\n@#endif\n",
        "@#for k in []\n@#include \"bad.inc\"\n@#include \"absent.inc\"\n@#endfor\n",
        "@#include \"outer.inc\"\n@#if 1\n@#if ENABLED\n@#include \"absent.inc\"\n@#else\n@#define CHOSEN=1\n@#endif\n@#elseif 1\n@#include \"bad.inc\"\n@#else\n@#include \"leak.inc\"\n@#endif\n@#if CHOSEN != 1\n@#include \"absent.inc\"\n@#endif\n",
        "@#if 0\n@#includepath \"absent-directory\"\n@#endif\n",
        "@#if 0\n@#include \"unknown.inc\"\n@#include \"duplicate.inc\"\n@#endif\n",
        "@#for k in []\n@#includepath 7\n@#includepath \"absent-directory\"\n@#endfor\n",
        "@#include \"outer.inc\"\n@#for k in 1:2\n@#if ENABLED\n@#includepath 7\n@#endif\n@#endfor\n",
    ]
    .into_iter()
    .map(|prefix| format!("{prefix}{MODEL}"))
    .collect()
}

const ORDERED_LOOP: &str = "var y_1 y_2; model;\n@#for k in 1:2\n@#if k == 1\n@#include \"loop.inc\"\n@#endif\n@#if ENABLED == 1\n@#include \"equation.inc\"\n@#else\n@#include \"absent.inc\"\n@#endif\n@#endfor\nend;\n";

#[test]
fn evaluated_include_path_uses_executed_macro_definitions() {
    let directory = Scratch::new();
    std::fs::create_dir(directory.0.join("visible")).unwrap();
    std::fs::write(directory.0.join("visible/chosen.inc"), MODEL).unwrap();
    std::fs::write(directory.0.join("paths.inc"), "@#define P=\"visible\"\n").unwrap();
    let root = directory.0.join("root.mod").to_string_lossy().into_owned();
    let files_for = |source: &str| {
        HashMap::from([
            (root.clone(), source.to_string()),
            (
                directory
                    .0
                    .join("visible/chosen.inc")
                    .to_string_lossy()
                    .into_owned(),
                MODEL.to_string(),
            ),
            (
                directory.0.join("paths.inc").to_string_lossy().into_owned(),
                "@#define P=\"visible\"\n".to_string(),
            ),
        ])
    };
    for prefix in [
        "@#define P=\"visible\"\n@#includepath P\n",
        "@#define P=\"vis\"\n@#includepath P + \"ible\"\n",
        "@#include \"paths.inc\"\n@#includepath P\n",
        "@#define path(x)=x+\"ible\"\n@#includepath path(\"vis\")\n",
        "@#for P in [\"hidden\"]\n@#define P=\"visible\"\n@#endfor\n@#includepath P\n",
    ] {
        let source = format!("{prefix}@#include \"chosen.inc\"\n");
        let files = files_for(&source);
        let diagnostics = dynare_diagnose(&source, Some(&root), Some(&files));
        assert!(
            !diagnostics.iter().any(|d| d.severity == "ERROR"),
            "{source}: {diagnostics:?}"
        );
        assert_eq!(
            dynare_model_info(&source, Some(&root), Some(&files))["n_equations"],
            1
        );
        let preview = dygnosis::mcp::dynare_expand(&source, Some(&root), Some(&files));
        assert_eq!(preview["complete"], true, "{preview}");
        assert_eq!(preview["navigation"].as_array().unwrap().len(), 1);
        assert_pinned(&directory.0, &source, None);
    }
    let unsupported = "@#includepath (string) \"visible\"\n@#include \"chosen.inc\"\n";
    let files = files_for(unsupported);
    let diagnostics = dynare_diagnose(unsupported, Some(&root), Some(&files));
    assert!(
        !diagnostics.iter().any(|d| d.severity == "ERROR"),
        "{diagnostics:?}"
    );
    let info = dynare_model_info(unsupported, Some(&root), Some(&files));
    assert_eq!(info["status"], "incomplete", "{info}");
    assert_pinned(&directory.0, unsupported, None);
}

#[test]
fn executed_invalid_directory_is_a_revision_input_until_it_is_created() {
    let directory = Scratch::new();
    let source = format!("@#define P=\"created\"\n@#includepath P\n{MODEL}");
    write_files(&directory.0, &source);
    let root = directory.0.join("root.mod").to_string_lossy().into_owned();
    let mut workspace = Workspace::new();
    workspace.update_document(&root, &source);
    let before = workspace.input_revision(&root).unwrap();
    assert!(dygnosis::check_file(&source, &root)
        .iter()
        .any(|d| d.code == "E304"));
    assert!(!workspace.expand_report(&root).unwrap().navigation_complete);
    std::fs::create_dir(directory.0.join("created")).unwrap();
    let after = workspace.input_revision(&root).unwrap();
    assert_ne!(after, before);
    assert!(!dygnosis::check_file(&source, &root)
        .iter()
        .any(|d| d.code == "E304"));
    assert!(workspace.expand_report(&root).unwrap().navigation_complete);
}

#[test]
fn loop_bindings_remain_after_execution_and_empty_loops_preserve_prior_values() {
    for prefix in [
        "@#define k=1\n@#for k in [0]\n@#include \"loop.inc\"\n@#endfor\n",
        "@#for k in [0]\n@#define seen=k\n@#endfor\n",
        "@#define k=1\n@#for k in [3]\n@#for k in [0]\n@#include \"loop.inc\"\n@#endfor\n@#endfor\n",
        "@#for k in [1]\n@#define k=0\n@#endfor\n",
        "@#define k=0\n@#for k in []\n@#define k=1\n@#endfor\n",
    ] {
        let source = format!("{prefix}@#if k\n@#include \"absent.inc\"\n@#endif\nvar y; model; y=@{{k}}; end;\n");
        let files = supplied(&source);
        let diagnostics = dynare_diagnose(&source, Some(ROOT), Some(&files));
        assert!(!diagnostics.iter().any(|d| d.severity == "ERROR"), "{source}: {diagnostics:?}");
        let preview = dygnosis::mcp::dynare_expand(&source, Some(ROOT), Some(&files));
        assert_eq!(preview["complete"], true, "{preview}");
        assert!(preview["effective_text"].as_str().unwrap().contains("y = 0"), "{preview}");
        assert_eq!(dynare_model_info(&source, Some(ROOT), Some(&files))["n_equations"], 1);
        let directory = Scratch::new();
        write_files(&directory.0, &source);
        assert_pinned(&directory.0, &source, None);
    }
    for prefix in [
        "@#define k=0\n@#for k in [1]\n@#include \"loop.inc\"\n@#endfor\n",
        "@#define k=1\n@#for k in []\n@#define k=0\n@#endfor\n",
        "@#for k in [0]\n@#define k=1\n@#endfor\n",
    ] {
        let source = format!("{prefix}@#if k\n@#include \"absent.inc\"\n@#endif\n{MODEL}");
        let files = supplied(&source);
        assert!(dynare_diagnose(&source, Some(ROOT), Some(&files))
            .iter()
            .any(|d| d.code == "E061"));
        let directory = Scratch::new();
        write_files(&directory.0, &source);
        assert_pinned(&directory.0, &source, Some("Could not open absent.inc"));
    }
}

#[test]
fn e063_uses_executed_evaluator_errors_in_every_macro_context() {
    let cases = [
        ("var y; model; y=@{missing}; end;\n", Some("Unknown variable missing")),
        ("var y; model; y=@{missing_function(0)}; end;\n", Some("Unknown function missing_function")),
        ("@#define f(x)=x+missing\nvar y; model; y=@{f(0)}; end;\n", Some("Unknown variable missing")),
        ("@#define v=missing\nvar y; model; y=0; end;\n", Some("Unknown variable missing")),
        ("var y; model;\n@#for k in [0]\ny=@{missing};\n@#endfor\nend;\n", Some("Unknown variable missing")),
        ("@#for k in []\n@#define seen=k\n@#endfor\nvar y; model; y=@{k}; end;\n", Some("Unknown variable k")),
        ("@#include \"macro_unknown.inc\"\n", Some("Unknown variable missing")),
        ("@#if 0\n@#define v=missing\n@#include \"macro_unknown.inc\"\nvar z; model; z=@{missing_function(0)}; end;\n@#endif\nvar y; model; y=0; end;\n", None),
        ("@#define f(x)=x+missing\nvar y; model; y=0; end;\n", None),
        ("@#include \"functions.inc\"\nvar y; model; y=@{f(0)}; end;\n", None),
    ];
    for (source, message) in cases {
        let mut files = supplied(source);
        files.insert(
            "C:/dygnosis-active-includes/macro_unknown.inc".to_string(),
            "var y; model; y=@{missing}; end;\n".to_string(),
        );
        files.insert(
            "C:/dygnosis-active-includes/functions.inc".to_string(),
            "@#define f(x)=x\n".to_string(),
        );
        let diagnostics = dynare_diagnose(source, Some(ROOT), Some(&files));
        let unknown: Vec<_> = diagnostics.iter().filter(|d| d.code == "E063").collect();
        if let Some(message) = message {
            assert!(
                !unknown.is_empty() && unknown.iter().all(|d| d.message == message),
                "{source}: {diagnostics:?}"
            );
        } else {
            assert!(unknown.is_empty(), "{source}: {diagnostics:?}");
            assert_eq!(
                dynare_model_info(source, Some(ROOT), Some(&files))["n_equations"],
                1
            );
        }
        let directory = Scratch::new();
        write_files(&directory.0, source);
        std::fs::write(
            directory.0.join("macro_unknown.inc"),
            "var y; model; y=@{missing}; end;\n",
        )
        .unwrap();
        std::fs::write(directory.0.join("functions.inc"), "@#define f(x)=x\n").unwrap();
        assert_pinned(&directory.0, source, message);
    }
}

#[test]
fn one_loop_index_binds_the_tuple_and_multiple_indices_enforce_arity() {
    for (directive, refusal) in [
        ("@#for(i) in [(1,2)]", false),
        ("@#for(i,j) in [(1,2)]", false),
        ("@#for(i,j) in [(1,2,3)]", true),
    ] {
        let source = format!("{directive}\n\n@#endfor\n{MODEL}");
        let diagnostics = dynare_diagnose(&source, None, None);
        assert_eq!(
            diagnostics.iter().any(|d| d.code == "E284"),
            refusal,
            "{source}: {diagnostics:?}"
        );
        let directory = Scratch::new();
        assert_pinned(
            &directory.0,
            &source,
            refusal.then_some("Encountered tuple of size 3 but only have 2 index variables"),
        );
        if !refusal {
            assert_eq!(dynare_model_info(&source, None, None)["n_equations"], 1);
        }
    }
    let unsupported = "@#for(i) in [(1,2)]\n\n@#endfor\nvar y; model; y=@{i[1]}; end;\n";
    let diagnostics = dynare_diagnose(unsupported, None, None);
    assert!(
        !diagnostics.iter().any(|d| d.severity == "ERROR"),
        "{diagnostics:?}"
    );
    let info = dynare_model_info(unsupported, None, None);
    assert_eq!(info["status"], "incomplete", "{info}");
    let directory = Scratch::new();
    assert_pinned(&directory.0, unsupported, Some("You cannot index a tuple"));
}

#[test]
fn empty_for_body_refuses_at_closer_but_blank_comment_and_nested_bodies_accept() {
    for (body, refusal) in [
        ("", true),
        ("\n", false),
        ("  \n", false),
        ("// comment\n", false),
        ("/* comment */\n", false),
        ("@#define seen=k\n", false),
        ("@#for j in []\n@#define seen=j\n@#endfor\n", false),
    ] {
        let source = format!("@#for k in []\n{body}@#endfor\n{MODEL}");
        let diagnostics = dynare_diagnose(&source, None, None);
        let syntax: Vec<_> = diagnostics.iter().filter(|d| d.code == "E062").collect();
        if refusal {
            assert_eq!(syntax.len(), 1, "{source}: {diagnostics:?}");
            assert_eq!(syntax[0].message, "syntax error, unexpected ENDFOR");
            assert_eq!(
                (
                    syntax[0].line,
                    syntax[0].column,
                    syntax[0].end_line,
                    syntax[0].end_column
                ),
                (2, 1, 2, 9)
            );
        } else {
            assert!(syntax.is_empty(), "{source}: {diagnostics:?}");
            assert_eq!(dynare_model_info(&source, None, None)["n_equations"], 1);
        }
        let directory = Scratch::new();
        assert_pinned(
            &directory.0,
            &source,
            refusal.then_some("syntax error, unexpected ENDFOR"),
        );
    }
    let source = format!("@#for k in []\n  @#endfor // trailing\n{MODEL}");
    let diagnostics = dynare_diagnose(&source, None, None);
    let syntax = diagnostics.iter().find(|d| d.code == "E062").unwrap();
    assert_eq!(
        (
            syntax.line,
            syntax.column,
            syntax.end_line,
            syntax.end_column
        ),
        (2, 3, 2, 11)
    );
    let directory = Scratch::new();
    assert_pinned(
        &directory.0,
        &source,
        Some("syntax error, unexpected ENDFOR"),
    );
}

fn assert_pinned(directory: &Path, source: &str, refusal: Option<&str>) {
    let binary = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("skip active include Check: pinned Dynare 7.2 is absent");
        return;
    }
    let official = dygnosis::run_preprocessor(
        source,
        &binary,
        Some(directory),
        Duration::from_secs(30),
        dygnosis::JsonStage::Check,
    );
    assert_eq!(
        official.success,
        refusal.is_none(),
        "{source}: {official:?}"
    );
    if let Some(needle) = refusal {
        assert!(
            format!("{official:?}").contains(needle),
            "{source}: {official:?}"
        );
    }
}

#[test]
fn executed_definitions_follow_nested_branch_and_loop_order() {
    for (source, equations) in quiet_cases()
        .into_iter()
        .map(|source| (source, 1))
        .chain(std::iter::once((ORDERED_LOOP.to_string(), 2)))
    {
        let files = supplied(&source);
        let diagnostics = dynare_diagnose(&source, Some(ROOT), Some(&files));
        assert!(
            !diagnostics.iter().any(|d| d.severity == "ERROR"),
            "{source}: {diagnostics:?}"
        );
        let mut workspace = Workspace::new();
        for (key, text) in &files {
            workspace.update_document(key, text);
        }
        let report = workspace.expand_report(ROOT).unwrap();
        assert!(report.complete, "{source}: {report:?}");
        assert_eq!(report.n_equations, equations, "{source}: {report:?}");
        assert_eq!(report.navigation.len(), equations, "{source}: {report:?}");
        assert_eq!(
            dynare_model_info(&source, Some(ROOT), Some(&files))["n_equations"],
            equations
        );
        assert!(workspace
            .include_records(ROOT)
            .unwrap()
            .unresolved
            .is_empty());
    }
}

#[test]
fn only_executed_search_paths_choose_files_and_uncertain_paths_stay_incomplete() {
    let directory = Scratch::new();
    write_files(&directory.0, "");
    for name in ["hidden", "visible"] {
        std::fs::create_dir(directory.0.join(name)).unwrap();
    }
    std::fs::write(
        directory.0.join("hidden/chosen.inc"),
        "var z; model; z=missing; end;\n",
    )
    .unwrap();
    std::fs::write(directory.0.join("visible/chosen.inc"), MODEL).unwrap();
    let root = directory.0.join("root.mod").to_string_lossy().into_owned();
    let files_for = |source: &str| -> HashMap<String, String> {
        std::iter::once((root.clone(), source.to_string()))
            .chain(FILES.iter().map(|(name, text)| {
                (
                    directory.0.join(name).to_string_lossy().into_owned(),
                    text.to_string(),
                )
            }))
            .chain(
                [
                    ("hidden/chosen.inc", "var z; model; z=missing; end;\n"),
                    ("visible/chosen.inc", MODEL),
                ]
                .into_iter()
                .map(|(name, text)| {
                    (
                        directory.0.join(name).to_string_lossy().into_owned(),
                        text.to_string(),
                    )
                }),
            )
            .collect()
    };
    for prefix in [
        "@#if 0\n@#includepath \"hidden\"\n@#endif\n",
        "@#for k in []\n@#includepath \"hidden\"\n@#endfor\n",
        "@#include \"flags.inc\"\n@#for k in 1:2\n@#if ENABLED\n@#includepath \"hidden\"\n@#endif\n@#endfor\n",
    ] {
        let source = format!("{prefix}@#includepath \"visible\"\n@#include \"chosen.inc\"\n");
        let files = files_for(&source);
        let diagnostics = dynare_diagnose(&source, Some(&root), Some(&files));
        assert!(!diagnostics.iter().any(|d| d.severity == "ERROR"), "{diagnostics:?}");
        assert_eq!(dynare_model_info(&source, Some(&root), Some(&files))["n_equations"], 1);
        let binary = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
        if binary.is_file() {
            let official = dygnosis::run_preprocessor(&source, &binary, Some(&directory.0),
                Duration::from_secs(30), dygnosis::JsonStage::Check);
            assert!(official.success, "{source}: {official:?}");
        }
    }
    let source = "@#if length([1])\n@#includepath \"visible\"\n@#endif\n@#include \"chosen.inc\"\n";
    let files = files_for(source);
    assert!(!dynare_diagnose(source, Some(&root), Some(&files))
        .iter()
        .any(|d| d.code == "E061"));
    let info = dynare_model_info(source, Some(&root), Some(&files));
    assert_eq!(info["status"], "incomplete", "{info}");
    assert!(info.get("n_equations").is_none());
}

#[test]
fn active_failures_keep_codes_and_written_locations() {
    for (source, code, line, child) in [
        (
            format!("@#if 1\n@#include \"absent.inc\"\n@#endif\n{MODEL}"),
            "E061",
            2,
            None,
        ),
        (
            format!("@#include \"missing.inc\"\n{MODEL}"),
            "E061",
            1,
            None,
        ),
        (
            format!("{MODEL}@#include \"bad.inc\"\n"),
            "E062",
            1,
            Some("bad.inc"),
        ),
        (
            "@#include \"unknown.inc\"\n".to_string(),
            "E020",
            1,
            Some("unknown.inc"),
        ),
        (
            "@#include \"duplicate.inc\"\n".to_string(),
            "W031",
            1,
            Some("duplicate.inc"),
        ),
        (
            format!("@#includepath \"absent-directory\"\n{MODEL}"),
            "E304",
            1,
            None,
        ),
        (format!("@#includepath 7\n{MODEL}"), "E305", 1, None),
    ] {
        let files = supplied(&source);
        let diagnostics = dynare_diagnose(&source, Some(ROOT), Some(&files));
        let diagnostic = diagnostics
            .iter()
            .find(|d| d.code == code)
            .unwrap_or_else(|| panic!("{source}: {diagnostics:?}"));
        assert_eq!(diagnostic.line, line, "{diagnostic:?}");
        if let Some(child) = child {
            assert!(
                diagnostic
                    .file
                    .as_deref()
                    .is_some_and(|file| file.ends_with(child)),
                "{diagnostic:?}"
            );
        } else {
            assert_eq!(diagnostic.file, None);
        }
    }
}

fn write_files(directory: &Path, source: &str) {
    std::fs::write(directory.join("root.mod"), source).unwrap();
    for (name, text) in FILES {
        std::fs::write(directory.join(name), text).unwrap();
    }
}

#[test]
fn dormant_disk_files_are_not_loaded_or_registered_as_dependencies() {
    let directory = Scratch::new();
    let source = format!("@#if 0\n@#include \"bad.inc\"\n@#endif\n{MODEL}");
    write_files(&directory.0, &source);
    let root = directory.0.join("root.mod").to_string_lossy().into_owned();
    let child = directory.0.join("bad.inc").to_string_lossy().into_owned();
    let mut workspace = Workspace::new();
    workspace.update_document(&root, &source);
    assert!(workspace.includes_complete(&root));
    assert!(
        workspace.get_source(&child).is_none(),
        "dormant file was read"
    );
    assert!(workspace
        .include_records(&root)
        .unwrap()
        .resolved
        .is_empty());
    assert!(workspace.owner_roots(&child).is_empty());
    let revision = workspace.input_revision(&root).unwrap();
    std::fs::write(
        directory.0.join("bad.inc"),
        "@#else\nvar z; model; z=missing; end;\n",
    )
    .unwrap();
    assert_eq!(
        workspace.input_revision(&root).unwrap(),
        revision,
        "dormant input changed the root revision"
    );
    let result = Command::new(env!("CARGO_BIN_EXE_dygnosis"))
        .arg("check")
        .arg(directory.0.join("root.mod"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stdout)
    );
}

#[test]
fn comparison_and_preview_share_the_corrected_dormant_unit() {
    let source = format!("@#if 0\n@#include \"leak.inc\"\n@#endif\n{MODEL}");
    let files = supplied(&source);
    let result =
        dygnosis::dynare_compare_models(&source, MODEL, Some(ROOT), None, Some(&files), None, None);
    for field in [
        "added_endogenous",
        "removed_endogenous",
        "changed_equations",
        "added_equations",
        "removed_equations",
    ] {
        assert_eq!(result[field], serde_json::json!([]), "{field}: {result}");
    }
    assert_eq!(result["navigation"]["before"]["complete"], true, "{result}");
    assert_eq!(result["navigation"]["after"]["complete"], true, "{result}");
    let preview = dygnosis::mcp::dynare_expand(&source, Some(ROOT), Some(&files));
    assert_eq!(preview["n_equations"], 1);
    assert_eq!(preview["navigation"].as_array().unwrap().len(), 1);
    assert!(!preview["effective_text"].as_str().unwrap().contains("z ="));
}

#[test]
fn pinned_check_agrees_on_dormant_and_active_include_cases() {
    let binary = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("skip active include Check: pinned Dynare 7.2 is absent");
        return;
    }
    let cases = quiet_cases()
        .into_iter()
        .map(|source| (source, None))
        .chain(std::iter::once((ORDERED_LOOP.to_string(), None)))
        .chain([
            (
                format!("@#if 1\n@#include \"absent.inc\"\n@#endif\n{MODEL}"),
                Some("Could not open absent.inc"),
            ),
            (
                format!("@#include \"missing.inc\"\n{MODEL}"),
                Some("Could not open absent.inc"),
            ),
            (
                format!("{MODEL}@#include \"bad.inc\"\n"),
                Some("syntax error"),
            ),
            (
                "@#include \"unknown.inc\"\n".to_string(),
                Some("Unknown symbol: missing"),
            ),
            (
                format!("@#includepath \"absent-directory\"\n{MODEL}"),
                Some("absent-directory does not evaluate to a valid directory"),
            ),
            (
                format!("@#includepath 7\n{MODEL}"),
                Some("File name does not evaluate to a string"),
            ),
        ]);
    for (source, refusal) in cases {
        let directory = Scratch::new();
        write_files(&directory.0, &source);
        let official = dygnosis::run_preprocessor(
            &source,
            &binary,
            Some(&directory.0),
            Duration::from_secs(30),
            dygnosis::JsonStage::Check,
        );
        assert_eq!(
            official.success,
            refusal.is_none(),
            "{source}: {official:?}"
        );
        if let Some(needle) = refusal {
            assert!(
                format!("{official:?}").contains(needle),
                "{source}: {official:?}"
            );
        }
    }
    let directory = Scratch::new();
    let source = "@#include \"duplicate.inc\"\n";
    write_files(&directory.0, source);
    let official = dygnosis::run_preprocessor(
        source,
        &binary,
        Some(&directory.0),
        Duration::from_secs(30),
        dygnosis::JsonStage::Check,
    );
    assert!(official.success, "{official:?}");
    assert!(
        format!("{official:?}").contains("Symbol y declared twice."),
        "{official:?}"
    );
}
