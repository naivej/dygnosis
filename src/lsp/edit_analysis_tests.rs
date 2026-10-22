//! First-request analysis reuse with current edited source locations.

use super::*;
use crate::engine::language::parser::PARSE_EXPANDED_BUILDS;

#[tokio::test]
async fn model_info_after_edits_reuses_the_checked_parse_and_keeps_current_origins() {
    // Guards a second core parse while preparing the first model-information
    // response after an edit, and stale semantic facts, include locations or
    // document versions.
    let root = Url::parse("file:///C:/dygnosis-edit-analysis/root.mod").unwrap();
    let child = Url::parse("file:///C:/dygnosis-edit-analysis/rows.inc").unwrap();
    let root_text = "var y z1 z2;\r\nparameters p;\r\np=1;\r\nmodel;\r\n#local=p;\r\n@#include \"rows.inc\"\r\nend;\r\n";
    let child_text = "/* 😀 */ [name='first'] y=local;\r\n@#for k in 1:2\r\n[name='copy'] z@{k}=y(-1)+@{k};\r\n@#endfor\r\n";
    let (service, _socket) = new_service();
    let server = service.inner();
    server.upsert(child.clone(), child_text.into(), 1);
    server.upsert(root.clone(), root_text.into(), 1);
    let arguments = [json!({"root_uri":root,"document_uri":child})];
    let mut previous_revision = None;

    for phase in 0..4 {
        if phase == 1 {
            server.upsert(root.clone(), format!("// root shift\r\n{root_text}"), 2);
        } else if phase == 2 {
            server.upsert(
                child.clone(),
                format!("// include shift\r\n{child_text}"),
                2,
            );
        } else if phase == 3 {
            let changed_root = root_text.replace("#local=p;", "#local=p+1;");
            server.upsert(root.clone(), format!("// root shift\r\n{changed_root}"), 3);
        }
        PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
        let result = server.model_info_command(&arguments);
        let parse_builds = PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get);

        assert_eq!(result["complete"], true, "{result}");
        assert_eq!(result["n_equations"], 3);
        assert_eq!(result["document_version"], if phase >= 2 { 2 } else { 1 });
        assert_eq!(
            result["model_locals"]["definitions"][0]["expression"],
            if phase == 3 { "p+1" } else { "p" }
        );
        assert_eq!(
            result["model_locals"]["definitions"][0]["origin"]["uri"],
            root.as_str()
        );
        let equations = result["equations"].as_array().unwrap();
        let facts: Vec<_> = equations
            .iter()
            .map(|row| {
                (
                    row["name"].as_str().unwrap(),
                    row["number"].as_u64().unwrap(),
                )
            })
            .collect();
        assert_eq!(facts, [("first", 1), ("copy", 2), ("copy", 3)]);
        assert_eq!(equations[0]["location"]["uri"], child.as_str());
        assert_eq!(
            equations[0]["location"]["range"]["start"],
            json!({"line":usize::from(phase >= 2),"character":9})
        );
        assert_eq!(equations[1]["location"], equations[2]["location"]);
        assert_eq!(
            equations[1]["location"]["range"]["start"]["line"],
            if phase >= 2 { 3 } else { 2 }
        );
        if let Some(previous_revision) = previous_revision {
            assert_ne!(result["revision"], previous_revision);
        }
        previous_revision = Some(result["revision"].clone());
        assert_eq!(
            parse_builds, 0,
            "first model-information request must reuse the checked core parse (phase {phase})"
        );

        PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
        assert_eq!(server.model_info_command(&arguments), result);
        assert_eq!(PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get), 0);
    }
}
