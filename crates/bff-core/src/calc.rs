//! Bounded calculation operations. A declared input table and a formula over it: sums, ratios, growth, and
//! scenario arithmetic, with units carried through. Nothing here runs generated code. A claim is checked by
//! recomputing it and comparing both the value and the unit, so a figure labelled with the wrong unit
//! fails even when its number is right. The record keeps inputs, formula, and outputs so it reproduces.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_FORMULA: usize = 500;
pub const MAX_INPUTS: usize = 200;
pub const MAX_DEPTH: usize = 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Input {
    pub name: String,
    pub value: f64,
    /// `usd/month`, `month/year`, `count`... An empty string is a plain number.
    pub unit: String,
}

/// Unit as base names with integer exponents. `usd/month` is {usd: 1, month: -1}.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Unit(pub BTreeMap<String, i32>);

#[derive(Debug, Clone, PartialEq)]
pub struct Quantity {
    pub value: f64,
    pub unit: Unit,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CalcError {
    TooLong,
    TooManyInputs,
    TooDeep,
    DuplicateInput(String),
    BadNumber(String),
    BadUnit(String),
    Syntax(String),
    UnknownName(String),
    UnknownFunction(String),
    UnitMismatch { left: String, right: String },
    DivideByZero,
    NotFinite,
}

fn valid_name(n: &str) -> bool {
    let mut c = n.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

impl Unit {
    /// `usd/month`, `usd * month^-1`, `month^2`, or an empty string for a plain number.
    pub fn parse(s: &str) -> Result<Unit, CalcError> {
        let bad = || CalcError::BadUnit(s.to_string());
        let mut map: BTreeMap<String, i32> = BTreeMap::new();
        if s.trim().is_empty() {
            return Ok(Unit(map));
        }
        // Split into factors on '*' and '/', remembering the operator in front of each.
        let mut op = '*';
        let mut cur = String::new();
        let mut factors: Vec<(char, String)> = Vec::new();
        for ch in s.chars() {
            if ch == '*' || ch == '/' {
                factors.push((op, std::mem::take(&mut cur)));
                op = ch;
            } else {
                cur.push(ch);
            }
        }
        factors.push((op, cur));
        for (op, f) in factors {
            let f = f.trim();
            let (name, exp) = match f.split_once('^') {
                Some((n, e)) => (n.trim(), e.trim().parse::<i32>().map_err(|_| bad())?),
                None => (f, 1),
            };
            if !valid_name(name) {
                return Err(bad());
            }
            *map.entry(name.to_string()).or_insert(0) += if op == '*' { exp } else { -exp };
        }
        map.retain(|_, e| *e != 0);
        Ok(Unit(map))
    }

    pub fn show(&self) -> String {
        let term = |(n, e): (&String, i32)| {
            if e == 1 {
                n.clone()
            } else {
                format!("{n}^{e}")
            }
        };
        let top: Vec<String> = self
            .0
            .iter()
            .filter(|(_, e)| **e > 0)
            .map(|(n, e)| term((n, *e)))
            .collect();
        let bottom: Vec<String> = self
            .0
            .iter()
            .filter(|(_, e)| **e < 0)
            .map(|(n, e)| term((n, -*e)))
            .collect();
        let num = if top.is_empty() {
            "1".to_string()
        } else {
            top.join("*")
        };
        match bottom.len() {
            0 if top.is_empty() => String::new(),
            0 => num,
            1 => format!("{num}/{}", bottom[0]),
            _ => format!("{num}/({})", bottom.join("*")),
        }
    }

    fn times(&self, other: &Unit, sign: i32) -> Unit {
        let mut m = self.0.clone();
        for (n, e) in &other.0 {
            *m.entry(n.clone()).or_insert(0) += sign * e;
        }
        m.retain(|_, e| *e != 0);
        Unit(m)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64, Option<String>),
    Name(String),
    Op(char),
    LParen,
    RParen,
    Comma,
}

fn lex(src: &str) -> Result<Vec<Tok>, CalcError> {
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = Vec::new();
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit() || c == '.' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let mut j = i + 1;
                if j < chars.len() && (chars[j] == '+' || chars[j] == '-') {
                    j += 1;
                }
                if j < chars.len() && chars[j].is_ascii_digit() {
                    while j < chars.len() && chars[j].is_ascii_digit() {
                        j += 1;
                    }
                    i = j;
                }
            }
            let text: String = chars[start..i].iter().collect();
            let v: f64 = text
                .parse()
                .map_err(|_| CalcError::BadNumber(text.clone()))?;
            if !v.is_finite() {
                return Err(CalcError::NotFinite);
            }
            // An optional unit literal in brackets right after the number.
            let mut j = i;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            let mut unit = None;
            if j < chars.len() && chars[j] == '[' {
                let close = chars[j..]
                    .iter()
                    .position(|c| *c == ']')
                    .ok_or_else(|| CalcError::Syntax("a unit in [ ] is not closed".into()))?;
                unit = Some(chars[j + 1..j + close].iter().collect::<String>());
                i = j + close + 1;
            }
            out.push(Tok::Num(v, unit));
        } else if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            out.push(Tok::Name(chars[start..i].iter().collect()));
        } else {
            i += 1;
            out.push(match c {
                '+' | '-' | '*' | '/' => Tok::Op(c),
                '(' => Tok::LParen,
                ')' => Tok::RParen,
                ',' => Tok::Comma,
                other => {
                    return Err(CalcError::Syntax(format!(
                        "{other:?} is not allowed in a formula"
                    )))
                }
            });
        }
    }
    Ok(out)
}

struct Parser<'a> {
    toks: Vec<Tok>,
    pos: usize,
    vars: &'a BTreeMap<String, Quantity>,
}

fn finite(q: Quantity) -> Result<Quantity, CalcError> {
    if q.value.is_finite() {
        Ok(q)
    } else {
        Err(CalcError::NotFinite)
    }
}

fn add(a: Quantity, b: Quantity, sign: f64) -> Result<Quantity, CalcError> {
    if a.unit != b.unit {
        return Err(CalcError::UnitMismatch {
            left: a.unit.show(),
            right: b.unit.show(),
        });
    }
    finite(Quantity {
        value: a.value + sign * b.value,
        unit: a.unit,
    })
}

fn mul(a: Quantity, b: Quantity) -> Result<Quantity, CalcError> {
    finite(Quantity {
        value: a.value * b.value,
        unit: a.unit.times(&b.unit, 1),
    })
}

fn div(a: Quantity, b: Quantity) -> Result<Quantity, CalcError> {
    if b.value == 0.0 {
        return Err(CalcError::DivideByZero);
    }
    finite(Quantity {
        value: a.value / b.value,
        unit: a.unit.times(&b.unit, -1),
    })
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos)
    }

    fn next(&mut self) -> Option<Tok> {
        let t = self.toks.get(self.pos).cloned();
        self.pos += 1;
        t
    }

    fn expr(&mut self, depth: usize) -> Result<Quantity, CalcError> {
        if depth > MAX_DEPTH {
            return Err(CalcError::TooDeep);
        }
        let mut acc = self.term(depth)?;
        while let Some(Tok::Op(c @ ('+' | '-'))) = self.peek().cloned() {
            self.pos += 1;
            let rhs = self.term(depth)?;
            acc = add(acc, rhs, if c == '+' { 1.0 } else { -1.0 })?;
        }
        Ok(acc)
    }

    fn term(&mut self, depth: usize) -> Result<Quantity, CalcError> {
        let mut acc = self.factor(depth)?;
        while let Some(Tok::Op(c @ ('*' | '/'))) = self.peek().cloned() {
            self.pos += 1;
            let rhs = self.factor(depth)?;
            acc = if c == '*' {
                mul(acc, rhs)?
            } else {
                div(acc, rhs)?
            };
        }
        Ok(acc)
    }

    fn factor(&mut self, depth: usize) -> Result<Quantity, CalcError> {
        if depth > MAX_DEPTH {
            return Err(CalcError::TooDeep);
        }
        match self.next() {
            Some(Tok::Op('-')) => {
                let q = self.factor(depth + 1)?;
                Ok(Quantity {
                    value: -q.value,
                    unit: q.unit,
                })
            }
            Some(Tok::Num(v, unit)) => Ok(Quantity {
                value: v,
                unit: match unit {
                    Some(u) => Unit::parse(&u)?,
                    None => Unit::default(),
                },
            }),
            Some(Tok::LParen) => {
                let q = self.expr(depth + 1)?;
                match self.next() {
                    Some(Tok::RParen) => Ok(q),
                    _ => Err(CalcError::Syntax("a ( is not closed".into())),
                }
            }
            Some(Tok::Name(n)) => {
                if self.peek() == Some(&Tok::LParen) {
                    self.pos += 1;
                    self.call(&n, depth)
                } else {
                    self.vars.get(&n).cloned().ok_or(CalcError::UnknownName(n))
                }
            }
            other => Err(CalcError::Syntax(format!("unexpected {other:?}"))),
        }
    }

    fn call(&mut self, name: &str, depth: usize) -> Result<Quantity, CalcError> {
        if !matches!(name, "sum" | "ratio" | "growth") {
            return Err(CalcError::UnknownFunction(name.to_string()));
        }
        let mut args = Vec::new();
        if self.peek() != Some(&Tok::RParen) {
            loop {
                args.push(self.expr(depth + 1)?);
                match self.next() {
                    Some(Tok::Comma) => continue,
                    Some(Tok::RParen) => break,
                    _ => return Err(CalcError::Syntax(format!("{name}( is not closed"))),
                }
            }
        } else {
            self.pos += 1;
        }
        match (name, args.len()) {
            ("sum", n) if n >= 1 => args
                .into_iter()
                .try_fold(None, |acc: Option<Quantity>, q| match acc {
                    None => Ok(Some(q)),
                    Some(a) => add(a, q, 1.0).map(Some),
                })?
                .ok_or_else(|| CalcError::Syntax("sum needs arguments".into())),
            ("ratio", 2) => {
                let b = args.pop().expect("two args");
                div(args.pop().expect("two args"), b)
            }
            ("growth", 2) => {
                let to = args.pop().expect("two args");
                let from = args.pop().expect("two args");
                if from.unit != to.unit {
                    return Err(CalcError::UnitMismatch {
                        left: from.unit.show(),
                        right: to.unit.show(),
                    });
                }
                if from.value == 0.0 {
                    return Err(CalcError::DivideByZero);
                }
                finite(Quantity {
                    value: (to.value - from.value) / from.value,
                    unit: Unit::default(),
                })
            }
            (n, k) => Err(CalcError::Syntax(format!("{n} cannot take {k} arguments"))),
        }
    }
}

/// Evaluates `formula` over `inputs`.
pub fn evaluate(inputs: &[Input], formula: &str) -> Result<Quantity, CalcError> {
    if formula.chars().count() > MAX_FORMULA {
        return Err(CalcError::TooLong);
    }
    if inputs.len() > MAX_INPUTS {
        return Err(CalcError::TooManyInputs);
    }
    let mut vars = BTreeMap::new();
    for i in inputs {
        if !valid_name(&i.name) {
            return Err(CalcError::Syntax(format!(
                "{:?} is not a valid input name",
                i.name
            )));
        }
        if !i.value.is_finite() {
            return Err(CalcError::BadNumber(i.name.clone()));
        }
        let q = Quantity {
            value: i.value,
            unit: Unit::parse(&i.unit)?,
        };
        if vars.insert(i.name.clone(), q).is_some() {
            return Err(CalcError::DuplicateInput(i.name.clone()));
        }
    }
    let toks = lex(formula)?;
    if toks.is_empty() {
        return Err(CalcError::Syntax("the formula is empty".into()));
    }
    let mut p = Parser {
        toks,
        pos: 0,
        vars: &vars,
    };
    let q = p.expr(0)?;
    if p.pos < p.toks.len() {
        return Err(CalcError::Syntax(format!("unexpected {:?}", p.toks[p.pos])));
    }
    finite(q)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    pub id: String,
    pub formula: String,
    pub stated_value: f64,
    pub stated_unit: String,
    /// Relative tolerance for the value, such as 0.001 for rounding to three places.
    pub tolerance: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    Pass,
    ValueMismatch,
    UnitMismatch,
    /// Both differ.
    ValueAndUnitMismatch,
    Error(String),
}

/// What is kept so the check can be repeated: inputs, formula, computed output, and the verdict.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Record {
    pub claim: Claim,
    pub inputs: Vec<Input>,
    pub computed_value: Option<f64>,
    pub computed_unit: Option<String>,
    pub verdict: Verdict,
}

/// Recomputes a claim and compares value and unit.
pub fn reproduce(inputs: &[Input], claim: &Claim) -> Record {
    let mut rec = Record {
        claim: claim.clone(),
        inputs: inputs.to_vec(),
        computed_value: None,
        computed_unit: None,
        verdict: Verdict::Pass,
    };
    let computed = match evaluate(inputs, &claim.formula) {
        Ok(q) => q,
        Err(e) => {
            rec.verdict = Verdict::Error(format!("{e:?}"));
            return rec;
        }
    };
    let stated_unit = match Unit::parse(&claim.stated_unit) {
        Ok(u) => u,
        Err(e) => {
            rec.verdict = Verdict::Error(format!("the stated unit: {e:?}"));
            return rec;
        }
    };
    rec.computed_value = Some(computed.value);
    rec.computed_unit = Some(computed.unit.show());
    let scale = computed.value.abs().max(claim.stated_value.abs());
    let value_ok = (computed.value - claim.stated_value).abs() <= claim.tolerance * scale;
    let unit_ok = computed.unit == stated_unit;
    rec.verdict = match (value_ok, unit_ok) {
        (true, true) => Verdict::Pass,
        (false, true) => Verdict::ValueMismatch,
        (true, false) => Verdict::UnitMismatch,
        (false, false) => Verdict::ValueAndUnitMismatch,
    };
    rec
}

impl Record {
    /// Runs the check again from the retained inputs and formula.
    pub fn repeat(&self) -> Record {
        reproduce(&self.inputs, &self.claim)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inp(name: &str, value: f64, unit: &str) -> Input {
        Input {
            name: name.into(),
            value,
            unit: unit.into(),
        }
    }

    fn table() -> Vec<Input> {
        vec![
            inp("licenses", 4.0, ""),
            inp("price", 5.0, "usd/month"),
            inp("budget", 300.0, "usd/year"),
            inp("last", 80.0, "usd"),
            inp("this", 100.0, "usd"),
        ]
    }

    fn value(f: &str) -> f64 {
        evaluate(&table(), f).unwrap().value
    }

    fn unit_of(f: &str) -> String {
        evaluate(&table(), f).unwrap().unit.show()
    }

    fn claim(formula: &str, v: f64, unit: &str) -> Claim {
        Claim {
            id: "c1".into(),
            formula: formula.into(),
            stated_value: v,
            stated_unit: unit.into(),
            tolerance: 0.001,
        }
    }

    #[test]
    fn arithmetic_follows_precedence_parentheses_and_unary_minus() {
        assert_eq!(value("2 + 3 * 4"), 14.0);
        assert_eq!(value("(2 + 3) * 4"), 20.0);
        assert_eq!(value("-2 + 5"), 3.0);
        assert_eq!(value("10 / 4"), 2.5);
        assert_eq!(value("2 * -3"), -6.0);
        assert_eq!(value("1 - 2 - 3"), -4.0, "left to right");
        assert_eq!(value("8 / 2 / 2"), 2.0);
    }

    #[test]
    fn units_multiply_divide_and_cancel() {
        assert_eq!(unit_of("licenses * price"), "usd/month");
        assert_eq!(value("licenses * price"), 20.0);
        assert_eq!(unit_of("licenses * price * 12 [month/year]"), "usd/year");
        assert_eq!(value("licenses * price * 12 [month/year]"), 240.0);
        assert_eq!(
            unit_of("price * 12 [month/year] / budget"),
            "",
            "usd/year over usd/year is a plain number"
        );
        assert_eq!(unit_of("this / last"), "");
        assert_eq!(
            unit_of("1 [month] / price"),
            "month^2/usd",
            "a unit nobody asked for stays visible"
        );
    }

    #[test]
    fn adding_or_comparing_different_units_is_an_error_not_a_guess() {
        assert!(matches!(
            evaluate(&table(), "price + licenses"),
            Err(CalcError::UnitMismatch { .. })
        ));
        assert!(matches!(
            evaluate(&table(), "price + budget"),
            Err(CalcError::UnitMismatch { .. })
        ));
        assert!(matches!(
            evaluate(&table(), "growth(price, budget)"),
            Err(CalcError::UnitMismatch { .. })
        ));
        assert_eq!(value("price + price"), 10.0);
        assert_eq!(unit_of("price - price"), "usd/month");
    }

    #[test]
    fn sum_ratio_and_growth_work_on_matching_units() {
        assert_eq!(value("sum(this, last)"), 180.0);
        assert_eq!(unit_of("sum(this, last)"), "usd");
        assert_eq!(value("ratio(this, last)"), 1.25);
        assert_eq!(unit_of("ratio(this, last)"), "");
        assert!((value("growth(last, this)") - 0.25).abs() < 1e-12);
        assert_eq!(unit_of("growth(last, this)"), "");
        assert!(matches!(
            evaluate(&table(), "sum()"),
            Err(CalcError::Syntax(_))
        ));
    }

    #[test]
    fn a_seeded_unit_error_is_caught_by_reproduction() {
        // The number is right and the unit is wrong: an annual figure labelled monthly.
        let r = reproduce(
            &table(),
            &claim("licenses * price * 12 [month/year]", 240.0, "usd/month"),
        );
        assert_eq!(r.verdict, Verdict::UnitMismatch);
        assert_eq!(r.computed_unit.as_deref(), Some("usd/year"));
        // The unit is right and the number is wrong: the monthly figure stated as the annual one.
        let r = reproduce(
            &table(),
            &claim("licenses * price * 12 [month/year]", 20.0, "usd/year"),
        );
        assert_eq!(r.verdict, Verdict::ValueMismatch);
        // Both wrong.
        let r = reproduce(
            &table(),
            &claim("licenses * price * 12 [month/year]", 20.0, "usd/month"),
        );
        assert_eq!(r.verdict, Verdict::ValueAndUnitMismatch);
        // Right on both.
        let r = reproduce(
            &table(),
            &claim("licenses * price * 12 [month/year]", 240.0, "usd/year"),
        );
        assert_eq!(r.verdict, Verdict::Pass);
        assert_eq!(r.computed_value, Some(240.0));
    }

    #[test]
    fn the_brief_style_arithmetic_error_is_caught() {
        // 4 licenses at $5 each, stated as $25.
        let r = reproduce(&table(), &claim("licenses * price", 25.0, "usd/month"));
        assert_eq!(r.verdict, Verdict::ValueMismatch);
    }

    #[test]
    fn the_tolerance_is_relative_and_rounding_passes() {
        let t = vec![inp("a", 100.0, "usd"), inp("b", 3.0, "")];
        assert_eq!(
            reproduce(&t, &claim("a / b", 33.33, "usd")).verdict,
            Verdict::Pass
        );
        assert_eq!(
            reproduce(&t, &claim("a / b", 33.0, "usd")).verdict,
            Verdict::ValueMismatch
        );
        let mut exact = claim("a / b", 33.33, "usd");
        exact.tolerance = 0.0;
        assert_eq!(reproduce(&t, &exact).verdict, Verdict::ValueMismatch);
        let mut zero = claim("a - a", 0.0, "usd");
        zero.tolerance = 0.0;
        assert_eq!(
            reproduce(&t, &zero).verdict,
            Verdict::Pass,
            "zero against zero"
        );
    }

    #[test]
    fn errors_are_reported_not_hidden() {
        assert_eq!(
            evaluate(&table(), "1 / 0").unwrap_err(),
            CalcError::DivideByZero
        );
        assert_eq!(
            evaluate(&table(), "1 / (price - price)").unwrap_err(),
            CalcError::DivideByZero
        );
        assert!(
            matches!(evaluate(&table(), "nope * 2"), Err(CalcError::UnknownName(n)) if n == "nope")
        );
        assert!(matches!(
            evaluate(&table(), "eval(1)"),
            Err(CalcError::UnknownFunction(_))
        ));
        for bad in [
            "", "2 +", "(2", "2 2", "2 $ 3", "1 [", "1 [usd", "* 2", "sum(1,", "1e999999",
        ] {
            assert!(
                evaluate(&table(), bad).is_err(),
                "{bad:?} should not evaluate"
            );
        }
        assert!(matches!(
            reproduce(&table(), &claim("1 / 0", 1.0, "")).verdict,
            Verdict::Error(_)
        ));
    }

    #[test]
    fn the_inputs_are_bounded_and_names_cannot_repeat() {
        let long = format!("1{}", " + 1".repeat(200));
        assert_eq!(evaluate(&table(), &long).unwrap_err(), CalcError::TooLong);
        let deep = format!("{}1{}", "(".repeat(30), ")".repeat(30));
        assert_eq!(evaluate(&table(), &deep).unwrap_err(), CalcError::TooDeep);
        let many: Vec<Input> = (0..201).map(|i| inp(&format!("v{i}"), 1.0, "")).collect();
        assert_eq!(evaluate(&many, "1").unwrap_err(), CalcError::TooManyInputs);
        let dup = vec![inp("a", 1.0, ""), inp("a", 2.0, "")];
        assert_eq!(
            evaluate(&dup, "a").unwrap_err(),
            CalcError::DuplicateInput("a".into())
        );
        let nan = vec![inp("a", f64::NAN, "")];
        assert!(matches!(
            evaluate(&nan, "a"),
            Err(CalcError::BadNumber(_)) | Err(CalcError::NotFinite)
        ));
        let bad_unit = vec![inp("a", 1.0, "usd//month")];
        assert!(matches!(
            evaluate(&bad_unit, "a"),
            Err(CalcError::BadUnit(_))
        ));
    }

    #[test]
    fn nothing_in_a_formula_can_run_code_or_reach_outside() {
        for f in [
            "`ls`",
            "$(ls)",
            "a; b",
            "system(1)",
            "import(1)",
            "__proto__",
            "1 // 2",
            "1 # 2",
        ] {
            assert!(evaluate(&table(), f).is_err(), "{f:?}");
        }
    }

    #[test]
    fn a_record_serializes_and_repeats_to_the_same_verdict() {
        let r = reproduce(
            &table(),
            &claim("licenses * price * 12 [month/year]", 240.0, "usd/month"),
        );
        let json = serde_json::to_string(&r).unwrap();
        let back: Record = serde_json::from_str(&json).unwrap();
        assert_eq!(back, r);
        assert_eq!(back.repeat(), r, "the same inputs give the same answer");
        assert_eq!(r.inputs, table());
    }

    #[test]
    fn unit_parsing_normalizes_order_and_rejects_nonsense() {
        assert_eq!(
            Unit::parse("usd/month").unwrap(),
            Unit::parse("usd * month^-1").unwrap()
        );
        assert_eq!(Unit::parse("").unwrap(), Unit::default());
        assert_eq!(Unit::parse("usd/usd").unwrap(), Unit::default());
        assert_eq!(Unit::parse("usd").unwrap().show(), "usd");
        for bad in ["usd//month", "/", "usd/", "us d", "usd^x", "3usd"] {
            assert!(Unit::parse(bad).is_err(), "{bad:?}");
        }
    }
}
