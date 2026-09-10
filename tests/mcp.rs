use std::collections::HashMap;
use std::path::PathBuf;

use dygnosis::explain::{explain, known_codes, render_markdown};
use dygnosis::{
    analyze, auto_fix, dynare_auto_fix, dynare_compare_models, dynare_diagnose,
    dynare_diagnose_workspace, dynare_explain, dynare_find_references,
    dynare_find_references_workspace, dynare_list_diagnostic_codes, dynare_list_options,
    dynare_model_info, dynare_parse_summary, dynare_rename, dynare_rename_workspace,
    find_preprocessor, has_structural_error, parse, reconcile_diagnostics, registered_tool_names,
    run_preprocessor, tools_list_json,
};

const RUST_TOOLS: &[&str] = &[
    "dynare_diagnose",
    "dynare_diagnose_workspace",
    "dynare_parse_summary",
    "dynare_explain",
    "dynare_list_diagnostic_codes",
    "dynare_list_options",
    "dynare_find_references",
    "dynare_find_references_workspace",
    "dynare_rename",
    "dynare_rename_workspace",
    "dynare_auto_fix",
    "dynare_run_preprocessor",
    "dynare_model_info",
    "dynare_compare_models",
];

const OUT_TOOLS: &[&str] = &[
    "dynare_compute_steady_state",
    "dynare_check_blanchard_kahn",
    "dynare_residuals",
    "dynare_check_identification",
    "dynare_run_dynare",
];

const OUT_CODES: &[&str] = &[
    "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
];

const FORBIDDEN: &[&str] = &[
    "Compute Steady State",
    "Gauss-Seidel",
    "trust-region",
    "homotopy",
    "python_dynare_lsp",
];

const P_CORE: &[&str] = &[
    "trend_rbc_gov_inv",
    "sims_wu_2019",
    "govt_rbc_irf_matching",
    "lk2024",
];

const MCP_SEVERITIES: &[&str] = &["ERROR", "WARNING", "INFORMATION", "HINT"];

fn copilot_file(archive_dir: &str, filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(filename)
}

fn read_copilot(archive_dir: &str, filename: &str) -> String {
    let path = copilot_file(archive_dir, filename);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn read_mod(archive_dir: &str) -> String {
    read_copilot(archive_dir, &format!("{archive_dir}.mod"))
}

fn is_out_code(code: &str) -> bool {
    OUT_CODES.contains(&code)
}

fn assert_no_out(codes: impl IntoIterator<Item = impl AsRef<str>>) {
    for code in codes {
        let code = code.as_ref();
        assert!(!is_out_code(code), "MCP must drop Out code {code}");
    }
}

fn swff_relative_files() -> HashMap<String, String> {
    let mut files = HashMap::new();
    files.insert("swff.mod".to_string(), read_copilot("swff", "swff.mod"));
    files.insert(
        "swff_params.inc".to_string(),
        read_copilot("swff", "swff_params.inc"),
    );
    files
}

fn slice_at(text: &str, line: u32, column: u32, end_column: u32) -> String {
    let line_idx = (line as usize).saturating_sub(1);
    let lines: Vec<&str> = text.split('\n').collect();
    let row = lines.get(line_idx).copied().unwrap_or("");
    let start = (column as usize).saturating_sub(1);
    let end = (end_column as usize)
        .saturating_sub(1)
        .min(row.chars().count());
    row.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

#[test]
fn registered_tools_include_wave_c() {
    let names = registered_tool_names();
    let mut got = names.clone();
    got.sort_unstable();
    let mut want: Vec<&str> = RUST_TOOLS.to_vec();
    want.sort_unstable();
    assert_eq!(got, want);
    assert!(names.contains(&"dynare_run_preprocessor"));
    assert!(names.contains(&"dynare_model_info"));
    assert!(names.contains(&"dynare_compare_models"));

    let blob = serde_json::to_string(&tools_list_json()).expect("tools list json");
    for name in OUT_TOOLS {
        assert!(
            !blob.contains(name),
            "tools/list must not contain {name}: {blob}"
        );
    }
    for phrase in FORBIDDEN {
        assert!(
            !blob.contains(phrase),
            "tools/list must not contain {phrase:?}: {blob}"
        );
    }
}

#[test]
fn diagnose_p_core_thin_codes() {
    for name in P_CORE {
        let text = read_mod(name);
        let from_analyze: Vec<String> = analyze(&parse(&text))
            .into_iter()
            .filter(|d| !is_out_code(&d.code))
            .map(|d| d.code.clone())
            .collect();
        let expected: Vec<String> = if let Some(pp) = find_preprocessor(None) {
            let own = analyze(&parse(&text));
            let pre = run_preprocessor(&text, &pp, None, std::time::Duration::from_secs(30));
            reconcile_diagnostics(&own, Some(&pre))
                .into_iter()
                .filter(|d| !is_out_code(&d.code))
                .map(|d| d.code.clone())
                .collect()
        } else {
            from_analyze
        };
        let diags = dynare_diagnose(&text);
        for d in &diags {
            assert!(
                MCP_SEVERITIES.contains(&d.severity.as_str()),
                "{name}: MCP severity must not use CLI INFO; got {}",
                d.severity
            );
            assert_ne!(d.severity, "INFO");
            assert!(d.line >= 1, "{name}: line must be 1-based, got {}", d.line);
            assert!(
                d.column >= 1,
                "{name}: column must be 1-based, got {}",
                d.column
            );
            assert!(
                d.end_line >= 1,
                "{name}: end_line must be 1-based, got {}",
                d.end_line
            );
            assert!(
                d.end_column >= 1,
                "{name}: end_column must be 1-based, got {}",
                d.end_column
            );
        }
        let from_mcp: Vec<String> = diags.iter().map(|d| d.code.clone()).collect();
        assert_eq!(from_mcp, expected, "diagnose codes for {name}");
        assert_no_out(&from_mcp);
    }
}

#[test]
fn diagnose_govt_rbc_cascade_is_e001_only() {
    let text = read_mod("govt_rbc_irf_matching");
    let analyze_codes: Vec<String> = analyze(&parse(&text)).into_iter().map(|d| d.code).collect();
    assert!(
        analyze_codes.iter().any(|c| c == "E001"),
        "analyze cascade expected E001, got {analyze_codes:?}"
    );
    let diags = dynare_diagnose(&text);
    let codes: Vec<String> = diags.iter().map(|d| d.code.clone()).collect();
    if find_preprocessor(None).is_some() {
        assert!(
            !codes.iter().any(|c| c == "E001"),
            "E001 should be dropped when the preprocessor ran; got {codes:?}"
        );
    } else {
        assert!(
            codes.contains(&"E001".to_string()),
            "expected E001, got {codes:?}"
        );
        assert_eq!(codes, analyze_codes);
    }
    for extra in ["E062", "E063", "E064", "E065"] {
        assert!(
            !codes.iter().any(|c| c == extra),
            "cascade must not emit {extra}; got {codes:?}"
        );
        assert!(
            !analyze_codes.iter().any(|c| c == extra),
            "analyze cascade must not emit {extra}; got {analyze_codes:?}"
        );
    }
}

#[test]
fn diagnose_workspace_swff() {
    let mod_path = copilot_file("swff", "swff.mod");
    let inc_path = copilot_file("swff", "swff_params.inc");
    let active = mod_path.to_string_lossy().into_owned();
    let inc_key = inc_path.to_string_lossy().into_owned();
    let mut files = HashMap::new();
    files.insert(active.clone(), read_copilot("swff", "swff.mod"));
    files.insert(inc_key, read_copilot("swff", "swff_params.inc"));

    let diags = dynare_diagnose_workspace(&active, &files);
    assert_no_out(diags.iter().map(|d| d.code.as_str()));
    for d in &diags {
        assert!(
            MCP_SEVERITIES.contains(&d.severity.as_str()),
            "MCP severity must not use CLI INFO; got {}",
            d.severity
        );
    }

    let empty = dynare_diagnose_workspace("missing.mod", &files);
    assert!(empty.is_empty(), "missing active_file must return []");
}

#[test]
fn run_preprocessor_degrades_via_empty_finder() {
    let value = dygnosis::mcp::dynare_run_preprocessor_with_finder("var y;\n", None, None, || None);
    assert_eq!(value["success"], false);
    assert_eq!(
        value["message"],
        dygnosis::preprocessor::MISSING_BINARY_MESSAGE
    );
    assert_eq!(value["diagnostics"], serde_json::json!([]));
}

#[test]
fn parse_summary_matches_expected() {
    let expected_files = [
        (
            "trend_rbc_gov_inv",
            include_str!("expected/trend_rbc_gov_inv.parse_summary.json"),
        ),
        (
            "sims_wu_2019",
            include_str!("expected/sims_wu_2019.parse_summary.json"),
        ),
        (
            "govt_rbc_irf_matching",
            include_str!("expected/govt_rbc_irf_matching.parse_summary.json"),
        ),
        ("lk2024", include_str!("expected/lk2024.parse_summary.json")),
    ];
    assert_eq!(expected_files.len(), P_CORE.len());
    for (name, expected_json) in expected_files {
        let text = read_mod(name);
        let got = serde_json::to_value(dynare_parse_summary(&text)).unwrap();
        let expected: serde_json::Value = serde_json::from_str(expected_json).unwrap();
        assert_eq!(got, expected, "parse_summary mismatch for {name}");
    }
}

#[test]
fn explain_e010_matches_library() {
    let expected = render_markdown("E010").expect("E010 is documented");
    assert_eq!(dynare_explain("E010"), expected);
}

#[test]
fn explain_e040_is_unknown() {
    let known = known_codes().join(", ");
    let expected = format!("No documentation found for code 'E040'. Known codes: {known}");
    assert_eq!(dynare_explain("E040"), expected);
}

#[test]
fn list_diagnostic_codes_is_54() {
    let list = dynare_list_diagnostic_codes();
    assert_eq!(list.len(), 54);
    let codes: Vec<&str> = list.iter().map(|item| item.code.as_str()).collect();
    assert_eq!(codes, known_codes());
    for item in &list {
        let title = explain(&item.code).map(|e| e.title).unwrap_or("");
        assert_eq!(item.title, title, "title mismatch for {}", item.code);
    }
}

#[test]
fn list_options_handler_matches_expected() {
    let omitted: serde_json::Value =
        serde_json::from_str(include_str!("expected/list_options.omitted.json")).unwrap();
    assert_eq!(dynare_list_options(None), omitted);
    assert_eq!(dynare_list_options(Some("")), omitted);

    let known: serde_json::Value =
        serde_json::from_str(include_str!("expected/list_options.known.json")).unwrap();
    let stoch = dynare_list_options(Some("stoch_simul"));
    assert_eq!(stoch, known["stoch_simul"]);
    assert_eq!(
        dynare_list_options(Some("Stoch_Simul")),
        known["stoch_simul"]
    );

    let unknown: serde_json::Value =
        serde_json::from_str(include_str!("expected/list_options.unknown.json")).unwrap();
    assert_eq!(
        dynare_list_options(Some("not_a_dynare_command")),
        unknown["not_a_dynare_command"]
    );
    assert_eq!(
        dynare_list_options(Some("stoch_simu")),
        unknown["stoch_simu"]
    );
}

#[test]
fn find_references_betta_skips_comment() {
    let base = read_mod("trend_rbc_gov_inv");
    let text = format!("// betta\n{base}");
    let hits = dynare_find_references(&text, "betta");
    assert!(!hits.is_empty(), "expected betta hits, got {hits:?}");
    assert!(dynare_find_references(&text, "").is_empty());

    let comment_line = 1u32;
    for hit in &hits {
        assert_ne!(
            hit.line, comment_line,
            "comment-only line must not be a hit: {hit:?}"
        );
        let slice = slice_at(&text, hit.line, hit.column, hit.end_column);
        assert_eq!(
            slice, "betta",
            "hit slice must be betta, got {slice:?} at {hit:?}"
        );
    }
}

#[test]
fn rename_betta_to_beta_disc() {
    let text = read_mod("trend_rbc_gov_inv");
    let out = dynare_rename(&text, "betta", "beta_disc");
    assert!(out.contains("beta_disc"));
    assert!(dynare_find_references(&out, "betta").is_empty());
    assert!(!dynare_find_references(&out, "beta_disc").is_empty());
    assert_ne!(out, text);
}

#[test]
fn rename_illegal_new_name_is_noop() {
    let text = read_mod("trend_rbc_gov_inv");
    assert_eq!(dynare_rename(&text, "betta", "1bad"), text);
    assert_eq!(dynare_rename(&text, "betta", "log"), text);
}

#[test]
fn find_references_workspace_swff() {
    let files = swff_relative_files();
    let shared = ["alppha", "rho", "gstar"]
        .into_iter()
        .find(|name| files["swff.mod"].contains(name) && files["swff_params.inc"].contains(name))
        .expect("shared ident in both swff files");

    let hits = dynare_find_references_workspace("swff.mod", shared, &files);
    let files_hit: std::collections::HashSet<&str> = hits.iter().map(|h| h.file.as_str()).collect();
    assert!(
        files_hit.contains("swff.mod"),
        "expected swff.mod hits, got {hits:?}"
    );
    assert!(
        files_hit.contains("swff_params.inc"),
        "expected swff_params.inc hits, got {hits:?}"
    );
    assert!(dynare_find_references_workspace("swff.mod", "", &files).is_empty());
    assert!(dynare_find_references_workspace("missing.mod", shared, &files).is_empty());
}

#[test]
fn rename_workspace_swff_changed_only() {
    let files = swff_relative_files();
    let shared = ["alppha", "rho", "gstar"]
        .into_iter()
        .find(|name| files["swff.mod"].contains(name) && files["swff_params.inc"].contains(name))
        .expect("shared ident in both swff files");
    let new_name = format!("{shared}_renamed");

    let changed = dynare_rename_workspace("swff.mod", shared, &new_name, &files);
    assert!(
        changed.contains_key("swff.mod"),
        "expected swff.mod changed, keys {:?}",
        changed.keys().collect::<Vec<_>>()
    );
    assert!(
        changed.contains_key("swff_params.inc"),
        "expected swff_params.inc changed, keys {:?}",
        changed.keys().collect::<Vec<_>>()
    );
    assert_eq!(changed.len(), 2);
    assert!(changed["swff.mod"].contains(&new_name));
    assert!(changed["swff_params.inc"].contains(&new_name));
    assert!(dynare_find_references(&changed["swff.mod"], shared).is_empty());
    assert!(dynare_find_references(&changed["swff_params.inc"], shared).is_empty());

    assert!(dynare_rename_workspace("swff.mod", shared, "1bad", &files).is_empty());
    assert!(dynare_rename_workspace("swff.mod", shared, "log", &files).is_empty());
    assert!(dynare_rename_workspace("missing.mod", shared, &new_name, &files).is_empty());
}

#[test]
fn auto_fix_clean_trend_noop() {
    let text = read_mod("trend_rbc_gov_inv");
    assert_eq!(dynare_auto_fix(&text), text);
    assert_eq!(dynare_auto_fix(&text), auto_fix(&text));
}

#[test]
fn auto_fix_applies_e001_missing_semi() {
    let mutated = read_mod("trend_rbc_gov_inv").replacen("betta   = 0.99;", "betta   = 0.99", 1);
    assert!(has_structural_error(&parse(&mutated)));
    let fixed = dynare_auto_fix(&mutated);
    assert_ne!(fixed, mutated);
    assert!(!has_structural_error(&parse(&fixed)));
    assert_eq!(fixed, auto_fix(&mutated));
}

#[test]
fn auto_fix_refuse_macro() {
    let text = "@#define C = 1\ny_@{C}\nparameters betta;\nbetta = 0.99;\n";
    assert_eq!(dynare_auto_fix(text), text);
}

#[test]
fn workspace_refs_skip_unrelated_file() {
    let mut files = swff_relative_files();
    let unrelated = read_mod("trend_rbc_gov_inv");
    files.insert("unrelated.mod".to_string(), unrelated.clone());
    let shared = ["alppha", "rho", "gstar"]
        .into_iter()
        .find(|name| files["swff.mod"].contains(name) && files["swff_params.inc"].contains(name))
        .expect("shared ident in both swff files");

    let hits = dynare_find_references_workspace("swff.mod", shared, &files);
    assert!(
        hits.iter().all(|h| h.file != "unrelated.mod"),
        "unrelated file must be out of scope: {hits:?}"
    );

    let new_name = format!("{shared}_renamed");
    let changed = dynare_rename_workspace("swff.mod", shared, &new_name, &files);
    assert!(
        !changed.contains_key("unrelated.mod"),
        "rename must not rewrite unrelated: {:?}",
        changed.keys().collect::<Vec<_>>()
    );
    assert_eq!(changed.len(), 2);
}

#[test]
fn model_info_trend_timing_no_blocks() {
    let text = read_mod("trend_rbc_gov_inv");
    let info = dynare_model_info(&text, None, None);
    let obj = info.as_object().expect("object");
    assert!(obj.contains_key("n_endogenous"));
    assert!(obj.contains_key("endogenous"));
    assert!(obj.contains_key("static"));
    assert!(obj.contains_key("predetermined"));
    assert!(obj.contains_key("forward_looking"));
    assert!(obj.contains_key("mixed"));
    assert!(obj.contains_key("n_static"));
    assert!(obj.contains_key("n_predetermined"));
    assert!(obj.contains_key("n_forward_looking"));
    assert!(obj.contains_key("n_mixed"));
    assert!(obj.contains_key("n_state_variables"));
    assert!(obj.contains_key("n_jumpers"));
    assert!(!obj.contains_key("blocks"));
    let n_endo = obj["n_endogenous"].as_u64().expect("n_endogenous");
    assert_eq!(
        n_endo,
        obj["endogenous"].as_array().expect("endogenous").len() as u64
    );
    assert!(n_endo > 0);
    let n_static = obj["n_static"].as_u64().expect("n_static");
    let n_pred = obj["n_predetermined"].as_u64().expect("n_predetermined");
    let n_fwd = obj["n_forward_looking"]
        .as_u64()
        .expect("n_forward_looking");
    let n_mixed = obj["n_mixed"].as_u64().expect("n_mixed");
    assert_eq!(
        n_static,
        obj["static"].as_array().expect("static").len() as u64
    );
    assert_eq!(
        n_pred,
        obj["predetermined"]
            .as_array()
            .expect("predetermined")
            .len() as u64
    );
    assert_eq!(
        n_fwd,
        obj["forward_looking"]
            .as_array()
            .expect("forward_looking")
            .len() as u64
    );
    assert_eq!(
        n_mixed,
        obj["mixed"].as_array().expect("mixed").len() as u64
    );
    assert_eq!(n_endo, n_static + n_pred + n_fwd + n_mixed);
    assert_eq!(
        obj["n_state_variables"].as_u64().expect("n_state"),
        n_pred + n_mixed
    );
    assert_eq!(
        obj["n_jumpers"].as_u64().expect("n_jumpers"),
        n_fwd + n_mixed
    );
    assert!(obj["n_equations"].as_u64().expect("n_equations") > 0);
    let blob = info.to_string();
    assert!(!blob.contains("blocks"));
    assert!(!blob.to_ascii_lowercase().contains("blanchard"));
}

#[test]
fn model_info_workspace_swff_splices_inc() {
    let mut files = swff_relative_files();
    let extra = format!("parameters mcp_only;\n{}", files["swff_params.inc"]);
    files.insert("swff_params.inc".to_string(), extra);
    let info = dynare_model_info(&files["swff.mod"], Some("swff.mod"), Some(&files));
    let params = info["parameters"].as_array().expect("parameters");
    let names: Vec<&str> = params.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        names.contains(&"mcp_only"),
        "workspace model_info should see a parameter declared in the include: {names:?}"
    );
}

#[test]
fn model_info_files_without_active_does_not_splice() {
    let mut files = swff_relative_files();
    let extra = format!("parameters mcp_only;\n{}", files["swff_params.inc"]);
    files.insert("swff_params.inc".to_string(), extra);
    let info = dynare_model_info(&files["swff.mod"], None, Some(&files));
    let params = info["parameters"].as_array().expect("parameters");
    let names: Vec<&str> = params.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        !names.contains(&"mcp_only"),
        "model_info without active_file must not splice includes: {names:?}"
    );
}

#[test]
fn compare_models_drop_var_and_param_raw() {
    let a = read_mod("trend_rbc_gov_inv");
    let b = a
        .replacen("    log_n    (long_name='log labor');", "", 1)
        .replacen("betta   = 0.99;", "betta   = 0.95;", 1);
    assert_ne!(a, b, "mutations must change the file");
    let diff = dynare_compare_models(&a, &b, None, None, None, None, None);
    let obj = diff.as_object().expect("object");
    let removed = obj["removed_endogenous"]
        .as_array()
        .expect("removed_endogenous");
    assert!(
        removed.iter().any(|v| v.as_str() == Some("log_n")),
        "dropped log_n should appear in removed_endogenous: {removed:?}"
    );
    let changed = obj["changed_parameter_values"]
        .as_array()
        .expect("changed_parameter_values");
    assert!(
        changed
            .iter()
            .any(|row| row.get("name").and_then(|v| v.as_str()) == Some("betta")),
        "betta raw change missing: {changed:?}"
    );
    for key in obj.keys() {
        let lower = key.to_ascii_lowercase();
        assert!(
            !lower.contains("steady_state")
                && !lower.contains("steadystate")
                && !lower.contains("computed"),
            "SS field in compare: {key}"
        );
    }
    assert!(!obj.contains_key("blocks"));
    assert!(!obj.contains_key("changed_steady_state_values"));
}

#[test]
fn compare_models_workspace_swff_param_overlay() {
    let files_a = swff_relative_files();
    let mut files_b = files_a.clone();
    let inc = files_a["swff_params.inc"].replacen("alppha = 0.178678;", "alppha = 0.20;", 1);
    assert_ne!(inc, files_a["swff_params.inc"]);
    files_b.insert("swff_params.inc".to_string(), inc);
    let diff = dynare_compare_models(
        &files_a["swff.mod"],
        &files_b["swff.mod"],
        Some("swff.mod"),
        Some("swff.mod"),
        Some(&files_a),
        Some(&files_b),
        None,
    );
    let changed = diff["changed_parameter_values"]
        .as_array()
        .expect("changed_parameter_values");
    assert!(
        changed
            .iter()
            .any(|row| row.get("name").and_then(|v| v.as_str()) == Some("alppha")),
        "workspace compare must see alppha change in the include: {changed:?}"
    );
}

#[test]
fn model_info_n_equations_skips_hash_and_static() {
    let text = "\
var y x;
varexo e;
parameters a;
a = 0.5;
model;
# helper = a;
[static] x = 0;
y = a*y(-1)+e;
end;
";
    let info = dynare_model_info(text, None, None);
    assert_eq!(info["n_equations"].as_u64().expect("n_equations"), 1);
    assert!(!info.as_object().expect("object").contains_key("blocks"));
}

#[test]
fn compare_models_synthesizes_without_active_file() {
    let files_a = swff_relative_files();
    let mut files_b = files_a.clone();
    let inc = files_a["swff_params.inc"].replacen("alppha = 0.178678;", "alppha = 0.20;", 1);
    files_b.insert("swff_params.inc".to_string(), inc);
    let diff = dynare_compare_models(
        &files_a["swff.mod"],
        &files_b["swff.mod"],
        None,
        None,
        Some(&files_a),
        Some(&files_b),
        None,
    );
    let changed = diff["changed_parameter_values"]
        .as_array()
        .expect("changed_parameter_values");
    assert!(
        changed
            .iter()
            .any(|row| row.get("name").and_then(|v| v.as_str()) == Some("alppha")),
        "compare without active_file should still see include overlays: {changed:?}"
    );
}

#[test]
fn compare_models_empty_files_a_falls_back_to_files() {
    let files_a = swff_relative_files();
    let mut files_b = files_a.clone();
    let inc = files_a["swff_params.inc"].replacen("alppha = 0.178678;", "alppha = 0.20;", 1);
    files_b.insert("swff_params.inc".to_string(), inc);
    let empty = HashMap::new();
    let diff = dynare_compare_models(
        &files_a["swff.mod"],
        &files_b["swff.mod"],
        Some("swff.mod"),
        Some("swff.mod"),
        Some(&empty),
        Some(&files_b),
        Some(&files_a),
    );
    let changed = diff["changed_parameter_values"]
        .as_array()
        .expect("changed_parameter_values");
    assert!(
        changed
            .iter()
            .any(|row| row.get("name").and_then(|v| v.as_str()) == Some("alppha")),
        "empty files_a must fall back to files: {changed:?}"
    );
}
