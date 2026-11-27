//! Format specs for `fmt(x, spec)` and `"{x:spec}"` (SPEC.md 12.6), a
//! subset of Python's format mini-language:
//!
//! ```text
//! spec  = [ [ fill ] align ] [ "0" ] [ width ] [ "," ] [ "." precision ] [ type ]
//! align = "<" | ">" | "^"
//! type  = "d" | "f" | "e" | "x" | "X" | "o" | "b" | "%"
//! ```

use crate::value::{Value, format_float};

/// A parsed format spec, made by [`parse_spec`] and used by [`apply`].
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    fill: char,
    align: Option<char>,
    zero: bool,
    /// The minimum width in characters. 0 when the spec gives none.
    width: usize,
    comma: bool,
    precision: Option<usize>,
    ty: Option<char>,
}

/// Parses a format spec. The error is a message, to which the caller adds
/// the span: the spec does not follow the grammar, names an unknown type,
/// or asks for a width above 10000 or a precision above 100.
pub fn parse_spec(s: &str) -> Result<Spec, String> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut spec = Spec {
        fill: ' ',
        align: None,
        zero: false,
        width: 0,
        comma: false,
        precision: None,
        ty: None,
    };
    let is_align = |c: char| matches!(c, '<' | '>' | '^');
    if chars.len() >= 2 && is_align(chars[1]) {
        spec.fill = chars[0];
        spec.align = Some(chars[1]);
        i = 2;
    } else if !chars.is_empty() && is_align(chars[0]) {
        spec.align = Some(chars[0]);
        i = 1;
    }
    if chars.get(i) == Some(&'0') {
        spec.zero = true;
        i += 1;
    }
    let mut width = String::new();
    while let Some(c) = chars.get(i).filter(|c| c.is_ascii_digit()) {
        width.push(*c);
        i += 1;
    }
    if !width.is_empty() {
        spec.width = width
            .parse()
            .map_err(|_| format!("width in format spec `{s}` is too large"))?;
        if spec.width > 10_000 {
            return Err(format!("width in format spec `{s}` is too large"));
        }
    }
    if chars.get(i) == Some(&',') {
        spec.comma = true;
        i += 1;
    }
    if chars.get(i) == Some(&'.') {
        i += 1;
        let mut p = String::new();
        while let Some(c) = chars.get(i).filter(|c| c.is_ascii_digit()) {
            p.push(*c);
            i += 1;
        }
        if p.is_empty() {
            return Err(format!("format spec `{s}` has a `.` without a precision"));
        }
        let p: usize = p
            .parse()
            .map_err(|_| format!("precision in format spec `{s}` is too large"))?;
        if p > 100 {
            return Err(format!("precision in format spec `{s}` is too large"));
        }
        spec.precision = Some(p);
    }
    if let Some(&c) = chars.get(i) {
        if !matches!(c, 'd' | 'f' | 'e' | 'x' | 'X' | 'o' | 'b' | '%') {
            return Err(format!(
                "unknown format type `{c}` in `{s}`; use one of d f e x X o b %"
            ));
        }
        spec.ty = Some(c);
        i += 1;
    }
    if i != chars.len() {
        return Err(format!(
            "invalid format spec `{s}`; it looks like [[fill]align][0][width][,][.precision][type], as in >8 or .2f"
        ));
    }
    Ok(spec)
}

fn group_thousands(digits: &str) -> String {
    let (int, frac) = match digits.find('.') {
        Some(i) => (&digits[..i], &digits[i..]),
        None => (digits, ""),
    };
    let mut out = String::new();
    let n = int.len();
    for (i, c) in int.chars().enumerate() {
        if i > 0 && (n - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out + frac
}

/// `abs` times 100 with `precision` decimals and a `%` sign. The decimal
/// point is moved in the text rather than multiplying the float, which
/// would overflow to infinity for values above about 1.8e306 (and print
/// `inf%`).
fn percent(abs: f64, precision: usize) -> String {
    let digits = format!("{:.*}", precision + 2, abs);
    let (int, frac) = digits.split_once('.').unwrap_or((&digits, "00"));
    let (moved, rest) = frac.split_at(2);
    let mut whole = format!("{int}{moved}");
    let trimmed = whole.trim_start_matches('0');
    whole = if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    };
    if rest.is_empty() {
        format!("{whole}%")
    } else {
        format!("{whole}.{rest}%")
    }
}

fn float_body(f: f64, spec: &Spec) -> Result<String, String> {
    if !f.is_finite() {
        return Ok(format_float(f));
    }
    let abs = f.abs();
    let body = match spec.ty {
        Some('f') => format!("{:.*}", spec.precision.unwrap_or(6), abs),
        Some('%') => percent(abs, spec.precision.unwrap_or(6)),
        Some('e') => {
            let s = format!("{:.*e}", spec.precision.unwrap_or(6), abs);
            let (m, e) = s.split_once('e').unwrap_or((&s, "0"));
            let e: i32 = e.parse().unwrap_or(0);
            format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs())
        }
        Some(t) => return Err(format!("format type `{t}` needs an int, got a float")),
        None => match spec.precision {
            Some(p) => format!("{abs:.p$}"),
            None => format_float(abs),
        },
    };
    Ok(body)
}

/// Format a value with a spec. The error is a message for a type the value
/// cannot take: any type on a value that is not a number, or an integer
/// type (`d`, `x`, `X`, `o`, `b`) on a float.
pub fn apply(v: &Value, spec: &Spec) -> Result<String, String> {
    let (sign, body, numeric) = match v {
        Value::Int(i) => {
            let neg = *i < 0;
            let abs = i.unsigned_abs();
            let body = match spec.ty {
                None | Some('d') if spec.precision.is_none() => abs.to_string(),
                Some('x') => format!("{abs:x}"),
                Some('X') => format!("{abs:X}"),
                Some('o') => format!("{abs:o}"),
                Some('b') => format!("{abs:b}"),
                _ => {
                    let s = Spec {
                        ty: spec.ty.filter(|t| *t != 'd').or(Some('f')),
                        ..spec.clone()
                    };
                    float_body(*i as f64, &s)?
                }
            };
            (if neg { "-" } else { "" }, body, true)
        }
        Value::Float(f) => (if *f < 0.0 { "-" } else { "" }, float_body(*f, spec)?, true),
        other => {
            if let Some(t) = spec.ty {
                return Err(format!(
                    "format type `{t}` needs a number, got {}",
                    other.a_type()
                ));
            }
            let mut s = other.display();
            if let Some(p) = spec.precision {
                s = s.chars().take(p).collect();
            }
            ("", s, false)
        }
    };
    let body = if spec.comma && numeric && !matches!(spec.ty, Some('x' | 'X' | 'o' | 'b' | 'e')) {
        group_thousands(&body)
    } else {
        body
    };
    let len = sign.chars().count() + body.chars().count();
    if len >= spec.width {
        return Ok(format!("{sign}{body}"));
    }
    let pad = spec.width - len;
    if spec.zero && numeric && spec.align.is_none() {
        return Ok(format!("{sign}{}{body}", "0".repeat(pad)));
    }
    let fill = |n: usize| spec.fill.to_string().repeat(n);
    let align = spec.align.unwrap_or(if numeric { '>' } else { '<' });
    Ok(match align {
        '<' => format!("{sign}{body}{}", fill(pad)),
        '^' => format!("{}{sign}{body}{}", fill(pad / 2), fill(pad - pad / 2)),
        _ => format!("{}{sign}{body}", fill(pad)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(v: Value, s: &str) -> String {
        apply(&v, &parse_spec(s).unwrap()).unwrap()
    }

    #[test]
    fn alignment_and_width() {
        assert_eq!(f(Value::Int(42), ">6"), "    42");
        assert_eq!(f(Value::Int(42), "6"), "    42");
        assert_eq!(f(Value::str("ab"), "6"), "ab    ");
        assert_eq!(f(Value::str("ab"), "*^6"), "**ab**");
        assert_eq!(f(Value::str("ERROR"), "<6"), "ERROR ");
    }

    #[test]
    fn numbers() {
        assert_eq!(f(Value::Float(1.23456), ".2f"), "1.23");
        assert_eq!(f(Value::Float(0.256), ".1%"), "25.6%");
        assert_eq!(f(Value::Float(0.004), ".0%"), "0%");
        assert_eq!(f(Value::Float(0.5), "%"), "50.000000%");
        assert_eq!(f(Value::Int(12), ".0%"), "1200%");
        assert_eq!(f(Value::Float(-0.256), ".2%"), "-25.60%");
        let huge = f(Value::Float(1.7e308), ",.0%");
        assert!(
            huge.starts_with("169,999,999,999,999,993,") && huge.ends_with("%"),
            "{huge}"
        );
        assert_eq!(f(Value::Int(1234567), ","), "1,234,567");
        assert_eq!(f(Value::Int(1234567), ">14,"), "     1,234,567");
        assert_eq!(f(Value::Float(-3.5), "08.3f"), "-003.500");
        assert_eq!(f(Value::Int(255), "x"), "ff");
        assert_eq!(f(Value::Int(-255), "X"), "-FF");
        assert_eq!(f(Value::Float(1500.0), ".2e"), "1.50e+03");
        assert_eq!(f(Value::Float(1.5), ".0f"), "2");
        assert_eq!(f(Value::Int(5), ".1f"), "5.0");
    }

    #[test]
    fn bad_specs() {
        assert!(parse_spec("q").is_err());
        assert!(parse_spec(".").is_err());
        assert!(apply(&Value::str("a"), &parse_spec("d").unwrap()).is_err());
    }
}
