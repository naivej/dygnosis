//! Dynare command/option catalog (completion and hover data).
//!
//! Tables are a mechanical port of `python_dynare_lsp/dynare_catalog.py`.
//! `list_options` JSON matches `dynare_list_options` in the Python MCP server.

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Serialize;

#[path = "catalog_data.rs"]
mod catalog_data;

/// One command option as serialized in the known-command `list_options` payload.
#[derive(Clone, Debug, Serialize)]
pub struct NamedOption {
    pub name: &'static str,
    pub description: &'static str,
}

/// JSON for `dynare_list_options`: omitted, known, or unknown command.
#[derive(Clone, Debug, Serialize)]
#[serde(untagged)]
pub enum ListOptions {
    Omitted {
        n_commands: usize,
        commands: Vec<&'static str>,
    },
    Known {
        command: String,
        known: bool,
        n_options: usize,
        options: Vec<NamedOption>,
    },
    Unknown {
        command: String,
        known: bool,
        message: String,
        suggestions: Vec<&'static str>,
    },
}

fn command_map() -> &'static HashMap<&'static str, &'static [(&'static str, &'static str)]> {
    static MAP: OnceLock<HashMap<&'static str, &'static [(&'static str, &'static str)]>> =
        OnceLock::new();
    MAP.get_or_init(|| catalog_data::COMMAND_OPTIONS.iter().copied().collect())
}

fn option_doc_map() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| catalog_data::OPTION_DOCS.iter().copied().collect())
}

fn sorted_commands() -> &'static [&'static str] {
    static KEYS: OnceLock<Vec<&'static str>> = OnceLock::new();
    KEYS.get_or_init(|| {
        let mut keys: Vec<&'static str> = catalog_data::COMMAND_OPTIONS
            .iter()
            .map(|(name, _)| *name)
            .collect();
        keys.sort_unstable();
        keys
    })
}

/// `(option, description)` pairs for `command` (empty if unknown).
///
/// The name is lowercased. `command_options("")` is empty.
pub fn command_options(command: &str) -> &'static [(&'static str, &'static str)] {
    command_map()
        .get(command.to_lowercase().as_str())
        .copied()
        .unwrap_or(&[])
}

/// One-line manual description for an option name (empty if none).
///
/// Does not case-fold: `option_doc("Nsam")` has the sensitivity doc;
/// `option_doc("nsam")` is `""`.
pub fn option_doc(option: &str) -> &'static str {
    option_doc_map().get(option).copied().unwrap_or("")
}

/// Whether `command` is a catalogued Dynare command (name is lowercased).
pub fn is_known_command(command: &str) -> bool {
    command_map().contains_key(command.to_lowercase().as_str())
}

/// `dynare_list_options` payload. `None` and `Some("")` omit the command.
pub fn list_options(command: Option<&str>) -> ListOptions {
    match command {
        None | Some("") => {
            let commands = sorted_commands();
            ListOptions::Omitted {
                n_commands: commands.len(),
                commands: commands.to_vec(),
            }
        }
        Some(command) => {
            let key = command.to_lowercase();
            if let Some(options) = command_map().get(key.as_str()).copied() {
                let options: Vec<NamedOption> = options
                    .iter()
                    .map(|&(name, description)| NamedOption { name, description })
                    .collect();
                ListOptions::Known {
                    command: key,
                    known: true,
                    n_options: options.len(),
                    options,
                }
            } else {
                let suggestions = difflib::get_close_matches(
                    &key,
                    catalog_data::COMMAND_OPTIONS.iter().map(|(name, _)| *name),
                    3,
                    0.6,
                );
                let mut message =
                    format!("'{key}' is not a Dynare command in the bundled 7.1 grammar.");
                if !suggestions.is_empty() {
                    message.push_str(" Did you mean: ");
                    message.push_str(&suggestions.join(", "));
                    message.push('?');
                }
                ListOptions::Unknown {
                    command: key,
                    known: false,
                    message,
                    suggestions,
                }
            }
        }
    }
}

/// Python 3 `difflib.SequenceMatcher.ratio` + `get_close_matches` (std only).
mod difflib {
    use std::collections::HashMap;

    fn calculate_ratio(matches: usize, length: usize) -> f64 {
        if length == 0 {
            1.0
        } else {
            2.0 * matches as f64 / length as f64
        }
    }

    struct SequenceMatcher {
        a: Vec<char>,
        b: Vec<char>,
        b2j: HashMap<char, Vec<usize>>,
        matching_blocks: Option<Vec<(usize, usize, usize)>>,
        fullbcount: Option<HashMap<char, usize>>,
    }

    impl SequenceMatcher {
        fn new() -> Self {
            let mut sm = Self {
                a: Vec::new(),
                b: Vec::new(),
                b2j: HashMap::new(),
                matching_blocks: None,
                fullbcount: None,
            };
            sm.chain_b();
            sm
        }

        fn set_seq1(&mut self, a: &str) {
            self.a = a.chars().collect();
            self.matching_blocks = None;
        }

        fn set_seq2(&mut self, b: &str) {
            self.b = b.chars().collect();
            self.matching_blocks = None;
            self.fullbcount = None;
            self.chain_b();
        }

        fn chain_b(&mut self) {
            self.b2j.clear();
            for (i, &elt) in self.b.iter().enumerate() {
                self.b2j.entry(elt).or_default().push(i);
            }
            let n = self.b.len();
            if n >= 200 {
                let ntest = n / 100 + 1;
                let popular: Vec<char> = self
                    .b2j
                    .iter()
                    .filter(|(_, idxs)| idxs.len() > ntest)
                    .map(|(&elt, _)| elt)
                    .collect();
                for elt in popular {
                    self.b2j.remove(&elt);
                }
            }
        }

        fn find_longest_match(
            &self,
            alo: usize,
            ahi: usize,
            blo: usize,
            bhi: usize,
        ) -> (usize, usize, usize) {
            let mut besti = alo;
            let mut bestj = blo;
            let mut bestsize = 0usize;
            let mut j2len: HashMap<usize, usize> = HashMap::new();
            for i in alo..ahi {
                let mut newj2len: HashMap<usize, usize> = HashMap::new();
                if let Some(js) = self.b2j.get(&self.a[i]) {
                    for &j in js {
                        if j < blo {
                            continue;
                        }
                        if j >= bhi {
                            break;
                        }
                        let prev = if j == 0 {
                            0
                        } else {
                            j2len.get(&(j - 1)).copied().unwrap_or(0)
                        };
                        let k = prev + 1;
                        newj2len.insert(j, k);
                        if k > bestsize {
                            besti = i + 1 - k;
                            bestj = j + 1 - k;
                            bestsize = k;
                        }
                    }
                }
                j2len = newj2len;
            }

            while besti > alo && bestj > blo && self.a[besti - 1] == self.b[bestj - 1] {
                besti -= 1;
                bestj -= 1;
                bestsize += 1;
            }
            while besti + bestsize < ahi
                && bestj + bestsize < bhi
                && self.a[besti + bestsize] == self.b[bestj + bestsize]
            {
                bestsize += 1;
            }

            (besti, bestj, bestsize)
        }

        fn get_matching_blocks(&mut self) -> &[(usize, usize, usize)] {
            if self.matching_blocks.is_none() {
                let la = self.a.len();
                let lb = self.b.len();
                let mut queue = vec![(0usize, la, 0usize, lb)];
                let mut matching_blocks = Vec::new();
                while let Some((alo, ahi, blo, bhi)) = queue.pop() {
                    let (i, j, k) = self.find_longest_match(alo, ahi, blo, bhi);
                    if k > 0 {
                        matching_blocks.push((i, j, k));
                        if alo < i && blo < j {
                            queue.push((alo, i, blo, j));
                        }
                        if i + k < ahi && j + k < bhi {
                            queue.push((i + k, ahi, j + k, bhi));
                        }
                    }
                }
                matching_blocks.sort_unstable();

                let mut i1 = 0usize;
                let mut j1 = 0usize;
                let mut k1 = 0usize;
                let mut non_adjacent = Vec::new();
                for (i2, j2, k2) in matching_blocks {
                    if i1 + k1 == i2 && j1 + k1 == j2 {
                        k1 += k2;
                    } else {
                        if k1 > 0 {
                            non_adjacent.push((i1, j1, k1));
                        }
                        i1 = i2;
                        j1 = j2;
                        k1 = k2;
                    }
                }
                if k1 > 0 {
                    non_adjacent.push((i1, j1, k1));
                }
                non_adjacent.push((la, lb, 0));
                self.matching_blocks = Some(non_adjacent);
            }
            self.matching_blocks.as_ref().unwrap()
        }

        fn ratio(&mut self) -> f64 {
            let matches: usize = self
                .get_matching_blocks()
                .iter()
                .map(|(_, _, size)| *size)
                .sum();
            calculate_ratio(matches, self.a.len() + self.b.len())
        }

        fn quick_ratio(&mut self) -> f64 {
            if self.fullbcount.is_none() {
                let mut fullbcount = HashMap::new();
                for &elt in &self.b {
                    *fullbcount.entry(elt).or_insert(0) += 1;
                }
                self.fullbcount = Some(fullbcount);
            }
            let fullbcount = self.fullbcount.as_ref().unwrap();
            let mut avail: HashMap<char, i64> = HashMap::new();
            let mut matches = 0usize;
            for &elt in &self.a {
                let numb = match avail.get(&elt) {
                    Some(&n) => n,
                    None => fullbcount.get(&elt).copied().unwrap_or(0) as i64,
                };
                avail.insert(elt, numb - 1);
                if numb > 0 {
                    matches += 1;
                }
            }
            calculate_ratio(matches, self.a.len() + self.b.len())
        }

        fn real_quick_ratio(&self) -> f64 {
            let la = self.a.len();
            let lb = self.b.len();
            calculate_ratio(la.min(lb), la + lb)
        }
    }

    /// Python 3 `difflib.get_close_matches`.
    pub(super) fn get_close_matches<'a>(
        word: &str,
        possibilities: impl IntoIterator<Item = &'a str>,
        n: usize,
        cutoff: f64,
    ) -> Vec<&'a str> {
        let mut sm = SequenceMatcher::new();
        sm.set_seq2(word);
        let mut result: Vec<(f64, &'a str)> = Vec::new();
        for x in possibilities {
            sm.set_seq1(x);
            if sm.real_quick_ratio() >= cutoff && sm.quick_ratio() >= cutoff && sm.ratio() >= cutoff
            {
                result.push((sm.ratio(), x));
            }
        }
        // heapq.nlargest(n, result) ≡ sorted(result, reverse=True)[:n]
        // on (score, name) tuples: higher score first, then larger name.
        result.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.1.cmp(a.1))
        });
        result.truncate(n);
        result.into_iter().map(|(_, x)| x).collect()
    }

    #[cfg(test)]
    pub(super) fn ratio(a: &str, b: &str) -> f64 {
        let mut sm = SequenceMatcher::new();
        sm.set_seq1(a);
        sm.set_seq2(b);
        sm.ratio()
    }
}

#[cfg(test)]
mod tests {
    use super::difflib;

    #[test]
    fn sequence_matcher_ratio_matches_python_doctest() {
        assert_eq!(difflib::ratio("abcd", "bcde"), 0.75);
    }

    #[test]
    fn get_close_matches_matches_python_doctest() {
        assert_eq!(
            difflib::get_close_matches("appel", ["ape", "apple", "peach", "puppy"], 3, 0.6),
            vec!["apple", "ape"]
        );
    }
}
