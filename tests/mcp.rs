use std::collections::HashMap;
use std::path::{Path, PathBuf};

use dygnosis::explain::{explain, known_codes, render_markdown};
use dygnosis::preprocessor::{run_workspace_preprocessor, DEFAULT_TIMEOUT};
use dygnosis::{
    analyze, auto_fix, check_e060, check_e061, check_w061, check_w160, count_gap, dynare_auto_fix,
    dynare_compare_models, dynare_diagnose, dynare_equations, dynare_explain,
    dynare_find_references, dynare_list_diagnostic_codes, dynare_list_options, dynare_model_info,
    dynare_related_files, dynare_rename, explain_equation, find_preprocessor, has_structural_error,
    parse, quiet_i050, reconcile_diagnostics, registered_tool_names, run_preprocessor,
    tools_list_json,
    Diagnostic, McpReference, McpWorkspaceReference, Workspace,
};
use serde_json::{json, Value};

const RUST_TOOLS: &[&str] = &[
    "dynare_diagnose",
    "dynare_model_info",
    "dynare_compare_models",
    "dynare_find_references",
    "dynare_rename",
    "dynare_auto_fix",
    "dynare_explain",
    "dynare_list_diagnostic_codes",
    "dynare_list_options",
    "dynare_equations",
    "dynare_related_files",
];

const DROPPED_TOOLS: &[&str] = &[
    "dynare_diagnose_workspace",
    "dynare_parse_summary",
    "dynare_find_references_workspace",
    "dynare_rename_workspace",
    "dynare_run_preprocessor",
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

const PARSE_SUMMARY_KEYS: &[&str] = &[
    "n_model_equations",
    "n_steady_state_equations",
    "n_initval_entries",
    "is_linear",
    "has_model_block",
    "has_steady_state_model_block",
    "has_initval_block",
    "has_shocks_block",
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

fn fixture_mod(rel: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn read_example(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/examples")
        .join(format!("{name}.mod"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("example missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn expected_mcp(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/expected/mcp")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("expected missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn compact_equation_list(value: &Value) -> Value {
    let equations = value["equations"]
        .as_array()
        .expect("equations array")
        .iter()
        .map(|row| {
            json!({
                "index": row["index"],
                "name": row["name"],
                "text": row["text"],
            })
        })
        .collect::<Vec<_>>();
    json!({
        "count_gap": value["count_gap"],
        "equations": equations,
    })
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

fn swff_related_files() -> HashMap<String, String> {
    let mut files = swff_relative_files();
    files.insert("run_swff.m".to_string(), read_copilot("swff", "run_swff.m"));
    files.insert(
        "swff_ff_coeffs.m".to_string(),
        read_copilot("swff", "swff_ff_coeffs.m"),
    );
    files
}

fn companion_fixture(rel: &str) -> (String, String, HashMap<String, String>) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/companions")
        .join(rel);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n");
    let key = path
        .to_str()
        .unwrap_or_else(|| panic!("non-utf8 path {}", path.display()))
        .to_string();
    let mut files = HashMap::new();
    files.insert(key.clone(), text.clone());
    (key, text, files)
}

fn compact_related(value: &Value) -> Value {
    let rows = value.as_array().expect("related files array");
    Value::Array(
        rows.iter()
            .map(|row| {
                json!({
                    "filename": row["filename"],
                    "kind": row["kind"],
                    "resolved": row["resolved"],
                })
            })
            .collect(),
    )
}

fn assert_related_shape(rows: &Value, files: &HashMap<String, String>) {
    let arr = rows.as_array().expect("related files array");
    for row in arr {
        let obj = row.as_object().expect("related file object");
        for (k, v) in obj {
            assert!(!v.is_null(), "{k} must not be null: {row}");
        }
        assert!(obj.contains_key("kind"), "kind required: {row}");
        assert!(obj.contains_key("filename"), "filename required: {row}");
        assert!(obj.contains_key("resolved"), "resolved required: {row}");
        let resolved = obj["resolved"].as_bool().expect("resolved bool");
        if resolved {
            let path = obj["path"].as_str().expect("resolved path string");
            assert!(!path.is_empty(), "resolved path nonempty: {row}");
            if files.contains_key(path) {
                // caller files-map key
            } else {
                let base = Path::new(path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                assert!(!base.is_empty(), "absolute path basename: {path}");
            }
        } else {
            assert!(
                !obj.contains_key("path"),
                "omit path when unresolved: {row}"
            );
        }
    }
}

fn related_row<'a>(rows: &'a Value, kind: &str, filename: &str) -> &'a Value {
    rows.as_array()
        .expect("array")
        .iter()
        .find(|row| {
            row["kind"].as_str() == Some(kind) && row["filename"].as_str() == Some(filename)
        })
        .unwrap_or_else(|| panic!("missing {kind} {filename}: {rows}"))
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

fn codes_of(diags: &[dygnosis::McpDiagnostic]) -> Vec<String> {
    diags.iter().map(|d| d.code.clone()).collect()
}

fn strip_out_codes(diags: impl IntoIterator<Item = Diagnostic>) -> Vec<String> {
    diags
        .into_iter()
        .filter(|d| !is_out_code(&d.code))
        .map(|d| d.code)
        .collect()
}

/// Same families as `check_in_workspace` (analyze + E060 / E061 / W061 + W160 / I050 quiet).
fn workspace_own(active: &str, files: &HashMap<String, String>) -> Vec<Diagnostic> {
    let mut ws = Workspace::new();
    for (name, content) in files {
        ws.update_document(name, content);
    }
    match ws.get_effective_model(active).cloned() {
        Some(model) => {
            let mut diags = analyze(&model);
            let records = ws.include_records(active).cloned().unwrap_or_default();
            diags.extend(check_e060(&records));
            diags.extend(check_e061(&records));
            diags.extend(check_w061(&mut ws, active));
            let companions = ws
                .companion_records(active)
                .map(|r| r.to_vec())
                .unwrap_or_default();
            diags.extend(check_w160(&companions));
            quiet_i050(&mut diags, &companions);
            diags
        }
        None => {
            let text = files.get(active).map(String::as_str).unwrap_or("");
            analyze(&parse(text))
        }
    }
}

fn expected_workspace_codes(active: &str, files: &HashMap<String, String>) -> Vec<String> {
    let own = workspace_own(active, files);
    let diags = if let Some(pp) = find_preprocessor(None) {
        let pre = run_workspace_preprocessor(active, files, &pp, DEFAULT_TIMEOUT);
        reconcile_diagnostics(&own, Some(&pre))
    } else {
        own
    };
    strip_out_codes(diags)
}

fn refs_single(value: Value) -> Vec<McpReference> {
    assert!(
        value.as_array().is_some_and(|rows| {
            rows.iter()
                .all(|row| row.get("file").is_none() && row.get("end_line").is_none())
        }),
        "no-map refs must omit file and end_line: {value}"
    );
    serde_json::from_value(value).expect("McpReference array")
}

fn refs_map(value: Value) -> Vec<McpWorkspaceReference> {
    assert!(
        value
            .as_array()
            .is_some_and(|rows| rows.iter().all(|row| row.get("file").is_some())),
        "map refs must include file: {value}"
    );
    serde_json::from_value(value).expect("McpWorkspaceReference array")
}

fn rename_single(value: &Value) -> &str {
    value.as_str().expect("no-map rename is a JSON string")
}

fn rename_map(value: Value) -> HashMap<String, String> {
    assert!(value.is_object(), "map rename is a JSON object: {value}");
    serde_json::from_value(value).expect("rename map")
}

#[test]
fn registered_tools_are_eleven() {
    let names = registered_tool_names();
    assert_eq!(names, RUST_TOOLS);
    assert_eq!(names.len(), 11);
    assert_eq!(names[9], "dynare_equations");
    assert_eq!(names[10], "dynare_related_files");

    let blob = serde_json::to_string(&tools_list_json()).expect("tools list json");
    for name in DROPPED_TOOLS {
        assert!(
            !blob.contains(name),
            "tools/list must not contain dropped {name}: {blob}"
        );
    }
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
    for name in ["dynare_count_gap", "dynare_explain_equation"] {
        assert!(
            !blob.contains(name),
            "tools/list must not contain {name}: {blob}"
        );
    }
}

#[test]
fn diagnose_p_core_thin_codes() {
    for name in P_CORE {
        let text = read_mod(name);
        let from_analyze = strip_out_codes(analyze(&parse(&text)));
        let expected: Vec<String> = if let Some(pp) = find_preprocessor(None) {
            let own = analyze(&parse(&text));
            let pre = run_preprocessor(&text, &pp, None, std::time::Duration::from_secs(30));
            strip_out_codes(reconcile_diagnostics(&own, Some(&pre)))
        } else {
            from_analyze
        };
        let diags = dynare_diagnose(&text, None, None);
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
        let from_mcp = codes_of(&diags);
        assert_eq!(from_mcp, expected, "diagnose codes for {name}");
        assert_no_out(&from_mcp);
    }
}

#[test]
fn diagnose_govt_rbc_has_no_option_list_e001() {
    let text = read_mod("govt_rbc_irf_matching");
    let analyze_codes: Vec<String> = analyze(&parse(&text)).into_iter().map(|d| d.code).collect();
    assert!(
        !analyze_codes.iter().any(|c| c == "E001"),
        "analyze must not emit option-list E001, got {analyze_codes:?}"
    );
    let diags = dynare_diagnose(&text, None, None);
    let codes = codes_of(&diags);
    assert!(
        !codes.iter().any(|c| c == "E001"),
        "dynare_diagnose must not emit option-list E001, got {codes:?}"
    );
    if find_preprocessor(None).is_some() {
        assert!(
            !codes.iter().any(|c| c == "E001"),
            "preprocessor path must not reintroduce option-list E001; got {codes:?}"
        );
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
    let files = swff_relative_files();
    let text = files["swff.mod"].clone();

    let diags = dynare_diagnose(&text, Some("swff.mod"), Some(&files));
    assert_no_out(diags.iter().map(|d| d.code.as_str()));
    for d in &diags {
        assert!(
            MCP_SEVERITIES.contains(&d.severity.as_str()),
            "MCP severity must not use CLI INFO; got {}",
            d.severity
        );
    }
    assert_eq!(
        codes_of(&diags),
        expected_workspace_codes("swff.mod", &files)
    );

    let empty = dynare_diagnose(&text, None, Some(&files));
    assert!(empty.is_empty(), "missing active_file must return []");
    let missing = dynare_diagnose(&text, Some("missing.mod"), Some(&files));
    assert!(missing.is_empty(), "unknown active_file must return []");

    let overlay = "var y\nmodel;\ny = 1;\nend;\n";
    assert_ne!(overlay, text);
    let overlay_diags = dynare_diagnose(overlay, Some("swff.mod"), Some(&files));
    let mut overlayed = files.clone();
    overlayed.insert("swff.mod".to_string(), overlay.to_string());
    assert_eq!(
        codes_of(&overlay_diags),
        expected_workspace_codes("swff.mod", &overlayed),
        "file_content overlay must win over files[active]"
    );
    assert_ne!(
        codes_of(&overlay_diags),
        codes_of(&diags),
        "overlay that differs from files[active] must change diagnostics"
    );
}

#[test]
fn model_info_includes_parse_summary() {
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
        let info = dynare_model_info(&text, None, None);
        let expected: Value = serde_json::from_str(expected_json).unwrap();
        for key in PARSE_SUMMARY_KEYS {
            assert_eq!(
                info[key], expected[key],
                "model_info parse_summary key {key} mismatch for {name}"
            );
        }
        assert!(
            !info.as_object().expect("object").contains_key("blocks"),
            "{name}: blocks must be absent"
        );
    }
}

#[test]
fn explain_e001_matches_library() {
    let expected = render_markdown("E001").expect("E001 is documented");
    assert_eq!(dynare_explain("E001"), expected);
}

#[test]
fn explain_e040_is_unknown() {
    let known = known_codes().join(", ");
    let expected = format!("No documentation found for code 'E040'. Known codes: {known}");
    assert_eq!(dynare_explain("E040"), expected);
}

#[test]
fn list_diagnostic_codes_matches_known_codes() {
    let list = dynare_list_diagnostic_codes();
    assert_eq!(list.len(), known_codes().len());
    let codes: Vec<&str> = list.iter().map(|item| item.code.as_str()).collect();
    assert_eq!(codes, known_codes());
    for item in &list {
        let title = explain(&item.code).map(|e| e.title).unwrap_or("");
        assert_eq!(item.title, title, "title mismatch for {}", item.code);
    }
}

#[test]
fn list_options_handler_matches_expected() {
    let omitted: Value =
        serde_json::from_str(include_str!("expected/list_options.omitted.json")).unwrap();
    assert_eq!(dynare_list_options(None), omitted);
    assert_eq!(dynare_list_options(Some("")), omitted);

    let known: Value =
        serde_json::from_str(include_str!("expected/list_options.known.json")).unwrap();
    let stoch = dynare_list_options(Some("stoch_simul"));
    assert_eq!(stoch, known["stoch_simul"]);
    assert_eq!(
        dynare_list_options(Some("Stoch_Simul")),
        known["stoch_simul"]
    );

    let unknown: Value =
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
    let hits = refs_single(dynare_find_references(&text, "betta", None, None));
    assert!(!hits.is_empty(), "expected betta hits, got {hits:?}");
    assert_eq!(dynare_find_references(&text, "", None, None), json!([]));

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
    let out_value = dynare_rename(&text, "betta", "beta_disc", None, None);
    let out = rename_single(&out_value);
    assert!(out.contains("beta_disc"));
    assert!(refs_single(dynare_find_references(out, "betta", None, None)).is_empty());
    assert!(!refs_single(dynare_find_references(out, "beta_disc", None, None)).is_empty());
    assert_ne!(out, text);
}

#[test]
fn rename_illegal_new_name_is_noop() {
    let text = read_mod("trend_rbc_gov_inv");
    assert_eq!(
        rename_single(&dynare_rename(&text, "betta", "1bad", None, None)),
        text
    );
    assert_eq!(
        rename_single(&dynare_rename(&text, "betta", "log", None, None)),
        text
    );
}

#[test]
fn find_references_workspace_swff() {
    let files = swff_relative_files();
    let shared = ["alppha", "rho", "gstar"]
        .into_iter()
        .find(|name| files["swff.mod"].contains(name) && files["swff_params.inc"].contains(name))
        .expect("shared ident in both swff files");

    let hits = refs_map(dynare_find_references(
        &files["swff.mod"],
        shared,
        Some("swff.mod"),
        Some(&files),
    ));
    let files_hit: std::collections::HashSet<&str> = hits.iter().map(|h| h.file.as_str()).collect();
    assert!(
        files_hit.contains("swff.mod"),
        "expected swff.mod hits, got {hits:?}"
    );
    assert!(
        files_hit.contains("swff_params.inc"),
        "expected swff_params.inc hits, got {hits:?}"
    );
    assert_eq!(
        dynare_find_references(&files["swff.mod"], "", Some("swff.mod"), Some(&files)),
        json!([])
    );
    assert_eq!(
        dynare_find_references(&files["swff.mod"], shared, None, Some(&files)),
        json!([])
    );
    assert_eq!(
        dynare_find_references(
            &files["swff.mod"],
            shared,
            Some("missing.mod"),
            Some(&files)
        ),
        json!([])
    );
}

#[test]
fn rename_workspace_swff_changed_only() {
    let files = swff_relative_files();
    let shared = ["alppha", "rho", "gstar"]
        .into_iter()
        .find(|name| files["swff.mod"].contains(name) && files["swff_params.inc"].contains(name))
        .expect("shared ident in both swff files");
    let new_name = format!("{shared}_renamed");

    let changed = rename_map(dynare_rename(
        &files["swff.mod"],
        shared,
        &new_name,
        Some("swff.mod"),
        Some(&files),
    ));
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
    assert!(refs_single(dynare_find_references(
        &changed["swff.mod"],
        shared,
        None,
        None
    ))
    .is_empty());
    assert!(refs_single(dynare_find_references(
        &changed["swff_params.inc"],
        shared,
        None,
        None
    ))
    .is_empty());

    assert_eq!(
        dynare_rename(
            &files["swff.mod"],
            shared,
            "1bad",
            Some("swff.mod"),
            Some(&files)
        ),
        json!({})
    );
    assert_eq!(
        dynare_rename(
            &files["swff.mod"],
            shared,
            "log",
            Some("swff.mod"),
            Some(&files)
        ),
        json!({})
    );
    assert_eq!(
        dynare_rename(&files["swff.mod"], shared, &new_name, None, Some(&files)),
        json!({})
    );
    assert_eq!(
        dynare_rename(
            &files["swff.mod"],
            shared,
            &new_name,
            Some("missing.mod"),
            Some(&files)
        ),
        json!({})
    );
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

    let hits = refs_map(dynare_find_references(
        &files["swff.mod"],
        shared,
        Some("swff.mod"),
        Some(&files),
    ));
    assert!(
        hits.iter().all(|h| h.file != "unrelated.mod"),
        "unrelated file must be out of scope: {hits:?}"
    );

    let new_name = format!("{shared}_renamed");
    let changed = rename_map(dynare_rename(
        &files["swff.mod"],
        shared,
        &new_name,
        Some("swff.mod"),
        Some(&files),
    ));
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
    for key in PARSE_SUMMARY_KEYS {
        assert!(obj.contains_key(*key), "missing parse_summary key {key}");
    }
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
    assert!(!obj.contains_key("common_equations"));
    assert!(obj["markdown"].as_str().is_some());
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
    let summary_n = parse(text).summary().n_model_equations as u64;
    assert_eq!(
        info["n_model_equations"]
            .as_u64()
            .expect("n_model_equations"),
        summary_n
    );
    assert_ne!(
        info["n_equations"].as_u64().expect("n_equations"),
        info["n_model_equations"]
            .as_u64()
            .expect("n_model_equations")
    );
    assert_eq!(
        info["n_equations"].as_u64().expect("n_equations"),
        count_gap(&parse(text)).n_equations as u64
    );
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

const COMPARE_MODEL_A: &str = r#"
var c k;
varexo e;
parameters betta alpha delta;
betta = 0.99;
alpha = 0.33;
delta = 0.025;
model;
c = betta*c(+1);
k = (1-delta)*k(-1) + e;
end;
"#;

const COMPARE_MODEL_B: &str = r#"
var c n;
varexo u;
parameters betta rho;
betta = 0.99;
rho = 0.9;
model;
c = betta*c(+1);
n = rho*n(-1) + u;
end;
"#;

#[test]
fn dynare_compare_models_indexed_equations_and_markdown() {
    let diff = dynare_compare_models(
        COMPARE_MODEL_A,
        COMPARE_MODEL_B,
        None,
        None,
        None,
        None,
        None,
    );
    let obj = diff.as_object().expect("object");
    assert!(!obj.contains_key("common_equations"));
    let md = obj["markdown"].as_str().expect("markdown");
    assert!(md.contains("# Model diff"));

    for key in ["added_equations", "removed_equations"] {
        let rows = obj[key].as_array().expect(key);
        for row in rows {
            assert!(row.get("index").is_some(), "{key} {row}");
            assert!(row.get("text").is_some(), "{key} {row}");
        }
        if let Some(first) = rows.first() {
            let idx = first["index"].as_u64().expect("index");
            assert!(
                md.contains(&format!("[{idx}]")),
                "markdown must show added/removed index [{idx}]: {md}"
            );
        }
    }
    for row in obj["changed_equations"].as_array().expect("changed") {
        assert!(row.get("index_old").is_some(), "{row}");
        assert!(row.get("index_new").is_some(), "{row}");
        assert!(row.get("text_old").is_some(), "{row}");
        assert!(row.get("text_new").is_some(), "{row}");
    }
    let blob = diff.to_string();
    assert!(
        !blob.contains("c = betta*c(+1)") && !blob.contains("c=betta*c(+1)"),
        "common Euler must be absent: {blob}"
    );
    let expected: Value =
        serde_json::from_str(&expected_mcp("dynare_compare_models.a_vs_b.json")).unwrap();
    assert_eq!(diff, expected);
}

fn assert_count_gap_shape(payload: &Value) {
    let gap = payload["count_gap"].as_object().expect("count_gap object");
    assert!(gap.contains_key("n_endogenous"));
    assert!(gap.contains_key("n_equations"));
    assert!(gap.contains_key("delta"));
    assert!(gap.contains_key("unreferenced_endogenous"));
    assert!(gap.contains_key("expected_delta"));
}

fn row_has_no_explain(row: &Value) {
    assert!(
        row.get("explain").is_none(),
        "full list must omit explain: {row}"
    );
}

#[test]
fn dynare_equations_trend_rbc_gov_inv() {
    let text = read_mod("trend_rbc_gov_inv");
    let payload = dynare_equations(&text, None, None, None, None);
    let obj = payload.as_object().expect("object");
    assert!(obj.contains_key("equations"));
    assert!(obj.contains_key("count_gap"));
    assert!(!obj.contains_key("message"));
    assert_count_gap_shape(&payload);

    let eqs = payload["equations"].as_array().expect("equations");
    assert_eq!(eqs[0]["index"], 0);
    assert_eq!(eqs[0]["lhs"], "y");
    assert_eq!(eqs[0]["idents"][2]["name"], "kg");
    assert_eq!(eqs[0]["idents"][2]["timing"], -1);
    assert_eq!(payload["count_gap"]["n_equations"], 16);
    for row in eqs {
        row_has_no_explain(row);
        assert!(row.get("span").is_none(), "span must be omitted: {row}");
        assert!(row.get("name").is_some(), "name required: {row}");
        assert!(row.get("text").is_some(), "text required: {row}");
        assert!(row.get("lhs").is_some());
        assert!(row.get("rhs").is_some());
        assert!(row.get("static_tag").is_some());
        assert!(row.get("dynamic_tag").is_some());
        assert!(row.get("idents").is_some());
    }

    let info = dynare_model_info(&text, None, None);
    let info_obj = info.as_object().expect("model_info object");
    assert!(
        !info_obj.contains_key("equations"),
        "dynare_model_info must not grow an equation list"
    );
    assert_eq!(info["n_equations"], payload["count_gap"]["n_equations"]);

    let named = dynare_equations(&text, None, None, Some("production function"), None);
    let named_eqs = named["equations"].as_array().expect("named equations");
    assert_eq!(named_eqs.len(), 1);
    assert!(named_eqs[0]
        .get("explain")
        .and_then(|v| v.as_str())
        .is_some());

    let expected: Value =
        serde_json::from_str(&expected_mcp("dynare_equations.trend_rbc_gov_inv.json")).unwrap();
    assert_eq!(payload, expected);
}

#[test]
fn dynare_equations_reader_filters() {
    let text = fixture_mod("equations/reader.mod");
    let model = parse(&text);
    let gap = count_gap(&model);
    assert_eq!(gap.n_equations, 2);

    let full = dynare_equations(&text, None, None, None, None);
    let eqs = full["equations"].as_array().expect("equations");
    assert_eq!(eqs.len(), 2);
    assert_eq!(eqs[0]["index"], 0);
    assert_eq!(eqs[1]["index"], 1);
    assert_eq!(full["count_gap"]["n_equations"], 2);
    assert!(full.get("message").is_none());
    for row in eqs {
        row_has_no_explain(row);
    }

    let by_index = dynare_equations(&text, None, None, None, Some(0));
    let idx_rows = by_index["equations"].as_array().expect("index equations");
    assert_eq!(idx_rows.len(), 1);
    assert_eq!(idx_rows[0]["index"], 0);
    assert_eq!(
        idx_rows[0]["explain"].as_str().expect("explain"),
        explain_equation(&dygnosis::equations(&model)[0])
    );
    assert_eq!(by_index["count_gap"]["n_equations"], 2);
    assert!(by_index.get("message").is_none());

    let by_name = dynare_equations(&text, None, None, Some("euler"), None);
    let name_rows = by_name["equations"].as_array().expect("name equations");
    assert_eq!(name_rows.len(), 1);
    assert_eq!(name_rows[0]["name"], "euler");
    assert_eq!(
        name_rows[0]["explain"].as_str().expect("explain"),
        explain_equation(&dygnosis::equations(&model)[0])
    );
    assert_eq!(by_name["count_gap"]["n_equations"], 2);

    let unnamed = dynare_equations(&text, None, None, Some(""), None);
    let unnamed_rows = unnamed["equations"].as_array().expect("unnamed equations");
    assert_eq!(unnamed_rows.len(), 1);
    assert_eq!(unnamed_rows[0]["name"], "");
    assert!(unnamed_rows[0].get("explain").is_some());

    let unknown = dynare_equations(&text, None, None, Some("no_such"), None);
    assert_eq!(unknown["equations"], json!([]));
    assert_eq!(unknown["message"], "no equation named 'no_such'");
    assert_eq!(unknown["count_gap"]["n_equations"], 2);
    assert_count_gap_shape(&unknown);

    let oob = dynare_equations(&text, None, None, None, Some(9));
    assert_eq!(oob["equations"], json!([]));
    assert_eq!(oob["message"], "index 9 is out of range (0..2)");
    assert_eq!(oob["count_gap"]["n_equations"], 2);

    let both = dynare_equations(&text, None, None, Some("euler"), Some(0));
    assert_eq!(both["equations"], json!([]));
    assert_eq!(both["message"], "name and index must not both be set");
    assert_eq!(both["count_gap"]["n_equations"], 2);
}

#[test]
fn dynare_equations_tags_duplicate_name() {
    let text = fixture_mod("equations/tags.mod");
    let payload = dynare_equations(&text, None, None, Some("policy"), None);
    let eqs = payload["equations"].as_array().expect("equations");
    assert_eq!(eqs.len(), 2, "JC9: one tag may hit several rows: {payload}");
    assert_eq!(eqs[0]["name"], "policy");
    assert_eq!(eqs[1]["name"], "policy");
    assert_ne!(eqs[0]["text"], eqs[1]["text"]);
    assert_ne!(eqs[0]["explain"], eqs[1]["explain"]);
    assert!(eqs[0].get("explain").and_then(|v| v.as_str()).is_some());
    assert!(eqs[1].get("explain").and_then(|v| v.as_str()).is_some());
    assert_eq!(payload["count_gap"]["n_equations"], 3);
}

#[test]
fn dynare_equations_zlb_qe_after_parse_expand() {
    let text = read_mod("zlb_qe");
    let payload = dynare_equations(&text, None, None, None, None);
    let eqs = payload["equations"].as_array().expect("equations");
    let names: Vec<&str> = eqs.iter().filter_map(|row| row["name"].as_str()).collect();
    assert!(
        names.contains(&"F16 Taylor rule (no ZLB)"),
        "default @#else F16 missing: {names:?}"
    );
    assert!(
        !names.contains(&"F16 Taylor rule + ZLB"),
        "ZLB-bind F16 must be absent: {names:?}"
    );
    let unknown = dynare_equations(&text, None, None, Some("F16 Taylor rule + ZLB"), None);
    assert_eq!(unknown["equations"], json!([]));
    assert_eq!(
        unknown["message"],
        "no equation named 'F16 Taylor rule + ZLB'"
    );
    let f17 = eqs
        .iter()
        .find(|row| row["name"].as_str() == Some("F17 QE rule"))
        .expect("F17");
    assert!(
        f17["text"].as_str().expect("text").contains("qe = 0"),
        "F17 body: {}",
        f17["text"]
    );
    for row in eqs {
        row_has_no_explain(row);
    }
    let expected: Value =
        serde_json::from_str(&expected_mcp("dynare_equations.zlb_qe.list.json")).unwrap();
    assert_eq!(compact_equation_list(&payload), expected);
}

#[test]
fn dynare_equations_us_re09_rep_expectation() {
    let text = read_example("US_RE09_rep");
    let payload = dynare_equations(&text, None, None, None, None);
    let eqs = payload["equations"].as_array().expect("equations");
    assert!(
        eqs.iter().any(|row| row["text"]
            .as_str()
            .is_some_and(|t| t.contains("EXPECTATION(-16)"))),
        "expected EXPECTATION(-16) in some text: {payload}"
    );
    let expected: Value =
        serde_json::from_str(&expected_mcp("dynare_equations.US_RE09_rep.list.json")).unwrap();
    assert_eq!(compact_equation_list(&payload), expected);
}

#[test]
fn dynare_equations_swff_map_matches_model_info() {
    let files = swff_relative_files();
    let with_map = dynare_equations(
        &files["swff.mod"],
        Some("swff.mod"),
        Some(&files),
        None,
        None,
    );
    let info_map = dynare_model_info(&files["swff.mod"], Some("swff.mod"), Some(&files));
    assert_eq!(
        with_map["count_gap"]["n_equations"],
        info_map["n_equations"]
    );

    let no_active = dynare_equations(&files["swff.mod"], None, Some(&files), None, None);
    let info_no_active = dynare_model_info(&files["swff.mod"], None, Some(&files));
    assert_eq!(
        no_active["count_gap"]["n_equations"],
        info_no_active["n_equations"]
    );
}

#[test]
fn dynare_equations_w100_ok_expected_delta() {
    let text = fixture_mod("w100/w100_ok.mod");
    let payload = dynare_equations(&text, None, None, None, None);
    assert_eq!(payload["count_gap"]["expected_delta"], -1);
}

#[test]
fn dynare_related_files_swff_dump() {
    let files = swff_related_files();
    let rows = dynare_related_files(&files["swff.mod"], Some("swff.mod"), Some(&files));
    assert_related_shape(&rows, &files);

    let expected: Value =
        serde_json::from_str(&expected_mcp("dynare_related_files.swff.json")).unwrap();
    assert_eq!(compact_related(&rows), expected);

    let inc = related_row(&rows, "include", "swff_params.inc");
    assert_eq!(inc["path"], "swff_params.inc");
    let run = related_row(&rows, "run_script", "run_swff.m");
    assert_eq!(run["path"], "run_swff.m");

    let arr = rows.as_array().expect("array");
    assert!(
        arr.iter().all(|row| {
            !row["filename"]
                .as_str()
                .unwrap_or("")
                .contains("swff_ff_coeffs")
                && !row["path"]
                    .as_str()
                    .unwrap_or("")
                    .contains("swff_ff_coeffs")
        }),
        "swff_ff_coeffs.m is not a companion of swff.mod: {rows}"
    );
    for row in arr {
        if row["filename"]
            .as_str()
            .is_some_and(|n| n.ends_with(".inc"))
        {
            assert_eq!(row["kind"], "include", ".inc is include-only: {row}");
        }
    }
}

#[test]
fn dynare_related_files_missing_active_is_empty() {
    let files = swff_related_files();
    let text = files["swff.mod"].clone();
    assert_eq!(dynare_related_files(&text, None, Some(&files)), json!([]));
    assert_eq!(
        dynare_related_files(&text, Some(""), Some(&files)),
        json!([])
    );
    assert_eq!(
        dynare_related_files(&text, Some("missing.mod"), Some(&files)),
        json!([])
    );
    assert_eq!(dynare_related_files(&text, None, None), json!([]));
    assert_eq!(
        dynare_related_files(&text, Some("swff.mod"), None),
        json!([])
    );
    let empty = HashMap::new();
    assert_eq!(
        dynare_related_files(&text, Some("swff.mod"), Some(&empty)),
        json!([])
    );
}

#[test]
fn dynare_related_files_overlay_drops_include() {
    let files = swff_related_files();
    let overlay = files["swff.mod"].replacen("@#include \"swff_params.inc\"\n", "", 1);
    assert_ne!(overlay, files["swff.mod"]);
    let rows = dynare_related_files(&overlay, Some("swff.mod"), Some(&files));
    assert_related_shape(&rows, &files);
    let arr = rows.as_array().expect("array");
    assert!(
        arr.iter()
            .all(|row| row["kind"].as_str() != Some("include")),
        "overlay without include must drop the .inc: {rows}"
    );
    related_row(&rows, "run_script", "run_swff.m");
}

#[test]
fn dynare_related_files_named_missing() {
    let (key, text, files) = companion_fixture("named_missing.mod");
    let rows = dynare_related_files(&text, Some(&key), Some(&files));
    assert_related_shape(&rows, &files);
    let expected = json!([
        {"filename": "missing_data.csv", "kind": "datafile", "resolved": false},
        {"filename": "missing_mode", "kind": "mode_file", "resolved": false},
        {"filename": "missing_data_file.csv", "kind": "datafile", "resolved": false},
        {"filename": "missing_gsa.mat", "kind": "datafile", "resolved": false},
        {"filename": "missing_initval.csv", "kind": "datafile", "resolved": false},
        {"filename": "missing_histval.csv", "kind": "datafile", "resolved": false},
        {"filename": "missing_ext", "kind": "helper_m", "resolved": false},
        {"filename": "missing_ext_d1", "kind": "helper_m", "resolved": false},
        {"filename": "missing_prior", "kind": "helper_m", "resolved": false},
        {"filename": "missing_helper.m", "kind": "helper_m", "resolved": false},
    ]);
    assert_eq!(compact_related(&rows), expected);
    for banned in ["commented_data.csv", "commented_helper.m", "0", "1"] {
        assert!(
            rows.as_array()
                .unwrap()
                .iter()
                .all(|row| row["filename"].as_str() != Some(banned)),
            "must not list {banned}: {rows}"
        );
    }
    for kind in ["run_script", "prior_restrictions", "steady_state_file"] {
        assert!(
            rows.as_array()
                .unwrap()
                .iter()
                .all(|row| row["kind"].as_str() != Some(kind)),
            "missing convention must not be listed as {kind}: {rows}"
        );
    }

    let overlay = "var y;\nmodel;\ny = 1;\nend;\n";
    let overlaid = dynare_related_files(overlay, Some(&key), Some(&files));
    assert_eq!(
        overlaid,
        json!([]),
        "overlay without named files: {overlaid}"
    );
}

#[test]
fn dynare_related_files_01_fixtures() {
    let cases = [
        (
            "data_file.mod",
            "datafile",
            "data_file.csv",
            true,
            "data_file.csv",
        ),
        (
            "leftover_csv.mod",
            "datafile",
            "leftover.csv",
            true,
            "leftover.csv",
        ),
        ("dup_path.mod", "datafile", "dup.csv", true, "dup.csv"),
        (
            "ident_helper/ident_helper.mod",
            "helper_m",
            "my_ss_helper",
            true,
            "my_ss_helper.m",
        ),
        (
            "ss_present/ss_present.mod",
            "steady_state_file",
            "ss_present_steadystate.m",
            true,
            "ss_present_steadystate.m",
        ),
    ];
    for (rel, kind, filename, resolved, basename) in cases {
        let (key, text, files) = companion_fixture(rel);
        let rows = dynare_related_files(&text, Some(&key), Some(&files));
        assert_related_shape(&rows, &files);
        let row = related_row(&rows, kind, filename);
        assert_eq!(row["resolved"], resolved, "{rel}");
        if resolved {
            let path = row["path"].as_str().expect("path");
            assert!(
                !files.contains_key(path),
                "{rel}: sibling not in files map, path should be absolute, got {path}"
            );
            assert!(
                Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case(basename)),
                "{rel} path basename {basename}: {path}"
            );
        }
        let hits: Vec<_> = rows
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["filename"].as_str() == Some(filename))
            .collect();
        assert_eq!(hits.len(), 1, "{rel} one record for {filename}: {rows}");
    }

    let (key, text, files) = companion_fixture("absent_convention.mod");
    let rows = dynare_related_files(&text, Some(&key), Some(&files));
    assert_related_shape(&rows, &files);
    for kind in [
        "run_script",
        "prior_restrictions",
        "helper_m",
        "steady_state_file",
    ] {
        assert!(
            rows.as_array()
                .unwrap()
                .iter()
                .all(|row| row["kind"].as_str() != Some(kind)),
            "absent convention / ident helper must not list {kind}: {rows}"
        );
    }
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|row| row["filename"].as_str() != Some("missing_ident_helper")),
        "missing ident helper must not be listed: {rows}"
    );

    let (key, text, files) = companion_fixture("plus_pkg/plus_pkg.mod");
    let rows = dynare_related_files(&text, Some(&key), Some(&files));
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|row| row["kind"].as_str() != Some("steady_state_file")),
        "+FILENAME/steadystate.m is not convention: {rows}"
    );
}

#[test]
fn dynare_related_files_nested_include_datafile_stays_off_root() {
    let mut files = HashMap::new();
    files.insert(
        "root.mod".to_string(),
        "@#include \"helper.inc\"\nvar y;\nvarexo e;\nparameters rho;\nrho = 0.5;\nmodel;\ny = rho * y(-1) + e;\nend;\n"
            .to_string(),
    );
    files.insert(
        "helper.inc".to_string(),
        "estimation(datafile='nested_data.csv');\n".to_string(),
    );
    files.insert("nested_data.csv".to_string(), "y\n1\n".to_string());

    let rows = dynare_related_files(&files["root.mod"], Some("root.mod"), Some(&files));
    assert_related_shape(&rows, &files);
    let inc = related_row(&rows, "include", "helper.inc");
    assert_eq!(inc["resolved"], true);
    assert_eq!(inc["path"], "helper.inc");
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|row| row["filename"].as_str() != Some("nested_data.csv")
                && row["kind"].as_str() != Some("datafile")),
        "nested include datafile must not appear on root: {rows}"
    );

    let mut with_unresolved = files.clone();
    with_unresolved.insert(
        "root.mod".to_string(),
        "@#include \"helper.inc\"\n@#include \"gone.inc\"\nvar y;\nmodel;\ny = 1;\nend;\n"
            .to_string(),
    );
    let rows = dynare_related_files(
        &with_unresolved["root.mod"],
        Some("root.mod"),
        Some(&with_unresolved),
    );
    assert_related_shape(&rows, &with_unresolved);
    let gone = related_row(&rows, "include", "gone.inc");
    assert_eq!(gone["resolved"], false);
    assert!(gone.get("path").is_none());
}

#[test]
fn dynare_related_files_synthetic_prior_and_irf() {
    let mut files = HashMap::new();
    files.insert(
        "kinds.mod".to_string(),
        "@#include \"missing.inc\"\nvar y;\nvarexo e;\nparameters rho;\nrho = 0.5;\nmodel;\ny = rho * y(-1) + e;\nend;\nestimation(irf_matching_file=trans);\n"
            .to_string(),
    );
    files.insert(
        "kinds_prior_restrictions.m".to_string(),
        "% prior\n".to_string(),
    );
    files.insert("trans.m".to_string(), "% trans\n".to_string());
    let rows = dynare_related_files(&files["kinds.mod"], Some("kinds.mod"), Some(&files));
    assert_related_shape(&rows, &files);
    let expected = json!([
        {"filename": "missing.inc", "kind": "include", "resolved": false},
        {"filename": "kinds_prior_restrictions.m", "kind": "prior_restrictions", "resolved": true},
        {"filename": "trans", "kind": "irf_matching_file", "resolved": true},
    ]);
    assert_eq!(compact_related(&rows), expected);
    assert_eq!(
        related_row(&rows, "prior_restrictions", "kinds_prior_restrictions.m")["path"],
        "kinds_prior_restrictions.m"
    );
    assert_eq!(
        related_row(&rows, "irf_matching_file", "trans")["path"],
        "trans.m"
    );
}
