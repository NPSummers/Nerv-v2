use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// Stable within a compilation. IDs are assigned once and retained by AST clones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SourceId(pub usize);

/// Half-open UTF-8 byte offsets into a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub source: SourceId,
    pub start: usize,
    pub end: usize,
}

#[derive(Debug, Clone)]
pub struct SourceFile {
    pub id: SourceId,
    pub path: PathBuf,
    pub text: Arc<str>,
}

impl SourceFile {
    pub fn location(&self, offset: usize) -> (usize, usize) {
        let offset = self.boundary(offset);
        let prefix = &self.text[..offset];
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
        (line, column)
    }

    fn boundary(&self, offset: usize) -> usize {
        let mut offset = offset.min(self.text.len());
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    pub fn offset(&self, line: usize, column: usize) -> usize {
        let start = self
            .text
            .split_inclusive('\n')
            .take(line.saturating_sub(1))
            .map(str::len)
            .sum::<usize>();
        let tail = &self.text[start.min(self.text.len())..];
        let line = tail.split(['\r', '\n']).next().unwrap_or("");
        start
            + line
                .char_indices()
                .nth(column.saturating_sub(1))
                .map_or(line.len(), |(i, _)| i)
    }

    pub fn render(&self, span: Span, message: &str, notes: &[String]) -> String {
        let start = self.boundary(span.start);
        let (line, column) = self.location(start);
        let line_start = self.text[..start].rfind('\n').map_or(0, |i| i + 1);
        let line_end = self.text[start..]
            .find(['\r', '\n'])
            .map_or(self.text.len(), |i| start + i);
        let excerpt = &self.text[line_start..line_end];
        let prefix = &self.text[line_start..start.min(line_end)];
        let width = |s: &str| {
            s.chars()
                .map(|ch| if ch == '\t' { 4 } else { 1 })
                .sum::<usize>()
        };
        let end = self.boundary(span.end).max(start).min(line_end);
        let underline = width(&self.text[start.min(line_end)..end]).max(1);
        let gutter = line.to_string().len();
        let mut result = format!(
            "{}:{line}:{column}: error: {message}\n{:gutter$} |\n{line} | {}\n{:gutter$} | {}{}",
            self.path.display(),
            "",
            excerpt.replace('\t', "    "),
            "",
            " ".repeat(width(prefix)),
            "^".repeat(underline)
        );
        for note in notes {
            result.push_str(&format!("\n  = note: {note}"));
        }
        result
    }
}

#[derive(Debug, Default)]
pub struct SourceMap {
    pub files: Vec<SourceFile>,
}

impl SourceMap {
    pub fn add(&mut self, path: PathBuf, text: impl Into<Arc<str>>) -> SourceId {
        if let Some(file) = self.files.iter().find(|file| file.path == path) {
            return file.id;
        }
        let id = SourceId(self.files.len());
        self.files.push(SourceFile {
            id,
            path,
            text: text.into(),
        });
        id
    }

    pub fn get(&self, id: SourceId) -> &SourceFile {
        &self.files[id.0]
    }
    pub fn find(&self, path: &Path) -> Option<&SourceFile> {
        self.files.iter().find(|file| file.path == path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_reused_and_distinct() {
        let mut map = SourceMap::default();
        let first = map.add("a.nerv".into(), "first");
        let second = map.add("b.nerv".into(), "second");
        assert_ne!(first, second);
        assert_eq!(map.add("a.nerv".into(), "changed"), first);
        assert_eq!(&*map.get(first).text, "first");
    }

    #[test]
    fn renders_unicode_tabs_crlf_and_multiline_ranges() {
        let file = SourceFile {
            id: SourceId(0),
            path: "main.nerv".into(),
            text: "\t\"é\" + missing\r\nnext\r\n".into(),
        };
        let start = file.text.find("missing").unwrap();
        let span = Span {
            source: file.id,
            start,
            end: file.text.len(),
        };
        assert_eq!(file.location(start), (1, 8));
        assert_eq!(file.offset(1, 8), start);
        assert_eq!(
            file.render(span, "unknown name", &[]),
            "main.nerv:1:8: error: unknown name\n  |\n1 |     \"é\" + missing\n  |           ^^^^^^^"
        );
    }

    #[test]
    fn renders_eof_and_empty_files() {
        for text in ["", "hello\n"] {
            let file = SourceFile {
                id: SourceId(0),
                path: "empty.nerv".into(),
                text: text.into(),
            };
            let span = Span {
                source: file.id,
                start: text.len(),
                end: text.len(),
            };
            assert!(
                file.render(span, "expected expression", &[])
                    .ends_with("| ^")
            );
        }
    }
}
