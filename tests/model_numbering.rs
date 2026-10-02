//! JC15: written counted rows match Dynare 7.2 Check before transformation.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use dygnosis::expand::expand_report;
use dygnosis::{parse, Workspace};
use serde_json::Value;

static NEXT: AtomicU64 = AtomicU64::new(0);

fn check_json(binary: &Path, text: &str, include: Option<&str>) -> Value {
    let directory = std::env::temp_dir().join(format!(
        "dygnosis-jc15-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("numbering.mod");
    std::fs::write(&file, text).unwrap();
    if let Some(include) = include {
        std::fs::write(directory.join("body.inc"), include).unwrap();
    }
    let output = Command::new(binary)
        .arg(&file)
        .args(["json=check", "onlyjson", "nopreprocessoroutput"])
        .current_dir(&directory)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let json = serde_json::from_slice(
        &std::fs::read(directory.join("numbering/model/json/modfile.json")).unwrap(),
    )
    .unwrap();
    let resolved = directory.canonicalize().unwrap();
    let temporary = std::env::temp_dir().canonicalize().unwrap();
    assert!(resolved.starts_with(&temporary) && resolved != temporary);
    std::fs::remove_dir_all(&resolved).unwrap();
    json
}

#[test]
fn jc15_aggregate_check_order_and_dimension_source_contract() {
    let binary = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("skip JC15: pinned Dynare 7.2 is not installed");
        return;
    }
    for (label, text, include) in [
        ("ordinary", "var y z; model; [name='first'] y=z; [name='second'] z=0; end;", None),
        ("multiple blocks and locals", "var y z; model; #a=1; [name='first'] y=a; end; model; [name='second'] z=y; end;", None),
        ("macro copies", "var y1 y2; model;\n@#for k in 1:2\n[name='row'] y@{k}=0;\n@#endfor\nend;", None),
        ("include inside model", "var y z; model; [name='first'] y=0;\n@#include \"body.inc\"\nend;", Some("[name='second'] z=y;\n")),
        ("model removal", "var y z; model; [name='drop'] y=0; [name='keep'] z=0; end; model_remove('drop');", None),
        ("model replacement", "var y z; model; [name='old'] y=0; [name='keep'] z=0; end; model_replace('old'); [name='new'] y=1; end;", None),
        ("static and dynamic", "var y z; model; #a=1; [static,name='steady'] y=0; [dynamic,name='law'] y=y(-1); [name='second'] z=a; end;", None),
    ] {
        let official = check_json(&binary, text, include);
        let report = if let Some(include) = include {
            let mut workspace = Workspace::new();
            workspace.update_document("C:/dygnosis-jc15/numbering.mod", text);
            workspace.update_document("C:/dygnosis-jc15/body.inc", include);
            workspace.expand_report("C:/dygnosis-jc15/numbering.mod").unwrap().clone()
        } else { expand_report(text) };
        let expected = official["abstract_syntax_tree"].as_array().unwrap();
        let actual: Vec<_> = report.model_map.equations.iter().filter(|row| row.dimension.is_none() && row.number.is_some()).collect();
        assert_eq!(actual.len(), expected.len(), "{label}");
        for (row, official) in actual.iter().zip(expected) {
            assert_eq!(row.number, Some(official["number"].as_u64().unwrap() as usize + 1), "{label}");
            assert_eq!(row.name, official["tags"]["name"].as_str().unwrap(), "{label}");
        }
        eprintln!("JC15 {label}: Check accepted; {} aggregate rows agree", actual.len());
    }
    let dimensions = "heterogeneity_dimension hh ff; var Y; var(heterogeneity=hh) c d; var(heterogeneity=ff) q; model(heterogeneity=hh); [name='hh1'] c=0; end; model; [name='agg1'] Y=0; end; model(heterogeneity=ff); [name='ff1'] q=0; end; model(heterogeneity=hh); [name='hh2'] d=c; end;";
    let official = check_json(&binary, dimensions, None);
    assert_eq!(
        official["heterogeneity_dimension"],
        serde_json::json!(["hh", "ff"])
    );
    assert_eq!(
        official["abstract_syntax_tree"].as_array().unwrap().len(),
        1
    );
    let model = parse(dimensions);
    assert_eq!(model.heterogeneous_models.len(), 3);
    let rows = expand_report(dimensions).model_map.equations;
    assert_eq!(
        rows.iter()
            .map(|row| (row.name.as_str(), row.number))
            .collect::<Vec<_>>(),
        [
            ("hh1", Some(1)),
            ("agg1", Some(1)),
            ("ff1", Some(1)),
            ("hh2", Some(2))
        ]
    );
    // Check JSON has no heterogeneous equation arrays. The pin's
    // begin_heterogeneous_model selects a per-dimension ModelTree, whose
    // addEquation appends to its own equations vector. Record that evidence,
    // rather than treating absent JSON rows as a zero-equation dimension.
    eprintln!("JC15 dimensions: Check accepted hh/ff and repeated hh opener; JSON omits heterogeneous equations; ordering follows pinned per-dimension ModelTree source");
}
