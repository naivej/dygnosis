use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use dygnosis::mcp::dynare_diagnose;
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
