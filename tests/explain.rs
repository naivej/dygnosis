use std::path::PathBuf;
use std::process::Command;

use dygnosis::explain::{explain, known_codes, render_markdown};

const RUST_CODES: &[&str] = &[
    "E001", "E020", "E021", "E023", "E024", "E025", "E030", "E058", "E059", "E060", "E061", "E062",
    "E063", "E064", "E065", "E090", "E093", "E095", "E100", "E101", "E103", "E111", "E130", "E999",
    "I050", "P000", "W010", "W011", "W012", "W013", "W020", "W022", "W042", "W051", "W052", "W054",
    "W055", "W056", "W057", "W060", "W061", "W070", "W091", "W092", "W094", "W102", "W110", "W112",
    "W120", "W121", "W122", "W131", "W140", "W150",
];

const SKIP: &[&str] = &[
    "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
];

const VACATED: &[&str] = &[
    "E010", "E050", "E051", "E052", "E053", "W021", "W050", "W053", "W090", "W093", "W095", "W100",
    "W101", "W103", "W111", "W130",
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

#[test]
fn known_codes_is_exactly_the_54_rust_keys() {
    assert_eq!(known_codes(), RUST_CODES);
    assert_eq!(known_codes().len(), 54);
}

#[test]
fn thin_markdown_matches_expected() {
    for code in RUST_CODES {
        assert_eq!(
            render_markdown(code).as_deref(),
            Some(expected_markdown(&format!("{code}.md")).as_str()),
            "markdown mismatch for {code}"
        );
    }
}

#[test]
fn skip_codes_are_unknown() {
    for code in SKIP.iter().chain(VACATED) {
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
fn p_digit_codes_route_to_p000() {
    let p000 = explain("P000").unwrap();
    assert_eq!(explain("P001").unwrap(), p000);
    assert_eq!(explain("P123").unwrap(), p000);
    assert_eq!(explain("p001").unwrap(), p000);
    assert_eq!(explain("p000").unwrap(), p000);

    let p001 = render_markdown("P001").unwrap();
    assert_eq!(p001, expected_markdown("P001.md"));
    assert!(p001.starts_with("### P001: "));
    assert!(p001.contains(p000.body));
    assert_eq!(
        render_markdown("P000").unwrap(),
        format!("### P000: {}\n\n{}\n", p000.title, p000.body)
    );
    assert!(render_markdown("p001").unwrap().starts_with("### p001: "));
    assert!(!known_codes().contains(&"P001"));
}

#[test]
fn lookup_is_case_insensitive() {
    assert_eq!(explain("w013").unwrap(), explain("W013").unwrap());
    assert!(render_markdown("w013").unwrap().starts_with("### w013: "));
    assert!(explain("dynr").is_none());
    assert!(explain("E040").is_none());
    assert!(explain("E010").is_none());
    assert!(explain("P").is_none());
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
    for code in ["E040", "DYNR", "W071", "E010"] {
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
            rest.len() >= 8,
            "list line too short for column layout: {line:?}"
        );
        let code = rest[..6].trim_end();
        assert_eq!(&rest[6..8], "  ", "column gap mismatch: {line:?}");
        let title = &rest[8..];
        entries.push((code, title));
        i += 1;
    }
    assert_eq!(entries.len(), 54);
    assert_eq!(
        entries.iter().map(|(c, _)| *c).collect::<Vec<_>>(),
        RUST_CODES
    );
    for code in SKIP.iter().chain(VACATED) {
        assert!(
            entries.iter().all(|(c, _)| c != code),
            "{code} present in --list"
        );
    }
    let w013_title = explain("W013").unwrap().title;
    assert_eq!(
        entries.iter().find(|(c, _)| *c == "W013").map(|(_, t)| *t),
        Some(w013_title)
    );
    assert_eq!(
        entries.iter().find(|(c, _)| *c == "I050").map(|(_, t)| *t),
        Some("No initval or steady_state_model block")
    );
    assert_eq!(
        lines.get(i).copied(),
        Some(""),
        "expected blank line before footer"
    );
    assert_eq!(
        lines.get(i + 1).copied(),
        Some("54 codes. Run `dygnosis explain <CODE>` for details.")
    );
    assert!(!stdout.contains("python_dynare_lsp"));
    assert!(!stdout.contains("DYNR"));
}

#[test]
fn cli_explain_missing_code_lists() {
    let listed = dygnosis().args(["explain", "--list"]).output().unwrap();
    let missing = dygnosis().args(["explain"]).output().unwrap();
    assert_eq!(listed.status.code(), Some(0));
    assert_eq!(missing.status.code(), Some(0));
    assert_eq!(listed.stdout, missing.stdout);
}

#[test]
fn cli_explain_p001_keeps_caller_heading() {
    let output = dygnosis().args(["explain", "P001"]).output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    let stdout = stdout_text(&output);
    assert_eq!(stdout, expected_markdown("P001.md") + "\n");
    assert!(stdout.starts_with("### P001: "));
}
