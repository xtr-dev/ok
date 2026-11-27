//! Source files and byte spans.
//!
//! Every token, AST node and error carries a [`Span`]: a file id plus a byte
//! range into that file's text. The [`SourceMap`] owns the text of every file
//! loaded during a run (the script, its imports, `-e` source) so errors can be
//! rendered with a snippet long after lexing.

use std::path::PathBuf;
use std::rc::Rc;

/// Index of a file in the [`SourceMap`].
pub type FileId = u32;

/// A byte range `start..end` in one source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub struct Span {
    pub file: FileId,
    pub start: u32,
    pub end: u32,
}

impl Span {
    pub fn new(file: FileId, start: usize, end: usize) -> Span {
        Span {
            file,
            start: start as u32,
            end: end as u32,
        }
    }

    /// The smallest span covering both `self` and `other`, which must be in
    /// the same file.
    pub fn to(self, other: Span) -> Span {
        Span {
            file: self.file,
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

/// One loaded source file.
pub struct SourceFile {
    /// Name shown in error messages: the path as the user gave it, or `<-e>`.
    pub name: String,
    /// Location on disk, used to resolve relative imports. `None` for `-e`.
    pub path: Option<PathBuf>,
    pub text: Rc<str>,
    /// Byte offset where each line starts. The first entry is 0.
    line_starts: Vec<usize>,
}

impl SourceFile {
    /// 1-based line and column (in characters) of a byte offset. An offset
    /// past the end of the text counts as the end.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.text.len());
        let line = match self.line_starts.binary_search(&offset) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        let start = self.line_starts[line];
        let col = self.text[start..offset].chars().count() + 1;
        (line + 1, col)
    }

    /// Text of a 1-based line without its line ending. Panics when the file
    /// has no such line.
    pub fn line_text(&self, line: usize) -> &str {
        let start = self.line_starts[line - 1];
        let end = self
            .line_starts
            .get(line)
            .copied()
            .unwrap_or(self.text.len());
        self.text[start..end].trim_end_matches(['\n', '\r'])
    }

    /// Byte offset where a 1-based line starts. Panics when the file has no
    /// such line.
    pub fn line_start(&self, line: usize) -> usize {
        self.line_starts[line - 1]
    }
}

/// All files loaded during one run.
#[derive(Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    /// Adds a file and returns its id. Ids stay valid for the life of the
    /// map, because files are never removed.
    pub fn add(&mut self, name: String, path: Option<PathBuf>, text: Rc<str>) -> FileId {
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        self.files.push(SourceFile {
            name,
            path,
            text,
            line_starts,
        });
        (self.files.len() - 1) as FileId
    }

    /// The file with this id. Panics when `id` did not come from
    /// [`SourceMap::add`] on this map.
    pub fn file(&self, id: FileId) -> &SourceFile {
        &self.files[id as usize]
    }
}
