//! Standalone expression evaluator for SRC script expressions.
//!
//! This module provides arithmetic expression evaluation with proper operator
//! precedence, type coercion, and a function registry. It is designed to
//! eventually replace the expression evaluation currently embedded in
//! `event_runtime.rs`.

mod eval;
pub mod functions;
mod operator;
mod value_type;

pub use eval::{eval, eval_float, eval_int};
pub use functions::{FunctionRegistry, IFunction};
pub use operator::OperatorType;
pub use value_type::ValueType;
