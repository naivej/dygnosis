//! Written language parsing, macro execution, and retained model facts.

pub(crate) mod command_skip;
pub(crate) mod constructor;
pub mod expand;
pub mod expr;
pub mod intern;
pub mod lexer;
pub mod macro_expand;
pub(crate) mod macro_expr;
pub mod model;
pub(crate) mod native_line;
pub mod parser;
pub(crate) mod pinned_options;
pub(crate) mod shape_gate;
pub mod span;
