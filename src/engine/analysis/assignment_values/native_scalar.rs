//! A bounded scalar proof for retained native RHS text, not a Dynare parse.

use std::collections::{HashMap, HashSet};

use crate::engine::language::intern::Name;
use crate::engine::language::model::Model;

pub(super) fn scalar_value(
    text: &str,
    model: &Model,
    known: &HashMap<Name, f64>,
    shadows: &HashSet<Name>,
) -> Option<(f64, bool)> {
    if text.len() > 16_384 {
        return None;
    }
    let mut reader = ScalarReader {
        text,
        position: 0,
        model,
        known,
        shadows,
        plain: true,
    };
    let value = reader.expression(0, 0)?;
    reader.trivia()?;
    if reader.peek() == Some(b';') {
        reader.position += 1;
        reader.trivia()?;
    }
    (reader.position == text.len()).then_some((value, reader.plain))
}

struct ScalarReader<'a> {
    text: &'a str,
    position: usize,
    model: &'a Model,
    known: &'a HashMap<Name, f64>,
    shadows: &'a HashSet<Name>,
    plain: bool,
}

impl ScalarReader<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.position).copied()
    }

    fn trivia(&mut self) -> Option<()> {
        loop {
            while self.peek().is_some_and(|byte| byte.is_ascii_whitespace()) {
                self.position += 1;
            }
            let tail = &self.text[self.position..];
            if tail.starts_with('%') || tail.starts_with("//") {
                self.position += tail.find('\n').unwrap_or(tail.len());
            } else if tail.starts_with("/*") {
                self.position += tail.find("*/")? + 2;
            } else {
                return Some(());
            }
        }
    }

    fn expression(&mut self, min_bp: u8, depth: usize) -> Option<f64> {
        if depth >= 128 {
            return None;
        }
        self.trivia()?;
        let mut lhs = match self.peek()? {
            sign @ (b'+' | b'-') => {
                self.position += 1;
                self.trivia()?;
                if matches!(self.peek(), Some(b'+' | b'-')) {
                    self.plain = false;
                }
                // MATLAB binds a signed exponent before the next power, while
                // an ordinary unary sign binds after power: -2^2, 2^-3^2.
                let unary_bp = if min_bp == 10 { 11 } else { 7 };
                let value = self.expression(unary_bp, depth + 1)?;
                if sign == b'-' {
                    -value
                } else {
                    value
                }
            }
            b'(' => {
                self.plain = false;
                self.position += 1;
                let value = self.expression(0, depth + 1)?;
                self.trivia()?;
                if self.peek()? != b')' {
                    return None;
                }
                self.position += 1;
                value
            }
            b'0'..=b'9' | b'.' => self.number()?,
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                self.plain = false;
                let start = self.position;
                while self
                    .peek()
                    .is_some_and(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                {
                    self.position += 1;
                }
                let word = &self.text[start..self.position];
                let name = self.model.intern.lookup(word);
                self.trivia()?;
                if self.peek() == Some(b'(') {
                    if name.is_some_and(|name| {
                        self.shadows.contains(&name) || self.known.contains_key(&name)
                    }) {
                        return None;
                    }
                    self.position += 1;
                    let arg = self.expression(0, depth + 1)?;
                    self.trivia()?;
                    if self.peek()? != b')' {
                        return None;
                    }
                    self.position += 1;
                    match word {
                        "exp" => arg.exp(),
                        "log" => arg.ln(),
                        "sqrt" => arg.sqrt(),
                        "abs" => arg.abs(),
                        "sin" => arg.sin(),
                        "cos" => arg.cos(),
                        _ => return None,
                    }
                } else {
                    *self.known.get(&name?)?
                }
            }
            _ => return None,
        };
        if !lhs.is_finite() {
            return None;
        }
        loop {
            self.trivia()?;
            let (operator, left_bp, right_bp) = match self.peek() {
                Some(operator @ (b'+' | b'-')) => (operator, 1, 2),
                Some(operator @ (b'*' | b'/')) => (operator, 3, 4),
                Some(b'^') => (b'^', 9, 10),
                _ => break,
            };
            if left_bp < min_bp {
                break;
            }
            self.plain = false;
            self.position += 1;
            let rhs = self.expression(right_bp, depth + 1)?;
            lhs = match operator {
                b'+' => lhs + rhs,
                b'-' => lhs - rhs,
                b'*' => lhs * rhs,
                b'/' => lhs / rhs,
                b'^' => lhs.powf(rhs),
                _ => unreachable!(),
            };
            if !lhs.is_finite() {
                return None;
            }
        }
        Some(lhs)
    }

    fn number(&mut self) -> Option<f64> {
        let start = self.position;
        while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
            self.position += 1;
        }
        if self.peek() == Some(b'.')
            && !matches!(
                self.text.as_bytes().get(self.position + 1),
                Some(b'*' | b'/' | b'^' | b'\\')
            )
        {
            self.position += 1;
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.position += 1;
            }
            while self.peek().is_some_and(|byte| byte.is_ascii_digit()) {
                self.position += 1;
            }
        }
        self.text[start..self.position]
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
    }
}
