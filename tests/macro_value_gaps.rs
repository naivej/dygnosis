//! Macro values, refusal order, and branch choices agree with pinned Dynare 7.2.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dygnosis::diagnostic::analyze;
use dygnosis::expand::expand_report;
use dygnosis::parse;

const MODEL: &str = "var y; model; y=0; end;\n";

struct ValueCase {
    source: &'static str,
    echo: &'static str,
}

const VALUES: &[ValueCase] = &[
    ValueCase {
        source: "@#define inflation=2\n@#echo inflation\n",
        echo: "2",
    },
    ValueCase {
        source: "@#define nanvalue=2\n@#echo nanvalue\n",
        echo: "2",
    },
    ValueCase {
        source: "@#echo [,1]\n",
        echo: "[1]",
    },
    ValueCase {
        source: "@#echo (,1)\n",
        echo: "(1)",
    },
    ValueCase {
        source: "@#echo (1,,2)\n",
        echo: "(1, 2)",
    },
    ValueCase {
        source: "@#define f(x)=x\n@#echo f(,2)\n",
        echo: "2",
    },
    ValueCase {
        source: "@#define a=[3]\n@#echo a[,1]\n",
        echo: "3",
    },
    ValueCase {
        source: "@#echo [1 for () in [()]]\n",
        echo: "[1]",
    },
    ValueCase {
        source: "@#echo [1 for i in []]\n",
        echo: "[]",
    },
    ValueCase {
        source: "@#define f()=1\n@#define g(x)=[f for f in [2]]\n@#echo g(0)\n",
        echo: "[2]",
    },
    ValueCase {
        source: "@#define i=0\n@#echo i in [i for i in [1]]\n",
        echo: "true",
    },
    ValueCase {
        source: "@#echo 1:inf:3\n",
        echo: "[1]",
    },
    ValueCase {
        source: "@#echo 1:-inf:-3\n",
        echo: "[1]",
    },
    ValueCase {
        source: "@#echo -1e308:1e308:1e308\n",
        echo: "[-1e+308, 0, 1e+308]",
    },
    ValueCase {
        source: "@#echo max(nan,1)\n",
        echo: "nan",
    },
    ValueCase {
        source: "@#echo min(nan,1)\n",
        echo: "nan",
    },
    ValueCase {
        source: "@#echo max(1,nan)\n",
        echo: "1",
    },
    ValueCase {
        source: "@#echo min(1,nan)\n",
        echo: "1",
    },
    ValueCase {
        source: "@#echo max(-0,0)\n",
        echo: "-0",
    },
    ValueCase {
        source: "@#echo min(-0,0)\n",
        echo: "-0",
    },
    ValueCase {
        source: "@#echo normpdf(38)\n",
        echo: "0",
    },
    ValueCase {
        source: "@#if normpdf(38)>0\n@#echo \"positive\"\n@#else\n@#echo \"zero\"\n@#endif\n",
        echo: "zero",
    },
    ValueCase {
        source: "@#if max(nan,1)==1\n@#echo \"one\"\n@#else\n@#echo \"nan\"\n@#endif\n",
        echo: "nan",
    },
];

struct RefusalCase {
    source: &'static str,
    code: &'static str,
    message: &'static str,
}

const REFUSALS: &[RefusalCase] = &[
    RefusalCase {
        source: "@#echo [1 for 2 in []]\n",
        code: "E285",
        message: "the loop variables must be either a tuple or a variable",
    },
    RefusalCase {
        source: "@#echo [1 for 2+2 in []]\n",
        code: "E285",
        message: "the loop variables must be either a tuple or a variable",
    },
    RefusalCase {
        source: "@#echo [1 in [] when true]\n",
        code: "E285",
        message: "the loop variables must be either a tuple or a variable",
    },
    RefusalCase {
        source: "@#define f()=1\n@#echo [f for f in [2]]\n",
        code: "E285",
        message: "Variable f was previously defined as a function",
    },
    RefusalCase {
        source: "@#define f()=1\n@#echo [a for (a,f) in [(1,2)]]\n",
        code: "E285",
        message: "Variable f was previously defined as a function",
    },
    RefusalCase {
        source: "@#define a=[3]\n@#echo a[[1 for a in [7]]]\n",
        code: "E285",
        message: "You cannot index a real",
    },
    RefusalCase {
        source: "parameters p; p=@{1 //2};\n",
        code: "E062",
        message: "syntax error, unexpected DIVIDE",
    },
    RefusalCase {
        source: "@#echo 1\u{a0}+2\n",
        code: "E062",
        message: "syntax error, unexpected TEXT",
    },
    RefusalCase {
        source: "@#echo max(true,1)\n",
        code: "E285",
        message: "Operator `max` does not exist for this type",
    },
    RefusalCase {
        source: "@#echo normpdf(true,0,1)\n",
        code: "E285",
        message: "Operator `normpdf` does not exist for this type",
    },
    RefusalCase {
        source: "@#echo normcdf(true,0,1)\n",
        code: "E285",
        message: "Operator `normcdf` does not exist for this type",
    },
    RefusalCase {
        source: "@#define a=[1]\n@#echo a[[true]]\n",
        code: "E285",
        message: "You cannot index a variable with a nested array",
    },
];

fn check_value(case: &ValueCase) {
    let source = format!("{}{MODEL}", case.source);
    let report = expand_report(&source);
    assert!(
        report.complete,
        "{}: {}",
        case.source, report.effective_text
    );
    assert_eq!(report.n_equations, 1, "{}", case.source);
    let echoes: Vec<_> = report
        .macro_messages
        .iter()
        .map(|m| m.message.as_str())
        .collect();
    assert_eq!(echoes, [case.echo], "{}", case.source);
    assert!(
        !analyze(&parse(&source))
            .iter()
            .any(|d| d.code.starts_with('E')),
        "{}",
        case.source
    );
}

fn check_refusal(case: &RefusalCase) {
    let source = format!("{}{MODEL}", case.source);
    assert!(!expand_report(&source).complete, "{}", case.source);
    let diagnostics = analyze(&parse(&source));
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == case.code && d.message.starts_with(case.message)),
        "{}: {diagnostics:?}",
        case.source
    );
}

#[test]
fn values_and_branch_choices_keep_pinned_results() {
    for case in VALUES {
        check_value(case);
    }
}

#[test]
fn refusals_keep_pinned_trigger_and_sentence() {
    for case in REFUSALS {
        check_refusal(case);
    }
}

struct ProbeDir(PathBuf);

impl ProbeDir {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dyg-value-review-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn check(&self, binary: &PathBuf, index: usize, source: &str) -> Output {
        let path = self.0.join(format!("probe_{index}.mod"));
        fs::write(&path, format!("{source}{MODEL}")).unwrap();
        let mut child = Command::new(binary)
            .args([path.as_os_str(), "json=check".as_ref(), "onlyjson".as_ref()])
            .current_dir(&self.0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let started = Instant::now();
        loop {
            if started.elapsed() > Duration::from_secs(8) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Dynare value probe timed out: {source}");
            }
            if child.try_wait().unwrap().is_some() {
                return child.wait_with_output().unwrap();
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for ProbeDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn dynare_7_2_value_gap_honesty_when_present() {
    let binary = PathBuf::from(r"C:\dynare\7.2\preprocessor\dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("SKIP macro value gap honesty: Dynare 7.2 is absent");
        return;
    }
    let probes = ProbeDir::new();
    for (index, case) in VALUES.iter().enumerate() {
        check_value(case);
        let output = probes.check(&binary, index, case.source);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{}: {stdout}", case.source);
        let echoes: Vec<_> = stdout
            .lines()
            .filter(|line| line.starts_with("@#echo ("))
            .filter_map(|line| line.split_once("): ").map(|(_, value)| value))
            .collect();
        assert_eq!(echoes, [case.echo], "{}", case.source);
    }
    for (index, case) in REFUSALS.iter().enumerate() {
        check_refusal(case);
        let output = probes.check(&binary, VALUES.len() + index, case.source);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!output.status.success(), "{}: {stdout}", case.source);
        assert!(stdout.contains(case.message), "{}: {stdout}", case.source);
    }
}
