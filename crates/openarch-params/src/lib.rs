//! Parameter, formula and schedule engine for OpenArch BIM.
//!
//! The same evaluator will drive family parameters, project variables and
//! schedule calculated columns, so a schedule total can be used anywhere.

pub mod expr;
pub mod model;
pub mod value;

pub use model::{Agg, Column, ColumnKind, Evaluation, Param, Project, Row, Schedule};
pub use value::{EvalError, Value};
