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
