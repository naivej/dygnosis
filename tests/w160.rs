use std::fs;
use std::path::{Path, PathBuf};

use dygnosis::span::Span;
use dygnosis::{
    analyze, check_file, check_w160, parse, quiet_i050, CompanionKind, CompanionRecord, Diagnostic,
    Severity, Workspace,
};

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/companions")
}

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn official_mod(rel: &str) -> Option<PathBuf> {
    let p = PathBuf::from(r"C:\dynare\7.1\examples").join(rel);
    p.is_file().then_some(p)
}

fn read_mod(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn load_disk(path: &Path) -> (Workspace, String, String) {
    let src = read_mod(path);
    let uri = path
        .to_str()
        .unwrap_or_else(|| panic!("non-utf8 path {}", path.display()))
        .to_string();
    let mut ws = Workspace::new();
    // Overlay LF text so named_in spans match read_mod (autocrlf checkout is CRLF).
    ws.update_document(&uri, &src);
    (ws, uri, src)
}

fn companion_records(path: &Path) -> (String, Vec<CompanionRecord>) {
    let (mut ws, uri, src) = load_disk(path);
    let recs = ws
        .companion_records(&uri)
        .unwrap_or_else(|| panic!("companion_records {}", path.display()))
        .to_vec();
    (src, recs)
}

fn codes_of(diags: &[Diagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.code.as_str()).collect()
}

fn w160_message(name: &str, kind: CompanionKind) -> String {
    format!(
        "Named companion '{name}' ({}) was not found. Fix: add the file next to this .mod, correct the path, or add its directory to the search paths.",
        kind.as_str()
    )
}

fn assert_no_w160_analyze(path: &Path, src: &str) {
    let analyzed = analyze(&parse(src));
    assert!(
        !analyzed.iter().any(|d| d.code == "W160"),
        "analyze must not emit W160 on {}: {:?}",
        path.display(),
        codes_of(&analyzed)
    );
}

fn assert_no_w160_check_file(path: &Path, src: &str) {
    let uri = path.to_str().unwrap();
    let checked = check_file(src, uri);
    assert!(
        !checked.iter().any(|d| d.code == "W160"),
        "check_file must not emit W160 on {}: {:?}",
        path.display(),
        codes_of(&checked)
    );
}

/// 1:1 lock: workspace W160 set equals `path: None` records.
fn assert_w160_matches_unresolved(path: &Path) {
    let (src, recs) = companion_records(path);
    let unresolved: Vec<&CompanionRecord> = recs.iter().filter(|r| r.path.is_none()).collect();
    assert!(
        !unresolved.is_empty(),
        "{} should have named unresolved records, got {recs:?}",
        path.display()
    );

    let from_fn = check_w160(&recs);
    assert_eq!(
        from_fn.len(),
        unresolved.len(),
        "check_w160 count on {}",
        path.display()
    );
    for rec in &unresolved {
        let hits: Vec<_> = from_fn.iter().filter(|d| d.span == rec.named_in).collect();
        assert_eq!(hits.len(), 1, "one W160 for named_in of {}", rec.name);
        assert_eq!(hits[0].severity, Severity::Warning);
        assert_eq!(hits[0].code, "W160");
        assert_eq!(hits[0].message, w160_message(&rec.name, rec.kind));
        assert!(hits[0].fix.is_none());
        assert!(hits[0].tags.is_empty());
    }

    let uri = path.to_str().unwrap();
    let checked = check_file(&src, uri);
    let w160: Vec<_> = checked.iter().filter(|d| d.code == "W160").collect();
    assert_eq!(
        w160.len(),
        unresolved.len(),
        "check_file W160 count on {}: {:?}",
        path.display(),
        codes_of(&checked)
    );
    for rec in &unresolved {
        let hits: Vec<_> = w160.iter().filter(|d| d.span == rec.named_in).collect();
        assert_eq!(
            hits.len(),
            1,
            "check_file one W160 for named_in of {}",
            rec.name
        );
        assert_eq!(hits[0].severity, Severity::Warning);
        assert_eq!(hits[0].message, w160_message(&rec.name, rec.kind));
        assert!(hits[0].fix.is_none());
        assert!(hits[0].tags.is_empty());
    }

    assert_no_w160_analyze(path, &src);
}

#[test]
fn named_missing_w160_matches_unresolved_records() {
    let path = fixtures_root().join("named_missing.mod");
    assert_w160_matches_unresolved(&path);

    let (src, recs) = companion_records(&path);
    for banned in ["0", "1", "commented_data.csv", "commented_helper.m"] {
        assert!(
            recs.iter().all(|r| r.name != banned),
            "{banned} must not be a record: {recs:?}"
        );
        let uri = path.to_str().unwrap();
        let checked = check_file(&src, uri);
        assert!(
            checked
                .iter()
                .all(|d| { d.code != "W160" || !d.message.contains(&format!("'{banned}'")) }),
            "{banned} must not be W160: {:?}",
            codes_of(&checked)
        );
    }
}

#[test]
fn resolved_and_absent_convention_have_no_w160() {
    let cases = [
        fixtures_root().join("data_file.mod"),
        fixtures_root().join("leftover_csv.mod"),
        fixtures_root().join("dup_path.mod"),
        fixtures_root().join("absent_convention.mod"),
        fixtures_root().join("ident_helper/ident_helper.mod"),
    ];
    for path in cases {
        let src = read_mod(&path);
        let recs = companion_records(&path).1;
        assert!(
            recs.iter().all(|r| r.path.is_some()),
            "{} must not have unresolved named records: {recs:?}",
            path.display()
        );
        assert!(
            check_w160(&recs).is_empty(),
            "check_w160 empty on {}",
            path.display()
        );
        assert_no_w160_analyze(&path, &src);
        assert_no_w160_check_file(&path, &src);
    }
}

#[test]
fn ss_present_check_file_quiets_i050_analyze_still_fires() {
    let path = fixtures_root().join("ss_present/ss_present.mod");
    let src = read_mod(&path);
    let analyzed = analyze(&parse(&src));
    assert!(
        analyzed.iter().any(|d| d.code == "I050"),
        "analyze() still I050 on ss_present, got {:?}",
        codes_of(&analyzed)
    );
    let uri = path.to_str().unwrap();
    let checked = check_file(&src, uri);
    assert!(
        !checked.iter().any(|d| d.code == "I050"),
        "check_file must quiet I050 on ss_present, got {:?}",
        codes_of(&checked)
    );
    assert!(
        !checked.iter().any(|d| d.code == "W160"),
        "ss_present must not emit W160, got {:?}",
        codes_of(&checked)
    );
}

#[test]
fn plus_pkg_does_not_quiet_i050() {
    let path = fixtures_root().join("plus_pkg/plus_pkg.mod");
    let src = read_mod(&path);
    let analyzed = analyze(&parse(&src));
    assert!(
        analyzed.iter().any(|d| d.code == "I050"),
        "plus_pkg.mod must be I050-shaped on analyze(); add companions/plus_not_ss/ if this is vacuous, got {:?}",
        codes_of(&analyzed)
    );
    let uri = path.to_str().unwrap();
    let checked = check_file(&src, uri);
    assert!(
        checked.iter().any(|d| d.code == "I050"),
        "+FILENAME/steadystate.m must not quiet I050, got {:?}",
        codes_of(&checked)
    );
}

#[test]
fn i050_none_still_fires_on_analyze_and_check_file() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/shape/i050_none.mod");
    let src = read_mod(&path);
    let analyzed = analyze(&parse(&src));
    assert!(
        analyzed.iter().any(|d| d.code == "I050"),
        "analyze I050, got {:?}",
        codes_of(&analyzed)
    );
    let i050 = analyzed.iter().find(|d| d.code == "I050").unwrap();
    let message = i050.message.clone();
    let uri = path.to_str().unwrap();
    let checked = check_file(&src, uri);
    let check_i050 = checked
        .iter()
        .find(|d| d.code == "I050")
        .expect("check_file still I050");
    assert_eq!(check_i050.message, message);
    assert!(
        !checked.iter().any(|d| d.code == "W160"),
        "no W160 on i050_none, got {:?}",
        codes_of(&checked)
    );
}

#[test]
fn swff_has_no_w160() {
    let path = copilot_mod("swff");
    let src = read_mod(&path);
    assert_no_w160_analyze(&path, &src);
    assert_no_w160_check_file(&path, &src);
}

#[test]
fn official_nk_baseline_check_file_no_i050_if_present() {
    let Some(path) = official_mod("stochastic_simulations/nk_baseline.mod") else {
        return;
    };
    let src = read_mod(&path);
    let checked = check_file(&src, path.to_str().unwrap());
    assert!(
        !checked.iter().any(|d| d.code == "I050"),
        "nk_baseline check_file must not emit I050, got {:?}",
        codes_of(&checked)
    );
}

#[test]
fn official_gali_2015_w160_only_if_sim_data_unresolved() {
    let Some(path) = official_mod("estimation/gali_2015.mod") else {
        return;
    };
    let (src, recs) = companion_records(&path);
    let sim = recs
        .iter()
        .find(|r| r.name == "sim_data")
        .expect("sim_data record");
    let uri = path.to_str().unwrap();
    let checked = check_file(&src, uri);
    let w160: Vec<_> = checked.iter().filter(|d| d.code == "W160").collect();
    if sim.path.is_none() {
        assert!(
            w160.iter()
                .any(|d| d.span == sim.named_in && d.message == w160_message(&sim.name, sim.kind)),
            "W160 on unresolved sim_data, got {:?}",
            w160.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    } else {
        assert!(
            w160.iter().all(|d| d.span != sim.named_in),
            "resolved sim_data must not be W160: {w160:?}"
        );
    }
}

#[test]
fn quiet_i050_drops_i050_keeps_w010() {
    let i050 = Diagnostic::new(Span::new(0, 1), Severity::Information, "I050", "info");
    let w010 = Diagnostic::new(Span::new(2, 3), Severity::Warning, "W010", "warn");
    let rec = CompanionRecord {
        kind: CompanionKind::SteadyStateFile,
        named_in: Span::new(0, 1),
        name: "foo_steadystate.m".into(),
        path: Some(PathBuf::from("foo_steadystate.m")),
    };
    let mut diags = vec![i050.clone(), w010.clone()];
    quiet_i050(&mut diags, &[rec]);
    assert_eq!(diags, vec![w010]);
}

#[test]
fn quiet_i050_empty_records_do_not_quiet() {
    let i050 = Diagnostic::new(Span::new(0, 1), Severity::Information, "I050", "info");
    let mut diags = vec![i050.clone()];
    quiet_i050(&mut diags, &[]);
    assert_eq!(diags, vec![i050]);
}
