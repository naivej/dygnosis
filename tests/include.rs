use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use dygnosis::include_resolver::{normalize_uri, resolve_include_path};
use dygnosis::workspace::split_includepath_argument;
use dygnosis::{analyze, find_workspace_root, parse, Workspace};

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn swff_path() -> PathBuf {
    copilot_mod("swff")
}

fn swff_inc_path() -> PathBuf {
    swff_path().with_file_name("swff_params.inc")
}

fn read_mod(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn swff_text() -> String {
    read_mod(&swff_path())
}

fn inc_assignment_names() -> Vec<String> {
    let inc = read_mod(&swff_inc_path());
    let model = parse(&inc);
    model
        .helper_assignments
        .iter()
        .map(|a| model.name(a.name).to_string())
        .collect()
}

fn virtual_uri(name: &str) -> String {
    if cfg!(windows) {
        format!(r"C:\dygnosis-virtual-include\{name}")
    } else {
        format!("/tmp/dygnosis-virtual-include/{name}")
    }
}

fn tmp_unique() -> PathBuf {
    let p = std::env::temp_dir().join(format!(
        "dygnosis-include-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&p).unwrap();
    p
}

fn no_include_codes(model: &dygnosis::Model) {
    let codes: Vec<_> = analyze(model).into_iter().map(|d| d.code).collect();
    for banned in [
        "E060", "E061", "E062", "E063", "E064", "E065", "W060", "W061",
    ] {
        assert!(
            !codes.iter().any(|c| c == banned),
            "slice 06 must not emit {banned}; got {codes:?}"
        );
    }
}

#[test]
fn parse_swff_records_one_quoted_include() {
    let src = swff_text();
    let model = parse(&src);
    assert_eq!(model.includes.len(), 1);
    assert_eq!(model.includes[0].filename, "swff_params.inc");
    let needle = "@#include \"swff_params.inc\"";
    let start = src.find(needle).expect("include line");
    assert_eq!(model.includes[0].span.start as usize, start);
    assert_eq!(model.includes[0].span.end as usize, start + needle.len());
    no_include_codes(&model);
}

#[test]
fn commented_include_is_not_recorded() {
    let src = swff_text().replace("@#include", "// @#include");
    let model = parse(&src);
    assert!(model.includes.is_empty());
}

#[test]
fn bare_identifier_include_is_skipped() {
    let model = parse("@#include FOO\nvar y;\n");
    assert!(model.includes.is_empty());
}

#[test]
fn backslash_continued_include_is_recorded() {
    let model = parse("@#include \\\n\"helper.inc\"\nvar y;\n");
    assert_eq!(model.includes.len(), 1);
    assert_eq!(model.includes[0].filename, "helper.inc");
}

#[test]
fn sibling_resolve_swff_params() {
    let path = resolve_include_path("swff_params.inc", swff_path().to_str().unwrap(), &[], None)
        .expect("sibling include");
    assert!(path.ends_with("swff_params.inc"), "{}", path.display());
    assert!(path.is_file());
}

#[test]
fn missing_file_resolves_none() {
    assert!(
        resolve_include_path("no_such_file.inc", swff_path().to_str().unwrap(), &[], None,)
            .is_none()
    );
}

#[test]
fn search_paths_after_sibling_miss() {
    let dir = tmp_unique();
    let main = dir.join("main.mod");
    fs::write(&main, "var y;\n").unwrap();
    let found = resolve_include_path(
        "swff_params.inc",
        main.to_str().unwrap(),
        &[swff_inc_path().parent().unwrap().to_path_buf()],
        None,
    )
    .expect("search_paths");
    assert!(found.ends_with("swff_params.inc"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn absolute_path_resolves() {
    let inc = swff_inc_path();
    let path = resolve_include_path(
        &inc.to_string_lossy().replace('\\', "/"),
        swff_path().to_str().unwrap(),
        &[],
        None,
    )
    .expect("absolute");
    assert!(path.ends_with("swff_params.inc"));
}

#[test]
fn virtual_only_file_via_known_paths() {
    let known_path = virtual_uri("only.inc");
    let mut known = HashSet::new();
    known.insert(normalize_uri(&known_path));
    let including = virtual_uri("main.mod");
    let path =
        resolve_include_path("only.inc", &including, &[], Some(&known)).expect("virtual suffix");
    assert!(path.ends_with("only.inc"), "{}", path.display());
}

#[test]
fn directory_with_right_name_is_not_a_match() {
    let dir = tmp_unique();
    let named = dir.join("helper.inc");
    fs::create_dir(&named).unwrap();
    let main = dir.join("main.mod");
    fs::write(&main, "").unwrap();
    assert!(resolve_include_path("helper.inc", main.to_str().unwrap(), &[], None).is_none());
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn windows_separators_in_directive() {
    let dir = tmp_unique();
    let nested = dir.join("sub");
    fs::create_dir(&nested).unwrap();
    let inc = nested.join("p.inc");
    fs::write(&inc, "a = 1;\n").unwrap();
    let main = dir.join("main.mod");
    fs::write(&main, "").unwrap();
    let path = resolve_include_path(r"sub\p.inc", main.to_str().unwrap(), &[], None)
        .expect("backslash directive");
    assert!(path.ends_with("p.inc"));
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn find_workspace_root_walks_to_git() {
    let dir = tmp_unique();
    fs::create_dir(dir.join(".git")).unwrap();
    let nested = dir.join("a").join("b");
    fs::create_dir_all(&nested).unwrap();
    let file = nested.join("x.mod");
    fs::write(&file, "").unwrap();
    let root = find_workspace_root(&file);
    assert_eq!(root, dir);
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn split_includepath_does_not_split_drive() {
    let parts = split_includepath_argument(r#""C:/models:other""#);
    assert_eq!(parts, ["C:/models", "other"]);
}

#[test]
fn workspace_swff_zero_cycles_one_resolved() {
    let path = swff_path();
    let mut ws = Workspace::new();
    ws.load_from_disk(&path).expect("load swff");
    let uri = path.to_str().unwrap();
    let records = ws.include_records(uri).expect("records").clone();
    assert!(records.cycles.is_empty());
    assert!(records.unresolved.is_empty());
    assert_eq!(records.resolved.len(), 1);
    assert!(
        records.resolved[0].path.ends_with("swff_params.inc"),
        "{}",
        records.resolved[0].path.display()
    );
    let included = ws.resolve_all_includes(uri);
    assert_eq!(included.len(), 1);
    assert!(included
        .keys()
        .any(|k| k.ends_with("swff_params.inc") || Path::new(k).ends_with("swff_params.inc")));
}

#[test]
fn overlay_beats_disk() {
    let path = swff_path();
    let inc = swff_inc_path();
    let mut ws = Workspace::new();
    ws.load_from_disk(&path).unwrap();
    ws.update_document(inc.to_str().unwrap(), "overlay_only = 1;\n");
    let uri = path.to_str().unwrap();
    let effective = ws.get_effective_model(uri).unwrap();
    let names: Vec<_> = effective
        .helper_assignments
        .iter()
        .map(|a| effective.name(a.name).to_string())
        .collect();
    assert!(
        names.contains(&"overlay_only".to_string()),
        "overlay body should be spliced, got {names:?}"
    );
    assert!(
        !effective
            .param_assignments
            .iter()
            .any(|a| effective.name(a.name) == "alppha"),
        "disk inc must not win over overlay"
    );
}

#[test]
fn drop_include_records_unresolved() {
    let src = swff_text().replace("swff_params.inc", "no_such_file.inc");
    let mut ws = Workspace::new();
    let path = swff_path();
    let uri = path.to_str().unwrap();
    ws.update_document(uri, src);
    let unresolved = ws.find_unresolved_includes(uri);
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].filename, "no_such_file.inc");
    assert!(unresolved[0].span.end > unresolved[0].span.start);
    assert!(ws.find_circular_includes(uri).is_empty());
    no_include_codes(ws.get_model(uri).unwrap());
}

#[test]
fn cycle_records_include_swff_mod() {
    let path = swff_path();
    let inc_path = swff_inc_path();
    let uri = path.to_str().unwrap();
    let inc_uri = inc_path.to_str().unwrap();
    let mut ws = Workspace::new();
    ws.update_document(uri, swff_text());
    ws.update_document(inc_uri, "@#include \"swff.mod\"\n");
    let cycles = ws.find_circular_includes(uri);
    assert!(
        cycles.iter().any(|c| c
            .chain
            .iter()
            .any(|p| p.ends_with("swff.mod") || Path::new(p).ends_with("swff.mod"))),
        "expected swff.mod in cycle names, got {cycles:?}"
    );
    assert!(cycles.iter().all(|c| c.span.end > c.span.start));
    no_include_codes(ws.get_model(uri).unwrap());
    let effective = ws.get_effective_model(uri).unwrap();
    assert!(
        !effective.source.contains("@#include"),
        "cyclic edge must splice empty, leftover @#: {}",
        effective.source
    );
}

#[test]
fn nested_unresolved_still_reported() {
    let main = virtual_uri("nested_main.mod");
    let helper = virtual_uri("nested_helper.inc");
    let mut ws = Workspace::new();
    ws.update_document(&main, "@#include \"nested_helper.inc\"\nvar y;\n");
    ws.update_document(&helper, "@#include \"missing_nested.inc\"\n");
    let unresolved = ws.find_unresolved_includes(&main);
    assert!(
        unresolved
            .iter()
            .any(|u| u.filename.contains("missing_nested.inc")),
        "nested miss must be reported, got {unresolved:?}"
    );
}

#[test]
fn includepath_resolves_non_sibling() {
    let main = virtual_uri("search_main.mod");
    let swff_mod = swff_path();
    let swff_dir = swff_mod.parent().unwrap();
    let posix = swff_dir.to_string_lossy().replace('\\', "/");
    let mut text = swff_text();
    text = text.replace(
        "@#include \"swff_params.inc\"",
        &format!("@#includepath \"{posix}\"\n@#include \"swff_params.inc\""),
    );
    let mut ws = Workspace::new();
    ws.update_document(&main, text);
    let records = ws.include_records(&main).unwrap();
    assert!(records.unresolved.is_empty(), "{:?}", records.unresolved);
    assert!(records
        .resolved
        .iter()
        .any(|r| r.path.ends_with("swff_params.inc")));
    let effective = ws.get_effective_model(&main).unwrap();
    let got: Vec<_> = effective
        .param_assignments
        .iter()
        .map(|a| effective.name(a.name).to_string())
        .collect();
    assert_eq!(got, inc_assignment_names());
}

#[test]
fn without_includepath_non_sibling_is_unresolved() {
    let main = virtual_uri("miss_main.mod");
    let mut ws = Workspace::new();
    ws.update_document(&main, swff_text());
    let unresolved = ws.find_unresolved_includes(&main);
    assert_eq!(unresolved.len(), 1);
    assert_eq!(unresolved[0].filename, "swff_params.inc");
}

#[test]
fn search_paths_mutation_resolves() {
    let main = virtual_uri("cfg_main.mod");
    let mut ws = Workspace::new();
    ws.update_document(&main, swff_text());
    assert_eq!(ws.find_unresolved_includes(&main).len(), 1);
    let swff_mod = swff_path();
    ws.add_search_path(swff_mod.parent().unwrap().to_path_buf());
    let records = ws.include_records(&main).unwrap();
    assert!(records.unresolved.is_empty());
    assert!(records
        .resolved
        .iter()
        .any(|r| r.path.ends_with("swff_params.inc")));
    let effective = ws.get_effective_model(&main).unwrap();
    let got: Vec<_> = effective
        .param_assignments
        .iter()
        .map(|a| effective.name(a.name).to_string())
        .collect();
    assert_eq!(got, inc_assignment_names());
}

#[test]
fn effective_unresolved_splices_empty() {
    let src = swff_text().replace("swff_params.inc", "no_such_file.inc");
    let mut ws = Workspace::new();
    let uri = virtual_uri("empty_splice.mod");
    ws.update_document(&uri, src);
    let effective = ws.get_effective_model(&uri).unwrap();
    assert!(effective.includes.is_empty());
    assert!(effective.param_assignments.is_empty());
}
