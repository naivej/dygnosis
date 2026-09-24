use std::path::PathBuf;
use std::process::Command;

use dygnosis::explain::{explain, known_codes, render_markdown, ExplainKind};
use dygnosis::{analyze, check_file, parse};

const RUST_CODES: &[&str] = &[
    "E001", "E020", "E021", "E023", "E024", "E025", "E026", "E027", "E028", "E030", "E058", "E059",
    "E061", "E062", "E063", "E064", "E065", "E090", "E093", "E095", "E100", "E101", "E103", "E104",
    "E111", "E113", "E130", "E170", "E171", "E172", "E173", "E174", "E175", "E176", "E177", "E178",
    "E179", "E180", "E181", "E182", "E183", "E184", "E185", "E186", "E187", "E188", "E189", "E190",
    "E191", "E192", "E193", "E194", "E200", "E201", "E202", "E203", "E204", "E205", "E206", "E207", "E208", "E209", "E210",
    "E211", "E212", "E213", "E214", "E215", "E216", "E217", "E218", "E219", "E220", "E221", "E222",
    "E223", "E224", "E225", "E226", "E227", "E228", "E229", "E230", "E231", "E232", "E233", "E234",
    "E235", "E236", "E237", "E238", "E239", "E240", "E241", "E242", "E243", "E244", "E245", "E246",
    "E247", "E248", "E249", "E250", "E251", "E252", "E253", "E254", "E255", "E256", "E257", "E258",
    "E259", "E260", "E261", "E262", "E263", "E264", "E265", "E266", "E267", "E268", "E269", "E270",
    "E271", "E272", "E273", "E274", "E275", "E276", "E277", "E278", "E279", "E280", "E281", "E282",
    "E283", "E284", "E285", "E286", "E287", "E288", "E289", "E290", "E291", "E292", "E293", "E294",
    "E295", "E296", "E297", "E298", "E299", "E300", "E301", "E302", "E303", "E304", "E305", "E306",
    "E307", "E308", "E309", "E310", "E311", "E312", "E313", "E314", "E315", "E316", "E317", "E318",
    "E319", "E320", "E321", "E322", "E323", "E324", "E325", "E326", "E327", "E328", "E329", "E330",
    "E331", "E332", "E333", "E334", "E335", "E336", "E337", "E338", "E339", "E340", "E341", "E342",
    "E343", "E344", "E345", "E346", "E347", "E348", "E349", "E350", "E351", "E352", "E353", "E354",
    "E355", "E356", "E357", "E358", "E359", "E360", "E361", "E362", "E363", "E364", "E365", "E366",
    "E367", "E368", "E369", "E370", "E371", "E372", "E373", "E374", "E375", "E376", "E377", "E378",
    "E379", "E380", "E381", "E382", "E383", "E384", "E385", "E386", "E387", "E388", "E389", "E390",
    "E391", "E392", "E393", "E394", "E395", "E396", "E397", "E398", "E399", "E400", "E401", "E402",
    "E403", "E404", "E405", "E406", "E407", "E408", "E409", "E410", "E411", "E412", "E413", "E414",
    "E415", "E416", "E417", "E418", "E419", "E420", "E421", "E422", "E423", "E424", "E425", "E426",
    "E427", "E428", "E429", "E430", "E431", "E432", "E433", "E434", "E435", "E436", "E437", "E438",
    "E439", "E440", "E441", "E442", "E443", "E444", "E445", "E446", "E447", "E448", "E449", "E450", "E451", "E452", "E453", "E454", "E455", "E456", "E457", "E458", "E999", "I050",
    "S020", "S035", "S040", "S054", "S063", "W010", "W011", "W012", "W013", "W020",
    "W022", "W031", "W042", "W051", "W052", "W054", "W055", "W056", "W057", "W060", "W061", "W062",
    "W070", "W091", "W092", "W094", "W102", "W110", "W112", "W120", "W121", "W122", "W131", "W140",
    "W150", "W160", "W170", "W186", "W187", "W200", "W201", "W202", "W203", "W204", "W205", "W206",
];

const THIN_CODES: &[&str] = &[
    "E001", "E020", "E021", "E023", "E024", "E025", "E026", "E027", "E028", "E030", "E058", "E059",
    "E061", "E062", "E063", "E064", "E065", "E090", "E093", "E095", "E100", "E101", "E103", "E104",
    "E111", "E113", "E130", "E170", "E171", "E172", "E173", "E174", "E175", "E176", "E177", "E178",
    "E179", "E180", "E181", "E182", "E183", "E184", "E185", "E200", "E201", "E202", "E203", "E204",
    "E205", "E206", "E207", "E208", "E209", "E210", "E211", "E212", "E213", "E214", "E215", "E216",
    "E217", "E218", "E219", "E220", "E221", "E222", "E223", "E224", "E225", "E226", "E227", "E228",
    "E229", "E230", "E231", "E232", "E233", "E234", "E235", "E236", "E237", "E238", "E239", "E240",
    "E241", "E242", "E243", "E244", "E245", "E246", "E247", "E248", "E249", "E250", "E251", "E252",
    "E253", "E254", "E255", "E256", "E257", "E258", "E259", "E260", "E261", "E262", "E263", "E264",
    "E265", "E266", "E267", "E268", "E269", "E270", "E271", "E272", "E273", "E274", "E275", "E276",
    "E277", "E278", "E279", "E280", "E281", "E282", "E283", "E284", "E285", "E286", "E287", "E288",
    "E289", "E290", "E291", "E292", "E293", "E294", "E295", "E296", "E297", "E298", "E299", "E300",
    "E301", "E302", "E303", "E304", "E305", "E306", "E307", "E308", "E309", "E310", "E311", "E312",
    "E313", "E314", "E315", "E316", "E317", "E318", "E319", "E320", "E321", "E322", "E323", "E324",
    "E325", "E326", "E327", "E328", "E329", "E330", "E331", "E332", "E333", "E334", "E335", "E336",
    "E337", "E338", "E339", "E340", "E341", "E342", "E343", "E344", "E345", "E346", "E347", "E348",
    "E349", "E350", "E351", "E352", "E353", "E354", "E355", "E356", "E357", "E358", "E359", "E360",
    "E361", "E362", "E363", "E364", "E365", "E366", "E367", "E368", "E369", "E370", "E371", "E372",
    "E373", "E374", "E375", "E376", "E377", "E378", "E379", "E380", "E381", "E382", "E383", "E384",
    "E385", "E386", "E387", "E388", "E389", "E390", "E391", "E392", "E393", "E394", "E395", "E396",
    "E397", "E398", "E399", "E400", "E401", "E402", "E403", "E404", "E405", "E406", "E407", "E408",
    "E409", "E410", "E411", "E412", "E413", "E414", "E415", "E416", "E417", "E418", "E419", "E420",
    "E421", "E422", "E423", "E424", "E425", "E426", "E427", "E428", "E429", "E430", "E431", "E432",
    "E433", "E434", "E435", "E436", "E437", "E438", "E439", "E440", "E441", "E442", "E443", "E444",
    "E445", "E446", "E447", "E448", "E449", "E450", "E451", "E452", "E453", "E454", "E455", "E456", "E457", "E458", "E999", "I050", "W010", "W011", "W012", "W013", "W020", "W022", "W031",
    "W042", "W051", "W052", "W054", "W055", "W056", "W057", "W060", "W061", "W062", "W070", "W091",
    "W092", "W094", "W102", "W110", "W112", "W120", "W121", "W122", "W131", "W140", "W150", "W160",
    "W170", "W200", "W201", "W202", "W203", "W204", "W205", "W206",
];

const SHARED: &[&str] = &[
    "E001", "E020", "E021", "E023", "E024", "E025", "E026", "E027", "E028", "E030", "E058", "E059",
    "E061", "E062", "E063", "E064", "E065", "E090", "E093", "E095", "E100", "E101", "E103", "E104",
    "E111", "E113", "E130", "E170", "E171", "E172", "E173", "E174", "E175", "E176", "E177", "E178",
    "E179", "E180", "E181", "E182", "E183", "E184", "E185", "E200", "E201", "E202", "E203", "E204",
    "E205", "E206", "E207", "E208", "E209", "E210", "E211", "E212", "E213", "E214", "E215", "E216",
    "E217", "E218", "E219", "E220", "E221", "E222", "E223", "E224", "E225", "E226", "E227", "E228",
    "E229", "E230", "E231", "E232", "E233", "E234", "E235", "E236", "E237", "E238", "E239", "E240",
    "E241", "E242", "E243", "E244", "E245", "E246", "E247", "E248", "E249", "E250", "E251", "E252",
    "E253", "E254", "E255", "E256", "E257", "E258", "E259", "E260", "E261", "E262", "E263", "E264",
    "E265", "E266", "E267", "E268", "E269", "E270", "E271", "E272", "E273", "E274", "E275", "E276",
    "E277", "E278", "E279", "E280", "E281", "E282", "E283", "E284", "E285", "E286", "E287", "E288",
    "E289", "E290", "E291", "E292", "E293", "E294", "E295", "E296", "E297", "E298", "E299", "E300",
    "E301", "E302", "E303", "E304", "E305", "E306", "E307", "E308", "E309", "E310", "E311", "E312",
    "E313", "E314", "E315", "E316", "E317", "E318", "E319", "E320", "E321", "E322", "E323", "E324",
    "E325", "E326", "E327", "E328", "E329", "E330", "E331", "E332", "E333", "E334", "E335", "E336",
    "E337", "E338", "E339", "E340", "E341", "E342", "E343", "E344", "E345", "E346", "E347", "E348",
    "E349", "E350", "E351", "E352", "E353", "E354", "E355", "E356", "E357", "E358", "E359", "E360",
    "E361", "E362", "E363", "E364", "E365", "E366", "E367", "E368", "E369", "E370", "E371", "E372",
    "E373", "E374", "E375", "E376", "E377", "E378", "E379", "E380", "E381", "E382", "E383", "E384",
    "E385", "E386", "E387", "E388", "E389", "E390", "E391", "E392", "E393", "E394", "E395", "E396",
    "E397", "E398", "E399", "E400", "E401", "E402", "E403", "E404", "E405", "E406", "E407", "E408",
    "E409", "E410", "E411", "E412", "E413", "E414", "E415", "E416", "E417", "E418", "E419", "E420",
    "E421", "E422", "E423", "E424", "E425", "E426", "E427", "E428", "E429", "E430", "E431", "E432",
    "E433", "E434", "E435", "E436", "E437", "E438", "E439", "E440", "E441", "E442", "E443", "E444",
    "E445", "E446", "E447", "E448", "E449", "E450", "E451", "E452", "E453", "E454", "E455", "E456", "E457", "E458", "W022", "W031", "W042", "W121", "W131", "W150", "W170", "W200", "W201",
    "W202", "W203", "W204", "W205", "W206",
];

const ADDED: &[&str] = &[
    "E999", "I050", "W010", "W011", "W012", "W013", "W020", "W051", "W052", "W054", "W055", "W056",
    "W057", "W060", "W061", "W062", "W070", "W091", "W092", "W094", "W102", "W110", "W112", "W120",
    "W122", "W140", "W160",
];

const SKIP_KEYS: &[&str] = &[
    "E186", "E187", "E188", "E189", "E190", "E191", "E192", "E193", "E194", "S020", "S035", "S040",
    "S054", "S063", "W186", "W187",
];

const OUT: &[&str] = &[
    "E040", "W040", "W041", "I041", "W071", "I070", "I071", "W080", "W081", "DYNR",
];

const VACATED: &[&str] = &[
    "E010", "E050", "E051", "E052", "E053", "E060", "W021", "W050", "W053", "W090", "W093", "W095",
    "W100", "W101", "W103", "W111", "W130",
];

const WARRANT_CODES: &[&str] = &[
    "E001", "E020", "E023", "E058", "E062", "E090", "E093", "E101", "E304", "E337", "W170",
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
fn known_codes_matches_the_rust_keys() {
    assert_eq!(SHARED.len(), 316);
    assert_eq!(ADDED.len(), 27);
    assert_eq!(SKIP_KEYS.len(), 16);
    assert_eq!(THIN_CODES.len(), 343);
    assert_eq!(RUST_CODES.len(), 359);
    assert_eq!(known_codes(), RUST_CODES);
    assert_eq!(known_codes().len(), 359);
    assert!(!RUST_CODES.contains(&"P000"));
    assert!(RUST_CODES.contains(&"E178"));
    assert!(RUST_CODES.contains(&"E179"));
    assert!(RUST_CODES.contains(&"E200"));
    assert!(RUST_CODES.contains(&"W200"));
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
fn explain_kinds_are_shared_skipped_added() {
    for code in SHARED {
        let entry = explain(code).unwrap_or_else(|| panic!("{code}"));
        assert_eq!(entry.kind, ExplainKind::Shared, "{code}");
        assert_eq!(entry.kind.as_str(), "shared", "{code}");
    }
    for code in ADDED {
        let entry = explain(code).unwrap_or_else(|| panic!("{code}"));
        assert_eq!(entry.kind, ExplainKind::Added, "{code}");
        assert_eq!(entry.kind.as_str(), "added", "{code}");
    }
    for code in SKIP_KEYS {
        let entry = explain(code).unwrap_or_else(|| panic!("{code}"));
        assert_eq!(entry.kind, ExplainKind::Skipped, "{code}");
        assert_eq!(entry.kind.as_str(), "skipped", "{code}");
    }
    assert!(explain("E060").is_none());
    assert_eq!(explain("W062").unwrap().kind, ExplainKind::Added);
    assert_eq!(explain("E186").unwrap().kind, ExplainKind::Skipped);
    assert_eq!(explain("W013").unwrap().kind, ExplainKind::Added);
    assert_eq!(explain("W186").unwrap().kind, ExplainKind::Skipped);
    for code in ["e192", "e193", "e194"] {
        assert_eq!(explain(code).unwrap().kind, ExplainKind::Skipped);
    }
}

#[test]
fn out_and_vacated_are_unknown() {
    for code in OUT.iter().chain(VACATED) {
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
    assert_eq!(explain("e192").unwrap(), explain("E192").unwrap());
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
    assert_eq!(lines[0], "Diagnostic codes and their relation to Dynare:");

    let mut entries = Vec::new();
    let mut i = 1;
    while i < lines.len() && !lines[i].is_empty() {
        let line = lines[i];
        assert!(line.starts_with("  "), "{line:?}");
        let rest = &line[2..];
        assert!(
            rest.len() >= 17,
            "list line too short for column layout: {line:?}"
        );
        let code = rest[..6].trim_end();
        assert_eq!(&rest[6..8], "  ", "column gap mismatch: {line:?}");
        let kind = rest[8..15].trim_end();
        assert_eq!(&rest[15..17], "  ", "kind column gap mismatch: {line:?}");
        let title = &rest[17..];
        assert!(
            matches!(kind, "shared" | "skipped" | "added"),
            "kind token must be shared/skipped/added: {line:?}"
        );
        entries.push((code, kind, title));
        i += 1;
    }
    assert_eq!(entries.len(), 359);
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
    assert_eq!(e001.1, "shared");
    let e186 = entries.iter().find(|(c, _, _)| *c == "E186").unwrap();
    assert_eq!(e186.1, "skipped");
    assert_eq!(e186.2, "Unused endogenous after substitution");
    let i050 = entries.iter().find(|(c, _, _)| *c == "I050").unwrap();
    assert_eq!(i050.1, "added");
    assert_eq!(i050.2, "No initval or steady_state_model block");
    let e192 = entries.iter().find(|(c, _, _)| *c == "E192").unwrap();
    assert_eq!(e192.1, "skipped");
    let w186 = entries.iter().find(|(c, _, _)| *c == "W186").unwrap();
    assert_eq!(w186.1, "skipped");
    assert_eq!(w186.2, "Possible auxiliary name in a symbol list");
    assert_eq!(
        lines.get(i).copied(),
        Some(""),
        "expected blank line before footer"
    );
    assert_eq!(
        lines.get(i + 1).copied(),
        Some("359 codes. Run `dygnosis explain <CODE>` for details.")
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
                md.contains("Dynare refuses:")
                    || md.contains("Dynare reports")
                    || md.contains("Dynare accepts and warns:")
                    || md.contains("`Unknown symbol")
                    || md.contains("bison")
                    || md.contains("perpendicular symbol"),
                "{code} warrant body must still quote Dynare's string",
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
