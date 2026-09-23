//! Static integer folding for namespace lag arguments. Dynare's parse tree
//! simplifies arithmetic before checking whether a lag is an integer.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LagFold {
    Integer(i32),
    NonInteger,
    Unresolved,
}

#[derive(Clone, Debug)]
struct Affine {
    constant: f64,
    names: HashMap<String, f64>,
}

impl Affine {
    fn constant(value: f64) -> Self {
        Self {
            constant: value,
            names: HashMap::new(),
        }
    }

    fn name(name: String) -> Self {
        Self {
            constant: 0.0,
            names: HashMap::from([(name, 1.0)]),
        }
    }

    fn scale(mut self, factor: f64) -> Self {
        self.constant *= factor;
        for value in self.names.values_mut() {
            *value *= factor;
        }
        self.names.retain(|_, value| *value != 0.0);
        self
    }

    fn add(mut self, other: Self) -> Self {
        self.constant += other.constant;
        for (name, value) in other.names {
            *self.names.entry(name).or_default() += value;
        }
        self.names.retain(|_, value| *value != 0.0);
        self
    }
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            at: 0,
        }
    }

    fn peek(&mut self) -> Option<char> {
        while self.chars.get(self.at).is_some_and(|ch| ch.is_whitespace()) {
            self.at += 1;
        }
        self.chars.get(self.at).copied()
    }

    fn eat(&mut self, ch: char) -> bool {
        if self.peek() == Some(ch) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expr(&mut self) -> Option<Affine> {
        let mut lhs = self.term()?;
        loop {
            if self.eat('+') {
                lhs = lhs.add(self.term()?);
            } else if self.eat('-') {
                lhs = lhs.add(self.term()?.scale(-1.0));
            } else {
                return Some(lhs);
            }
        }
    }

    fn term(&mut self) -> Option<Affine> {
        let mut lhs = self.factor()?;
        loop {
            if self.eat('*') {
                let rhs = self.factor()?;
                lhs = if lhs.names.is_empty() {
                    rhs.scale(lhs.constant)
                } else if rhs.names.is_empty() {
                    lhs.scale(rhs.constant)
                } else {
                    return None;
                };
            } else if self.eat('/') {
                let rhs = self.factor()?;
                if !rhs.names.is_empty() || rhs.constant == 0.0 {
                    return None;
                }
                lhs = lhs.scale(1.0 / rhs.constant);
            } else {
                return Some(lhs);
            }
        }
    }

    fn factor(&mut self) -> Option<Affine> {
        if self.eat('+') {
            return self.factor();
        }
        if self.eat('-') {
            return Some(self.factor()?.scale(-1.0));
        }
        if self.eat('(') {
            let value = self.expr()?;
            return self.eat(')').then_some(value);
        }
        let ch = self.peek()?;
        let start = self.at;
        match ch {
            ch if ch.is_ascii_digit()
                || (ch == '.'
                    && self
                        .chars
                        .get(self.at + 1)
                        .is_some_and(|next| next.is_ascii_digit())) =>
            {
                self.at += 1;
                while self
                    .chars
                    .get(self.at)
                    .is_some_and(|ch| ch.is_ascii_digit() || *ch == '.')
                {
                    self.at += 1;
                }
                let text: String = self.chars[start..self.at].iter().collect();
                Some(Affine::constant(text.parse::<f64>().ok()?))
            }
            ch if ch.is_alphabetic() || ch == '_' => {
                self.at += 1;
                while self
                    .chars
                    .get(self.at)
                    .is_some_and(|ch| ch.is_alphanumeric() || *ch == '_')
                {
                    self.at += 1;
                }
                Some(Affine::name(self.chars[start..self.at].iter().collect()))
            }
            _ => None,
        }
    }
}

pub(crate) fn fold_lag(text: &str) -> LagFold {
    let mut parser = Parser::new(text);
    let Some(value) = parser.expr() else {
        return if simple_symbol_product(text) {
            LagFold::NonInteger
        } else {
            LagFold::Unresolved
        };
    };
    if parser.peek().is_some() {
        return LagFold::Unresolved;
    }
    if !value.names.is_empty() {
        return LagFold::NonInteger;
    }
    if value.constant.is_finite()
        && value.constant.fract() == 0.0
        && value.constant >= i32::MIN as f64
        && value.constant <= i32::MAX as f64
    {
        LagFold::Integer(value.constant as i32)
    } else {
        LagFold::NonInteger
    }
}

/// The affine parser leaves products of two symbolic names unresolved. A bare
/// product cannot fold to an integer; larger expressions may cancel, so stay
/// silent on those until the tree can prove a value.
fn simple_symbol_product(text: &str) -> bool {
    let mut parts = text.split('*');
    let Some(left) = parts.next() else {
        return false;
    };
    let Some(right) = parts.next() else {
        return false;
    };
    parts.next().is_none() && plain_name(left.trim()) && plain_name(right.trim())
}

fn plain_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|ch| ch.is_alphabetic() || ch == '_')
        && chars.all(|ch| ch.is_alphanumeric() || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_only_proven_integer_lags() {
        for (source, expected) in [
            ("-1", LagFold::Integer(-1)),
            ("1+0", LagFold::Integer(1)),
            ("0-1", LagFold::Integer(-1)),
            ("p-p", LagFold::Integer(0)),
            ("2*(p-p)", LagFold::Integer(0)),
            ("p+1", LagFold::NonInteger),
            ("p", LagFold::NonInteger),
            ("1.5", LagFold::NonInteger),
            (".5", LagFold::NonInteger),
            ("p*p", LagFold::NonInteger),
            ("p*p-p*p", LagFold::Unresolved),
        ] {
            assert_eq!(fold_lag(source), expected, "{source}");
        }
    }
}
