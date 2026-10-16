use std::collections::HashSet;
use std::path::{Path, PathBuf};

use dygnosis::{Diagnostic, Severity, check_file, check_walk, dynare_workspace_diagnose};

const OUT: &[&str] = &[
    "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
];

const I050_MESSAGE: &str = "No initval or steady_state_model block. Add an initval block with initial guesses, or a steady_state_model block with closed-form assignments.";

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/use-dynare/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn read_mod_file(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn read_mod(archive_dir: &str) -> String {
    read_mod_file(&copilot_mod(archive_dir))
}

fn check_dir_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("check_dir")
}

fn check_dir_path(rel: &str) -> PathBuf {
    let mut path = check_dir_root();
    for part in rel.split('/') {
        path.push(part);
    }
    path
}

fn is_p_digits(code: &str) -> bool {
    let rest = match code.strip_prefix('P') {
        Some(r) => r,
        None => return false,
    };
    !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
}

fn assert_no_out(diags: &[Diagnostic]) {
    let codes: HashSet<_> = diags.iter().map(|diag| diag.code.as_str()).collect();
    for code in OUT {
        assert!(
            !codes.contains(*code),
            "Out code {code} must not be emitted; got {codes:?}"
        );
    }
}

fn assert_no_out_or_preproc(diags: &[Diagnostic]) {
    assert_no_out(diags);
    for code in diags.iter().map(|diag| diag.code.as_str()) {
        assert!(
            !is_p_digits(code),
            "preprocessor code {code} must not be emitted"
        );
    }
}

#[test]
fn p_core_check_file_no_preproc() {
    let path = copilot_mod("trend_rbc_gov_inv");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_mod("trend_rbc_gov_inv");
    let diags = check_file(&text, path_str);
    assert!(diags.is_empty(), "{diags:?}");

    for name in ["sims_wu_2019", "lk2024"] {
        let path = copilot_mod(name);
        let path_str = path.to_str().expect("utf-8 path");
        let text = read_mod(name);
        let diags = check_file(&text, path_str);
        assert!(
            diags
                .iter()
                .any(|d| d.code == "I050" && d.message == I050_MESSAGE),
            "{name} library check should emit I050, got {:?}",
            diags
                .iter()
                .map(|d| (&d.code, &d.message))
                .collect::<Vec<_>>()
        );
        assert!(
            diags
                .iter()
                .all(|d| matches!(d.code.as_str(), "I050" | "I208" | "I209")),
            "{name} library check codes: {:?}",
            diags.iter().map(|d| &d.code).collect::<Vec<_>>()
        );
        assert_no_out_or_preproc(&diags);
    }

    let path = copilot_mod("govt_rbc_irf_matching");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_mod("govt_rbc_irf_matching");
    let diags = check_file(&text, path_str);
    let codes: Vec<&str> = diags.iter().map(|d| d.code.as_str()).collect();
    assert!(
        !codes.contains(&"E001"),
        "govt_rbc_irf_matching must not emit option-list E001, got {codes:?}"
    );
    for extra in ["E062", "E063", "E064", "E065"] {
        assert!(
            !codes.contains(&extra),
            "cascade must not emit {extra}; got {codes:?}"
        );
    }
    assert_no_out_or_preproc(&diags);
}

#[test]
fn swff_check_has_no_out_codes() {
    let path = copilot_mod("swff");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_mod("swff");
    assert_no_out(&check_file(&text, path_str));
}

#[test]
fn delete_model_end_emits_e001() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/e001/delete_model_end.mod");
    let path_str = path.to_str().expect("utf-8 path");
    let text = read_mod_file(&path);
    let diags = check_file(&text, path_str);
    assert!(
        diags
            .iter()
            .any(|d| d.code == "E001" && d.severity == Severity::Error),
        "{diags:?}"
    );
}

#[test]
fn saved_model_walk_skips_plus_directories_and_non_mod_files() {
    let files = check_walk::collect_model_files(&check_dir_root()).expect("walk");
    let joined = files.join("\n").replace('\\', "/");
    assert!(!joined.contains("bad.mod"), "{joined}");
    assert!(!joined.contains("fragment.inc"), "{joined}");
    assert!(joined.contains("nested/ok.mod"), "{joined}");
    for name in ["err.mod", "clean.mod", "warn.mod", "info.mod"] {
        assert!(joined.contains(name), "{name} missing:\n{joined}");
    }
    assert_eq!(files.len(), 5, "{joined}");
}

#[test]
fn explicit_include_path_is_diagnosed() {
    let arg = check_dir_path("fragment.inc")
        .to_string_lossy()
        .into_owned();
    let body = dynare_workspace_diagnose(None, None, Some(&[arg])).expect("path");
    let diags = body["roots"][0]["diagnostics"]
        .as_array()
        .expect("diagnostics");
    assert!(
        diags.iter().any(|diag| diag["severity"] == "ERROR"),
        "{body}"
    );
}

#[test]
fn path_batch_keeps_warnings_and_information_separate() {
    let warn = check_dir_path("warn.mod").to_string_lossy().into_owned();
    let warned = dynare_workspace_diagnose(None, None, Some(&[warn])).expect("warn");
    assert_eq!(warned["summary"]["errors"], 0, "{warned}");
    assert!(
        warned["summary"]["warnings"].as_u64().unwrap() > 0,
        "{warned}"
    );

    let info = check_dir_path("info.mod").to_string_lossy().into_owned();
    let informed = dynare_workspace_diagnose(None, None, Some(&[info])).expect("info");
    assert_eq!(informed["summary"]["errors"], 0, "{informed}");
    assert_eq!(informed["summary"]["warnings"], 0, "{informed}");
    assert!(
        informed["summary"]["information"].as_u64().unwrap() > 0,
        "{informed}"
    );
}
