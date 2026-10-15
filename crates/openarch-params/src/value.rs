use std::fmt;

/// A parameter value. Units are not modelled yet (Phase 0 spike); lengths are
/// plain millimetres and areas plain square metres by convention.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Num(f64),
    Text(String),
    Bool(bool),
}

impl Value {
    pub fn as_num(&self) -> Result<f64, EvalError> {
        match self {
            Value::Num(n) => Ok(*n),
            Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Value::Text(t) => Err(EvalError::Type(format!("expected a number, got text \"{t}\""))),
        }
    }

    pub fn as_bool(&self) -> Result<bool, EvalError> {
        match self {
            Value::Bool(b) => Ok(*b),
            Value::Num(n) => Ok(*n != 0.0),
            Value::Text(t) => Err(EvalError::Type(format!("expected true/false, got text \"{t}\""))),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Num(n) => write!(f, "{n}"),
            Value::Text(t) => write!(f, "{t}"),
            Value::Bool(b) => write!(f, "{b}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EvalError {
    Parse(String),
    UnknownName(String),
    Type(String),
    DivideByZero,
    BadCall(String),
    /// The path of names that form a reference cycle.
    Cycle(Vec<String>),
    MissingField(String),
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EvalError::Parse(m) => write!(f, "formula error: {m}"),
            EvalError::UnknownName(n) => write!(f, "unknown name [{n}]"),
            EvalError::Type(m) => write!(f, "type error: {m}"),
            EvalError::DivideByZero => write!(f, "divide by zero"),
            EvalError::BadCall(m) => write!(f, "bad function call: {m}"),
            EvalError::Cycle(p) => write!(f, "circular reference: {}", p.join(" -> ")),
            EvalError::MissingField(m) => write!(f, "missing value: {m}"),
        }
    }
}

impl std::error::Error for EvalError {}
