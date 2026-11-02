//! Configuration guards survive cache eviction and closed-file refresh.
use super::*;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "dygnosis-configuration-refresh-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, text: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, text).unwrap();
        path_key(&path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let path = self.0.canonicalize().unwrap();
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        assert!(path.starts_with(&temporary) && path != temporary);
        std::fs::remove_dir_all(path).unwrap();
    }
}

fn select_target(ws: &mut Workspace, root: &str, file: &str) -> AnalysisSelection {
    let source = ws
        .source_structure(
            root,
            &SourceStructureQuery {
                files: vec![SourceFileSelection::File(file.to_owned())],
            },
        )
        .unwrap();
    let target = source.files[0].sites[0].target.clone();
    ws.set_root_analysis_configuration(
        root,
        AnalysisConfiguration {
            name: Some("Selected".into()),
            overrides: vec![AnalysisOverride::Definition {
                target,
                expression: "1".into(),
            }],
            ..Default::default()
        },
    )
    .unwrap()
}

fn selector_file_choices(file: &str) -> AnalysisConfiguration {
    serde_json::from_value(serde_json::json!({
        "equation_selection": {"mode": "include", "file": file}
    }))
    .unwrap()
}

fn selector_replacement_race_is_refused(file_size: usize) {
    let files = Fixture::new();
    let root = files.write(
        "main.mod",
        "var a b; model; [name='a'] a=a(-1); [name='b'] b=b(-1); end;\n",
    );
    let padding = " ".repeat(file_size.saturating_sub(3));
    let original = format!("a{padding}\n");
    let selector = files.write("equations.txt", &original);
    let replaced = format!("b{padding}\n");
    let modified = std::fs::metadata(&selector).unwrap().modified().unwrap()
        + std::time::Duration::from_secs(3);
    let selector_path = PathBuf::from(&selector);
    SELECTOR_BEFORE_FINAL_GUARD.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            assert_eq!(original.len(), replaced.len());
            std::fs::write(&selector_path, replaced).unwrap();
            std::fs::OpenOptions::new()
                .write(true)
                .open(&selector_path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(modified))
                .unwrap();
        }));
    });
    let mut ws = Workspace::new();
    let error = ws
        .set_root_analysis_configuration(&root, selector_file_choices("equations.txt"))
        .unwrap_err();
    assert_eq!(error.code, "ANALYSIS_SELECTOR_FILE_STALE");
    assert!(
        ws.root_analysis_configuration(&root)
            .equation_selection
            .is_none()
    );
}

#[test]
fn selector_bytes_and_guard_are_kept_from_one_small_file_revision() {
    // Guards refusing publication if a small selector changes after its
    // cached bytes and guard were captured but before configuration commits.
    selector_replacement_race_is_refused(2);
}

#[test]
fn selector_bytes_and_guard_are_kept_from_one_large_file_revision() {
    // Guards refusing publication for uncached (>1 MB) selector bytes when a
    // same-length replacement changes the filesystem modification time.
    selector_replacement_race_is_refused(INPUT_FILE_BYTE_CAP + 64);
}

#[test]
fn disk_selector_refuses_an_open_editor_overlay_and_preserves_it() {
    // Guards keeping saved-file selection isolated from unsaved Dynare editor
    // text, and retaining the previous configuration after refusal.
    let files = Fixture::new();
    let root = files.write(
        "main.mod",
        "var a b; model; [name='a'] a=a(-1); [name='b'] b=b(-1); end;\n",
    );
    let selector = files.write("selector.mod", "a\n");
    let mut ws = Workspace::new();
    ws.set_root_analysis_configuration(
        &root,
        AnalysisConfiguration {
            name: Some("Existing".into()),
            ..Default::default()
        },
    )
    .unwrap();
    ws.update_document(&selector, "b\n");

    let error = ws
        .set_root_analysis_configuration(&root, selector_file_choices("selector.mod"))
        .unwrap_err();
    assert_eq!(error.code, "ANALYSIS_SELECTOR_FILE_OVERLAY");
    assert!(error.message.contains("Saving alone does not close"));
    assert!(ws.docs[&selector].overlay);
    assert_eq!(ws.docs[&selector].source.as_str(), "b\n");
    let accepted = ws.root_analysis_configuration(&root);
    assert_eq!(accepted.name.as_deref(), Some("Existing"));
    assert!(accepted.equation_selection.is_none());

    ws.remove_document(&selector);
    ws.set_root_analysis_configuration(&root, selector_file_choices("selector.mod"))
        .unwrap();
    assert_eq!(
        ws.get_effective_model(&root).unwrap().equations[0].name,
        "a"
    );
}

#[test]
fn saved_selector_file_edits_refresh_selection_and_removal_withholds_facts() {
    // Guards same-choice content refresh, raw-byte receipt changes, selector
    // dependency tracking, and withholding facts after the active file vanishes.
    let files = Fixture::new();
    let root = files.write(
        "main.mod",
        "var a b; model; [name='a'] a=a(-1); [name='b'] b=b(-1); end;\n",
    );
    let selector = files.write("equations.txt", "name=\na\r\n");
    let mut ws = Workspace::new();
    ws.set_root_analysis_configuration(&root, selector_file_choices("equations.txt"))
        .unwrap();
    let first = ws.analysis_metadata(&root).unwrap();
    assert_eq!(first.selector_file.as_ref().unwrap().value_count, 1);
    assert_eq!(
        first.selector_file.as_ref().unwrap().byte_sha256,
        format!("{:x}", sha2::Sha256::digest(b"name=\na\r\n"))
    );
    assert_eq!(
        ws.get_effective_model(&root).unwrap().equations[0].name,
        "a"
    );

    files.write("equations.txt", "name=\nb\n");
    let refreshed = ws.analysis_metadata(&root).unwrap();
    assert_ne!(first.revision, refreshed.revision);
    assert_ne!(
        first.selector_file.as_ref().unwrap().byte_sha256,
        refreshed.selector_file.as_ref().unwrap().byte_sha256
    );
    assert_eq!(
        ws.get_effective_model(&root).unwrap().equations[0].name,
        "b"
    );

    std::fs::remove_file(&selector).unwrap();
    assert!(ws.get_effective_model(&root).is_none());
    assert_eq!(
        ws.analysis_metadata(&root).unwrap_err().code,
        "ANALYSIS_SELECTOR_FILE_MISSING"
    );
    assert!(matches!(
        ws.analysis_selection(&root),
        AnalysisSelectionState::Stale { .. }
    ));
    files.write("equations.txt", "name=\na\n");
    ws.analysis_metadata(&root).unwrap();
    assert_eq!(
        ws.get_effective_model(&root).unwrap().equations[0].name,
        "a"
    );
}

#[test]
fn selector_only_include_refresh_reuses_splice_and_macro_source() {
    // Guards reuse of the joined-source and macro-source owners. The current
    // projection needs one joined parse after the selector changes; it must
    // not rerun include/macro acquisition or parse the selector as Dynare.
    let files = Fixture::new();
    let root = files.write("main.mod", "@#include \"body.inc\"\n");
    let selector = files.write("equations.txt", "name=\na\n");
    files.write(
        "body.inc",
        "var a b; model; [name='a'] a=a(-1); [name='b'] b=b(-1); end;\n",
    );
    let mut ws = Workspace::new();
    ws.set_root_analysis_configuration(&root, selector_file_choices("equations.txt"))
        .unwrap();
    assert_eq!(
        ws.get_effective_model(&root).unwrap().equations[0].name,
        "a"
    );
    let before = crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|count| count.get());
    let source = Arc::clone(&ws.spliced[&root]);
    let macro_source = Arc::clone(source.macro_source.as_ref().unwrap());
    let splice_work = source.work_used;
    let macro_work = source.macro_source_preparation_work;
    assert!(ws.docs[&selector].model.get().is_none());

    files.write("equations.txt", "name=\nb\n");
    assert_eq!(
        ws.analysis_metadata(&root)
            .unwrap()
            .selector_file
            .as_ref()
            .unwrap()
            .value_count,
        1
    );
    assert_eq!(
        ws.get_effective_model(&root).unwrap().equations[0].name,
        "b"
    );
    let after = crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|count| count.get());
    let refreshed = &ws.spliced[&root];
    assert_eq!(after - before, 1);
    assert_eq!(refreshed.work_used, splice_work);
    assert_eq!(refreshed.macro_source_preparation_work, macro_work);
    assert!(Arc::ptr_eq(
        &macro_source,
        refreshed.macro_source.as_ref().unwrap()
    ));
}

#[test]
fn missing_selector_candidate_can_be_created_without_accepting_failed_choices() {
    // Guards retaining a negative dependency for a failed selection while
    // preserving the prior accepted source configuration.
    let files = Fixture::new();
    let root = files.write("main.mod", "var y; model; y=0; end;\n");
    let mut ws = Workspace::new();
    assert_eq!(
        ws.set_root_analysis_configuration(&root, selector_file_choices("later.txt"))
            .unwrap_err()
            .code,
        "ANALYSIS_SELECTOR_FILE_MISSING"
    );
    assert!(
        ws.root_analysis_configuration(&root)
            .equation_selection
            .is_none()
    );
    files.write("later.txt", "y\n");
    ws.set_root_analysis_configuration(&root, selector_file_choices("later.txt"))
        .unwrap();
    assert_eq!(ws.get_effective_model(&root).unwrap().equations.len(), 1);
}

#[test]
fn evicted_configurations_reacquire_targets_without_accepting_changed_text() {
    // Guards treating cache eviction as a missing target, and trusting a
    // reloaded target whose bytes no longer match the accepted source guard.
    let files = Fixture::new();
    let root = files.write("main.mod", "var y; model; y=0; end;\n");
    let target = files.write("dormant.inc", "@#define FLAG=0\n");
    let mut ws = Workspace::new();
    let selected = select_target(&mut ws, &root, &target);
    assert!(ws.analysis_metadata(&root).unwrap().model_complete);
    ws.retain_analysis_roots(&HashSet::new());
    assert!(ws.docs.is_empty());
    assert!(ws.input_snapshots.is_empty());
    assert!(matches!(
        ws.analysis_selection(&root),
        AnalysisSelectionState::Selected(ref current) if current == &selected
    ));
    let metadata = ws.analysis_metadata(&root).unwrap();
    assert!(metadata.model_complete);
    assert_eq!(
        metadata.overrides[0].application,
        OverrideApplication::NotReached
    );
    assert_eq!(metadata.overrides[0].execution_count, Some(0));
    assert!(!ws.docs[&target].overlay);

    ws.retain_analysis_roots(&HashSet::new());
    files.write("dormant.inc", "@#define FLAG=2\n");
    let AnalysisSelectionState::Stale { selection, cause } = ws.analysis_selection(&root) else {
        panic!("changed evicted target was accepted");
    };
    assert_eq!(selection, selected);
    assert_eq!(cause.code, "ANALYSIS_TARGET_STALE");
    assert!(ws.get_effective_model(&root).is_none());
    assert_eq!(ws.analysis_metadata(&root).unwrap_err().code, cause.code);
    let source = ws
        .source_structure(&root, &SourceStructureQuery::default())
        .unwrap();
    assert!(source.analysis_revision.is_none());
}

#[test]
fn shared_target_is_revalidated_after_its_configured_root_is_evicted() {
    // A different retained root can keep the old target Doc alive. Fresh input
    // guards must not certify that cached text after its disk bytes change.
    let files = Fixture::new();
    let root = files.write("main.mod", "var y; model; y=0; end;\n");
    let target = files.write("dormant.inc", "@#define FLAG=0\n");
    let other = files.write(
        "other.mod",
        "@#include \"dormant.inc\"\nvar y; model; y=0; end;\n",
    );
    let mut ws = Workspace::new();
    select_target(&mut ws, &root, &target);
    ws.analysis_metadata(&root).unwrap();
    ws.input_revision(&other).unwrap();
    ws.retain_analysis_roots(&HashSet::from([other]));
    assert!(!ws.input_snapshots.contains_key(&root));
    assert!(ws.docs.contains_key(&target));
    files.write("dormant.inc", "@#define FLAG=2\n");
    assert_eq!(
        ws.analysis_metadata(&root).unwrap_err().code,
        "ANALYSIS_TARGET_STALE"
    );
    assert!(ws.get_effective_model(&root).is_none());
}

#[test]
fn missing_target_reacquisition_obeys_source_policy_and_work_limit() {
    // An absent supplied target must not read a same-named host file, and a
    // closed target read must obey the selected work allowance and cancellation.
    let files = Fixture::new();
    let root = files.write("main.mod", "var y; model; y=0; end;\n");
    let target = files.write("dormant.inc", "@#define FLAG=0\n");
    let mut supplied = Workspace::overlay_documents(&BTreeMap::from([
        (root.clone(), "var y; model; y=0; end;\n".into()),
        (target.clone(), "@#define FLAG=0\n".into()),
    ]));
    select_target(&mut supplied, &root, &target);
    supplied.remove_document(&target);
    let AnalysisSelectionState::Stale { cause, .. } = supplied.analysis_selection(&root) else {
        panic!("supplied target was reacquired from host disk");
    };
    assert_eq!(cause.code, "ANALYSIS_TARGET_MISSING");
    assert!(!supplied.docs.contains_key(&target));

    let mut disk = Workspace::new();
    select_target(&mut disk, &root, &target);
    disk.retain_analysis_roots(&HashSet::new());
    disk.set_root_macro_work_limit(&root, MacroWorkLimit::new(1).unwrap());
    assert!(disk.checked_analysis(&root).is_err());
    assert!(!disk.docs.contains_key(&target));
    disk.set_root_macro_work_limit(&root, MacroWorkLimit::default());
    disk.set_cancellation(Arc::new(AtomicBool::new(true)));
    assert!(disk.checked_analysis(&root).is_err());
    assert!(!disk.docs.contains_key(&target));
}

#[test]
fn metadata_refreshes_closed_inputs_before_capturing_override_receipts() {
    // Guards pairing the previous execution count with the revision of a
    // changed included wrapper that now visits the same override three times.
    let files = Fixture::new();
    let root = files.write(
        "main.mod",
        "@#include \"wrapper.inc\"\nvar y; model; y=0; end;\n",
    );
    files.write("wrapper.inc", "@#include \"target.inc\"\n");
    let target = files.write("target.inc", "@#define FLAG=0\n");
    let mut ws = Workspace::new();
    select_target(&mut ws, &root, &target);
    let before = ws.analysis_metadata(&root).unwrap();
    assert_eq!(before.overrides[0].execution_count, Some(1));
    files.write(
        "wrapper.inc",
        "@#for i in 1:3\n@#include \"target.inc\"\n@#endfor\n",
    );
    let after = ws.analysis_metadata(&root).unwrap();
    assert_ne!(before.revision, after.revision);
    assert!(after.macro_complete && after.model_complete);
    assert_eq!(after.overrides[0].execution_count, Some(3));
    assert_eq!(after.selection, before.selection);
}
