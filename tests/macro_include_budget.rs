//! Shared include scanning and splice limits, with model claims withheld.

use std::collections::HashMap;

use dygnosis::{dynare_diagnose, dynare_expand, dynare_model_info, Workspace};

const ROOT: &str = "/budget/root.mod";

fn files(copies: usize, dormant_bytes: usize) -> HashMap<String, String> {
    let root = "@#include \"child.inc\"\n".repeat(copies) + "var y; model; y=0; end;\n";
    let child = "@#if 0\n".to_string() + &" ".repeat(dormant_bytes) + "\n@#endif\n";
    HashMap::from([
        (ROOT.to_string(), root),
        ("/budget/child.inc".to_string(), child),
    ])
}

#[test]
fn repeated_dormant_includes_spend_the_root_work_before_splicing() {
    let supplied = files(100, 20_000);
    let root = &supplied[ROOT];
    let diags = dynare_diagnose(root, Some(ROOT), Some(&supplied));
    assert!(
        diags
            .iter()
            .any(|diag| diag.code == "I211" && diag.message.contains("iteration work")),
        "{diags:?}"
    );
    assert!(
        !diags.iter().any(|diag| diag.code.starts_with('E')),
        "{diags:?}"
    );
    let expanded = dynare_expand(root, Some(ROOT), Some(&supplied));
    assert_eq!(expanded["complete"], false);
    assert_eq!(expanded["n_equations"], 0);
    assert!(expanded["navigation"].as_array().unwrap().is_empty());
    let info = dynare_model_info(root, Some(ROOT), Some(&supplied));
    assert_eq!(info["status"], "incomplete");
    let mut workspace = Workspace::new();
    for (file, source) in &supplied {
        workspace.update_document(file, source.clone());
    }
    let model = workspace.get_effective_model(ROOT).unwrap();
    assert!(model.macro_incomplete());
    assert!(model
        .incomplete_reasons
        .iter()
        .any(|reason| reason.message.contains("iteration work")));
}

#[test]
fn small_dormant_includes_keep_complete_counts() {
    let supplied = files(3, 20);
    let root = &supplied[ROOT];
    let expanded = dynare_expand(root, Some(ROOT), Some(&supplied));
    assert_eq!(expanded["complete"], true, "{expanded:?}");
    assert_eq!(expanded["n_equations"], 1);
    assert!(!dynare_diagnose(root, Some(ROOT), Some(&supplied))
        .iter()
        .any(|diag| diag.code == "I211"));
}
