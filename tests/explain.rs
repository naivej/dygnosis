use std::path::PathBuf;
use std::process::Command;

use dygnosis::explain::{explain, known_codes, render_markdown, ExplainKind};
use dygnosis::{analyze, check_file, parse};

const RUST_CODES: &[&str] = &[
    "E001", "E020", "E021", "E023", "E024", "E025", "E030", "E058", "E059", "E061", "E062",
    "E063", "E064", "E065", "E090", "E093", "E095", "E100", "E101", "E103", "E111", "E130", "E170",
    "E171", "E172", "E173", "E174", "E175", "E176", "E177", "E180", "E181", "E182", "E183", "E184",
    "E185", "E186", "E187", "E188", "E189", "E190", "E999", "I050", "S001", "S002", "S003", "S004",
    "S005", "S006", "S007", "S008", "S009", "S010", "S011", "S012", "S013", "S014", "S015", "S016",
    "S017", "S018", "S019", "S020", "S021", "S022", "S023", "S024", "S025", "S026", "S027", "S028",
    "S029", "S030", "S031", "S032", "S033", "S034", "S035", "S036", "S037", "S038", "S039", "S040",
    "S041", "S042", "S043", "S044", "S045", "S046", "S047", "S048", "S049", "S050", "S051", "S052",
    "S053", "S054", "S055", "S056", "S057", "S058", "S059", "S060", "W010", "W011", "W012", "W013",
    "W020", "W022", "W031", "W042", "W051", "W052", "W054", "W055", "W056", "W057", "W060", "W061",
    "W062", "W070", "W091", "W092", "W094", "W102", "W110", "W112", "W120", "W121", "W122", "W131",
    "W140", "W150", "W160", "W170", "W186",
];

const THIN_CODES: &[&str] = &[
    "E001", "E020", "E021", "E023", "E024", "E025", "E030", "E058", "E059", "E061", "E062",
    "E063", "E064", "E065", "E090", "E093", "E095", "E100", "E101", "E103", "E111", "E130", "E170",
    "E171", "E172", "E173", "E174", "E175", "E176", "E177", "E180", "E181", "E182", "E183", "E184",
    "E185", "E999", "I050", "W010", "W011", "W012", "W013", "W020", "W022", "W031", "W042", "W051",
    "W052", "W054", "W055", "W056", "W057", "W060", "W061", "W062", "W070", "W091", "W092", "W094",
    "W102", "W110", "W112", "W120", "W121", "W122", "W131", "W140", "W150", "W160", "W170",
];

const EMIT: &[&str] = &[
    "E001", "E020", "E021", "E023", "E024", "E025", "E030", "E058", "E059", "E061", "E062", "E063",
    "E064", "E065", "E090", "E093", "E095", "E100", "E101", "E103", "E111", "E130", "E170", "E171",
    "E172", "E173", "E174", "E175", "E176", "E177", "E180", "E181", "E182", "E183", "E184", "E185",
    "W022", "W031", "W042", "W121", "W131", "W150", "W170",
];

const ADDED: &[&str] = &[
    "E999", "I050", "W010", "W011", "W012", "W013", "W020", "W051", "W052", "W054", "W055", "W056",
    "W057", "W060", "W061", "W062", "W070", "W091", "W092", "W094", "W102", "W110", "W112", "W120",
    "W122", "W140", "W160",
];

const SKIP_KEYS: &[&str] = &[
    "E186", "E187", "E188", "E189", "E190", "S001", "S002", "S003", "S004", "S005", "S006", "S007",
    "S008", "S009", "S010", "S011", "S012", "S013", "S014", "S015", "S016", "S017", "S018", "S019",
    "S020", "S021", "S022", "S023", "S024", "S025", "S026", "S027", "S028", "S029", "S030", "S031",
    "S032", "S033", "S034", "S035", "S036", "S037", "S038", "S039", "S040", "S041", "S042", "S043",
    "S044", "S045", "S046", "S047", "S048", "S049", "S050", "S051", "S052", "S053", "S054", "S055",
    "S056", "S057", "S058", "S059", "S060", "W186",
];

const OUT: &[&str] = &[
    "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
];

const VACATED: &[&str] = &[
    "E010", "E050", "E051", "E052", "E053", "E060", "W021", "W050", "W053", "W090", "W093", "W095",
    "W100", "W101", "W103", "W111", "W130",
];

const WARRANT_CODES: &[&str] = &[
    "E001", "E020", "E023", "E058", "E062", "E090", "E093", "E101", "W170",
];

const FORBIDDEN: &[&str] = &[
    "Compute Steady State",
    "code action",
    "Gauss-Seidel",
    "trust-region",
    "homotopy",
    "random restarts",
];

fn expected_markdown(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/expected/explain")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("expected markdown missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn dygnosis() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_dygnosis"));
    cmd.env("RUST_LOG", "off");
    cmd
}

fn stdout_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .replace("\r\n", "\n")
}

fn stderr_text(output: &std::process::Output) -> String {
    String::from_utf8(output.stderr.clone())
        .unwrap()
        .replace("\r\n", "\n")
}

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn read_mod(archive_dir: &str) -> String {
    let path = copilot_mod(archive_dir);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

#[test]
fn known_codes_is_exactly_the_136_rust_keys() {
    assert_eq!(EMIT.len(), 43);
    assert_eq!(ADDED.len(), 27);
    assert_eq!(SKIP_KEYS.len(), 66);
    assert_eq!(THIN_CODES.len(), 70);
    assert_eq!(RUST_CODES.len(), 136);
    assert_eq!(known_codes(), RUST_CODES);
    assert_eq!(known_codes().len(), 136);
    assert!(!RUST_CODES.contains(&"P000"));
    assert!(!RUST_CODES.contains(&"E178"));
    assert!(!RUST_CODES.contains(&"E179"));
}

#[test]
fn thin_markdown_matches_expected() {
    for code in THIN_CODES {
        assert_eq!(
            render_markdown(code).as_deref(),
            Some(expected_markdown(&format!("{code}.md")).as_str()),
            "markdown mismatch for {code}"
        );
    }
}

#[test]
fn skip_markdown_matches_expected() {
    for code in SKIP_KEYS {
        assert_eq!(
            render_markdown(code).as_deref(),
            Some(expected_markdown(&format!("{code}.md")).as_str()),
            "markdown mismatch for {code}"
        );
        let md = render_markdown(code).expect(code);
        assert!(
            md.contains("This code is never emitted."),
            "{code} markdown must say it is never emitted"
        );
        for phrase in FORBIDDEN {
            assert!(!md.contains(phrase), "{code} markdown contains {phrase:?}");
        }
    }
}

#[test]
fn explain_kinds_are_emit_skip_added() {
    for code in EMIT {
        let entry = explain(code).unwrap_or_else(|| panic!("{code}"));
        assert_eq!(entry.kind, ExplainKind::Emit, "{code}");
        assert_eq!(entry.kind.as_str(), "emit", "{code}");
    }
    for code in ADDED {
        let entry = explain(code).unwrap_or_else(|| panic!("{code}"));
        assert_eq!(entry.kind, ExplainKind::Added, "{code}");
        assert_eq!(entry.kind.as_str(), "added", "{code}");
    }
    for code in SKIP_KEYS {
        let entry = explain(code).unwrap_or_else(|| panic!("{code}"));
        assert_eq!(entry.kind, ExplainKind::Skip, "{code}");
        assert_eq!(entry.kind.as_str(), "skip", "{code}");
    }
    assert!(explain("E060").is_none());
    assert_eq!(explain("W062").unwrap().kind, ExplainKind::Added);
    assert_eq!(explain("E186").unwrap().kind, ExplainKind::Skip);
    assert_eq!(explain("W013").unwrap().kind, ExplainKind::Added);
    assert_eq!(explain("W186").unwrap().kind, ExplainKind::Skip);
    assert_eq!(explain("s001").unwrap().kind, ExplainKind::Skip);
}

#[test]
fn out_and_vacated_are_unknown() {
    for code in OUT.iter().chain(VACATED).chain(["E178", "E179"].iter()) {
        assert!(explain(code).is_none(), "{code} should be unknown");
        assert!(
            render_markdown(code).is_none(),
            "{code} should have no markdown"
        );
        assert!(
            !known_codes().iter().any(|c| c == code),
            "{code} must not be a catalog key"
        );
    }
}

#[test]
fn i050_matches_recorded_rewrite() {
    let md = render_markdown("I050").expect("I050");
    assert_eq!(md, expected_markdown("I050.md"));
    let entry = explain("I050").unwrap();
    for phrase in FORBIDDEN {
        assert!(
            !entry.title.contains(phrase),
            "I050 title contains {phrase:?}"
        );
        assert!(
            !entry.body.contains(phrase),
            "I050 body contains {phrase:?}"
        );
        assert!(!md.contains(phrase), "I050 markdown contains {phrase:?}");
    }
}

#[test]
fn w042_matches_recorded_rewrite() {
    let md = render_markdown("W042").expect("W042");
    assert_eq!(md, expected_markdown("W042.md"));
    let entry = explain("W042").unwrap();
    assert_eq!(
        entry.title,
        "Endogenous variable missing from steady_state_model"
    );
    for phrase in FORBIDDEN {
        assert!(
            !entry.body.contains(phrase),
            "W042 body contains {phrase:?}"
        );
        assert!(!md.contains(phrase), "W042 markdown contains {phrase:?}");
    }
}

#[test]
fn p_codes_are_unknown() {
    for code in ["P000", "P001", "P123", "p001", "p000"] {
        assert!(explain(code).is_none(), "{code} should be unknown");
        assert!(
            render_markdown(code).is_none(),
            "{code} should have no markdown"
        );
        assert!(
            !known_codes().iter().any(|c| c.eq_ignore_ascii_case(code)),
            "{code} must not be a catalog key"
        );
    }
}

#[test]
fn lookup_is_case_insensitive() {
    assert_eq!(explain("w013").unwrap(), explain("W013").unwrap());
    assert!(render_markdown("w013").unwrap().starts_with("### w013: "));
    assert_eq!(explain("s001").unwrap(), explain("S001").unwrap());
    assert!(explain("dynr").is_none());
    assert!(explain("E040").is_none());
    assert!(explain("E010").is_none());
    assert!(explain("P").is_none());
}

#[test]
fn skip_keys_are_not_emitted_on_trend_rbc_gov_inv() {
    let text = read_mod("trend_rbc_gov_inv");
    let path = copilot_mod("trend_rbc_gov_inv");
    let path_str = path.to_str().expect("utf-8 path");
    let from_analyze: Vec<String> = analyze(&parse(&text)).into_iter().map(|d| d.code).collect();
    let from_check: Vec<String> = check_file(&text, path_str)
        .into_iter()
        .map(|d| d.code)
        .collect();
    for code in SKIP_KEYS {
        assert!(
            !from_analyze.iter().any(|c| c == code),
            "analyze emitted skip key {code}: {from_analyze:?}"
        );
        assert!(
            !from_check.iter().any(|c| c == code),
            "check_file emitted skip key {code}: {from_check:?}"
        );
    }
}

#[test]
fn cli_explain_known_code() {
    let output = dygnosis().args(["explain", "W013"]).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = stdout_text(&output);
    assert_eq!(stdout, render_markdown("W013").unwrap() + "\n");
    assert_eq!(stdout, expected_markdown("W013.md") + "\n");
}

#[test]
fn cli_explain_unknown_code() {
    for code in ["E040", "DYNR", "W071", "E010", "P001"] {
        let output = dygnosis().args(["explain", code]).output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{code}");
        let stderr = stderr_text(&output);
        assert!(
            stderr.contains(&format!(
                "No documentation found for diagnostic code '{code}'. Run `dygnosis explain --list` to see known codes."
            )),
            "{code} stderr: {stderr:?}"
        );
        assert!(
            !stderr.contains("python_dynare_lsp"),
            "{code} stderr names the Python module"
        );
    }
}

#[test]
fn cli_explain_list() {
    let output = dygnosis().args(["explain", "--list"]).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = stdout_text(&output);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], "Documented diagnostic codes:");

    let mut entries = Vec::new();
    let mut i = 1;
    while i < lines.len() && !lines[i].is_empty() {
        let line = lines[i];
        assert!(line.starts_with("  "), "{line:?}");
        let rest = &line[2..];
        assert!(
            rest.len() >= 15,
            "list line too short for column layout: {line:?}"
        );
        let code = rest[..6].trim_end();
        assert_eq!(&rest[6..8], "  ", "column gap mismatch: {line:?}");
        let kind = rest[8..13].trim_end();
        assert_eq!(&rest[13..15], "  ", "kind column gap mismatch: {line:?}");
        let title = &rest[15..];
        assert!(
            matches!(kind, "emit" | "skip" | "added"),
            "kind token must be emit/skip/added: {line:?}"
        );
        entries.push((code, kind, title));
        i += 1;
    }
    assert_eq!(entries.len(), 136);
    assert_eq!(
        entries.iter().map(|(c, _, _)| *c).collect::<Vec<_>>(),
        RUST_CODES
    );
    for code in OUT.iter().chain(VACATED) {
        assert!(
            entries.iter().all(|(c, _, _)| c != code),
            "{code} present in --list"
        );
    }
    let w013 = entries.iter().find(|(c, _, _)| *c == "W013").unwrap();
    assert_eq!(w013.1, "added");
    assert_eq!(w013.2, explain("W013").unwrap().title);
    let e001 = entries.iter().find(|(c, _, _)| *c == "E001").unwrap();
    assert_eq!(e001.1, "emit");
    let e186 = entries.iter().find(|(c, _, _)| *c == "E186").unwrap();
    assert_eq!(e186.1, "skip");
    assert_eq!(e186.2, "Unused endogenous after substitution");
    let i050 = entries.iter().find(|(c, _, _)| *c == "I050").unwrap();
    assert_eq!(i050.1, "added");
    assert_eq!(i050.2, "No initval or steady_state_model block");
    let s001 = entries.iter().find(|(c, _, _)| *c == "S001").unwrap();
    assert_eq!(s001.1, "skip");
    let w186 = entries.iter().find(|(c, _, _)| *c == "W186").unwrap();
    assert_eq!(w186.1, "skip");
    assert_eq!(w186.2, "Possible auxiliary name in a symbol list");
    assert_eq!(
        lines.get(i).copied(),
        Some(""),
        "expected blank line before footer"
    );
    assert_eq!(
        lines.get(i + 1).copied(),
        Some("136 codes. Run `dygnosis explain <CODE>` for details.")
    );
    assert!(!stdout.contains("python_dynare_lsp"));
    assert!(!stdout.contains("DYNR"));
    assert!(!stdout.contains("P000"));
    assert!(!stdout.contains("omit"));
}

#[test]
fn warrant_heading_only_on_warrant_codes() {
    for code in THIN_CODES {
        let md = render_markdown(code).expect(code);
        let has = md.contains("**Warrant**");
        let should = WARRANT_CODES.contains(code);
        assert_eq!(
            has, should,
            "{code}: Warrant heading present={has}, expected={should}"
        );
        if should {
            assert!(
                md.contains("They refuse:")
                    || md.contains("They report")
                    || md.contains("They accept and WARN:")
                    || md.contains("`Unknown symbol")
                    || md.contains("bison")
                    || md.contains("perpendicular symbol"),
                "{code} warrant body must still quote their string",
            );
        }
    }
    for code in SKIP_KEYS {
        let md = render_markdown(code).expect(code);
        assert!(
            !md.contains("**Warrant**"),
            "{code} skip markdown must not have a Warrant heading"
        );
    }
}

#[test]
fn cli_explain_missing_code_lists() {
    let listed = dygnosis().args(["explain", "--list"]).output().unwrap();
    let missing = dygnosis().args(["explain"]).output().unwrap();
    assert_eq!(listed.status.code(), Some(0));
    assert_eq!(missing.status.code(), Some(0));
    assert_eq!(listed.stdout, missing.stdout);
}
