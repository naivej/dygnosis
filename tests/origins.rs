use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use dygnosis::mcp::{dynare_diagnose, dynare_workspace_diagnose};
use dygnosis::{check_file_with_origins, format_check_lines_with_origins};

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "dygnosis-origin-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn cli_names_child_for_included_unknown_symbol() {
    let dir = scratch("cli");
    let root = dir.join("root.mod");
    let child = dir.join("child.inc");
    let root_text = "@#include \"child.inc\"\n";
    let child_text = "var y; model; y=zz; end;\n";
    fs::write(&root, root_text).unwrap();
    fs::write(&child, child_text).unwrap();
    let path = root.to_str().unwrap();

    let set = check_file_with_origins(root_text, path);
    let output = format_check_lines_with_origins(path, &set, root_text);
    let line = output
        .lines()
        .find(|line| line.contains("[E020]"))
        .expect("E020");
    assert!(
        line.to_ascii_lowercase()
            .starts_with(&format!("{}:1:17:", child.display()).to_ascii_lowercase()),
        "{line}"
    );
    assert!(line.contains("zz"), "{line}");

    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn mcp_names_child_for_included_unknown_symbol() {
    let root = "audit/root.mod";
    let child = "audit/child.inc";
    let root_text = "@#include \"child.inc\"\n";
    let child_text = "var y; model; y=zz; end;\n";
    let files = HashMap::from([
        (root.to_string(), root_text.to_string()),
        (child.to_string(), child_text.to_string()),
    ]);
    let result = dynare_diagnose(root_text, Some(root), Some(&files));
    let e020 = result
        .iter()
        .find(|diag| diag.code == "E020")
        .expect("E020");
    assert_eq!(e020.file.as_deref(), Some(child), "{result:?}");
    assert_eq!((e020.line, e020.column), (1, 17));
}

#[test]
fn included_declaration_warning_has_one_owner_and_explicit_roots_stay_separate() {
    let root = "audit/root.mod";
    let child = "audit/child.mod";
    let root_text = "@#include \"child.mod\"\nmodel; y=0; end;\n";
    let child_text = "parameters unused; var y; unused=.5;\n";
    let files = HashMap::from([
        (root.to_string(), root_text.to_string()),
        (child.to_string(), child_text.to_string()),
    ]);
    let result = dynare_diagnose(root_text, Some(root), Some(&files));
    let warnings: Vec<_> = result.iter().filter(|diag| diag.code == "W022").collect();
    assert_eq!(warnings.len(), 1, "{result:?}");
    assert_eq!(warnings[0].file.as_deref(), Some(child));
    assert_eq!((warnings[0].line, warnings[0].column), (1, 12));
    assert!(result.iter().all(|diag| diag.code != "E186"), "{result:?}");

    let report = dynare_workspace_diagnose(
        Some(&files),
        Some(&[root.to_string(), child.to_string()]),
        None,
    )
    .unwrap();
    let roots = report["roots"].as_array().unwrap();
    assert_eq!(roots.len(), 2, "{report}");
    for name in [root, child] {
        let row = roots.iter().find(|row| row["root"] == name).unwrap();
        let diagnostics = row["diagnostics"].as_array().unwrap();
        let warnings: Vec<_> = diagnostics
            .iter()
            .filter(|diag| diag["code"] == "W022")
            .collect();
        assert_eq!(warnings.len(), 1, "{row}");
        assert_eq!(warnings[0]["file"], child);
        assert_eq!(warnings[0]["line"], 1);
        assert_eq!(warnings[0]["column"], 12);
        assert_eq!(
            diagnostics.iter().any(|diag| diag["code"] == "E186"),
            name == child,
            "{row}"
        );
    }
}

#[test]
fn incomplete_include_graph_withholds_no_model_unused_name_extension() {
    let root = "audit/root.mod";
    let source = "var y; varexo eps_z; parameters alppha;\n@#include \"child.inc\"\n";
    for files in [
        HashMap::from([(root.to_string(), source.to_string())]),
        HashMap::from([
            (root.to_string(), source.to_string()),
            (
                "audit/child.inc".to_string(),
                "@#include \"root.mod\"\n".to_string(),
            ),
        ]),
    ] {
        let result = dynare_diagnose(source, Some(root), Some(&files));
        assert!(
            result
                .iter()
                .all(|diag| !matches!(diag.code.as_str(), "E021" | "W022" | "E186" | "I209")),
            "{result:?}"
        );
    }
}

#[test]
fn empty_aggregate_model_refusal_points_at_included_end() {
    let root = "audit/root.mod";
    let child = "audit/empty.inc";
    let root_text = "var y c; varexo e;\n@#include \"empty.inc\"\n";
    let child_text = "model;\nend;\n";
    let files = HashMap::from([
        (root.to_string(), root_text.to_string()),
        (child.to_string(), child_text.to_string()),
    ]);
    let result = dynare_diagnose(root_text, Some(root), Some(&files));
    let errors: Vec<_> = result.iter().filter(|diag| diag.code == "E001").collect();
    assert_eq!(errors.len(), 1, "{result:?}");
    assert_eq!(errors[0].message, "syntax error, unexpected END");
    assert_eq!(errors[0].file.as_deref(), Some(child));
    assert_eq!(
        (
            errors[0].line,
            errors[0].column,
            errors[0].end_line,
            errors[0].end_column
        ),
        (2, 1, 2, 4)
    );
    assert!(
        result
            .iter()
            .all(|diag| !matches!(diag.code.as_str(), "E186" | "I209")),
        "{result:?}"
    );
}

#[test]
fn mcp_maps_each_audited_check_error_to_its_child() {
    let cases = [
        ("E001", "@#include \"child.inc\"\n", "var y; model; y=dsge_prior_weight; end;\n"),
        (
            "E020",
            "@#include \"child.inc\"\n",
            "var y; model; y=zz; end;\n",
        ),
        (
            "E030",
            "var y;\n@#include \"child.inc\"\n",
            "varexo y; model; y=y(-1); end;\n",
        ),
        (
            "E058",
            "var y; model; y=y(-1); end;\n@#include \"child.inc\"\n",
            "initval; zz=1; end;\n",
        ),
        (
            "E317",
            "var y z; varexo e; parameters rho; rho=.9; model; y=rho*y(-1)+e; z=y; end; osr_params rho;\n@#include \"child.inc\"\n",
            "optim_weights; e 1; end; osr;\n",
        ),
    ];
    for (code, root_text, child_text) in cases {
        let root = "audit/root.mod";
        let child = "audit/child.inc";
        let files = HashMap::from([
            (root.to_string(), root_text.to_string()),
            (child.to_string(), child_text.to_string()),
        ]);
        let result = dynare_diagnose(root_text, Some(root), Some(&files));
        let diag = result
            .iter()
            .find(|diag| diag.code == code)
            .unwrap_or_else(|| panic!("{code}: {result:?}"));
        assert_eq!(diag.file.as_deref(), Some(child), "{code}: {result:?}");
        assert_eq!(diag.line, 1, "{code}: {result:?}");
    }
}

#[test]
fn nested_include_uses_overlay_text_for_child_position() {
    let root = "audit/root.mod";
    let middle = "audit/middle.inc";
    let child = "audit/child.inc";
    let root_text = "@#include \"middle.inc\"\n";
    let child_text = "/*😀*/var y; model; y=zz; end;\n";
    let files = HashMap::from([
        (root.to_string(), "var wrong;\n".to_string()),
        (middle.to_string(), "@#include \"child.inc\"\n".to_string()),
        (child.to_string(), child_text.to_string()),
    ]);
    let result = dynare_diagnose(root_text, Some(root), Some(&files));
    let e020 = result
        .iter()
        .find(|diag| diag.code == "E020")
        .expect("E020");
    assert_eq!(e020.file.as_deref(), Some(child), "{result:?}");
    assert_eq!((e020.line, e020.column), (1, 22));
}

#[test]
fn root_fix_after_include_uses_root_line() {
    let dir = scratch("root-fix");
    let root = dir.join("root.mod");
    let child = dir.join("child.inc");
    let root_text = "@#include \"child.inc\"\nvar y\n";
    fs::write(&root, root_text).unwrap();
    fs::write(&child, "parameters a;\n").unwrap();
    let set = check_file_with_origins(root_text, root.to_str().unwrap());
    let diag = set
        .diagnostics
        .iter()
        .find(|d| d.code == "E001")
        .expect("E001");
    let fix = diag.fix.as_ref().expect("same-source fix");
    assert_eq!(fix.start_line, 1, "{fix:?}");
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn child_diagnostics_share_one_source_snapshot() {
    let dir = scratch("shared-text");
    let root = dir.join("root.mod");
    let child = dir.join("child.inc");
    let root_text = "@#include \"child.inc\"\n";
    fs::write(&root, root_text).unwrap();
    fs::write(&child, "var y; model; y=aa; y=bb; end;\n").unwrap();
    let set = check_file_with_origins(root_text, root.to_str().unwrap());
    let owned: Vec<_> = set
        .diagnostics
        .iter()
        .enumerate()
        .filter(|(_, diag)| diag.code == "E020")
        .filter_map(|(i, _)| set.origins.get(i).and_then(Option::as_ref))
        .collect();
    assert_eq!(owned.len(), 2);
    assert!(Arc::ptr_eq(&owned[0].text, &owned[1].text));
    fs::remove_dir_all(dir).unwrap();
}
