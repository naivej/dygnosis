//! Generated macro markers stay native text and preserve the following declaration.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dygnosis::diagnostic::analyze;
use dygnosis::expand::expand_report;
use dygnosis::parse;

const MARKERS: &[&str] = &["@#echo 123", "@#define x=1", "@{missing}", "disp x"];

fn source(marker: &str) -> String {
    format!("@#define gen=\"{marker}\n\"\n@{{gen}}\nvar y; model; y=0; end;\n")
}

fn check(marker: &str) {
    let source = source(marker);
    let model = parse(&source);
    let diagnostics = analyze(&model);
    assert!(
        !diagnostics.iter().any(|row| row.code.starts_with('E')),
        "{marker}: {diagnostics:?}"
    );
    assert_eq!(model.endogenous.len(), 1, "{marker}");
    assert_eq!(model.name(model.endogenous[0].name), "y", "{marker}");
    assert_eq!(model.equations.len(), 1, "{marker}");
    // Only the written define and interpolation belong to macro metadata.
    assert_eq!(model.macro_directives.len(), 1, "{marker}");
    assert_eq!(model.macro_interps.len(), 1, "{marker}");
    let report = expand_report(&source);
    assert!(report.complete, "{marker}: {}", report.effective_text);
    assert_eq!(report.n_equations, 1, "{marker}");
    assert!(report.macro_messages.is_empty(), "{marker}");
    assert!(
        report.effective_text.contains(marker),
        "{marker}: {}",
        report.effective_text
    );
    assert!(
        report.effective_text.contains("\nvar y"),
        "{marker}: {}",
        report.effective_text
    );
}

#[test]
fn generated_macro_markers_do_not_absorb_the_next_line() {
    for marker in MARKERS {
        check(marker);
    }
}

#[test]
fn generated_interpolation_keeps_the_rest_of_its_line_native() {
    check("@{missing} var hidden;");
}

#[test]
fn an_identical_replacement_still_stays_generated() {
    let source = "@#define gen=\"@{gen}\"\n@{gen}\nvar y; model; y=0; end;\n";
    let model = parse(source);
    assert_eq!(model.endogenous.len(), 1, "{:?}", analyze(&model));
    assert_eq!(model.name(model.endogenous[0].name), "y");
    assert!(!analyze(&model).iter().any(|row| row.code.starts_with('E')));
    let report = expand_report(source);
    assert!(report.complete, "{}", report.effective_text);
    assert!(
        report.effective_text.contains("@{gen}\nvar y"),
        "{}",
        report.effective_text
    );
}

#[test]
fn generated_declared_heads_and_commands_keep_dynare_parsing() {
    let source = "parameters p;\n@#define gen=\"p=2;\n\"\n@{gen}\nvar y; model; y=p; end;\n";
    let model = parse(source);
    assert_eq!(model.param_assignments.len(), 1, "{:?}", analyze(&model));
    assert_eq!(model.param_assignments[0].expression, "2");
    assert_eq!(model.endogenous.len(), 1);
    assert!(!analyze(&model).iter().any(|row| row.code.starts_with('E')));
    let source = "@#define gen=\"var y;\n\"\n@{gen}\nmodel; y=0; end;\n";
    let model = parse(source);
    assert_eq!(model.endogenous.len(), 1, "{:?}", analyze(&model));
    assert_eq!(model.equations.len(), 1);
    assert!(!analyze(&model).iter().any(|row| row.code.starts_with('E')));
}

struct ProbeDir(PathBuf);

impl ProbeDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dyg-generated-literal-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for ProbeDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(binary: &Path, path: &Path, args: &[String], dir: &Path) -> Output {
    let mut child = Command::new(binary)
        .arg(path)
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(8) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("Dynare generated literal probe timed out");
        }
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn dynare_7_2_generated_literals_honesty_when_present() {
    let binary = Path::new(r"C:\dynare\7.2\preprocessor\dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("SKIP generated literal honesty: Dynare 7.2 is absent");
        return;
    }
    let dir = ProbeDir::new();
    for (index, marker) in MARKERS.iter().enumerate() {
        check(marker);
        let path = dir.0.join(format!("probe_{index}.mod"));
        let expanded = dir.0.join(format!("expanded_{index}.mod"));
        fs::write(&path, source(marker)).unwrap();
        let output = run(
            binary,
            &path,
            &[
                "onlymacro".into(),
                format!("savemacro={}", expanded.display()),
            ],
            &dir.0,
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{marker}: {stdout}");
        assert!(!stdout.contains("@#echo ("), "{marker}: {stdout}");
        let text = fs::read_to_string(&expanded).unwrap().replace("\r\n", "\n");
        assert!(
            text.contains(&format!("{marker}\nvar y;")),
            "{marker}: {text}"
        );
        let output = run(
            binary,
            &path,
            &["json=check".into(), "onlyjson".into()],
            &dir.0,
        );
        assert!(
            output.status.success(),
            "{marker}: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
