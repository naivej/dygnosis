//! Include calls retain loop occurrence identity, nesting, and execution order.

use dygnosis::{dynare_diagnose, dynare_expand};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const ROOT: &str = "/occurrences/root.mod";
const MODEL: &str = "var y; model; y=0; end;\n";

fn assert_messages(prefix: &str, children: &[(&str, &str)], expected: &[&str]) {
    let root = format!("{prefix}{MODEL}");
    let mut files = HashMap::from([(ROOT.to_string(), root.clone())]);
    for (name, text) in children {
        files.insert(format!("/occurrences/{name}"), text.to_string());
    }
    let expanded = dynare_expand(&root, Some(ROOT), Some(&files));
    assert_eq!(expanded["complete"], true, "{expanded:?}");
    assert_eq!(expanded["n_equations"], 1, "{expanded:?}");
    let messages: Vec<_> = expanded["macro_messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|message| message["message"].as_str().unwrap())
        .collect();
    assert_eq!(messages, expected, "{expanded:?}");
    assert!(!dynare_diagnose(&root, Some(ROOT), Some(&files))
        .iter()
        .any(|diag| diag.code.starts_with('E') || diag.code == "I211"));
}

#[test]
fn repeated_index_values_keep_distinct_include_calls() {
    assert_messages(
        "@#define n=0\n@#for i in [1,1]\n@#define n=n+1\n@#include (string)n+\".inc\"\n@#endfor\n",
        &[("1.inc", "@#echo \"one\"\n"), ("2.inc", "@#echo \"two\"\n")],
        &["one", "two"],
    );
}

#[test]
fn repeated_child_calls_resolve_their_own_nested_target() {
    assert_messages(
        "@#define n=0\n@#for i in [1,1]\n@#define n=n+1\n@#include \"child.inc\"\n@#endfor\n",
        &[
            ("child.inc", "@#include (string)n+\".inc\"\n"),
            ("1.inc", "@#echo \"one\"\n"),
            ("2.inc", "@#echo \"two\"\n"),
        ],
        &["one", "two"],
    );
}

#[test]
fn nested_loops_expand_every_include_in_written_order() {
    assert_messages("@#for i in [1,2]\n@#for j in [1,2]\n@#include (string)i+(string)j+\".inc\"\n@#endfor\n@#endfor\n",
        &[("11.inc", "@#echo \"11\"\n"), ("12.inc", "@#echo \"12\"\n"), ("21.inc", "@#echo \"21\"\n"), ("22.inc", "@#echo \"22\"\n")], &["11", "12", "21", "22"]);
}

#[test]
fn an_inner_index_does_not_replace_the_outer_iteration_binding() {
    assert_messages("@#for i in [1,2]\n@#echo i\n@#for i in [3,4]\n@#include (string)i+\".inc\"\n@#endfor\n@#echo i\n@#endfor\n",
        &[("3.inc", "@#echo \"three\"\n"), ("4.inc", "@#echo \"four\"\n")], &["1", "three", "four", "4", "2", "three", "four", "4"]);
}

#[test]
fn partially_executed_sites_keep_the_first_iteration_first() {
    assert_messages("@#for i in [1,2,3]\n@#if i>=2\n@#include \"a\"+(string)i+\".inc\"\n@#endif\n@#if i<=2\n@#include \"b\"+(string)i+\".inc\"\n@#endif\n@#endfor\n",
        &[("a2.inc", "@#echo \"a2\"\n"), ("a3.inc", "@#echo \"a3\"\n"), ("b1.inc", "@#echo \"b1\"\n"), ("b2.inc", "@#echo \"b2\"\n")], &["b1", "a2", "b2", "a3"]);
}

#[test]
fn empty_tuple_indices_do_not_create_an_internal_name() {
    assert_messages("@#define n=0\n@#for () in [(),()]\n@#define n=n+1\n@#include (string)n+\".inc\"\n@#echo defined(_i)\n@#endfor\n",
        &[("1.inc", "@#echo \"one\"\n"), ("2.inc", "@#echo \"two\"\n")], &["one", "false", "two", "false"]);
}

struct Composition {
    prefix: &'static str,
    children: &'static [(&'static str, &'static str)],
    messages: &'static [&'static str],
}

const COMPOSITIONS: &[Composition] = &[
    Composition {
        prefix: "@#define a=1\n@#define b=0\n@#for (a,b) in [1,2]\n@#define b=b+1\n@#include (string)b+\".inc\"\n@#echo (a,b)\n@#endfor\n",
        children: &[("1.inc", ""), ("2.inc", "")],
        messages: &["(1, 1)", "(1, 2)"],
    },
    Composition {
        prefix: "@#for (i,i) in [(1,2),(3,4)]\n@#include (string)i+\".inc\"\n@#echo i\n@#endfor\n",
        children: &[("2.inc", ""), ("4.inc", "")],
        messages: &["2", "4"],
    },
    Composition {
        prefix: "@#for i in [1/3,2/3]\n@#include (string)i+\".inc\"\n@#echo i==1/3\n@#endfor\n",
        children: &[("0.333333333333333.inc", ""), ("0.666666666666667.inc", "")],
        messages: &["true", "false"],
    },
    Composition {
        prefix: "@#for i in [(1,),(2,)]\n@#include (string)(real)i+\".inc\"\n@#echo istuple(i)\n@#endfor\n",
        children: &[("1.inc", ""), ("2.inc", "")],
        messages: &["true", "true"],
    },
    Composition {
        prefix: "@#for i in [1,2,3]\n@#echo i\n@#if i<=2\n@#include (string)i+\".inc\"\n@#endif\n@#endfor\n@#echo i\n",
        children: &[("1.inc", ""), ("2.inc", "")],
        messages: &["1", "2", "3", "3"],
    },
    Composition {
        prefix: "@#for i in [1,2]\n@#for j in [1,2,3]\n@#echo (i,j)\n@#if j<=2\n@#include (string)i+(string)j+\".inc\"\n@#endif\n@#endfor\n@#endfor\n",
        children: &[("11.inc", ""), ("12.inc", ""), ("21.inc", ""), ("22.inc", "")],
        messages: &["(1, 1)", "(1, 2)", "(1, 3)", "(2, 1)", "(2, 2)", "(2, 3)"],
    },
    Composition {
        prefix: "@#define k=0\n@#for i in [k for k in [1,2]]\n@#include (string)i+\".inc\"\n@#endfor\n@#echo k\n",
        children: &[("1.inc", "@#echo k\n"), ("2.inc", "@#echo k\n")],
        messages: &["2", "2", "2"],
    },
    Composition {
        prefix: "@#define k=0\n@#for i in [1,2] when (real)[k for k in [i]]>0\n@#include (string)i+\".inc\"\n@#endfor\n@#echo k\n",
        children: &[("1.inc", "@#echo k\n"), ("2.inc", "@#echo k\n")],
        messages: &["2", "2", "2"],
    },
    Composition {
        prefix: "@#define k=0\n@#for i in [k for k in [k+1] when false]\n@#include \"unused.inc\"\n@#endfor\n@#echo k\n",
        children: &[],
        messages: &["1"],
    },
    Composition {
        prefix: "@#define k=0\n@#for i in [1,2] when (real)[k for k in [i]]<0\n@#include \"unused.inc\"\n@#endfor\n@#echo k\n",
        children: &[],
        messages: &["2"],
    },
    Composition {
        prefix: "@#define k=0\n@#include (string)((real)[k+1 for k in [k+1]])+\".inc\"\n@#echo k\n",
        children: &[("2.inc", "@#echo k\n")],
        messages: &["1", "1"],
    },
    Composition {
        prefix: "@#define k=0\n@#include (string)[(string)(k+1)+\".inc\" for k in [k+1]]\n@#echo k\n",
        children: &[("[2.inc]", "@#echo k\n")],
        messages: &["1", "1"],
    },
    Composition {
        prefix: "@#define k=0\n@#include \"middle.inc\"\n@#echo k\n",
        children: &[("middle.inc", "@#include (string)((real)[k+1 for k in [k+1]])+\".inc\"\n@#echo k\n"), ("2.inc", "@#echo k\n")],
        messages: &["1", "1", "1"],
    },
    Composition {
        prefix: "@#define k=0\n@#for i in [1,1]\n@#include (string)((real)[k+1 for k in [k+1]])+\".inc\"\n@#endfor\n@#echo k\n",
        children: &[("2.inc", "@#echo k\n"), ("3.inc", "@#echo k\n")],
        messages: &["1", "2", "2"],
    },
    Composition {
        prefix: "@#define k=0\n@#for i in [k for k in [k+1]]\n@#echo k\n@#endfor\n@#echo k\n",
        children: &[],
        messages: &["1", "1"],
    },
];

#[test]
fn original_expressions_keep_effects_when_includes_are_projected() {
    for case in COMPOSITIONS {
        assert_messages(case.prefix, case.children, case.messages);
    }
}

struct ProbeDir(PathBuf);

impl Drop for ProbeDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn dynare_7_2_include_composition_honesty_when_present() {
    let binary = PathBuf::from(r"C:\dynare\7.2\preprocessor\dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("SKIP include composition honesty: Dynare 7.2 is absent");
        return;
    }
    let dir = ProbeDir(std::env::temp_dir().join(format!(
        "dyg-include-composition-{}-{}", std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    )));
    fs::create_dir_all(&dir.0).unwrap();
    for (index, case) in COMPOSITIONS.iter().enumerate() {
        assert_messages(case.prefix, case.children, case.messages);
        let folder = dir.0.join(index.to_string());
        fs::create_dir_all(&folder).unwrap();
        let root = folder.join("root.mod");
        fs::write(&root, format!("{}{MODEL}", case.prefix)).unwrap();
        for (name, text) in case.children {
            fs::write(folder.join(name), text).unwrap();
        }
        let mut child = Command::new(&binary)
            .arg(&root)
            .args(["json=check", "onlyjson"])
            .current_dir(&folder)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let started = Instant::now();
        loop {
            if started.elapsed() > Duration::from_secs(8) {
                let _ = child.kill();
                let _ = child.wait();
                panic!("Dynare composition probe timed out: {}", case.prefix);
            }
            if child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{}: {stdout}", case.prefix);
        let messages: Vec<_> = stdout
            .lines()
            .filter(|line| line.starts_with("@#echo ("))
            .filter_map(|line| line.split_once("): ").map(|(_, value)| value))
            .collect();
        assert_eq!(messages, case.messages, "{}: {stdout}", case.prefix);
    }
}
