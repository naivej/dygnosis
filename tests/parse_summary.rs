use std::path::PathBuf;

use dygnosis::{parse, ParseSummary};

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn copilot_example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/examples")
        .join(format!("{name}.mod"))
}

fn assert_parse_summary_file(path: PathBuf, label: &str, expected_json: &str) {
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("fixture missing at {}: {e}", path.display());
    });
    let got = parse(&text).summary();
    let expected: ParseSummary = serde_json::from_str(expected_json).unwrap();
    assert_eq!(got, expected, "parse_summary mismatch for {label}");
}

fn assert_parse_summary(archive_dir: &str, expected_json: &str) {
    assert_parse_summary_file(copilot_mod(archive_dir), archive_dir, expected_json);
}

fn swff_inc_assignment_names() -> Vec<String> {
    let inc = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive/swff/swff_params.inc");
    let text = std::fs::read_to_string(&inc)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", inc.display()))
        .replace("\r\n", "\n");
    let model = parse(&text);
    model
        .helper_assignments
        .iter()
        .map(|a| model.name(a.name).to_string())
        .collect()
}

#[test]
fn parse_summary_matches_expected_on_trend_rbc_gov_inv() {
    assert_parse_summary(
        "trend_rbc_gov_inv",
        include_str!("expected/trend_rbc_gov_inv.parse_summary.json"),
    );
}

#[test]
fn parse_summary_matches_expected_on_sims_wu_2019() {
    assert_parse_summary(
        "sims_wu_2019",
        include_str!("expected/sims_wu_2019.parse_summary.json"),
    );
}

#[test]
fn parse_summary_matches_expected_on_govt_rbc_irf_matching() {
    assert_parse_summary(
        "govt_rbc_irf_matching",
        include_str!("expected/govt_rbc_irf_matching.parse_summary.json"),
    );
}

#[test]
fn parse_summary_matches_expected_on_lk2024() {
    assert_parse_summary("lk2024", include_str!("expected/lk2024.parse_summary.json"));
}

#[test]
fn parse_summary_matches_expected_on_zlb_qe() {
    assert_parse_summary("zlb_qe", include_str!("expected/zlb_qe.parse_summary.json"));
}

#[test]
#[allow(non_snake_case)]
fn parse_summary_matches_expected_on_US_RE09_rep() {
    assert_parse_summary_file(
        copilot_example("US_RE09_rep"),
        "US_RE09_rep",
        include_str!("expected/US_RE09_rep.parse_summary.json"),
    );
}

#[test]
fn parse_summary_matches_expected_on_swff_single_file() {
    assert_parse_summary("swff", include_str!("expected/swff.parse_summary.json"));
}

#[test]
fn parse_summary_swff_via_workspace_splices_inc() {
    use dygnosis::Workspace;

    let path = copilot_mod("swff");
    let mut ws = Workspace::new();
    ws.load_from_disk(&path)
        .unwrap_or_else(|| panic!("failed to load {}", path.display()));
    let uri = path.to_str().expect("utf-8 path");
    let effective = ws
        .get_effective_model(uri)
        .unwrap_or_else(|| panic!("effective model for {uri}"));
    let expected: ParseSummary =
        serde_json::from_str(include_str!("expected/swff.parse_summary.json")).unwrap();
    assert_eq!(
        effective.summary(),
        expected,
        "workspace parse_summary for swff"
    );
    let got: Vec<_> = effective
        .param_assignments
        .iter()
        .map(|a| effective.name(a.name).to_string())
        .collect();
    assert_eq!(got, swff_inc_assignment_names());
}
