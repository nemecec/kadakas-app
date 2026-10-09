//! Formula evaluation. The caller gives an [`Env`] that looks up cells, names and properties.

use std::cmp::Ordering;

use crate::formula::{BinOp, CellRef, Expr, UnOp};
use crate::value::{ErrorKind, Value};

/// Where an expression finds its data.
pub trait Env {
    /// The value of one cell.
    fn cell(&self, at: CellRef) -> Value;
    /// The values of the cells in a block, row by row. Empty cells may be left out.
    fn range(&self, from: CellRef, to: CellRef) -> Vec<Value>;
    /// A property name such as `Area`. `Error(Name)` for an unknown name.
    fn name(&self, name: &str) -> Value;
    /// A property path such as `Soil.Depth`.
    fn path(&self, path: &[String]) -> Value;
    /// True if the current object has a record of this name.
    fn has_record(&self, name: &str) -> bool;
    /// In a cell, an empty value counts as 0 or empty text when it is compared. For objects it
    /// does not: a property that an object does not have is never "less than 200".
    fn blank_is_zero(&self) -> bool {
        true
    }
}

/// The names of the functions, for help texts.
pub const FUNCTIONS: &[(&str, &str)] = &[
    ("SUM", "SUM(values...): adds the numbers"),
    ("AVERAGE", "AVERAGE(values...): the mean of the numbers"),
    ("MIN", "MIN(values...): the smallest number"),
    ("MAX", "MAX(values...): the largest number"),
    ("COUNT", "COUNT(values...): how many numbers"),
    ("COUNTA", "COUNTA(values...): how many values are not empty"),
    ("IF", "IF(test, then, else): chooses a value"),
    ("IFERROR", "IFERROR(value, other): other if value is an error"),
    ("ROUND", "ROUND(number, digits): rounds to a number of decimals"),
    ("ABS", "ABS(number): the size of a number"),
    ("CONCAT", "CONCAT(values...): joins text"),
    ("AND", "AND(tests...): true if all are true"),
    ("OR", "OR(tests...): true if one is true"),
    ("NOT", "NOT(test): reverses a test"),
    ("LEN", "LEN(text): the number of characters"),
    ("UPPER", "UPPER(text): upper case"),
    ("LOWER", "LOWER(text): lower case"),
    ("FIELD", "FIELD(record, field): a record field of the object (database rows only)"),
    ("HASRECORD", "HASRECORD(record): true if the object has the record (database rows only)"),
];

fn err(e: ErrorKind) -> Value {
    Value::Error(e)
}

/// Evaluates an expression. Errors are values: the first error in an operation is the result.
pub fn eval(e: &Expr, env: &dyn Env) -> Value {
    match e {
        Expr::Num(n) => Value::num(*n),
        Expr::Str(s) => Value::Text(s.clone()),
        Expr::Bool(b) => Value::Bool(*b),
        Expr::Cell(c) => env.cell(*c),
        Expr::Range(..) => err(ErrorKind::Value),
        Expr::Name(n) => env.name(n),
        Expr::Path(p) => env.path(p),
        Expr::Unary(UnOp::Neg, x) => match eval(x, env).to_number() {
            Ok(n) => Value::num(-n),
            Err(k) => err(k),
        },
        Expr::Unary(UnOp::Percent, x) => match eval(x, env).to_number() {
            Ok(n) => Value::num(n / 100.0),
            Err(k) => err(k),
        },
        Expr::Unary(UnOp::Not, x) => match eval(x, env).to_bool() {
            Ok(b) => Value::Bool(!b),
            Err(k) => err(k),
        },
        Expr::Binary(op, l, r) => binary(*op, l, r, env),
        Expr::Call(name, args) => call(name, args, env),
    }
}

fn binary(op: BinOp, l: &Expr, r: &Expr, env: &dyn Env) -> Value {
    // AND and OR stop at the first answer, so `Count > 0 OR 1/0` is fine.
    if matches!(op, BinOp::And | BinOp::Or) {
        let a = match eval(l, env).to_bool() {
            Ok(a) => a,
            Err(k) => return err(k),
        };
        if a == (op == BinOp::Or) {
            return Value::Bool(a);
        }
        return match eval(r, env).to_bool() {
            Ok(b) => Value::Bool(b),
            Err(k) => err(k),
        };
    }
    let (a, b) = (eval(l, env), eval(r, env));
    if let Value::Error(k) = a {
        return err(k);
    }
    if let Value::Error(k) = b {
        return err(k);
    }
    match op {
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            if !env.blank_is_zero() && (a.is_empty() || b.is_empty()) {
                let both = a.is_empty() && b.is_empty();
                return Value::Bool(match op {
                    BinOp::Eq => both,
                    BinOp::Ne => !both,
                    BinOp::Le | BinOp::Ge => both,
                    _ => false,
                });
            }
            let o = a.compare(&b);
            Value::Bool(match op {
                BinOp::Eq => o == Ordering::Equal,
                BinOp::Ne => o != Ordering::Equal,
                BinOp::Lt => o == Ordering::Less,
                BinOp::Le => o != Ordering::Greater,
                BinOp::Gt => o == Ordering::Greater,
                _ => o != Ordering::Less,
            })
        }
        BinOp::Concat => Value::Text(a.to_text() + &b.to_text()),
        _ => {
            let (x, y) = match (a.to_number(), b.to_number()) {
                (Ok(x), Ok(y)) => (x, y),
                (Err(k), _) | (_, Err(k)) => return err(k),
            };
            match op {
                BinOp::Add => Value::num(x + y),
                BinOp::Sub => Value::num(x - y),
                BinOp::Mul => Value::num(x * y),
                BinOp::Div if y == 0.0 => err(ErrorKind::Div0),
                BinOp::Div => Value::num(x / y),
                _ => Value::num(x.powf(y)),
            }
        }
    }
}

/// Evaluates the arguments of a function. A range gives all of its values.
fn flatten(args: &[Expr], env: &dyn Env) -> Vec<Value> {
    let mut out = Vec::new();
    for a in args {
        match a {
            Expr::Range(from, to) => out.extend(env.range(*from, *to)),
            other => out.push(eval(other, env)),
        }
    }
    out
}

/// The numbers among the values. The first error is returned as the error.
fn numbers(vals: &[Value]) -> Result<Vec<f64>, ErrorKind> {
    let mut out = Vec::new();
    for v in vals {
        match v {
            Value::Number(n) => out.push(*n),
            Value::Error(k) => return Err(*k),
            _ => {}
        }
    }
    Ok(out)
}

fn arity(args: &[Expr], min: usize, max: usize) -> Result<(), Value> {
    if args.len() < min || args.len() > max { Err(err(ErrorKind::Value)) } else { Ok(()) }
}

fn call(name: &str, args: &[Expr], env: &dyn Env) -> Value {
    match name {
        "IF" => {
            if let Err(v) = arity(args, 2, 3) {
                return v;
            }
            match eval(&args[0], env).to_bool() {
                Ok(true) => eval(&args[1], env),
                Ok(false) => args.get(2).map_or(Value::Bool(false), |e| eval(e, env)),
                Err(k) => err(k),
            }
        }
        "IFERROR" => {
            if let Err(v) = arity(args, 2, 2) {
                return v;
            }
            match eval(&args[0], env) {
                Value::Error(_) => eval(&args[1], env),
                v => v,
            }
        }
        "SUM" | "AVERAGE" | "MIN" | "MAX" | "COUNT" | "COUNTA" => {
            let vals = flatten(args, env);
            if name == "COUNTA" {
                return Value::Number(vals.iter().filter(|v| !v.is_empty()).count() as f64);
            }
            let nums = match numbers(&vals) {
                Ok(n) => n,
                Err(k) => return err(k),
            };
            match name {
                "SUM" => Value::num(nums.iter().sum()),
                "COUNT" => Value::Number(nums.len() as f64),
                "AVERAGE" if nums.is_empty() => err(ErrorKind::Div0),
                "AVERAGE" => Value::num(nums.iter().sum::<f64>() / nums.len() as f64),
                "MIN" => Value::num(nums.iter().copied().fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.min(x)))).unwrap_or(0.0)),
                _ => Value::num(nums.iter().copied().fold(None, |m: Option<f64>, x| Some(m.map_or(x, |m| m.max(x)))).unwrap_or(0.0)),
            }
        }
        "ROUND" => {
            if let Err(v) = arity(args, 1, 2) {
                return v;
            }
            let x = match eval(&args[0], env).to_number() {
                Ok(x) => x,
                Err(k) => return err(k),
            };
            let digits = match args.get(1).map_or(Ok(0.0), |e| eval(e, env).to_number()) {
                Ok(d) => d.trunc().clamp(-15.0, 15.0) as i32,
                Err(k) => return err(k),
            };
            let f = 10f64.powi(digits);
            // 1.005 * 100 is 100.49999999999999. A few units of rounding error are forgiven, so
            // that ROUND(1.005, 2) is 1.01 as people expect.
            let scaled = x * f;
            let nudged = scaled + scaled.abs() * 4.0 * f64::EPSILON * scaled.signum();
            Value::num(nudged.round() / f)
        }
        "ABS" => {
            if let Err(v) = arity(args, 1, 1) {
                return v;
            }
            match eval(&args[0], env).to_number() {
                Ok(x) => Value::num(x.abs()),
                Err(k) => err(k),
            }
        }
        "CONCAT" => {
            let vals = flatten(args, env);
            if let Some(Value::Error(k)) = vals.iter().find(|v| v.is_error()) {
                return err(*k);
            }
            Value::Text(vals.iter().map(Value::to_text).collect())
        }
        "AND" | "OR" => {
            let vals = flatten(args, env);
            if vals.is_empty() {
                return err(ErrorKind::Value);
            }
            let mut result = name == "AND";
            for v in vals.iter().filter(|v| !v.is_empty()) {
                match v.to_bool() {
                    Ok(b) if name == "AND" => result &= b,
                    Ok(b) => result |= b,
                    Err(k) => return err(k),
                }
            }
            Value::Bool(result)
        }
        "NOT" => {
            if let Err(v) = arity(args, 1, 1) {
                return v;
            }
            match eval(&args[0], env).to_bool() {
                Ok(b) => Value::Bool(!b),
                Err(k) => err(k),
            }
        }
        "LEN" | "UPPER" | "LOWER" => {
            if let Err(v) = arity(args, 1, 1) {
                return v;
            }
            let v = eval(&args[0], env);
            if let Value::Error(k) = v {
                return err(k);
            }
            let t = v.to_text();
            match name {
                "LEN" => Value::Number(t.chars().count() as f64),
                "UPPER" => Value::Text(t.to_uppercase()),
                _ => Value::Text(t.to_lowercase()),
            }
        }
        "FIELD" => {
            if let Err(v) = arity(args, 2, 2) {
                return v;
            }
            let (a, b) = (eval(&args[0], env), eval(&args[1], env));
            if let Value::Error(k) = a {
                return err(k);
            }
            if let Value::Error(k) = b {
                return err(k);
            }
            env.path(&[a.to_text(), b.to_text()])
        }
        "HASRECORD" => {
            if let Err(v) = arity(args, 1, 1) {
                return v;
            }
            match eval(&args[0], env) {
                Value::Error(k) => err(k),
                v => Value::Bool(env.has_record(&v.to_text())),
            }
        }
        _ => err(ErrorKind::Name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formula::parse;
    use std::collections::HashMap;

    #[derive(Default)]
    struct TestEnv {
        cells: HashMap<(u32, u32), Value>,
    }

    impl Env for TestEnv {
        fn cell(&self, at: CellRef) -> Value {
            self.cells.get(&(at.row, at.col)).cloned().unwrap_or_default()
        }
        fn range(&self, from: CellRef, to: CellRef) -> Vec<Value> {
            let mut out = vec![];
            for r in from.row..=to.row {
                for c in from.col..=to.col {
                    out.push(self.cell(CellRef { row: r, col: c }));
                }
            }
            out
        }
        fn name(&self, name: &str) -> Value {
            match name {
                "Area" => Value::Number(12.5),
                "Kind" => Value::Text("plant".into()),
                "Gone" => Value::Empty,
                _ => Value::Error(ErrorKind::Name),
            }
        }
        fn path(&self, path: &[String]) -> Value {
            Value::Text(path.join("/"))
        }
        fn has_record(&self, name: &str) -> bool {
            name == "Soil"
        }
    }

    fn env() -> TestEnv {
        let mut e = TestEnv::default();
        e.cells.insert((1, 1), Value::Number(1.0));
        e.cells.insert((2, 1), Value::Number(2.0));
        e.cells.insert((3, 1), Value::Text("x".into()));
        e.cells.insert((1, 2), Value::Number(10.0));
        e.cells.insert((2, 2), Value::Error(ErrorKind::Ref));
        e
    }

    fn ev(s: &str) -> Value {
        eval(&parse(s).unwrap(), &env())
    }

    fn n(x: f64) -> Value {
        Value::Number(x)
    }

    fn t(s: &str) -> Value {
        Value::Text(s.into())
    }

    #[test]
    fn percent_and_negative_zero() {
        assert_eq!(ev("50%"), n(0.5));
        assert_eq!(ev("200 * 15%"), n(30.0));
        assert_eq!(ev("-50%"), n(-0.5));
        assert_eq!(ev("5%%"), n(0.0005));
        assert_eq!(ev("\"x\"%"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("B2%"), Value::Error(ErrorKind::Ref));
        // Negative zero is zero everywhere.
        assert_eq!(ev("SUM(C1:C3) = 0"), Value::Bool(true));
        assert_eq!(ev("-0 = 0"), Value::Bool(true));
        assert_eq!(ev("0 * -1 = 0"), Value::Bool(true));
        assert_eq!(ev("SUM(-0, -0) = 0"), Value::Bool(true));
        assert_eq!(ev("-0"), n(0.0));
        assert_eq!(ev("ROUND(-0.001, 2) = 0"), Value::Bool(true));
        assert_eq!(ev("CONCAT(-0)"), t("0"));
    }

    #[test]
    fn arithmetic() {
        assert_eq!(ev("1+2*3"), n(7.0));
        assert_eq!(ev("(1+2)*3"), n(9.0));
        assert_eq!(ev("2^10"), n(1024.0));
        assert_eq!(ev("-2^2"), n(4.0));
        assert_eq!(ev("10/4"), n(2.5));
        assert_eq!(ev("1/0"), Value::Error(ErrorKind::Div0));
        assert_eq!(ev("(-8)^0.5"), Value::Error(ErrorKind::Num));
        assert_eq!(ev("10^400"), Value::Error(ErrorKind::Num));
        assert_eq!(ev("A1+A2"), n(3.0));
        assert_eq!(ev("A1+A3"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("A1+B2"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("B2+A3"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("Z9+1"), n(1.0));
        assert_eq!(ev("\"3\"+1"), n(4.0));
        assert_eq!(ev("TRUE+1"), n(2.0));
        assert_eq!(ev("-B2"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("-A1"), n(-1.0));
        assert_eq!(ev("A1:A2"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("1e308*10"), Value::Error(ErrorKind::Num));
    }

    #[test]
    fn comparison_and_logic() {
        assert_eq!(ev("1<2"), Value::Bool(true));
        assert_eq!(ev("2<=2"), Value::Bool(true));
        assert_eq!(ev("2>2"), Value::Bool(false));
        assert_eq!(ev("2>=3"), Value::Bool(false));
        assert_eq!(ev("1<>1"), Value::Bool(false));
        assert_eq!(ev("\"a\"=\"A\""), Value::Bool(true));
        assert_eq!(ev("Kind = 'PLANT'"), Value::Bool(true));
        assert_eq!(ev("Gone = 0"), Value::Bool(true));
        assert_eq!(ev("Gone > 0"), Value::Bool(false));
        assert_eq!(ev("1 < \"a\""), Value::Bool(true));
        assert_eq!(ev("B2 = 1"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("1 = B2"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("1<2 and 2<3"), Value::Bool(true));
        assert_eq!(ev("1>2 or 2<3"), Value::Bool(true));
        assert_eq!(ev("1>2 or 2>3"), Value::Bool(false));
        assert_eq!(ev("1<2 and 3<2"), Value::Bool(false));
        assert_eq!(ev("not 1<2"), Value::Bool(false));
        assert_eq!(ev("1<2 or 1/0"), Value::Bool(true));
        assert_eq!(ev("1>2 and 1/0"), Value::Bool(false));
        assert_eq!(ev("1>2 or 1/0"), Value::Error(ErrorKind::Div0));
        assert_eq!(ev("B2 or 1"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("not \"x\""), Value::Error(ErrorKind::Value));
        assert_eq!(ev("1 & 2 & \"a\""), t("12a"));
        assert_eq!(ev("A1 & B2"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("B2 & A1"), Value::Error(ErrorKind::Ref));
    }

    #[test]
    fn functions() {
        assert_eq!(ev("SUM(A1:A3)"), n(3.0));
        assert_eq!(ev("SUM(A1:B1, 5)"), n(16.0));
        assert_eq!(ev("SUM(A1:B2)"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("SUM()"), n(0.0));
        assert_eq!(ev("AVERAGE(A1:A3)"), n(1.5));
        assert_eq!(ev("AVERAGE(C1:C3)"), Value::Error(ErrorKind::Div0));
        assert_eq!(ev("MIN(A1:B1)"), n(1.0));
        assert_eq!(ev("MAX(A1:B1)"), n(10.0));
        assert_eq!(ev("MIN(C1:C2)"), n(0.0));
        assert_eq!(ev("MAX(C1:C2)"), n(0.0));
        assert_eq!(ev("COUNT(A1:A3)"), n(2.0));
        assert_eq!(ev("COUNTA(A1:A4)"), n(3.0));
        assert_eq!(ev("IF(1<2, \"y\", \"n\")"), t("y"));
        assert_eq!(ev("IF(1>2, \"y\", \"n\")"), t("n"));
        assert_eq!(ev("IF(1>2, \"y\")"), Value::Bool(false));
        assert_eq!(ev("IF(1<2, 5, 1/0)"), n(5.0));
        assert_eq!(ev("IF(\"x\", 1, 2)"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("IF(1)"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("IFERROR(1/0, 7)"), n(7.0));
        assert_eq!(ev("IFERROR(3, 7)"), n(3.0));
        assert_eq!(ev("IFERROR(3)"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("ROUND(2.567, 2)"), n(2.57));
        assert_eq!(ev("ROUND(2.5)"), n(3.0));
        assert_eq!(ev("ROUND(-2.5)"), n(-3.0));
        assert_eq!(ev("ROUND(1.005, 2)"), n(1.01));
        assert_eq!(ev("ROUND(-1.005, 2)"), n(-1.01));
        assert_eq!(ev("ROUND(2.675, 2)"), n(2.68));
        assert_eq!(ev("ROUND(0.285, 2)"), n(0.29));
        assert_eq!(ev("ROUND(1.004, 2)"), n(1.0));
        assert_eq!(ev("ROUND(0, 2)"), n(0.0));
        assert_eq!(ev("ROUND(1234, -2)"), n(1200.0));
        assert_eq!(ev("ROUND(\"x\")"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("ROUND(1, \"x\")"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("ROUND()"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("ABS(-4)"), n(4.0));
        assert_eq!(ev("ABS(\"x\")"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("CONCAT(\"a\", 1, A1:A2)"), t("a112"));
        assert_eq!(ev("CONCAT(B2)"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("AND(1, 1)"), Value::Bool(true));
        assert_eq!(ev("AND(1, 0)"), Value::Bool(false));
        assert_eq!(ev("OR(0, 0, 1)"), Value::Bool(true));
        assert_eq!(ev("OR(0, C1)"), Value::Bool(false));
        assert_eq!(ev("AND()"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("AND(\"x\")"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("NOT(0)"), Value::Bool(true));
        assert_eq!(ev("NOT(B2)"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("LEN(\"abc\")"), n(3.0));
        assert_eq!(ev("UPPER(\"abc\")"), t("ABC"));
        assert_eq!(ev("LOWER(\"ABC\")"), t("abc"));
        assert_eq!(ev("LEN(B2)"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("LEN()"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("NOPE(1)"), Value::Error(ErrorKind::Name));
        assert_eq!(ev("Nope"), Value::Error(ErrorKind::Name));
        assert_eq!(ev("Area * 2"), n(25.0));
        assert_eq!(ev("Soil.Depth"), t("Soil/Depth"));
        assert_eq!(ev("FIELD(\"Soil\", \"Depth\")"), t("Soil/Depth"));
        assert_eq!(ev("FIELD(B2, \"Depth\")"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("FIELD(\"Soil\", B2)"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("FIELD(\"Soil\")"), Value::Error(ErrorKind::Value));
        assert_eq!(ev("HASRECORD(\"Soil\")"), Value::Bool(true));
        assert_eq!(ev("HASRECORD(\"Other\")"), Value::Bool(false));
        assert_eq!(ev("HASRECORD(B2)"), Value::Error(ErrorKind::Ref));
        assert_eq!(ev("HASRECORD()"), Value::Error(ErrorKind::Value));
    }

    struct StrictEnv;

    impl Env for StrictEnv {
        fn cell(&self, _: CellRef) -> Value {
            Value::Empty
        }
        fn range(&self, _: CellRef, _: CellRef) -> Vec<Value> {
            vec![]
        }
        fn name(&self, _: &str) -> Value {
            Value::Empty
        }
        fn path(&self, _: &[String]) -> Value {
            Value::Empty
        }
        fn has_record(&self, _: &str) -> bool {
            false
        }
        fn blank_is_zero(&self) -> bool {
            false
        }
    }

    #[test]
    fn missing_properties_do_not_compare_as_zero() {
        let q = |s: &str| eval(&parse(s).unwrap(), &StrictEnv);
        assert_eq!(q("Area <= 200"), Value::Bool(false));
        assert_eq!(q("Area < 200"), Value::Bool(false));
        assert_eq!(q("Area >= 0"), Value::Bool(false));
        assert_eq!(q("Area > 0"), Value::Bool(false));
        assert_eq!(q("Area = 0"), Value::Bool(false));
        assert_eq!(q("Area <> 0"), Value::Bool(true));
        assert_eq!(q("Area = Area"), Value::Bool(true));
        assert_eq!(q("Area <= Area"), Value::Bool(true));
        assert_eq!(q("Area <> Area"), Value::Bool(false));
        assert_eq!(q("1 < 2"), Value::Bool(true));
    }

    #[test]
    fn every_listed_function_exists() {
        for (name, _) in FUNCTIONS {
            let v = ev(&format!("{name}(1, 2, 3, 4)"));
            assert_ne!(v, Value::Error(ErrorKind::Name), "{name} is listed but not known");
        }
    }

    #[test]
    fn manual_expressions_with_bad_numbers_do_not_panic() {
        let e = Expr::Num(f64::NAN);
        assert_eq!(eval(&e, &env()), Value::Error(ErrorKind::Num));
    }
}
