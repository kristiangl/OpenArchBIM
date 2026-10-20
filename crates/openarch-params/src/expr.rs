//! Formula language: lexer, parser and evaluator.
//!
//! Syntax (Excel-like):
//!   numbers `12`, `2.5`   text `"abc"`   booleans `true`/`false`
//!   names `Width` or `[Floor Area]` or `[Rooms.Area.Sum]`
//!   operators `+ - * / % ^`, comparisons `= == != <> < <= > >=`, `and or not`
//!   functions `if round roundup rounddown min max abs sqrt floor ceil`

use crate::value::{EvalError, Value};

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Ident(String),
    LParen,
    RParen,
    Comma,
    Op(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Num(f64),
    Text(String),
    Bool(bool),
    Name(String),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

fn perr<T>(msg: impl Into<String>) -> Result<T, EvalError> {
    Err(EvalError::Parse(msg.into()))
}

fn lex(src: &str) -> Result<Vec<Tok>, EvalError> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            ',' => {
                out.push(Tok::Comma);
                i += 1;
            }
            '"' => {
                let mut s = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return perr("unterminated text"),
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some(ch) => {
                            s.push(*ch);
                            i += 1;
                        }
                    }
                }
                out.push(Tok::Str(s));
            }
            '[' => {
                let mut s = String::new();
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return perr("missing closing ]"),
                        Some(']') => {
                            i += 1;
                            break;
                        }
                        Some(ch) => {
                            s.push(*ch);
                            i += 1;
                        }
                    }
                }
                let s = s.trim().to_string();
                if s.is_empty() {
                    return perr("empty [] name");
                }
                out.push(Tok::Ident(s));
            }
            '0'..='9' | '.' => {
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                match text.parse::<f64>() {
                    Ok(n) => out.push(Tok::Num(n)),
                    Err(_) => return perr(format!("bad number '{text}'")),
                }
            }
            c if c.is_alphabetic() || c == '_' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
                {
                    i += 1;
                }
                let text: String = chars[start..i].iter().collect();
                out.push(Tok::Ident(text));
            }
            '+' => {
                out.push(Tok::Op("+"));
                i += 1;
            }
            '-' => {
                out.push(Tok::Op("-"));
                i += 1;
            }
            '*' => {
                out.push(Tok::Op("*"));
                i += 1;
            }
            '/' => {
                out.push(Tok::Op("/"));
                i += 1;
            }
            '%' => {
                out.push(Tok::Op("%"));
                i += 1;
            }
            '^' => {
                out.push(Tok::Op("^"));
                i += 1;
            }
            '=' => {
                i += 1;
                if chars.get(i) == Some(&'=') {
                    i += 1;
                }
                out.push(Tok::Op("=="));
            }
            '!' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Tok::Op("!="));
                    i += 2;
                } else {
                    out.push(Tok::Op("not"));
                    i += 1;
                }
            }
            '<' => {
                match chars.get(i + 1) {
                    Some('=') => {
                        out.push(Tok::Op("<="));
                        i += 2;
                    }
                    Some('>') => {
                        out.push(Tok::Op("!="));
                        i += 2;
                    }
                    _ => {
                        out.push(Tok::Op("<"));
                        i += 1;
                    }
                }
            }
            '>' => {
                if chars.get(i + 1) == Some(&'=') {
                    out.push(Tok::Op(">="));
                    i += 2;
                } else {
                    out.push(Tok::Op(">"));
                    i += 1;
                }
            }
            other => return perr(format!("unexpected character '{other}'")),
        }
    }
    Ok(out)
}

struct Parser {
    toks: Vec<Tok>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn peek_word(&self, w: &str) -> bool {
        matches!(self.peek(), Some(Tok::Ident(s)) if s.eq_ignore_ascii_case(w))
    }

    fn parse_or(&mut self) -> Result<Expr, EvalError> {
        let mut lhs = self.parse_and()?;
        while self.peek_word("or") {
            self.pos += 1;
            let rhs = self.parse_and()?;
            lhs = Expr::Binary(BinOp::Or, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_and(&mut self) -> Result<Expr, EvalError> {
        let mut lhs = self.parse_cmp()?;
        while self.peek_word("and") {
            self.pos += 1;
            let rhs = self.parse_cmp()?;
            lhs = Expr::Binary(BinOp::And, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_cmp(&mut self) -> Result<Expr, EvalError> {
        let mut lhs = self.parse_add()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op("==")) => BinOp::Eq,
                Some(Tok::Op("!=")) => BinOp::Ne,
                Some(Tok::Op("<")) => BinOp::Lt,
                Some(Tok::Op("<=")) => BinOp::Le,
                Some(Tok::Op(">")) => BinOp::Gt,
                Some(Tok::Op(">=")) => BinOp::Ge,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_add()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_add(&mut self) -> Result<Expr, EvalError> {
        let mut lhs = self.parse_mul()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op("+")) => BinOp::Add,
                Some(Tok::Op("-")) => BinOp::Sub,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_mul()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_mul(&mut self) -> Result<Expr, EvalError> {
        let mut lhs = self.parse_unary()?;
        loop {
            let op = match self.peek() {
                Some(Tok::Op("*")) => BinOp::Mul,
                Some(Tok::Op("/")) => BinOp::Div,
                Some(Tok::Op("%")) => BinOp::Rem,
                _ => break,
            };
            self.pos += 1;
            let rhs = self.parse_unary()?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Expr, EvalError> {
        if matches!(self.peek(), Some(Tok::Op("-"))) {
            self.pos += 1;
            let e = self.parse_unary()?;
            return Ok(Expr::Unary(UnOp::Neg, Box::new(e)));
        }
        if matches!(self.peek(), Some(Tok::Op("+"))) {
            self.pos += 1;
            return self.parse_unary();
        }
        if matches!(self.peek(), Some(Tok::Op("not"))) || self.peek_word("not") {
            self.pos += 1;
            let e = self.parse_unary()?;
            return Ok(Expr::Unary(UnOp::Not, Box::new(e)));
        }
        self.parse_pow()
    }

    fn parse_pow(&mut self) -> Result<Expr, EvalError> {
        let base = self.parse_atom()?;
        if matches!(self.peek(), Some(Tok::Op("^"))) {
            self.pos += 1;
            // right associative, and allows a negative exponent
            let exp = self.parse_unary()?;
            return Ok(Expr::Binary(BinOp::Pow, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn parse_atom(&mut self) -> Result<Expr, EvalError> {
        match self.next() {
            Some(Tok::Num(n)) => Ok(Expr::Num(n)),
            Some(Tok::Str(s)) => Ok(Expr::Text(s)),
            Some(Tok::LParen) => {
                let e = self.parse_or()?;
                match self.next() {
                    Some(Tok::RParen) => Ok(e),
                    _ => perr("missing closing )"),
                }
            }
            Some(Tok::Ident(name)) => {
                if name.eq_ignore_ascii_case("true") {
                    return Ok(Expr::Bool(true));
                }
                if name.eq_ignore_ascii_case("false") {
                    return Ok(Expr::Bool(false));
                }
                if matches!(self.peek(), Some(Tok::LParen)) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if matches!(self.peek(), Some(Tok::RParen)) {
                        self.pos += 1;
                        return Ok(Expr::Call(name.to_ascii_lowercase(), args));
                    }
                    loop {
                        args.push(self.parse_or()?);
                        match self.next() {
                            Some(Tok::Comma) => continue,
                            Some(Tok::RParen) => break,
                            _ => return perr("expected , or ) in function call"),
                        }
                    }
                    return Ok(Expr::Call(name.to_ascii_lowercase(), args));
                }
                Ok(Expr::Name(name))
            }
            Some(t) => perr(format!("unexpected {t:?}")),
            None => perr("formula ended unexpectedly"),
        }
    }
}

pub fn parse(src: &str) -> Result<Expr, EvalError> {
    let toks = lex(src)?;
    if toks.is_empty() {
        return perr("empty formula");
    }
    let mut p = Parser { toks, pos: 0 };
    let e = p.parse_or()?;
    if p.pos < p.toks.len() {
        return perr(format!("unexpected {:?} after end of formula", p.toks[p.pos]));
    }
    Ok(e)
}

/// All names a formula refers to (for dependency display and dependents lookups).
pub fn names_in(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Name(n) => out.push(n.clone()),
        Expr::Unary(_, e) => names_in(e, out),
        Expr::Binary(_, a, b) => {
            names_in(a, out);
            names_in(b, out);
        }
        // the table and column of size_lookup are names of a table and a column, not parameters
        Expr::Call(f, args) if f == "size_lookup" => args.iter().skip(2).for_each(|a| names_in(a, out)),
        Expr::Call(_, args) => args.iter().for_each(|a| names_in(a, out)),
        _ => {}
    }
}

fn round_to(x: f64, digits: f64, mode: u8) -> f64 {
    let m = 10f64.powi(digits as i32);
    let y = x * m;
    let r = match mode {
        0 => y.round(),                          // half away from zero, like Excel
        1 => y.signum() * y.abs().ceil(),        // round up (away from zero)
        _ => y.signum() * y.abs().floor(),       // round down (toward zero)
    };
    r / m
}

/// Evaluate `expr`. `resolve` supplies the value of any name.
pub fn eval(
    expr: &Expr,
    resolve: &mut dyn FnMut(&str) -> Result<Value, EvalError>,
) -> Result<Value, EvalError> {
    match expr {
        Expr::Num(n) => Ok(Value::Num(*n)),
        Expr::Text(s) => Ok(Value::Text(s.clone())),
        Expr::Bool(b) => Ok(Value::Bool(*b)),
        Expr::Name(n) => match resolve(n) {
            Err(EvalError::UnknownName(_)) if n.eq_ignore_ascii_case("pi") => {
                Ok(Value::Num(std::f64::consts::PI))
            }
            other => other,
        },
        Expr::Unary(op, e) => {
            let v = eval(e, resolve)?;
            match op {
                UnOp::Neg => Ok(Value::Num(-v.as_num()?)),
                UnOp::Not => Ok(Value::Bool(!v.as_bool()?)),
            }
        }
        Expr::Binary(op, a, b) => {
            // and/or short-circuit so an untaken side never raises an error
            if *op == BinOp::And {
                let l = eval(a, resolve)?.as_bool()?;
                if !l {
                    return Ok(Value::Bool(false));
                }
                return Ok(Value::Bool(eval(b, resolve)?.as_bool()?));
            }
            if *op == BinOp::Or {
                let l = eval(a, resolve)?.as_bool()?;
                if l {
                    return Ok(Value::Bool(true));
                }
                return Ok(Value::Bool(eval(b, resolve)?.as_bool()?));
            }
            let l = eval(a, resolve)?;
            let r = eval(b, resolve)?;
            binary(*op, l, r)
        }
        Expr::Call(name, args) => call(name, args, resolve),
    }
}

fn binary(op: BinOp, l: Value, r: Value) -> Result<Value, EvalError> {
    use BinOp::*;
    match op {
        Add => {
            // text on either side concatenates
            if matches!(l, Value::Text(_)) || matches!(r, Value::Text(_)) {
                return Ok(Value::Text(format!("{l}{r}")));
            }
            Ok(Value::Num(l.as_num()? + r.as_num()?))
        }
        Sub => Ok(Value::Num(l.as_num()? - r.as_num()?)),
        Mul => Ok(Value::Num(l.as_num()? * r.as_num()?)),
        Div => {
            let d = r.as_num()?;
            if d == 0.0 {
                return Err(EvalError::DivideByZero);
            }
            Ok(Value::Num(l.as_num()? / d))
        }
        Rem => {
            let d = r.as_num()?;
            if d == 0.0 {
                return Err(EvalError::DivideByZero);
            }
            Ok(Value::Num(l.as_num()? % d))
        }
        Pow => Ok(Value::Num(l.as_num()?.powf(r.as_num()?))),
        Eq | Ne | Lt | Le | Gt | Ge => {
            let ord = match (&l, &r) {
                (Value::Text(a), Value::Text(b)) => a.partial_cmp(b),
                (Value::Bool(a), Value::Bool(b)) => a.partial_cmp(b),
                _ => l.as_num()?.partial_cmp(&r.as_num()?),
            };
            use std::cmp::Ordering::*;
            let res = match (op, ord) {
                (Eq, Some(Equal)) => true,
                (Ne, Some(Equal)) => false,
                (Ne, _) => true,
                (Lt, Some(Less)) | (Le, Some(Less | Equal)) => true,
                (Gt, Some(Greater)) | (Ge, Some(Greater | Equal)) => true,
                _ => false,
            };
            Ok(Value::Bool(res))
        }
        And | Or => unreachable!("handled with short-circuit"),
    }
}

fn call(
    name: &str,
    args: &[Expr],
    resolve: &mut dyn FnMut(&str) -> Result<Value, EvalError>,
) -> Result<Value, EvalError> {
    let bad = |m: &str| Err(EvalError::BadCall(format!("{name}: {m}")));
    match name {
        // lazy: only the chosen branch is evaluated
        "if" => {
            if args.len() != 3 {
                return bad("expects (condition, then, else)");
            }
            if eval(&args[0], resolve)?.as_bool()? {
                eval(&args[1], resolve)
            } else {
                eval(&args[2], resolve)
            }
        }
        "round" | "roundup" | "rounddown" => {
            if args.is_empty() || args.len() > 2 {
                return bad("expects (number, digits)");
            }
            let x = eval(&args[0], resolve)?.as_num()?;
            let d = if args.len() == 2 { eval(&args[1], resolve)?.as_num()? } else { 0.0 };
            let mode = match name {
                "round" => 0,
                "roundup" => 1,
                _ => 2,
            };
            Ok(Value::Num(round_to(x, d, mode)))
        }
        "min" | "max" => {
            if args.is_empty() {
                return bad("needs at least one value");
            }
            let mut acc: Option<f64> = None;
            for a in args {
                let v = eval(a, resolve)?.as_num()?;
                acc = Some(match (acc, name) {
                    (None, _) => v,
                    (Some(p), "min") => p.min(v),
                    (Some(p), _) => p.max(v),
                });
            }
            Ok(Value::Num(acc.unwrap()))
        }
        "abs" | "sqrt" | "floor" | "ceil" => {
            if args.len() != 1 {
                return bad("expects one number");
            }
            let x = eval(&args[0], resolve)?.as_num()?;
            match name {
                "abs" => Ok(Value::Num(x.abs())),
                "floor" => Ok(Value::Num(x.floor())),
                "ceil" => Ok(Value::Num(x.ceil())),
                _ => {
                    if x < 0.0 {
                        bad("cannot take the square root of a negative number")
                    } else {
                        Ok(Value::Num(x.sqrt()))
                    }
                }
            }
        }
        "size_lookup" => {
            if args.len() < 4 {
                return bad("expects (table, column, default, value, ...)");
            }
            // the table and the column can be bare names or quoted text
            let word = |e: &Expr, resolve: &mut dyn FnMut(&str) -> Result<Value, EvalError>| -> Result<String, EvalError> {
                match e {
                    Expr::Name(n) | Expr::Text(n) => Ok(n.clone()),
                    other => Ok(eval(other, resolve)?.to_string()),
                }
            };
            let table = word(&args[0], resolve)?;
            let column = word(&args[1], resolve)?;
            let default = eval(&args[2], resolve)?;
            let mut keys = Vec::new();
            for a in &args[3..] {
                keys.push(eval(a, resolve)?);
            }
            crate::lookup::size_lookup(&table, &column, default, &keys)
        }
        other => Err(EvalError::BadCall(format!("unknown function '{other}'"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> Result<Value, EvalError> {
        let e = parse(src)?;
        eval(&e, &mut |n| Err(EvalError::UnknownName(n.to_string())))
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(run("1 + 2 * 3"), Ok(Value::Num(7.0)));
        assert_eq!(run("(1 + 2) * 3"), Ok(Value::Num(9.0)));
        assert_eq!(run("2 ^ 3 ^ 2"), Ok(Value::Num(512.0)));
        assert_eq!(run("-2 ^ 2"), Ok(Value::Num(-4.0)));
        assert_eq!(run("10 % 4"), Ok(Value::Num(2.0)));
    }

    #[test]
    fn functions_and_logic() {
        assert_eq!(run("round(2.456, 2)"), Ok(Value::Num(2.46)));
        assert_eq!(run("round(-2.5)"), Ok(Value::Num(-3.0)));
        assert_eq!(run("roundup(2.01, 1)"), Ok(Value::Num(2.1)));
        assert_eq!(run("rounddown(2.99, 1)"), Ok(Value::Num(2.9)));
        assert_eq!(run("max(1, 5, 3) - min(4, 2)"), Ok(Value::Num(3.0)));
        assert_eq!(run("if(3 > 2 and not false, \"yes\", \"no\")"), Ok(Value::Text("yes".into())));
        assert_eq!(run("pi > 3.14"), Ok(Value::Bool(true)));
    }

    #[test]
    fn if_is_lazy() {
        assert_eq!(run("if(true, 1, [Nope])"), Ok(Value::Num(1.0)));
        assert_eq!(run("false and [Nope]"), Ok(Value::Bool(false)));
    }

    #[test]
    fn errors() {
        assert_eq!(run("1 / 0"), Err(EvalError::DivideByZero));
        assert!(matches!(run("1 +"), Err(EvalError::Parse(_))));
        assert!(matches!(run("(1"), Err(EvalError::Parse(_))));
        assert!(matches!(run("sqrt(-1)"), Err(EvalError::BadCall(_))));
        assert_eq!(run("[Floor Area] * 2"), Err(EvalError::UnknownName("Floor Area".into())));
    }

    #[test]
    fn text_concat_and_compare() {
        assert_eq!(run("\"Lvl \" + 3"), Ok(Value::Text("Lvl 3".into())));
        assert_eq!(run("\"a\" == \"a\""), Ok(Value::Bool(true)));
        assert_eq!(run("2 <> 3"), Ok(Value::Bool(true)));
    }

    #[test]
    fn names_collected() {
        let e = parse("[Wall Height] * Length + if(x > 1, y, 0)").unwrap();
        let mut n = vec![];
        names_in(&e, &mut n);
        assert_eq!(n, vec!["Wall Height", "Length", "x", "y"]);
    }
}
