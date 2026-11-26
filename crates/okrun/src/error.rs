//! Errors and their rendering (SPEC.md section 10).
//!
//! An [`OkError`] is what every failing operation produces: lexing, parsing,
//! loading and evaluation all use the same type, so all errors render the
//! same way: kind, message, `file:line:col`, the source line with the span
//! underlined, then notes, help and a short call trace.

use std::fmt::Write as _;

use crate::source::{SourceMap, Span};

/// The error kinds visible to scripts as `e.kind` (SPEC.md 10.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrKind {
    Syntax,
    Name,
    Type,
    Value,
    Index,
    Key,
    Arg,
    Io,
    Format,
    Command,
    Timeout,
    Net,
    Fail,
    Trust,
    /// A feature specified in SPEC.md whose milestone has not been reached.
    /// Not catchable as a distinct kind by scripts; it reports as `value`.
    Unimplemented,
}

impl ErrKind {
    pub fn name(self) -> &'static str {
        match self {
            ErrKind::Syntax => "syntax",
            ErrKind::Name => "name",
            ErrKind::Type => "type",
            ErrKind::Value | ErrKind::Unimplemented => "value",
            ErrKind::Index => "index",
            ErrKind::Key => "key",
            ErrKind::Arg => "arg",
            ErrKind::Io => "io",
            ErrKind::Format => "format",
            ErrKind::Command => "command",
            ErrKind::Timeout => "timeout",
            ErrKind::Net => "net",
            ErrKind::Fail => "fail",
            ErrKind::Trust => "trust",
        }
    }

    pub fn from_name(name: &str) -> Option<ErrKind> {
        Some(match name {
            "syntax" => ErrKind::Syntax,
            "name" => ErrKind::Name,
            "type" => ErrKind::Type,
            "value" => ErrKind::Value,
            "index" => ErrKind::Index,
            "key" => ErrKind::Key,
            "arg" => ErrKind::Arg,
            "io" => ErrKind::Io,
            "format" => ErrKind::Format,
            "command" => ErrKind::Command,
            "timeout" => ErrKind::Timeout,
            "net" => ErrKind::Net,
            "fail" => ErrKind::Fail,
            "trust" => ErrKind::Trust,
            _ => return None,
        })
    }
}

/// One entry of the call trace: the function that was running and where it
/// was called from.
#[derive(Clone, Debug)]
pub struct Frame {
    pub function: String,
    /// Where the function was called; `None` when ok itself started it (a
    /// script's `main`, or the task named on the command line), which has
    /// no call site in the source.
    pub call_site: Option<Span>,
}

/// Maximum number of call-trace frames shown under an error.
pub const MAX_TRACE: usize = 8;

#[derive(Clone, Debug)]
pub struct OkError {
    pub kind: ErrKind,
    pub message: String,
    pub span: Option<Span>,
    /// Short text printed after the underline, e.g. "not defined here".
    pub label: Option<String>,
    pub notes: Vec<String>,
    pub help: Option<String>,
    pub trace: Vec<Frame>,
    /// Process exit code to use when this error is uncaught (command errors
    /// exit with the child's code). `None` means the default, 1.
    pub exit_code: Option<i32>,
    /// Labelled detail lines shown as `= label: text` (for example
    /// `command: cargo test`). Text after the first line is indented.
    pub details: Vec<String>,
    /// The command of a command error, as its argument list.
    pub command: Option<Vec<String>>,
    /// The captured stderr tail of a command error.
    pub stderr: Option<String>,
    /// HTTP status of a `net` error that got a response.
    pub status: Option<u16>,
    /// Text printed after everything else, such as a usage line.
    pub trailer: Option<String>,
    /// A finding of `ok self check` that does not stop the program from
    /// running (a lint); rendered as `warning[kind]:`.
    pub warning: bool,
}

impl OkError {
    pub fn new(kind: ErrKind, message: impl Into<String>) -> OkError {
        OkError {
            kind,
            message: message.into(),
            span: None,
            label: None,
            notes: Vec::new(),
            help: None,
            trace: Vec::new(),
            exit_code: None,
            details: Vec::new(),
            command: None,
            stderr: None,
            status: None,
            trailer: None,
            warning: false,
        }
    }

    /// Turn the error into a warning.
    pub fn as_warning(mut self) -> OkError {
        self.warning = true;
        self
    }

    pub fn at(kind: ErrKind, span: Span, message: impl Into<String>) -> OkError {
        OkError::new(kind, message).with_span(span)
    }

    pub fn with_span(mut self, span: Span) -> OkError {
        self.span = Some(span);
        self
    }

    /// Set the span only if the error does not have one yet.
    pub fn or_span(mut self, span: Span) -> OkError {
        if self.span.is_none() {
            self.span = Some(span);
        }
        self
    }

    pub fn with_label(mut self, label: impl Into<String>) -> OkError {
        self.label = Some(label.into());
        self
    }

    pub fn with_help(mut self, help: impl Into<String>) -> OkError {
        self.help = Some(help.into());
        self
    }

    pub fn with_note(mut self, note: impl Into<String>) -> OkError {
        self.notes.push(note.into());
        self
    }

    pub fn maybe_note(mut self, note: Option<String>) -> OkError {
        self.notes.extend(note);
        self
    }

    pub fn maybe_help(mut self, help: Option<String>) -> OkError {
        if help.is_some() {
            self.help = help;
        }
        self
    }

    pub fn unimplemented(span: Span, what: &str, milestone: &str) -> OkError {
        OkError::at(
            ErrKind::Unimplemented,
            span,
            format!("{what} is not implemented yet (milestone {milestone})"),
        )
    }
}

/// Whether to use ANSI colors when rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Color {
    Never,
    Always,
}

struct Paint {
    on: bool,
}

impl Paint {
    fn wrap(&self, code: &str, text: &str) -> String {
        if self.on {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_string()
        }
    }
    fn error(&self, t: &str) -> String {
        self.wrap("1;31", t)
    }
    fn warning(&self, t: &str) -> String {
        self.wrap("1;33", t)
    }
    fn bold(&self, t: &str) -> String {
        self.wrap("1", t)
    }
    fn gutter(&self, t: &str) -> String {
        self.wrap("1;34", t)
    }
    fn help(&self, t: &str) -> String {
        self.wrap("1;36", t)
    }
}

/// Render an error the way SPEC.md 10.1 shows it. Errors with a source
/// location show `error[kind]:`; errors without one (command-line and load
/// errors) show plain `error:`.
pub fn render(err: &OkError, sources: &SourceMap, color: Color) -> String {
    let p = Paint {
        on: color == Color::Always,
    };
    let mut out = String::new();
    let word = if err.warning { "warning" } else { "error" };
    let head = match err.span {
        Some(_) => format!("{word}[{}]", err.kind.name()),
        None => word.to_string(),
    };
    let head = if err.warning {
        p.warning(&head)
    } else {
        p.error(&head)
    };
    let _ = writeln!(out, "{head}{}", p.bold(&format!(": {}", err.message)));

    let mut gutter_width = 1;
    if let Some(span) = err.span {
        let file = sources.file(span.file);
        let (line, col) = file.line_col(span.start as usize);
        let line_str = line.to_string();
        gutter_width = line_str.len();
        let pad = " ".repeat(gutter_width);
        let _ = writeln!(out, "{pad}{} {}:{line}:{col}", p.gutter("-->"), file.name);
        let _ = writeln!(out, "{pad} {}", p.gutter("|"));

        let text = file.line_text(line);
        let line_start = file.line_start(line);
        // Columns are computed on the displayed text, where tabs become 4 spaces.
        let start_in_line = (span.start as usize)
            .saturating_sub(line_start)
            .min(text.len());
        let end_in_line = (span.end as usize)
            .saturating_sub(line_start)
            .min(text.len());
        let display_width = |s: &str| {
            s.chars()
                .map(|c| if c == '\t' { 4 } else { 1 })
                .sum::<usize>()
        };
        let before = safe_slice(text, 0, start_in_line);
        let inside = safe_slice(text, start_in_line, end_in_line.max(start_in_line));
        let caret_pad = display_width(before);
        let carets = display_width(inside).max(1);
        let shown = text.replace('\t', "    ");
        let _ = writeln!(out, "{} {} {shown}", p.gutter(&line_str), p.gutter("|"));
        let mut underline = format!("{}{}", " ".repeat(caret_pad), "^".repeat(carets));
        if let Some(label) = &err.label {
            underline.push(' ');
            underline.push_str(label);
        }
        let _ = writeln!(out, "{pad} {} {}", p.gutter("|"), p.error(&underline));
    }

    let pad = " ".repeat(gutter_width);
    for detail in &err.details {
        let mut lines = detail.lines();
        if let Some(first) = lines.next() {
            let _ = writeln!(out, "{pad} {} {first}", p.gutter("="));
        }
        for more in lines {
            let _ = writeln!(out, "{pad}     {more}");
        }
    }
    let has_extra = !err.notes.is_empty() || err.help.is_some() || !err.trace.is_empty();
    if err.span.is_some() && has_extra {
        let _ = writeln!(out, "{pad} {}", p.gutter("|"));
    }
    for note in &err.notes {
        let _ = writeln!(out, "{pad} {} {}{note}", p.gutter("="), p.bold("note: "));
    }
    let frames: Vec<&Frame> = err.trace.iter().take(MAX_TRACE).collect();
    let mut i = 0;
    while i < frames.len() {
        let frame = frames[i];
        // Recursion produces identical frames; show each run once.
        let mut run = 1;
        while i + run < frames.len()
            && frames[i + run].function == frame.function
            && frames[i + run].call_site == frame.call_site
        {
            run += 1;
        }
        let repeat = if run > 1 { " (repeated)" } else { "" };
        let site = match frame.call_site {
            Some(span) => {
                let file = sources.file(span.file);
                let (line, col) = file.line_col(span.start as usize);
                format!("called at {}:{line}:{col}", file.name)
            }
            None => "started by ok".to_string(),
        };
        let _ = writeln!(
            out,
            "{pad} {} in `{}`, {site}{repeat}",
            p.gutter("="),
            frame.function,
        );
        i += run;
    }
    if let Some(help) = &err.help {
        let _ = writeln!(out, "{pad} {} {}{help}", p.gutter("="), p.help("help: "));
    }
    if let Some(t) = &err.trailer {
        let _ = writeln!(out, "{t}");
    }
    out
}

/// Slice `s[a..b]`, moving the bounds back to character boundaries.
fn safe_slice(s: &str, mut a: usize, mut b: usize) -> &str {
    while a > 0 && !s.is_char_boundary(a) {
        a -= 1;
    }
    while b > a && !s.is_char_boundary(b) {
        b -= 1;
    }
    &s[a..b]
}

/// Edit distance with adjacent transpositions (optimal string alignment).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[a.len()][b.len()]
}

/// The closest candidate to `name`: a case-only difference, or an edit
/// distance of at most 2 (at most 1 for names of three characters or fewer,
/// so that short names do not suggest unrelated ones).
pub fn suggest<'a>(name: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<String> {
    let limit = if name.chars().count() <= 3 { 1 } else { 2 };
    let mut best: Option<(usize, &str)> = None;
    for c in candidates {
        if c == name {
            continue;
        }
        let d = if c.eq_ignore_ascii_case(name) {
            0
        } else {
            edit_distance(name, c)
        };
        if d <= limit && best.is_none_or(|(bd, bc)| d < bd || (d == bd && c < bc)) {
            best = Some((d, c));
        }
    }
    best.map(|(_, c)| c.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_distance_counts_transpositions_once() {
        assert_eq!(edit_distance("relase", "release"), 1);
        assert_eq!(edit_distance("lenght", "length"), 1);
        assert_eq!(edit_distance("abc", "abc"), 0);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn suggest_picks_the_closest_name() {
        let names = ["release", "version", "verbose"];
        assert_eq!(suggest("relase", names), Some("release".into()));
        assert_eq!(suggest("verbos", names), Some("verbose".into()));
        assert_eq!(suggest("zzz", names), None);
        assert_eq!(suggest("Release", names), Some("release".into()));
    }
}
