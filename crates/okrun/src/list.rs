//! List and record functions (SPEC.md 12.5).
//!
//! Every function takes the list first, so it reads well as a pipeline stage.
//! The streaming ones (`where`, `map`, `take`, `skip`, `uniq`, `flat`,
//! `enumerate`, `lines`) return a lazy list when given one; the consuming
//! ones (`count`, `sum`, `first`, ...) pull elements one at a time and stop
//! early when they can.

use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use crate::builtins::{arg_err, eager, streaming, type_err};
use crate::error::{ErrKind, OkError, suggest};
use crate::interp::{Args, Builtin, Interp, R};
use crate::lazy::{self, Seen, Source, adapt, at_span, canonical_key, cursor};
use crate::source::Span;
use crate::value::{Record, Value, compare, total_order};

#[rustfmt::skip]
pub static BUILTINS: &[Builtin] = &[
    streaming("where", where_),
    streaming("map", map),
    streaming("each", each),
    streaming("take", take),
    streaming("skip", skip),
    streaming("uniq", uniq),
    streaming("flat", flat),
    streaming("enumerate", enumerate),
    streaming("count", count),
    streaming("sum", sum),
    streaming("min", min),
    streaming("max", max),
    streaming("first", first),
    streaming("last", last),
    streaming("any", any),
    streaming("all", all),
    streaming("join", join),
    streaming("lines", lines),
    eager("sort", sort),
    eager("reverse", reverse),
    eager("group", group),
    eager("keys", keys),
    eager("values", values),
    eager("entries", entries),
    eager("from_entries", from_entries),
    eager("get", get),
    eager("has", has),
];

/// Named arguments a builtin accepts, checked against what was passed.
pub(crate) fn named(args: &mut Args, fname: &str, allowed: &[&str]) -> R<HashMap<String, Value>> {
    let mut out = HashMap::new();
    for (n, v, span) in std::mem::take(&mut args.named) {
        if !allowed.contains(&n.as_str()) {
            let help = suggest(&n, allowed.iter().copied()).map(|s| format!("did you mean `{s}`?"));
            let list = if allowed.is_empty() {
                "no named arguments".to_string()
            } else {
                format!("named arguments {}", allowed.join(", "))
            };
            return Err(OkError::at(
                ErrKind::Arg,
                span,
                format!("`{fname}` has no parameter `{n}`"),
            )
            .with_note(format!("`{fname}` takes {list}"))
            .maybe_help(help)
            .into());
        }
        out.insert(n, v);
    }
    Ok(out)
}

/// Check the positional argument count.
pub(crate) fn count_args(args: &Args, fname: &str, min: usize, max: usize) -> R<()> {
    let n = args.pos.len();
    if n < min || n > max {
        let want = if min == max {
            format!("{min}")
        } else {
            format!("{min} to {max}")
        };
        return arg_err(
            args,
            format!(
                "`{fname}` takes {want} argument{}, got {n}",
                if want == "1" { "" } else { "s" }
            ),
        );
    }
    Ok(())
}

pub(crate) fn bool_named(opts: &HashMap<String, Value>, name: &str, args: &Args) -> R<bool> {
    match opts.get(name) {
        None => Ok(false),
        Some(Value::Bool(b)) => Ok(*b),
        Some(other) => type_err(
            args,
            format!("`{name}` must be true or false, got {}", other.a_type()),
        ),
    }
}

/// A size or count limit given as a named argument (`max_size:`,
/// `max_entries:`, `max_line:`): `default` when the argument is absent,
/// `None` (no limit) for `null`, otherwise a positive int.
pub(crate) fn limit_named(
    opts: &HashMap<String, Value>,
    name: &str,
    default: u64,
    args: &Args,
) -> R<Option<u64>> {
    match opts.get(name) {
        None => Ok(Some(default)),
        Some(Value::Null) => Ok(None),
        Some(Value::Int(n)) if *n > 0 => Ok(Some(*n as u64)),
        Some(other) => type_err(
            args,
            format!(
                "`{name}` must be a positive int, or null for no limit, got {}",
                other.repr()
            ),
        ),
    }
}

/// A byte count for messages: whole KiB, MiB or GiB when it is one, so the
/// default limits read as `1 GiB` rather than `1073741824 bytes`.
pub(crate) fn byte_size(n: u64) -> String {
    for (unit, size) in [("GiB", 1u64 << 30), ("MiB", 1 << 20), ("KiB", 1 << 10)] {
        if n >= size && n.is_multiple_of(size) {
            return format!("{} {unit}", n / size);
        }
    }
    if n == 1 {
        "1 byte".to_string()
    } else {
        format!("{n} bytes")
    }
}

fn expect_fn(args: &Args, i: usize, fname: &str) -> R<Value> {
    match args.pos.get(i) {
        Some(f @ Value::Func(_)) => Ok(f.clone()),
        Some(other) => type_err(
            args,
            format!(
                "`{fname}` needs a function as argument {}, got {}",
                i + 1,
                other.a_type()
            ),
        )
        .map_err(|f| {
            with_help(
                f,
                "pass a closure like x => x.name, or shorthand like .name",
            )
        }),
        None => arg_err(args, format!("`{fname}` needs a function")),
    }
}

fn optional_fn(args: &Args, i: usize, fname: &str) -> R<Option<Value>> {
    match args.pos.get(i) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => expect_fn(args, i, fname).map(Some),
    }
}

fn with_help(f: crate::interp::Flow, help: &str) -> crate::interp::Flow {
    match f {
        crate::interp::Flow::Err(mut e) => {
            e.help = Some(help.to_string());
            crate::interp::Flow::Err(e)
        }
        other => other,
    }
}

fn int_pos(args: &Args, i: usize, fname: &str) -> R<i64> {
    match args.pos.get(i) {
        Some(Value::Int(n)) => Ok(*n),
        Some(other) => type_err(
            args,
            format!("`{fname}` needs an int, got {}", other.a_type()),
        ),
        None => arg_err(args, format!("`{fname}` needs a count")),
    }
}

/// Apply a predicate and insist on a bool (SPEC.md 5.3).
fn test(interp: &mut Interp, f: &Value, v: Value, fname: &str, span: Span) -> R<bool> {
    match interp.call(f, vec![v], span)? {
        Value::Bool(b) => Ok(b),
        other => Err(OkError::at(
            ErrKind::Type,
            span,
            format!(
                "the test given to `{fname}` must return a bool, got {}",
                other.a_type()
            ),
        )
        .with_help("compare explicitly, like .count > 0 or .name != \"\"")
        .into()),
    }
}

/// Pull the next element, attaching the consumer's span to source errors.
fn pull(interp: &mut Interp, src: &mut Box<dyn Source>, span: Span) -> R<Option<Value>> {
    at_span(src.next(interp), span)
}

// ----- streaming stages ---------------------------------------------------------

struct Where {
    up: Box<dyn Source>,
    f: Value,
    span: Span,
}

impl Source for Where {
    fn next(&mut self, interp: &mut Interp) -> R<Option<Value>> {
        while let Some(v) = self.up.next(interp)? {
            if test(interp, &self.f, v.clone(), "where", self.span)? {
                return Ok(Some(v));
            }
        }
        Ok(None)
    }
}

fn where_(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "where", &[])?;
    count_args(&args, "where", 2, 2)?;
    let f = expect_fn(&args, 1, "where")?;
    let span = args.span;
    let input = args.pos.swap_remove(0);
    adapt(interp, input, span, "`where`", |up| {
        Box::new(Where { up, f, span })
    })
}

struct Map {
    up: Box<dyn Source>,
    f: Value,
    span: Span,
}

impl Source for Map {
    fn next(&mut self, interp: &mut Interp) -> R<Option<Value>> {
        match self.up.next(interp)? {
            Some(v) => {
                let r = interp.call(&self.f, vec![v], self.span)?;
                // A mapped value is an element of a list, so it is materialised.
                Ok(Some(lazy::force(interp, r, self.span)?))
            }
            None => Ok(None),
        }
    }
}

fn map(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "map", &[])?;
    count_args(&args, "map", 2, 2)?;
    let f = expect_fn(&args, 1, "map")?;
    let span = args.span;
    let input = args.pos.swap_remove(0);
    adapt(interp, input, span, "`map`", |up| {
        Box::new(Map { up, f, span })
    })
}

fn each(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "each", &[])?;
    count_args(&args, "each", 2, 2)?;
    let f = expect_fn(&args, 1, "each")?;
    let span = args.span;
    let mut src = cursor(interp, args.pos.swap_remove(0), span, "`each`")?;
    while let Some(v) = pull(interp, &mut src, span)? {
        interp.call(&f, vec![v], span)?;
    }
    Ok(Value::Null)
}

struct Take {
    up: Box<dyn Source>,
    left: i64,
}

impl Source for Take {
    fn next(&mut self, interp: &mut Interp) -> R<Option<Value>> {
        if self.left <= 0 {
            return Ok(None);
        }
        self.left -= 1;
        self.up.next(interp)
    }
}

fn take(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "take", &[])?;
    count_args(&args, "take", 2, 2)?;
    let n = int_pos(&args, 1, "take")?;
    if n < 0 {
        return arg_err(&args, format!("`take` needs a count of 0 or more, got {n}"));
    }
    let span = args.span;
    let input = args.pos.swap_remove(0);
    adapt(interp, input, span, "`take`", |up| {
        Box::new(Take { up, left: n })
    })
}

struct Skip {
    up: Box<dyn Source>,
    left: i64,
}

impl Source for Skip {
    fn next(&mut self, interp: &mut Interp) -> R<Option<Value>> {
        while self.left > 0 {
            self.left -= 1;
            if self.up.next(interp)?.is_none() {
                return Ok(None);
            }
        }
        self.up.next(interp)
    }
}

fn skip(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "skip", &[])?;
    count_args(&args, "skip", 2, 2)?;
    let n = int_pos(&args, 1, "skip")?;
    if n < 0 {
        return arg_err(&args, format!("`skip` needs a count of 0 or more, got {n}"));
    }
    let span = args.span;
    let input = args.pos.swap_remove(0);
    adapt(interp, input, span, "`skip`", |up| {
        Box::new(Skip { up, left: n })
    })
}

struct Uniq {
    up: Box<dyn Source>,
    key: Option<Value>,
    seen: Seen,
    span: Span,
}

impl Source for Uniq {
    fn next(&mut self, interp: &mut Interp) -> R<Option<Value>> {
        while let Some(v) = self.up.next(interp)? {
            let k = match &self.key {
                Some(f) => interp.call(f, vec![v.clone()], self.span)?,
                None => v.clone(),
            };
            if self.seen.insert(canonical_key(&k)) {
                return Ok(Some(v));
            }
        }
        Ok(None)
    }
}

fn uniq(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "uniq", &[])?;
    count_args(&args, "uniq", 1, 2)?;
    let key = optional_fn(&args, 1, "uniq")?;
    let span = args.span;
    let input = args.pos.swap_remove(0);
    adapt(interp, input, span, "`uniq`", |up| {
        Box::new(Uniq {
            up,
            key,
            seen: Seen::new(),
            span,
        })
    })
}

struct Flat {
    up: Box<dyn Source>,
    pending: VecDeque<Value>,
}

impl Source for Flat {
    fn next(&mut self, interp: &mut Interp) -> R<Option<Value>> {
        loop {
            if let Some(v) = self.pending.pop_front() {
                return Ok(Some(v));
            }
            match self.up.next(interp)? {
                None => return Ok(None),
                Some(Value::List(items)) => self.pending.extend(items.iter().cloned()),
                Some(other) => return Ok(Some(other)),
            }
        }
    }
}

fn flat(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "flat", &[])?;
    count_args(&args, "flat", 1, 1)?;
    let span = args.span;
    let input = args.pos.swap_remove(0);
    adapt(interp, input, span, "`flat`", |up| {
        Box::new(Flat {
            up,
            pending: VecDeque::new(),
        })
    })
}

struct Enumerate {
    up: Box<dyn Source>,
    i: i64,
}

impl Source for Enumerate {
    fn next(&mut self, interp: &mut Interp) -> R<Option<Value>> {
        match self.up.next(interp)? {
            Some(v) => {
                let mut r = Record::new();
                r.insert("index", Value::Int(self.i));
                r.insert("value", v);
                self.i += 1;
                Ok(Some(Value::record(r)))
            }
            None => Ok(None),
        }
    }
}

fn enumerate(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "enumerate", &[])?;
    count_args(&args, "enumerate", 1, 1)?;
    let span = args.span;
    let input = args.pos.swap_remove(0);
    adapt(interp, input, span, "`enumerate`", |up| {
        Box::new(Enumerate { up, i: 0 })
    })
}

fn lines(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "lines", &[])?;
    count_args(&args, "lines", 1, 1)?;
    let span = args.span;
    match args.pos.swap_remove(0) {
        Value::Str(s) => Ok(Value::list(lazy::split_lines(&s))),
        Value::Lazy(l) if l.text => {
            let up = cursor_text(interp, Value::Lazy(l), span)?;
            Ok(lazy::Lazy::list(Box::new(lazy::TextToLines { up })))
        }
        other @ (Value::Lazy(_) | Value::List(_)) => adapt(interp, other, span, "`lines`", |up| {
            Box::new(lazy::SplitEach {
                up,
                pending: VecDeque::new(),
                span,
            })
        }),
        other => type_err(
            &args,
            format!(
                "`lines` needs a string or a list of strings, got {}",
                other.a_type()
            ),
        ),
    }
}

/// The chunk source behind lazy text.
fn cursor_text(interp: &mut Interp, v: Value, span: Span) -> R<Box<dyn Source>> {
    lazy::text_cursor(interp, v, span)
}

// ----- consumers ------------------------------------------------------------------

fn count(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "count", &[])?;
    count_args(&args, "count", 1, 2)?;
    let pred = optional_fn(&args, 1, "count")?;
    let span = args.span;
    let mut src = cursor(interp, args.pos.swap_remove(0), span, "`count`")?;
    let mut n = 0i64;
    while let Some(v) = pull(interp, &mut src, span)? {
        if match &pred {
            Some(f) => test(interp, f, v, "count", span)?,
            None => true,
        } {
            n += 1;
        }
    }
    Ok(Value::Int(n))
}

fn keyed(interp: &mut Interp, key: &Option<Value>, v: &Value, span: Span) -> R<Value> {
    match key {
        Some(f) => {
            let k = interp.call(f, vec![v.clone()], span)?;
            lazy::force(interp, k, span)
        }
        None => Ok(v.clone()),
    }
}

fn sum(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "sum", &[])?;
    count_args(&args, "sum", 1, 2)?;
    let key = optional_fn(&args, 1, "sum")?;
    let span = args.span;
    let mut src = cursor(interp, args.pos.swap_remove(0), span, "`sum`")?;
    let mut total = Value::Int(0);
    let mut first = true;
    while let Some(v) = pull(interp, &mut src, span)? {
        let v = keyed(interp, &key, &v, span)?;
        if !matches!(v, Value::Int(_) | Value::Float(_) | Value::Duration(_)) {
            return Err(OkError::at(
                ErrKind::Type,
                span,
                format!("`sum` needs numbers or durations, got {}", v.a_type()),
            )
            .with_help(
                "CSV values are strings; convert them first, like sum(rows, r => int(r.amount))",
            )
            .into());
        }
        total = if first {
            v
        } else {
            interp.binop(crate::ast::BinOp::Add, total, v, span)?
        };
        first = false;
    }
    Ok(total)
}

fn extreme(interp: &mut Interp, mut args: Args, fname: &str, want: std::cmp::Ordering) -> R<Value> {
    named(&mut args, fname, &[])?;
    count_args(&args, fname, 1, 2)?;
    let key = optional_fn(&args, 1, fname)?;
    let span = args.span;
    let mut src = cursor(interp, args.pos.swap_remove(0), span, &format!("`{fname}`"))?;
    let mut best: Option<(Value, Value)> = None;
    while let Some(v) = pull(interp, &mut src, span)? {
        let k = keyed(interp, &key, &v, span)?;
        let better = match &best {
            None => true,
            Some((bk, _)) => match compare(&k, bk) {
                Some(o) => o == want,
                None => {
                    return Err(OkError::at(
                        ErrKind::Type,
                        span,
                        format!(
                            "`{fname}` cannot compare {} with {}",
                            k.a_type(),
                            bk.a_type()
                        ),
                    )
                    .into());
                }
            },
        };
        if better {
            best = Some((k, v));
        }
    }
    Ok(best.map(|(_, v)| v).unwrap_or(Value::Null))
}

fn min(interp: &mut Interp, args: Args) -> R<Value> {
    extreme(interp, args, "min", std::cmp::Ordering::Less)
}

fn max(interp: &mut Interp, args: Args) -> R<Value> {
    extreme(interp, args, "max", std::cmp::Ordering::Greater)
}

fn first(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "first", &[])?;
    count_args(&args, "first", 1, 2)?;
    let pred = optional_fn(&args, 1, "first")?;
    let span = args.span;
    let mut src = cursor(interp, args.pos.swap_remove(0), span, "`first`")?;
    while let Some(v) = pull(interp, &mut src, span)? {
        let hit = match &pred {
            Some(f) => test(interp, f, v.clone(), "first", span)?,
            None => true,
        };
        if hit {
            return Ok(v);
        }
    }
    Ok(Value::Null)
}

fn last(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "last", &[])?;
    count_args(&args, "last", 1, 2)?;
    let pred = optional_fn(&args, 1, "last")?;
    let span = args.span;
    let mut src = cursor(interp, args.pos.swap_remove(0), span, "`last`")?;
    let mut found = Value::Null;
    while let Some(v) = pull(interp, &mut src, span)? {
        let hit = match &pred {
            Some(f) => test(interp, f, v.clone(), "last", span)?,
            None => true,
        };
        if hit {
            found = v;
        }
    }
    Ok(found)
}

fn any_all(interp: &mut Interp, mut args: Args, fname: &str, want_any: bool) -> R<Value> {
    named(&mut args, fname, &[])?;
    count_args(&args, fname, 2, 2)?;
    let f = expect_fn(&args, 1, fname)?;
    let span = args.span;
    let mut src = cursor(interp, args.pos.swap_remove(0), span, &format!("`{fname}`"))?;
    while let Some(v) = pull(interp, &mut src, span)? {
        let hit = test(interp, &f, v, fname, span)?;
        if hit == want_any {
            return Ok(Value::Bool(want_any));
        }
    }
    Ok(Value::Bool(!want_any))
}

fn any(interp: &mut Interp, args: Args) -> R<Value> {
    any_all(interp, args, "any", true)
}

fn all(interp: &mut Interp, args: Args) -> R<Value> {
    any_all(interp, args, "all", false)
}

fn join(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "join", &[])?;
    count_args(&args, "join", 1, 2)?;
    let sep = match args.pos.get(1) {
        None => String::new(),
        Some(Value::Str(s)) => s.to_string(),
        Some(other) => {
            return type_err(
                &args,
                format!(
                    "the separator of `join` must be a string, got {}",
                    other.a_type()
                ),
            );
        }
    };
    let span = args.span;
    let mut src = cursor(interp, args.pos.swap_remove(0), span, "`join`")?;
    let mut out = String::new();
    let mut first = true;
    while let Some(v) = pull(interp, &mut src, span)? {
        if matches!(
            v,
            Value::List(_) | Value::Record(_) | Value::Bytes(_) | Value::Func(_)
        ) {
            return Err(OkError::at(
                ErrKind::Type,
                span,
                format!("`join` can only join text and numbers, got {}", v.a_type()),
            )
            .into());
        }
        if !first {
            out.push_str(&sep);
        }
        first = false;
        out.push_str(&v.display());
    }
    Ok(Value::str(out))
}

// ----- materialising functions -------------------------------------------------

fn list_arg(args: &Args, fname: &str) -> R<Rc<crate::value::List>> {
    match args.pos.first() {
        Some(Value::List(items)) => Ok(items.clone()),
        Some(other) => type_err(
            args,
            format!("`{fname}` needs a list, got {}", other.a_type()),
        ),
        None => arg_err(args, format!("`{fname}` needs a list")),
    }
}

fn record_arg(args: &Args, fname: &str) -> R<Rc<Record>> {
    match args.pos.first() {
        Some(Value::Record(r)) => Ok(r.clone()),
        Some(other) => type_err(
            args,
            format!("`{fname}` needs a record, got {}", other.a_type()),
        ),
        None => arg_err(args, format!("`{fname}` needs a record")),
    }
}

fn sort(interp: &mut Interp, mut args: Args) -> R<Value> {
    let opts = named(&mut args, "sort", &["desc"])?;
    count_args(&args, "sort", 1, 2)?;
    let desc = bool_named(&opts, "desc", &args)?;
    let items = list_arg(&args, "sort")?;
    let key = optional_fn(&args, 1, "sort")?;
    let span = args.span;
    let mut keyed_items = Vec::with_capacity(items.len());
    for v in items.iter() {
        keyed_items.push((keyed(interp, &key, v, span)?, v.clone()));
    }
    let mut problem: Option<(String, String)> = None;
    // The comparator must be a total order even when some keys cannot be
    // compared (the standard sort may panic otherwise), so incomparable
    // pairs are ordered by `total_order`'s fallback and reported afterwards.
    keyed_items.sort_by(|(a, _), (b, _)| {
        let (ord, comparable) = total_order(a, b);
        if !comparable {
            problem.get_or_insert((a.a_type(), b.a_type()));
        }
        if desc { ord.reverse() } else { ord }
    });
    if let Some((a, b)) = problem {
        return Err(OkError::at(
            ErrKind::Type,
            span,
            format!("`sort` cannot compare {a} with {b}"),
        )
        .with_help("sort by a key that gives comparable values, like sort(.name)")
        .into());
    }
    Ok(Value::list(
        keyed_items.into_iter().map(|(_, v)| v).collect(),
    ))
}

fn reverse(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "reverse", &[])?;
    count_args(&args, "reverse", 1, 1)?;
    let items = list_arg(&args, "reverse")?;
    Ok(Value::list(items.iter().rev().cloned().collect()))
}

fn group(interp: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "group", &[])?;
    count_args(&args, "group", 2, 2)?;
    let items = list_arg(&args, "group")?;
    let key = Some(expect_fn(&args, 1, "group")?);
    let span = args.span;
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for v in items.iter() {
        let k = keyed(interp, &key, v, span)?;
        if matches!(
            k,
            Value::List(_) | Value::Record(_) | Value::Bytes(_) | Value::Func(_)
        ) {
            return type_err(
                &args,
                format!("`group` keys must be text or numbers, got {}", k.a_type()),
            );
        }
        let k = k.display();
        match index.get(&k) {
            Some(&i) => groups[i].1.push(v.clone()),
            None => {
                index.insert(k.clone(), groups.len());
                groups.push((k, vec![v.clone()]));
            }
        }
    }
    let mut r = Record::new();
    for (k, vs) in groups {
        r.insert(k.as_str(), Value::list(vs));
    }
    Ok(Value::record(r))
}

fn keys(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "keys", &[])?;
    count_args(&args, "keys", 1, 1)?;
    let r = record_arg(&args, "keys")?;
    Ok(Value::list(
        r.keys().map(|k| Value::Str(k.clone())).collect(),
    ))
}

fn values(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "values", &[])?;
    count_args(&args, "values", 1, 1)?;
    let r = record_arg(&args, "values")?;
    Ok(Value::list(r.iter().map(|(_, v)| v.clone()).collect()))
}

fn entries(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "entries", &[])?;
    count_args(&args, "entries", 1, 1)?;
    let r = record_arg(&args, "entries")?;
    Ok(Value::list(
        r.iter()
            .map(|(k, v)| {
                let mut e = Record::new();
                e.insert("key", Value::Str(k.clone()));
                e.insert("value", v.clone());
                Value::record(e)
            })
            .collect(),
    ))
}

fn from_entries(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "from_entries", &[])?;
    count_args(&args, "from_entries", 1, 1)?;
    let items = list_arg(&args, "from_entries")?;
    let mut r = Record::new();
    for (i, e) in items.iter().enumerate() {
        let (Value::Record(e), true) = (e, true) else {
            return type_err(
                &args,
                format!(
                    "`from_entries` needs records like {{key, value}}; element {i} is {}",
                    e.a_type()
                ),
            );
        };
        let key = match e.get("key") {
            Some(Value::Str(k)) => k.clone(),
            Some(other) => {
                return type_err(
                    &args,
                    format!(
                        "entry {i} has a key that is {}, not a string",
                        other.a_type()
                    ),
                );
            }
            None => return arg_err(&args, format!("entry {i} has no `key` field")),
        };
        r.insert(key, e.get("value").cloned().unwrap_or(Value::Null));
    }
    Ok(Value::record(r))
}

fn get(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "get", &[])?;
    count_args(&args, "get", 2, 3)?;
    let r = record_arg(&args, "get")?;
    let Value::Str(k) = &args.pos[1] else {
        return type_err(
            &args,
            format!("record keys are strings, got {}", args.pos[1].a_type()),
        );
    };
    Ok(r.get(k)
        .cloned()
        .unwrap_or_else(|| args.pos.get(2).cloned().unwrap_or(Value::Null)))
}

fn has(_: &mut Interp, mut args: Args) -> R<Value> {
    named(&mut args, "has", &[])?;
    count_args(&args, "has", 2, 2)?;
    let r = record_arg(&args, "has")?;
    let Value::Str(k) = &args.pos[1] else {
        return type_err(
            &args,
            format!("record keys are strings, got {}", args.pos[1].a_type()),
        );
    };
    Ok(Value::Bool(r.contains(k)))
}
