//! Runtime values (SPEC.md section 5).
//!
//! Lists and records are reference-counted and copied on write
//! (`Rc::make_mut`), which gives value semantics cheaply: passing a list
//! around never copies it, and changing a `var` copy never affects others.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::fmt::Write as _;
use std::rc::Rc;

use crate::ast::FnDecl;
use crate::interp::{Builtin, Scope};
use crate::path::OkPath;

#[derive(Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Bytes(Rc<[u8]>),
    List(Rc<Vec<Value>>),
    Record(Rc<Record>),
    Path(Rc<OkPath>),
    /// Nanoseconds.
    Duration(i64),
    /// Nanoseconds since the Unix epoch, UTC.
    Time(i64),
    Func(Rc<Func>),
    /// A streaming list or string; see `lazy.rs`. Never visible to scripts
    /// as a separate kind.
    Lazy(Rc<crate::lazy::Lazy>),
}

pub enum Func {
    User { decl: Rc<FnDecl>, env: Rc<Scope> },
    Builtin(&'static Builtin),
}

impl Func {
    pub fn name(&self) -> &str {
        match self {
            Func::User { decl, .. } => &decl.name,
            Func::Builtin(b) => b.name,
        }
    }
}

/// An insertion-ordered map from string keys to values.
#[derive(Clone, Default)]
pub struct Record {
    keys: Vec<Rc<str>>,
    values: Vec<Value>,
    index: HashMap<Rc<str>, usize>,
}

impl Record {
    pub fn new() -> Record {
        Record::default()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.index.get(key).map(|&i| &self.values[i])
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        let i = *self.index.get(key)?;
        Some(&mut self.values[i])
    }

    pub fn contains(&self, key: &str) -> bool {
        self.index.contains_key(key)
    }

    /// Insert or replace. A replaced key keeps its position.
    pub fn insert(&mut self, key: impl Into<Rc<str>>, value: Value) {
        let key = key.into();
        if let Some(&i) = self.index.get(&key) {
            self.values[i] = value;
        } else {
            self.index.insert(key.clone(), self.keys.len());
            self.keys.push(key);
            self.values.push(value);
        }
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&Rc<str>, &Value)> {
        self.keys.iter().zip(self.values.iter())
    }

    pub fn keys(&self) -> impl Iterator<Item = &Rc<str>> {
        self.keys.iter()
    }
}

impl Value {
    pub fn str(s: impl Into<Rc<str>>) -> Value {
        Value::Str(s.into())
    }

    pub fn list(items: Vec<Value>) -> Value {
        Value::List(Rc::new(items))
    }

    pub fn record(r: Record) -> Value {
        Value::Record(Rc::new(r))
    }

    pub fn path(p: OkPath) -> Value {
        Value::Path(Rc::new(p))
    }

    /// The kind name returned by `type(x)`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Str(_) => "string",
            Value::Bytes(_) => "bytes",
            Value::List(_) => "list",
            Value::Record(_) => "record",
            Value::Path(_) => "path",
            Value::Duration(_) => "duration",
            Value::Time(_) => "time",
            Value::Func(_) => "fn",
            Value::Lazy(l) => {
                if l.text {
                    "string"
                } else {
                    "list"
                }
            }
        }
    }

    /// "a list", "an int": the kind with an article, for messages.
    pub fn a_type(&self) -> String {
        with_article(self.type_name())
    }

    /// The display form used by `print`, `str` and interpolation (SPEC.md 5.1).
    pub fn display(&self) -> String {
        match self {
            Value::Str(s) => s.to_string(),
            Value::Path(p) => p.display(),
            other => other.repr(),
        }
    }

    /// The form used inside lists and records: strings are quoted, paths
    /// show as `path("...")`, so the structure is unambiguous.
    pub fn repr(&self) -> String {
        let mut out = String::new();
        self.write_repr(&mut out);
        out
    }

    fn write_repr(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Int(i) => {
                let _ = write!(out, "{i}");
            }
            Value::Float(f) => out.push_str(&format_float(*f)),
            Value::Str(s) => quote_into(s, out),
            Value::Bytes(b) => {
                let _ = write!(out, "<bytes {}>", b.len());
            }
            Value::List(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    v.write_repr(out);
                }
                out.push(']');
            }
            Value::Record(r) => {
                out.push('{');
                for (i, (k, v)) in r.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    if is_plain_key(k) {
                        out.push_str(k);
                    } else {
                        quote_into(k, out);
                    }
                    out.push_str(": ");
                    v.write_repr(out);
                }
                out.push('}');
            }
            Value::Path(p) => {
                out.push_str("path(");
                quote_into(&p.display(), out);
                out.push(')');
            }
            Value::Duration(d) => out.push_str(&format_duration(*d)),
            Value::Time(t) => out.push_str(&format_time_utc(*t)),
            Value::Func(f) => {
                let _ = write!(out, "<fn {}>", f.name());
            }
            Value::Lazy(_) => out.push_str("<stream>"),
        }
    }
}

pub fn with_article(name: &str) -> String {
    let vowel = matches!(
        name.as_bytes().first(),
        Some(b'a' | b'e' | b'i' | b'o' | b'u')
    );
    if name == "null" {
        "null".into()
    } else if vowel {
        format!("an {name}")
    } else {
        format!("a {name}")
    }
}

fn is_plain_key(k: &str) -> bool {
    let mut chars = k.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn quote_into(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Shortest round-trip form, keeping `.0` on integral values.
pub fn format_float(f: f64) -> String {
    let s = format!("{f}");
    if s.contains(['.', 'e', 'E', 'i', 'N']) {
        s
    } else {
        format!("{s}.0")
    }
}

/// Display a duration in literal syntax: `1h30m`, `1.5s`, `250ms`, `0s`.
pub fn format_duration(ns: i64) -> String {
    if ns == 0 {
        return "0s".into();
    }
    let neg = ns < 0;
    let mut n = (ns as i128).unsigned_abs();
    let mut out = String::new();
    if neg {
        out.push('-');
    }
    const MS: u128 = 1_000_000;
    const S: u128 = 1_000_000_000;
    if n < S {
        out.push_str(&trim_decimal(n, MS, 6));
        out.push_str("ms");
        return out;
    }
    for (unit, size) in [("d", 86_400 * S), ("h", 3_600 * S), ("m", 60 * S)] {
        if n >= size {
            let _ = write!(out, "{}{unit}", n / size);
            n %= size;
        }
    }
    if n > 0 {
        out.push_str(&trim_decimal(n, S, 9));
        out.push('s');
    }
    out
}

/// `n / unit` with up to `digits` decimals, trailing zeros removed.
fn trim_decimal(n: u128, unit: u128, digits: usize) -> String {
    let whole = n / unit;
    let frac = n % unit;
    if frac == 0 {
        return whole.to_string();
    }
    let s = format!("{frac:0width$}", width = digits);
    format!("{whole}.{}", s.trim_end_matches('0'))
}

/// RFC 3339 in UTC. (Local-time display arrives with the time functions.)
pub fn format_time_utc(ns: i64) -> String {
    let secs = ns.div_euclid(1_000_000_000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// Days since 1970-01-01 to (year, month, day), proleptic Gregorian.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (year, month, day) to days since 1970-01-01, proleptic Gregorian; the
/// inverse of [`civil_from_days`].
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Structural equality (SPEC.md 5.4). Records compare as maps: same keys and
/// equal values, in any order.
pub fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Int(x), Value::Float(y)) | (Value::Float(y), Value::Int(x)) => {
            (*x as f64) == *y && (*y as i64) == *x
        }
        (Value::Str(x), Value::Str(y)) => x == y,
        (Value::Bytes(x), Value::Bytes(y)) => x == y,
        (Value::List(x), Value::List(y)) => {
            Rc::ptr_eq(x, y)
                || (x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| equal(a, b)))
        }
        (Value::Record(x), Value::Record(y)) => {
            Rc::ptr_eq(x, y)
                || (x.len() == y.len()
                    && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| equal(v, w))))
        }
        (Value::Path(x), Value::Path(y)) => x == y,
        (Value::Duration(x), Value::Duration(y)) => x == y,
        (Value::Time(x), Value::Time(y)) => x == y,
        (Value::Func(x), Value::Func(y)) => Rc::ptr_eq(x, y),
        _ => false,
    }
}

/// Ordering for `<`, `sort`, `min` and `max` (SPEC.md 5.4). `None` when the
/// two values cannot be ordered.
pub fn compare(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Some(x.cmp(y)),
        (Value::Float(x), Value::Float(y)) => x.partial_cmp(y),
        (Value::Int(x), Value::Float(y)) => (*x as f64).partial_cmp(y),
        (Value::Float(x), Value::Int(y)) => x.partial_cmp(&(*y as f64)),
        (Value::Str(x), Value::Str(y)) => Some(x.cmp(y)),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        (Value::Duration(x), Value::Duration(y)) => Some(x.cmp(y)),
        (Value::Time(x), Value::Time(y)) => Some(x.cmp(y)),
        (Value::Path(x), Value::Path(y)) => Some(x.display().cmp(&y.display())),
        (Value::List(x), Value::List(y)) => {
            for (a, b) in x.iter().zip(y.iter()) {
                match compare(a, b)? {
                    Ordering::Equal => continue,
                    other => return Some(other),
                }
            }
            Some(x.len().cmp(&y.len()))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_display_in_literal_syntax() {
        let s = 1_000_000_000;
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(250_000_000), "250ms");
        assert_eq!(format_duration(1_500_000), "1.5ms");
        assert_eq!(format_duration(3 * s / 2), "1.5s");
        assert_eq!(format_duration(5400 * s), "1h30m");
        assert_eq!(format_duration(86_400 * s + 5 * s), "1d5s");
        assert_eq!(format_duration(-2 * 60 * s), "-2m");
    }

    #[test]
    fn floats_keep_a_decimal_point() {
        assert_eq!(format_float(2.0), "2.0");
        assert_eq!(format_float(0.1 + 0.2), "0.30000000000000004");
        assert_eq!(format_float(-1.5), "-1.5");
    }

    #[test]
    fn repr_quotes_strings_inside_collections() {
        let mut r = Record::new();
        r.insert("name", Value::str("ok"));
        r.insert("content-type", Value::Int(1));
        let v = Value::list(vec![Value::str("a\"b"), Value::record(r), Value::Null]);
        assert_eq!(
            v.display(),
            r#"["a\"b", {name: "ok", "content-type": 1}, null]"#
        );
    }

    #[test]
    fn equality_is_structural_and_numeric() {
        assert!(equal(&Value::Int(1), &Value::Float(1.0)));
        assert!(!equal(&Value::str("1"), &Value::Int(1)));
        let mut a = Record::new();
        a.insert("x", Value::Int(1));
        a.insert("y", Value::Int(2));
        let mut b = Record::new();
        b.insert("y", Value::Int(2));
        b.insert("x", Value::Int(1));
        assert!(equal(&Value::record(a), &Value::record(b)));
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_416), (2025, 11, 24));
        for d in [-1000, 0, 59, 60, 20_416, 100_000] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d);
        }
        assert_eq!(format_time_utc(0), "1970-01-01T00:00:00Z");
    }
}
