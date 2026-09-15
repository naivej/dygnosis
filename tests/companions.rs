use std::fs;
use std::path::{Path, PathBuf};

use dygnosis::span::Span;
use dygnosis::{analyze, check_file, parse, CompanionKind, CompanionRecord, Workspace};

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

fn first_line_span(src: &str) -> Span {
    Span::new(0, src.find('\n').unwrap_or(src.len()))
}

fn span_of(src: &str, needle: &str) -> Span {
    let start = src
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle:?} in fixture"));
    Span::new(start, start + needle.len())
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

fn codes_of(diags: &[dygnosis::Diagnostic]) -> Vec<&str> {
    diags.iter().map(|d| d.code.as_str()).collect()
}

fn assert_no_w160(path: &Path, src: &str) {
    let model = parse(src);
    let analyzed = analyze(&model);
    assert!(
        !analyzed.iter().any(|d| d.code == "W160"),
        "analyze must not emit W160 on {}: {:?}",
        path.display(),
        codes_of(&analyzed)
    );
    let uri = path.to_str().unwrap();
    let checked = check_file(src, uri);
    assert!(
        !checked.iter().any(|d| d.code == "W160"),
        "check_file must not emit W160 on {}: {:?}",
        path.display(),
        codes_of(&checked)
    );
}

fn find_named<'a>(recs: &'a [CompanionRecord], name: &str) -> &'a CompanionRecord {
    recs.iter()
        .find(|r| r.name == name)
        .unwrap_or_else(|| panic!("missing record named {name:?}; got {recs:?}"))
}

fn path_ends_with(path: &Path, name: &str) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.eq_ignore_ascii_case(name))
        || path.ends_with(name)
}

fn virtual_uri(name: &str) -> String {
    if cfg!(windows) {
        format!(r"C:\dygnosis-virtual-companion\{name}")
    } else {
        format!("/tmp/dygnosis-virtual-companion/{name}")
    }
}

fn tmp_unique() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "dygnosis-companion-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn tiny_mod(extra: &str) -> String {
    format!(
        "var y;\nvarexo e;\nparameters rho;\nrho = 0.5;\nmodel;\ny = rho * y(-1) + e;\nend;\n{extra}"
    )
}

#[test]
fn named_missing_unresolved_records() {
    let path = fixtures_root().join("named_missing.mod");
    let (src, recs) = companion_records(&path);

    let expect = [
        (
            "missing_data.csv",
            CompanionKind::Datafile,
            "'missing_data.csv'",
        ),
        ("missing_mode", CompanionKind::ModeFile, "missing_mode"),
        (
            "missing_data_file.csv",
            CompanionKind::Datafile,
            "'missing_data_file.csv'",
        ),
        (
            "missing_gsa.mat",
            CompanionKind::Datafile,
            "'missing_gsa.mat'",
        ),
        (
            "missing_initval.csv",
            CompanionKind::Datafile,
            "'missing_initval.csv'",
        ),
        (
            "missing_histval.csv",
            CompanionKind::Datafile,
            "'missing_histval.csv'",
        ),
        ("missing_ext", CompanionKind::HelperM, "missing_ext"),
        ("missing_ext_d1", CompanionKind::HelperM, "missing_ext_d1"),
        ("missing_prior", CompanionKind::HelperM, "missing_prior"),
        (
            "missing_helper.m",
            CompanionKind::HelperM,
            "'missing_helper.m'",
        ),
    ];
    for (name, kind, needle) in expect {
        let rec = find_named(&recs, name);
        assert_eq!(rec.kind, kind, "{name}");
        assert!(rec.path.is_none(), "{name} should be unresolved");
        assert_eq!(rec.named_in, span_of(&src, needle), "{name} named_in");
    }

    assert!(
        recs.iter().all(|r| r.name != "0" && r.name != "1"),
        "gsa_sample_file=0 must not be a record: {recs:?}"
    );
    for banned in ["commented_data.csv", "commented_helper.m"] {
        assert!(
            recs.iter().all(|r| r.name != banned),
            "commented {banned} must not be a record: {recs:?}"
        );
    }

    let model = parse(&src);
    let analyzed = analyze(&model);
    assert!(
        !analyzed.iter().any(|d| d.code == "W160"),
        "analyze must not emit W160, got {:?}",
        codes_of(&analyzed)
    );
}

#[test]
fn data_file_resolves_sibling_csv() {
    let path = fixtures_root().join("data_file.mod");
    let (src, recs) = companion_records(&path);
    let rec = find_named(&recs, "data_file.csv");
    assert_eq!(rec.kind, CompanionKind::Datafile);
    let p = rec.path.as_ref().expect("resolved data_file.csv");
    assert!(path_ends_with(p, "data_file.csv"), "{}", p.display());
    assert_eq!(rec.named_in, span_of(&src, "'data_file.csv'"));
    assert_no_w160(&path, &src);
}

#[test]
fn leftover_csv_is_datafile() {
    let path = fixtures_root().join("leftover_csv.mod");
    let (src, recs) = companion_records(&path);
    let rec = find_named(&recs, "leftover.csv");
    assert_eq!(rec.kind, CompanionKind::Datafile);
    let p = rec.path.as_ref().expect("resolved leftover.csv");
    assert!(path_ends_with(p, "leftover.csv"), "{}", p.display());
    assert_eq!(rec.named_in, span_of(&src, "'leftover.csv'"));
    assert_no_w160(&path, &src);
}

#[test]
fn dup_path_option_kind_wins() {
    let path = fixtures_root().join("dup_path.mod");
    let (src, recs) = companion_records(&path);
    let hits: Vec<_> = recs.iter().filter(|r| r.name == "dup.csv").collect();
    assert_eq!(hits.len(), 1, "one record for dup.csv, got {recs:?}");
    assert_eq!(hits[0].kind, CompanionKind::Datafile);
    let p = hits[0].path.as_ref().expect("resolved dup.csv");
    assert!(path_ends_with(p, "dup.csv"), "{}", p.display());
    assert_eq!(hits[0].named_in, span_of(&src, "'dup.csv'"));
    assert_no_w160(&path, &src);
}

#[test]
fn absent_convention_has_no_optional_records() {
    let path = fixtures_root().join("absent_convention.mod");
    let (src, recs) = companion_records(&path);
    for kind in [
        CompanionKind::RunScript,
        CompanionKind::PriorRestrictions,
        CompanionKind::HelperM,
        CompanionKind::SteadyStateFile,
    ] {
        assert!(
            recs.iter().all(|r| r.kind != kind),
            "absent {kind:?} must not be recorded: {recs:?}"
        );
    }
    assert!(
        recs.iter().all(|r| r.name != "missing_ident_helper"),
        "missing ident helper must not be a record: {recs:?}"
    );
    assert_no_w160(&path, &src);
}

#[test]
fn ident_helper_present_is_helper_m() {
    let path = fixtures_root().join("ident_helper/ident_helper.mod");
    let (src, recs) = companion_records(&path);
    let rec = find_named(&recs, "my_ss_helper");
    assert_eq!(rec.kind, CompanionKind::HelperM);
    let p = rec.path.as_ref().expect("resolved my_ss_helper");
    assert!(path_ends_with(p, "my_ss_helper.m"), "{}", p.display());
    assert_eq!(rec.named_in, span_of(&src, "my_ss_helper"));
    assert_no_w160(&path, &src);
}

#[test]
fn ss_present_records_convention_and_quiets_i050() {
    let path = fixtures_root().join("ss_present/ss_present.mod");
    let (src, recs) = companion_records(&path);
    let hits: Vec<_> = recs
        .iter()
        .filter(|r| r.kind == CompanionKind::SteadyStateFile)
        .collect();
    assert_eq!(hits.len(), 1, "one steady_state_file, got {recs:?}");
    let rec = hits[0];
    let p = rec.path.as_ref().expect("resolved ss file");
    assert!(
        path_ends_with(p, "ss_present_steadystate.m"),
        "{}",
        p.display()
    );
    assert_eq!(
        &src[rec.named_in.start as usize..rec.named_in.end as usize],
        "steady"
    );

    let analyzed = analyze(&parse(&src));
    assert!(
        analyzed.iter().any(|d| d.code == "I050"),
        "analyze() still I050 on ss_present, got {:?}",
        codes_of(&analyzed)
    );
    let diags = check_file(&src, path.to_str().unwrap());
    assert!(
        !diags.iter().any(|d| d.code == "I050"),
        "check_file quiets I050 when _steadystate.m resolves, got {:?}",
        codes_of(&diags)
    );
    assert!(
        !diags.iter().any(|d| d.code == "W160"),
        "no W160, got {:?}",
        codes_of(&diags)
    );
}

#[test]
fn plus_pkg_is_not_steady_state_file() {
    let path = fixtures_root().join("plus_pkg/plus_pkg.mod");
    let (src, recs) = companion_records(&path);
    assert!(
        recs.iter()
            .all(|r| r.kind != CompanionKind::SteadyStateFile),
        "generated +FILENAME/steadystate.m must not be convention: {recs:?}"
    );
    assert_no_w160(&path, &src);
}

#[test]
fn swff_run_script_not_ff_coeffs_not_inc() {
    let path = copilot_mod("swff");
    let (mut ws, uri, src) = load_disk(&path);
    let recs = ws
        .companion_records(&uri)
        .expect("swff companions")
        .to_vec();
    let includes = ws.include_records(&uri).expect("swff includes").clone();

    let run_hits: Vec<_> = recs
        .iter()
        .filter(|r| r.kind == CompanionKind::RunScript)
        .collect();
    assert_eq!(run_hits.len(), 1, "run_swff.m present, got {recs:?}");
    let p = run_hits[0].path.as_ref().expect("run_script path");
    assert!(path_ends_with(p, "run_swff.m"), "{}", p.display());
    assert_eq!(run_hits[0].named_in, first_line_span(&src));

    assert!(
        recs.iter().all(|r| {
            !r.name.contains("swff_ff_coeffs")
                && r.path
                    .as_ref()
                    .and_then(|p| p.to_str())
                    .is_none_or(|s| !s.contains("swff_ff_coeffs"))
        }),
        "swff_ff_coeffs.m is not a companion of swff.mod: {recs:?}"
    );
    assert!(
        recs.iter().all(|r| r
            .path
            .as_ref()
            .and_then(|p| p.extension())
            .and_then(|e| e.to_str())
            .is_none_or(|e| e != "inc")),
        "includes are not companions: {recs:?}"
    );
    assert!(
        includes
            .resolved
            .iter()
            .any(|r| path_ends_with(&r.path, "swff_params.inc")),
        "include records still have the .inc: {:?}",
        includes.resolved
    );

    let analyzed = analyze(&parse(&src));
    assert!(
        !analyzed
            .iter()
            .any(|d| d.code == "E060" || d.code == "E061"),
        "analyze(parse(swff)) must stay records-only, got {:?}",
        codes_of(&analyzed)
    );
    assert_no_w160(&path, &src);
}

#[test]
fn official_nk_baseline_steadystate_if_present() {
    let Some(path) = official_mod("stochastic_simulations/nk_baseline.mod") else {
        return;
    };
    let (src, recs) = companion_records(&path);
    let rec = recs
        .iter()
        .find(|r| r.kind == CompanionKind::SteadyStateFile)
        .expect("nk_baseline_steadystate.m");
    let p = rec.path.as_ref().expect("resolved");
    assert!(
        path_ends_with(p, "nk_baseline_steadystate.m"),
        "{}",
        p.display()
    );
    assert_eq!(
        &src[rec.named_in.start as usize..rec.named_in.end as usize],
        "steady"
    );
}

#[test]
fn official_collard_ident_helper_if_present() {
    let Some(path) =
        official_mod("stochastic_simulations/collard_2001_analytical_steady_state.mod")
    else {
        return;
    };
    let (_src, recs) = companion_records(&path);
    let rec = find_named(&recs, "collard_2001_analytical_steady_state_helper");
    assert_eq!(rec.kind, CompanionKind::HelperM);
    let p = rec.path.as_ref().expect("resolved helper");
    assert!(
        path_ends_with(p, "collard_2001_analytical_steady_state_helper.m"),
        "{}",
        p.display()
    );
}

#[test]
fn official_rbc_irf_matching_if_present() {
    let Some(path) = official_mod("estimation/rbc_irf_matching.mod") else {
        return;
    };
    let (_src, recs) = companion_records(&path);
    let irf = recs
        .iter()
        .find(|r| r.kind == CompanionKind::IrfMatchingFile)
        .expect("irf_matching_file");
    let p = irf.path.as_ref().expect("resolved transformations");
    assert!(
        path_ends_with(p, "rbc_irf_matching_transformations.m"),
        "{}",
        p.display()
    );

    let csv: Vec<_> = recs
        .iter()
        .filter(|r| {
            r.name == "rbc_irf_matching_data.csv" || path_is_csv(r, "rbc_irf_matching_data.csv")
        })
        .collect();
    assert_eq!(
        csv.len(),
        1,
        "one csv record even if quoted twice: {recs:?}"
    );
    assert_eq!(csv[0].kind, CompanionKind::Datafile);
    let csv_path = csv[0].path.as_ref().expect("resolved csv");
    assert!(
        path_ends_with(csv_path, "rbc_irf_matching_data.csv"),
        "{}",
        csv_path.display()
    );
}

fn path_is_csv(rec: &CompanionRecord, name: &str) -> bool {
    rec.path.as_ref().is_some_and(|p| path_ends_with(p, name))
}

#[test]
fn official_gali_2015_if_present() {
    let Some(path) = official_mod("estimation/gali_2015.mod") else {
        return;
    };
    let (src, recs) = companion_records(&path);
    let prior = recs
        .iter()
        .find(|r| r.kind == CompanionKind::PriorRestrictions)
        .expect("gali_2015_prior_restrictions.m");
    let p = prior.path.as_ref().expect("resolved prior restrictions");
    assert!(
        path_ends_with(p, "gali_2015_prior_restrictions.m"),
        "{}",
        p.display()
    );
    assert_eq!(
        &src[prior.named_in.start as usize..prior.named_in.end as usize],
        "estimation"
    );

    let helper = recs
        .iter()
        .find(|r| {
            r.kind == CompanionKind::HelperM
                && (r.name == "gali_2015_phillips_curve_slope"
                    || r.path
                        .as_ref()
                        .is_some_and(|p| path_ends_with(p, "gali_2015_phillips_curve_slope.m")))
        })
        .expect("gali_2015_phillips_curve_slope helper_m");
    let hp = helper.path.as_ref().expect("resolved helper");
    assert!(
        path_ends_with(hp, "gali_2015_phillips_curve_slope.m"),
        "{}",
        hp.display()
    );

    let sim = find_named(&recs, "sim_data");
    assert_eq!(sim.kind, CompanionKind::Datafile);
    let sim_dir = path.parent().unwrap();
    let suffix_exists = [".m", ".mat", ".csv", ".xls", ".xlsx"]
        .iter()
        .any(|suf| sim_dir.join(format!("sim_data{suf}")).is_file());
    if suffix_exists {
        assert!(sim.path.is_some(), "sim_data suffix exists: {sim:?}");
    } else {
        assert!(sim.path.is_none(), "sim_data should be unresolved: {sim:?}");
    }
}

#[test]
fn overlay_convention_ss_beats_missing_disk() {
    let mod_uri = virtual_uri("overlay_ss.mod");
    let ss_uri = virtual_uri("overlay_ss_steadystate.m");
    let src = tiny_mod("");
    let mut ws = Workspace::new();
    ws.update_document(&mod_uri, src.clone());
    ws.update_document(&ss_uri, "% overlay ss\n");
    let recs = ws
        .companion_records(&mod_uri)
        .expect("overlay companions")
        .to_vec();
    let rec = recs
        .iter()
        .find(|r| r.kind == CompanionKind::SteadyStateFile)
        .expect("overlay ss");
    let p = rec.path.as_ref().expect("resolved overlay ss");
    assert!(
        path_ends_with(p, "overlay_ss_steadystate.m"),
        "{}",
        p.display()
    );
    assert_eq!(rec.named_in, first_line_span(&src));
}

#[test]
fn search_paths_resolve_named_datafile() {
    let dir = tmp_unique();
    let main_dir = dir.join("main");
    let search = dir.join("search");
    fs::create_dir_all(&main_dir).unwrap();
    fs::create_dir_all(&search).unwrap();
    let main = main_dir.join("main.mod");
    let data = search.join("search_hit.csv");
    let src = tiny_mod("estimation(datafile='search_hit.csv');\n");
    fs::write(&main, &src).unwrap();
    fs::write(&data, "y\n1\n").unwrap();

    let mut ws = Workspace::new();
    ws.load_from_disk(&main).expect("load main");
    let uri = main.to_str().unwrap().to_string();
    let before = ws.companion_records(&uri).expect("before").to_vec();
    let rec = find_named(&before, "search_hit.csv");
    assert!(rec.path.is_none(), "sibling miss: {rec:?}");

    ws.add_search_path(search);
    let after = ws.companion_records(&uri).expect("after").to_vec();
    let rec = find_named(&after, "search_hit.csv");
    let p = rec.path.as_ref().expect("search_paths hit");
    assert!(path_ends_with(p, "search_hit.csv"), "{}", p.display());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn includepath_resolves_named_datafile() {
    let dir = tmp_unique();
    let main_dir = dir.join("main");
    let search = dir.join("search");
    fs::create_dir_all(&main_dir).unwrap();
    fs::create_dir_all(&search).unwrap();
    let main = main_dir.join("main.mod");
    let data = search.join("include_hit.csv");
    fs::write(&data, "y\n1\n").unwrap();
    let posix = search.to_string_lossy().replace('\\', "/");
    let with = tiny_mod(&format!(
        "@#includepath \"{posix}\"\nestimation(datafile='include_hit.csv');\n"
    ));
    fs::write(&main, &with).unwrap();
    let mut ws = Workspace::new();
    ws.load_from_disk(&main).expect("load with includepath");
    let uri = main.to_str().unwrap().to_string();
    let recs = ws.companion_records(&uri).expect("with").to_vec();
    let rec = find_named(&recs, "include_hit.csv");
    let p = rec.path.as_ref().expect("includepath hit");
    assert!(path_ends_with(p, "include_hit.csv"), "{}", p.display());

    let without_src = tiny_mod("estimation(datafile='include_hit.csv');\n");
    let without_path = main_dir.join("without.mod");
    fs::write(&without_path, &without_src).unwrap();
    let mut ws2 = Workspace::new();
    ws2.load_from_disk(&without_path).expect("load without");
    let uri2 = without_path.to_str().unwrap().to_string();
    let recs2 = ws2.companion_records(&uri2).expect("without").to_vec();
    let rec2 = find_named(&recs2, "include_hit.csv");
    assert!(rec2.path.is_none(), "without includepath: {rec2:?}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn directory_named_foo_m_is_not_a_helper() {
    let dir = tmp_unique();
    let named = dir.join("foo.m");
    fs::create_dir(&named).unwrap();
    let main = dir.join("main.mod");
    let src = tiny_mod("initval;\ny = foo(1);\nend;\n");
    fs::write(&main, &src).unwrap();
    let mut ws = Workspace::new();
    ws.load_from_disk(&main).expect("load");
    let uri = main.to_str().unwrap().to_string();
    let recs = ws.companion_records(&uri).expect("recs").to_vec();
    assert!(
        recs.iter()
            .all(|r| r.name != "foo" && r.kind != CompanionKind::HelperM),
        "directory foo.m is not a helper match: {recs:?}"
    );
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn check_file_i050_none_still_fires() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/shape/i050_none.mod");
    let src = read_mod(&path);
    let diags = check_file(&src, path.to_str().unwrap());
    assert!(
        diags.iter().any(|d| d.code == "I050"),
        "i050_none.mod must still emit I050, got {:?}",
        codes_of(&diags)
    );
    assert!(
        !diags.iter().any(|d| d.code == "W160"),
        "no W160, got {:?}",
        codes_of(&diags)
    );
}
