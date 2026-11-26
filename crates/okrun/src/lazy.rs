//! Lazy values: streaming lists and text (SPEC.md 8.3, 13.2).
//!
//! `read` of a line- or row-oriented format, `lines`, `range` and `stdin()`
//! produce a [`Lazy`] instead of a finished list or string. Streaming stages
//! (`where`, `map`, `take`, ...) wrap it in another lazy value, and consumers
//! (`count`, `each`, `write`, `for`, ...) pull elements one at a time, so a
//! pipeline over a large file never holds it in memory.
//!
//! Laziness never escapes into ordinary code: binding a lazy value to a name,
//! storing it in a list or record, passing it to a function that does not
//! stream, or printing it **materialises** it first (see `Interp::eval`). A
//! lazy value is therefore consumed at most once; it either streams through
//! one pipeline or becomes a normal value.
//!
//! Pulling an element may call back into ok code (a `where` predicate), so
//! sources take the interpreter as an argument instead of implementing
//! `Iterator`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::io::{BufRead, Read};
use std::rc::Rc;

use crate::error::{ErrKind, OkError};
use crate::interp::{Flow, Interp, R};
use crate::source::Span;
use crate::value::Value;

/// A producer of elements. Errors from sources usually have no span; the
/// consumer attaches its own with [`at_span`].
pub trait Source {
    fn next(&mut self, interp: &mut Interp) -> R<Option<Value>>;

    /// For the stdout of a piped command: hand over the raw pipe so the next
    /// command can read it directly (`run a | run b`).
    fn take_child_stdout(&mut self) -> Option<std::process::Stdio> {
        None
    }

    /// Finish without reading further, reporting failures (used after a
    /// command's stdout was handed to another command).
    fn finish(&mut self) -> R<()> {
        Ok(())
    }
}

enum State {
    Pending(Box<dyn Source>),
    /// Handed to a streaming consumer.
    Taken,
    Done(Value),
}

pub struct Lazy {
    /// A lazy string (its elements are chunks of text, normally lines with
    /// their line endings) rather than a lazy list.
    pub text: bool,
    state: RefCell<State>,
}

impl Lazy {
    pub fn list(src: Box<dyn Source>) -> Value {
        Value::Lazy(Rc::new(Lazy {
            text: false,
            state: RefCell::new(State::Pending(src)),
        }))
    }

    pub fn text(src: Box<dyn Source>) -> Value {
        Value::Lazy(Rc::new(Lazy {
            text: true,
            state: RefCell::new(State::Pending(src)),
        }))
    }
}

/// Give an error from a source the span of the consuming expression.
pub fn at_span<T>(r: R<T>, span: Span) -> R<T> {
    r.map_err(|f| match f {
        Flow::Err(e) => Flow::Err(Box::new(e.or_span(span))),
        other => other,
    })
}

/// Materialise a lazy value; any other value is returned unchanged.
pub fn force(interp: &mut Interp, v: Value, span: Span) -> R<Value> {
    let Value::Lazy(lazy) = v else { return Ok(v) };
    let state = std::mem::replace(&mut *lazy.state.borrow_mut(), State::Taken);
    match state {
        State::Done(v) => {
            *lazy.state.borrow_mut() = State::Done(v.clone());
            Ok(v)
        }
        State::Taken => Err(OkError::at(
            ErrKind::Value,
            span,
            "internal error: a streamed value was used twice",
        )
        .into()),
        State::Pending(mut src) => {
            let v = if lazy.text {
                let mut s = String::new();
                while let Some(chunk) = at_span(src.next(interp), span)? {
                    if let Value::Str(c) = chunk {
                        s.push_str(&c);
                    }
                }
                Value::str(s)
            } else {
                let mut items = Vec::new();
                while let Some(item) = at_span(src.next(interp), span)? {
                    items.push(item);
                }
                Value::list(items)
            };
            *lazy.state.borrow_mut() = State::Done(v.clone());
            Ok(v)
        }
    }
}

/// A source over the elements of a list or lazy list. Lazy text and strings
/// are not lists; `what` names the function for the error message.
pub fn cursor(interp: &mut Interp, v: Value, span: Span, what: &str) -> R<Box<dyn Source>> {
    match v {
        Value::Lazy(lazy) if !lazy.text => {
            let state = std::mem::replace(&mut *lazy.state.borrow_mut(), State::Taken);
            match state {
                State::Pending(src) => Ok(src),
                State::Done(v) => {
                    *lazy.state.borrow_mut() = State::Done(v.clone());
                    cursor(interp, v, span, what)
                }
                State::Taken => Err(OkError::at(
                    ErrKind::Value,
                    span,
                    "internal error: a streamed value was used twice",
                )
                .into()),
            }
        }
        Value::List(items) => Ok(Box::new(VecSource { items, i: 0 })),
        other => {
            let other = force(interp, other, span)?;
            let mut e = OkError::at(
                ErrKind::Type,
                span,
                format!("{what} needs a list, got {}", other.a_type()),
            );
            if let Value::Str(_) = other {
                e = e.with_help("split text into a list first, for example with lines(s)");
            }
            Err(e.into())
        }
    }
}

/// Whether a value streams (a lazy list), so a stage should stay lazy.
pub fn is_lazy(v: &Value) -> bool {
    matches!(v, Value::Lazy(_))
}

/// Pull everything from a source into a list.
pub fn collect(interp: &mut Interp, mut src: Box<dyn Source>, span: Span) -> R<Value> {
    let mut items = Vec::new();
    while let Some(v) = at_span(src.next(interp), span)? {
        items.push(v);
    }
    Ok(Value::list(items))
}

/// Run a streaming stage: lazy input gives lazy output, a plain list gives a
/// plain list.
pub fn adapt(
    interp: &mut Interp,
    input: Value,
    span: Span,
    what: &str,
    make: impl FnOnce(Box<dyn Source>) -> Box<dyn Source>,
) -> R<Value> {
    let lazy = is_lazy(&input);
    let src = make(cursor(interp, input, span, what)?);
    if lazy {
        Ok(Lazy::list(src))
    } else {
        collect(interp, src, span)
    }
}

// ----- basic sources ----------------------------------------------------------

pub struct VecSource {
    items: Rc<Vec<Value>>,
    i: usize,
}

impl Source for VecSource {
    fn next(&mut self, _: &mut Interp) -> R<Option<Value>> {
        let v = self.items.get(self.i).cloned();
        self.i += 1;
        Ok(v)
    }
}

pub struct RangeSource {
    pub next: i64,
    pub end: i64,
    pub step: i64,
    pub done: bool,
}

impl Source for RangeSource {
    fn next(&mut self, _: &mut Interp) -> R<Option<Value>> {
        if self.done
            || (self.step > 0 && self.next >= self.end)
            || (self.step < 0 && self.next <= self.end)
        {
            return Ok(None);
        }
        let v = self.next;
        match self.next.checked_add(self.step) {
            Some(n) => self.next = n,
            None => self.done = true,
        }
        Ok(Some(Value::Int(v)))
    }
}

/// Lines of a reader, each with its line ending, as strings. Invalid UTF-8
/// is an error naming the line, and so is a line longer than `max_line`
/// bytes, which keeps memory bounded when the data has no line breaks.
pub struct ReaderLines {
    reader: Box<dyn BufRead>,
    name: String,
    line: usize,
    max_line: Option<u64>,
}

impl ReaderLines {
    pub fn new(reader: Box<dyn BufRead>, name: String) -> ReaderLines {
        ReaderLines {
            reader,
            name,
            line: 0,
            max_line: None,
        }
    }

    /// Limit each line to `max` bytes (`None`: no limit).
    pub fn with_max_line(mut self, max: Option<u64>) -> ReaderLines {
        self.max_line = max;
        self
    }
}

impl Source for ReaderLines {
    fn next(&mut self, _: &mut Interp) -> R<Option<Value>> {
        let mut buf = Vec::new();
        let read_err = |e: std::io::Error, name: &str| {
            OkError::new(ErrKind::Io, format!("cannot read {name}: {e}"))
        };
        let n = match self.max_line {
            // One byte more than the limit, plus the line ending, shows
            // whether the line goes past it.
            Some(max) => (&mut self.reader)
                .take(max.saturating_add(2))
                .read_until(b'\n', &mut buf),
            None => self.reader.read_until(b'\n', &mut buf),
        }
        .map_err(|e| read_err(e, &self.name))?;
        if n == 0 {
            return Ok(None);
        }
        self.line += 1;
        if let Some(max) = self.max_line {
            let body = strip_eol_bytes(&buf).len() as u64;
            if body > max {
                return Err(OkError::new(
                    ErrKind::Io,
                    format!(
                        "cannot read {}: line {} is longer than {}",
                        self.name,
                        self.line,
                        crate::list::byte_size(max)
                    ),
                )
                .with_help("pass max_line: (in bytes) to allow longer lines, or null for no limit")
                .into());
            }
        }
        if self.line == 1 && buf.starts_with(b"\xEF\xBB\xBF") {
            buf.drain(..3);
        }
        match String::from_utf8(buf) {
            Ok(s) => Ok(Some(Value::str(s))),
            Err(_) => Err(OkError::new(
                ErrKind::Format,
                format!(
                    "line {} of {} is not valid UTF-8 text",
                    self.line, self.name
                ),
            )
            .with_help("read binary data with read(p, \"bytes\")")
            .into()),
        }
    }
}

/// `buf` without one trailing `\n` or `\r\n`.
fn strip_eol_bytes(buf: &[u8]) -> &[u8] {
    let b = buf.strip_suffix(b"\n").unwrap_or(buf);
    if b.len() < buf.len() {
        b.strip_suffix(b"\r").unwrap_or(b)
    } else {
        b
    }
}

/// Strip one trailing `\n` or `\r\n`.
pub fn strip_eol(s: &str) -> &str {
    s.strip_suffix('\n')
        .map(|t| t.strip_suffix('\r').unwrap_or(t))
        .unwrap_or(s)
}

/// Split a string into lines (SPEC.md 12.4): `\n` and `\r\n` end lines, and
/// a final empty line is dropped.
pub fn split_lines(s: &str) -> Vec<Value> {
    if s.is_empty() {
        return Vec::new();
    }
    let body = s
        .strip_suffix('\n')
        .map(|t| t.strip_suffix('\r').unwrap_or(t))
        .unwrap_or(s);
    body.split('\n')
        .map(|l| Value::str(l.strip_suffix('\r').unwrap_or(l)))
        .collect()
}

/// `lines` over lazy text: chunks are whole lines with their endings.
pub struct TextToLines {
    pub up: Box<dyn Source>,
}

impl Source for TextToLines {
    fn next(&mut self, interp: &mut Interp) -> R<Option<Value>> {
        match self.up.next(interp)? {
            Some(Value::Str(s)) => Ok(Some(Value::str(strip_eol(&s)))),
            Some(other) => Ok(Some(other)),
            None => Ok(None),
        }
    }
}

/// `lines` over a list of strings: split each and flatten.
pub struct SplitEach {
    pub up: Box<dyn Source>,
    pub pending: std::collections::VecDeque<Value>,
    pub span: Span,
}

impl Source for SplitEach {
    fn next(&mut self, interp: &mut Interp) -> R<Option<Value>> {
        loop {
            if let Some(v) = self.pending.pop_front() {
                return Ok(Some(v));
            }
            match self.up.next(interp)? {
                None => return Ok(None),
                Some(Value::Str(s)) => self.pending.extend(split_lines(&s)),
                Some(other) => {
                    return Err(OkError::at(
                        ErrKind::Type,
                        self.span,
                        format!("`lines` needs strings, got {} in the list", other.a_type()),
                    )
                    .into());
                }
            }
        }
    }
}

/// A canonical key for a value, used by `uniq`: equal values (records in any
/// key order, `1` and `1.0`) get equal keys.
pub fn canonical_key(v: &Value) -> String {
    match v {
        Value::Record(r) => {
            let mut parts: Vec<(String, String)> = r
                .iter()
                .map(|(k, v)| (k.to_string(), canonical_key(v)))
                .collect();
            parts.sort();
            let inner: Vec<String> = parts
                .into_iter()
                .map(|(k, v)| format!("{k:?}:{v}"))
                .collect();
            format!("{{{}}}", inner.join(","))
        }
        Value::List(items) => format!(
            "[{}]",
            items
                .iter()
                .map(canonical_key)
                .collect::<Vec<_>>()
                .join(",")
        ),
        // An int and a float are equal exactly when the float is integral
        // and holds the same number (`value::equal`), so both get the same
        // key. -2^63 <= f < 2^63 makes the conversion exact.
        Value::Int(i) => format!("int:{i}"),
        Value::Float(f)
            if f.fract() == 0.0
                && *f >= -9.223_372_036_854_775_808e18
                && *f < 9.223_372_036_854_775_808e18 =>
        {
            format!("int:{}", *f as i64)
        }
        other => format!("{}:{}", other.type_name(), other.repr()),
    }
}

/// A set of canonical keys, for `uniq`.
pub type Seen = HashSet<String>;

/// A chunk source for text: lazy text streams its chunks, a plain string is
/// one chunk.
pub fn text_cursor(interp: &mut Interp, v: Value, span: Span) -> R<Box<dyn Source>> {
    match v {
        Value::Lazy(lazy) if lazy.text => {
            let state = std::mem::replace(&mut *lazy.state.borrow_mut(), State::Taken);
            match state {
                State::Pending(src) => Ok(src),
                State::Done(v) => {
                    *lazy.state.borrow_mut() = State::Done(v.clone());
                    text_cursor(interp, v, span)
                }
                State::Taken => Err(OkError::at(
                    ErrKind::Value,
                    span,
                    "internal error: a streamed value was used twice",
                )
                .into()),
            }
        }
        Value::Str(s) => Ok(Box::new(VecSource {
            items: Rc::new(vec![Value::Str(s)]),
            i: 0,
        })),
        other => {
            let other = force(interp, other, span)?;
            Err(OkError::at(
                ErrKind::Type,
                span,
                format!("expected text, got {}", other.a_type()),
            )
            .into())
        }
    }
}
