//! The tree-walking evaluator.

pub mod builtins;
pub mod console;
pub mod eval_core;
pub mod expressions;
pub mod iterator;
pub mod match_eval;
pub mod methods;
pub mod string_lit;

pub use eval_core::{eval_node, eval_program, Eval};

#[cfg(test)]
mod tests;
