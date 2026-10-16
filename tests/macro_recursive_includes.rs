//! Recursive include occurrences, state order, source origins and bounded stops.

use std::collections::HashMap;
use std::path::PathBuf;

use dygnosis::{Workspace, dynare_diagnose, dynare_expand, dynare_model_info};

const ROOT: &str = "/recursive/root.mod";
const STEP: &str =
    "@#if depth > 0\nvar y@{depth};\n@#define depth = depth-1\n@#include \"step.inc\"\n@#endif\n";
const MINIMAL: &str =
    "@#define depth=3\n@#include \"step.inc\"\nmodel;\n@#for n in 1:3\ny@{n}=0;\n@#endfor\nend;\n";
const DTE_INDICES: &str = include_str!("fixtures/macro_action/CreateIndicesArray.mod");
const DTE_ROOT: &str = "@#define Numbers=[(string)n for n in 0:4096]\n@#define Indices=[]\n@#define Depth=0\n@#define SpatialDimensions=2\n@#define SpatialPointsPerDimension=2\n@#define IndicesStringArray=[]\n@#define IndicesArray=[]\n@#include \"CreateIndicesArray.mod\"\n@#for Index in IndicesStringArray\nvar x@{Index};\n@#endfor\nmodel;\n@#for Index in IndicesStringArray\nx@{Index}=0;\n@#endfor\nend;\n";

fn supplied(root: &str, children: &[(&str, &str)]) -> HashMap<String, String> {
    std::iter::once((ROOT.to_string(), root.to_string()))
        .chain(
            children
                .iter()
                .map(|(name, source)| (format!("/recursive/{name}"), source.to_string())),
        )
        .collect()
}

fn assert_complete(root: &str, children: &[(&str, &str)], equations: usize) {
    let files = supplied(root, children);
    let diagnostics = dynare_diagnose(root, Some(ROOT), Some(&files));
    assert!(
        !diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code.starts_with('E') || diagnostic.code == "I211" }),
        "{diagnostics:?}"
    );
    let expanded = dynare_expand(root, Some(ROOT), Some(&files));
    assert_eq!(expanded["complete"], true, "{expanded}");
    assert_eq!(expanded["n_equations"], equations, "{expanded}");
}

#[test]
fn state_changing_self_include_retains_all_declarations() {
    // Guards a filename re-entry refusal or a splice that drops later calls.
    assert_complete(MINIMAL, &[("step.inc", STEP)], 3);
    let mut workspace = Workspace::new();
    for (file, source) in supplied(MINIMAL, &[("step.inc", STEP)]) {
        workspace.update_document(&file, source);
    }
    let model = workspace.get_effective_model(ROOT).unwrap();
    let names: Vec<_> = model
        .endogenous
        .iter()
        .map(|row| model.name(row.name))
        .collect();
    assert_eq!(names, ["y3", "y2", "y1"]);
}

#[test]
fn adapted_dte_index_helper_keeps_all_recursive_loop_occurrences() {
    // Guards dropping a recursive invocation from a stateful nested loop.
    assert_complete(DTE_ROOT, &[("CreateIndicesArray.mod", DTE_INDICES)], 4);
    let mut workspace = Workspace::new();
    for (file, source) in supplied(DTE_ROOT, &[("CreateIndicesArray.mod", DTE_INDICES)]) {
        workspace.update_document(&file, source);
    }
    let model = workspace.get_effective_model(ROOT).unwrap();
    let names: Vec<_> = model
        .endogenous
        .iter()
        .map(|row| model.name(row.name))
        .collect();
    assert_eq!(names, ["x_1_1", "x_1_2", "x_2_1", "x_2_2"]);
}

const REACH_CASES: &[(&str, &str, &str)] = &[
    (
        "E020",
        "var y; model; y=missing; end;\n",
        "Unknown symbol: missing",
    ),
    ("W031", "var y y; model; y=0; end;\n", "declared twice"),
    (
        "E059",
        "var y; parameters p; p=.5; model; y=p; end; endval; p=0; end;\n",
        "neither endogenous or exogenous",
    ),
    (
        "E001",
        "var y; model; y=0; end; resid(1);\n",
        "syntax error",
    ),
];

fn recursive_leaf(payload: &str) -> String {
    format!(
        "@#if depth>0\n@#define depth=depth-1\n@#include \"step.inc\"\n@#else\n{payload}@#endif\n"
    )
}

#[test]
fn recursive_expansion_reaches_existing_unknown_duplicate_type_and_syntax_checks() {
    // Guards generic shipped checks failing to reach newly retained statements.
    let root = "@#define depth=2\n@#include \"step.inc\"\n";
    for (code, payload, _) in REACH_CASES {
        let child = recursive_leaf(payload);
        let files = supplied(root, &[("step.inc", &child)]);
        let diagnostics = dynare_diagnose(root, Some(ROOT), Some(&files));
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == *code),
            "{code}: {diagnostics:?}"
        );
        let quiet_child = recursive_leaf("var y; model; y=0; end;\n");
        let quiet = supplied(root, &[("step.inc", &quiet_child)]);
        assert!(
            !dynare_diagnose(root, Some(ROOT), Some(&quiet))
                .iter()
                .any(|diagnostic| diagnostic.code == *code)
        );
    }
}

#[test]
fn terminating_mutual_reentry_keeps_shared_state() {
    // Guards a remaining filename guard in a two-file caller chain.
    let other = STEP.replace("step.inc", "other.inc");
    assert_complete(MINIMAL, &[("step.inc", &other), ("other.inc", STEP)], 3);
}

#[test]
fn different_recursive_call_shapes_in_a_loop_keep_order_and_written_origins() {
    // Guards replay of the first recursive tree for a later, deeper invocation.
    let root = "var y; model;\n@#define depth=0\n@#for n in [1,2]\n@#define depth=n\n@#include \"equations.inc\"\n@#endfor\nend;\n";
    let child = "@#if depth>0\n# local@{n}_@{depth} = @{n*10+depth};\ny=local@{n}_@{depth};\n@#define depth=depth-1\n@#include \"equations.inc\"\n@#endif\n";
    let files = supplied(root, &[("equations.inc", child)]);
    let expanded = dynare_expand(root, Some(ROOT), Some(&files));
    assert_eq!(expanded["complete"], true, "{expanded}");
    assert_eq!(expanded["n_equations"], 3, "{expanded}");
    let text = expanded["effective_text"].as_str().unwrap();
    assert!(
        text.find("local1_1").unwrap() < text.find("local2_2").unwrap(),
        "{text}"
    );
    assert!(
        text.find("local2_2").unwrap() < text.find("local2_1").unwrap(),
        "{text}"
    );
    for origin in expanded["origins"].as_array().unwrap() {
        assert_eq!(origin["origin_uri"], "/recursive/equations.inc", "{origin}");
        assert!(
            origin["origin"]["line"].as_u64().unwrap() <= 3
                && origin["origin"]["end_line"].as_u64().unwrap() >= 3,
            "{origin}"
        );
    }
    let info = dynare_model_info(root, Some(ROOT), Some(&files));
    let definitions = info["model_locals"]["definitions"].as_array().unwrap();
    let expressions: Vec<_> = definitions
        .iter()
        .map(|row| row["expression"].as_str().unwrap())
        .collect();
    assert_eq!(expressions, ["11", "22", "21"], "{info}");
}

#[test]
fn equal_include_targets_with_different_loop_inputs_keep_each_call_value() {
    // Guards reusing the first child's replay values when later calls follow
    // the same include paths with a different evaluated loop collection.
    let root = "@#for n in [1,2]\n@#include \"child.inc\"\n@#endfor\nmodel; x1=0; x11=0; x2=0; x12=0; end;\n";
    let child = "@#for j in [n,n+10]\n@#if j<10\n@#define leaf=\"a.inc\"\n@#else\n@#define leaf=\"b.inc\"\n@#endif\n@#include leaf\n@#endfor\n";
    let files = supplied(
        root,
        &[
            ("child.inc", child),
            ("a.inc", "var x@{j};\n"),
            ("b.inc", "var x@{j};\n"),
        ],
    );
    let mut workspace = Workspace::new();
    for (file, source) in files {
        workspace.update_document(&file, source);
    }
    let model = workspace.get_effective_model(ROOT).unwrap();
    let names: Vec<_> = model
        .endogenous
        .iter()
        .map(|row| model.name(row.name))
        .collect();
    assert_eq!(names, ["x1", "x11", "x2", "x12"]);
    assert!(workspace.expand_report(ROOT).unwrap().complete);
}

#[test]
fn an_inactive_recursive_call_does_not_read_the_file() {
    // Guards treating a dormant edge as required source or incomplete coverage.
    assert_complete(
        "@#if false\n@#include \"root.mod\"\n@#include \"absent.inc\"\n@#endif\nvar y; model; y=0; end;",
        &[],
        1,
    );
}

#[test]
fn unbounded_direct_and_mutual_calls_stop_without_complete_claims() {
    // Product-only: the official binary is never given endless recursion.
    for children in [
        vec![("step.inc", "@#include \"step.inc\"\n")],
        vec![
            ("step.inc", "@#include \"other.inc\"\n"),
            ("other.inc", "@#include \"step.inc\"\n"),
        ],
    ] {
        let root = "@#include \"step.inc\"\nvar y; model; y=0; end;\n";
        let files = supplied(root, &children);
        let expanded = dynare_expand(root, Some(ROOT), Some(&files));
        assert_eq!(expanded["complete"], false, "{expanded}");
        assert_eq!(expanded["n_equations"], 0, "{expanded}");
        assert!(expanded["navigation"].as_array().unwrap().is_empty());
        let reason = &expanded["incomplete_reasons"][0];
        assert_eq!(reason["code"], "I211", "{expanded}");
        assert!(
            reason["message"]
                .as_str()
                .unwrap()
                .contains("execution depth"),
            "{expanded}"
        );
        assert!(
            reason["file"]
                .as_str()
                .is_some_and(|file| file.ends_with(".inc")),
            "{reason}"
        );
        assert!(
            !reason["include_chain"].as_array().unwrap().is_empty(),
            "{reason}"
        );
        assert!(
            reason["omitted"].as_str().unwrap().contains("counts"),
            "{reason}"
        );
    }
}

#[test]
fn dynare_7_2_accepts_terminating_recursive_cases_when_present() {
    let binary = PathBuf::from(r"C:\dynare\7.2\preprocessor\dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("SKIP recursive include honesty: Dynare 7.2 is absent");
        return;
    }
    let directory = std::env::temp_dir().join(format!(
        "dyg-recursive-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let path = self.0.canonicalize().unwrap();
            let temporary = std::env::temp_dir().canonicalize().unwrap();
            assert!(path.starts_with(&temporary) && path != temporary);
            let _ = std::fs::remove_dir_all(path);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    let other = STEP.replace("step.inc", "other.inc");
    for (root, children, equations) in [
        (MINIMAL, vec![("step.inc", STEP)], 3),
        (
            MINIMAL,
            vec![("step.inc", other.as_str()), ("other.inc", STEP)],
            3,
        ),
        (DTE_ROOT, vec![("CreateIndicesArray.mod", DTE_INDICES)], 4),
    ] {
        assert_complete(root, &children, equations);
        std::fs::write(directory.join("root.mod"), root).unwrap();
        for (name, source) in children {
            std::fs::write(directory.join(name), source).unwrap();
        }
        let result = std::process::Command::new(&binary)
            .arg("root.mod")
            .args(["json=check", "onlyjson", "nolog"])
            .current_dir(&directory)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    let root = "@#define depth=2\n@#include \"step.inc\"\n";
    for (code, payload, needle) in REACH_CASES {
        std::fs::write(directory.join("root.mod"), root).unwrap();
        std::fs::write(directory.join("step.inc"), recursive_leaf(payload)).unwrap();
        let result = std::process::Command::new(&binary)
            .arg("root.mod")
            .args(["json=check", "onlyjson", "nolog"])
            .current_dir(&directory)
            .output()
            .unwrap();
        let output = format!(
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(output.contains(needle), "{code}: {output}");
        assert_eq!(result.status.success(), *code == "W031", "{code}: {output}");
        std::fs::write(
            directory.join("step.inc"),
            recursive_leaf("var y; model; y=0; end;\n"),
        )
        .unwrap();
        let quiet = std::process::Command::new(&binary)
            .arg("root.mod")
            .args(["json=check", "onlyjson", "nolog"])
            .current_dir(&directory)
            .output()
            .unwrap();
        assert!(
            quiet.status.success(),
            "{code}: {}",
            String::from_utf8_lossy(&quiet.stdout)
        );
        assert!(!String::from_utf8_lossy(&quiet.stdout).contains(needle));
    }
}
