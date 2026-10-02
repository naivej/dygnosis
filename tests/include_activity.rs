use std::collections::HashMap;

use dygnosis::{dynare_model_info, Workspace};

fn workspace(text: &str, files: &[(&str, &str)]) -> (Workspace, String, HashMap<String, String>) {
    let root = "C:/dygnosis-include-activity/root.mod".to_string();
    let mut supplied = HashMap::from([(root.clone(), text.to_string())]);
    for (name, body) in files {
        supplied.insert(
            format!("C:/dygnosis-include-activity/{name}"),
            body.to_string(),
        );
    }
    let mut workspace = Workspace::new();
    for (name, body) in &supplied {
        workspace.update_document(name, body);
    }
    (workspace, root, supplied)
}

const MODEL: &str = "var y; model; y=0; end;\n";

#[test]
fn only_executed_unresolved_and_cyclic_include_sites_withhold_counts() {
    for (prefix, files, complete) in [
        ("@#include \"absent.inc\"\n", vec![], false),
        ("@#if 0\n@#include \"absent.inc\"\n@#endif\n", vec![], true),
        (
            "@#if 1\n@#include \"cycle.inc\"\n@#endif\n",
            vec![("cycle.inc", "@#include \"root.mod\"\n")],
            false,
        ),
        (
            "@#if 0\n@#include \"cycle.inc\"\n@#endif\n",
            vec![("cycle.inc", "@#include \"root.mod\"\n")],
            true,
        ),
        (
            "@#include \"flags.inc\"\n@#if ENABLED\n@#include \"absent.inc\"\n@#endif\n",
            vec![("flags.inc", "@#define ENABLED = 0\n")],
            true,
        ),
        (
            "@#include \"flags.inc\"\n@#if ENABLED\n@#include \"absent.inc\"\n@#endif\n",
            vec![("flags.inc", "@#define ENABLED = 1\n")],
            false,
        ),
        (
            "@#if unavailable\n@#include \"absent.inc\"\n@#endif\n",
            vec![],
            false,
        ),
    ] {
        let text = format!("{prefix}{MODEL}");
        let (mut workspace, root, files) = workspace(&text, &files);
        assert_eq!(workspace.includes_complete(&root), complete, "{prefix}");
        let info = dynare_model_info(&text, Some(&root), Some(&files));
        assert_eq!(
            info.get("n_equations").is_some(),
            complete,
            "{prefix}: {info}"
        );
        if complete {
            assert_eq!(info["n_equations"], 1);
        } else {
            assert_eq!(info["status"], "incomplete");
        }
    }
}

#[test]
fn nonliteral_and_text_only_include_activity_is_not_invented_as_resolution() {
    for prefix in ["@#include fname\n", "@#include \"absent.inc\"\n"] {
        let active = format!("{prefix}{MODEL}");
        let inactive = format!("@#if 0\n{prefix}@#endif\n{MODEL}");
        for (text, complete) in [(active, false), (inactive, true)] {
            let (mut workspace, root, files) = workspace(&text, &[]);
            assert_eq!(workspace.includes_complete(&root), complete, "{text}");
            for info in [
                dynare_model_info(&text, Some(&root), Some(&files)),
                dynare_model_info(&text, None, None),
            ] {
                assert_eq!(
                    info.get("n_equations").is_some(),
                    complete,
                    "{text}: {info}"
                );
            }
        }
    }
}

#[test]
fn cached_activity_rechecks_definitions_and_preserves_raw_diagnostics() {
    let text = format!(
        "@#include \"flags.inc\"\n@#if ENABLED\n@#include \"absent.inc\"\n@#endif\n{MODEL}"
    );
    let (mut workspace, root, _) = workspace(&text, &[("flags.inc", "@#define ENABLED=0\n")]);
    assert!(workspace.includes_complete(&root));
    assert_eq!(
        workspace.include_records(&root).unwrap().unresolved.len(),
        1
    );
    let before = workspace.get_effective_model(&root).unwrap().clone();
    // This existing raw-site E061 is scheduled for the activity-aware
    // diagnostics fix in 0.11.4; new count authority must not inherit it.
    assert!(
        dygnosis::check_e061(workspace.include_records(&root).unwrap())
            .iter()
            .any(|diag| diag.code == "E061")
    );
    workspace.update_document(
        "C:/dygnosis-include-activity/flags.inc",
        "@#define ENABLED=1\n",
    );
    assert!(!workspace.includes_complete(&root));
    assert_eq!(
        before.non_local_equation_count(),
        workspace
            .get_effective_model(&root)
            .unwrap()
            .non_local_equation_count()
    );
}

#[test]
fn accepted_inactive_include_cases_agree_with_pinned_check() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let binary = std::path::PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("skip include activity Check: pinned Dynare 7.2 is absent");
        return;
    }
    for prefix in [
        "@#if 0\n@#include \"absent.inc\"\n@#endif\n",
        "@#if 0\n@#include fname\n@#endif\n",
        "@#if 0\n@#include \"cycle.inc\"\n@#endif\n",
        "@#include \"flags.inc\"\n@#if ENABLED\n@#include \"absent.inc\"\n@#endif\n",
    ] {
        let text = format!("{prefix}{MODEL}");
        let directory = std::env::temp_dir().join(format!(
            "dygnosis-include-activity-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("root.mod"), &text).unwrap();
        std::fs::write(directory.join("cycle.inc"), "@#include \"root.mod\"\n").unwrap();
        std::fs::write(directory.join("flags.inc"), "@#define ENABLED=0\n").unwrap();
        let official = dygnosis::run_preprocessor(
            &text,
            &binary,
            Some(&directory),
            std::time::Duration::from_secs(30),
            dygnosis::JsonStage::Check,
        );
        assert!(official.success, "{prefix}: {official:?}");
        let (mut workspace, root, files) = workspace(
            &text,
            &[
                ("cycle.inc", "@#include \"root.mod\"\n"),
                ("flags.inc", "@#define ENABLED=0\n"),
            ],
        );
        assert!(workspace.includes_complete(&root));
        assert_eq!(
            dynare_model_info(&text, Some(&root), Some(&files))["n_equations"],
            1
        );
        let resolved = directory.canonicalize().unwrap();
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        assert!(resolved.starts_with(&temporary) && resolved != temporary);
        std::fs::remove_dir_all(resolved).unwrap();
    }
    let mixed = format!("@#if 0\r@#include \"absent.inc\"\r@#endif\r{MODEL}");
    assert_eq!(dynare_model_info(&mixed, None, None)["n_equations"], 1);
    let native = format!("{MODEL}helper=2\n");
    let official = dygnosis::run_preprocessor(
        &native,
        &binary,
        None,
        std::time::Duration::from_secs(30),
        dygnosis::JsonStage::Check,
    );
    assert!(official.success, "native optional semicolon: {official:?}");
    assert_eq!(dynare_model_info(&native, None, None)["n_equations"], 1);
}
