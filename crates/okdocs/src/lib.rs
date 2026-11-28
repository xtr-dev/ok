//! The user guide at <https://okrun.dev/docs/>: Markdown pages in `docs/`
//! turned into HTML pages styled like the landing page.
//!
//! Code samples are part of the test suite (`tests/samples.rs`), so the guide
//! cannot drift from the language. A fence's info string says what a block is:
//!
//! - ```` ```ok ````: an ok snippet. It must parse and be in the format
//!   `ok self fmt` writes. Add `nocheck` for code that is wrong on purpose.
//! - ```` ```LANG file=NAME ````: a file the following runs see in their
//!   working directory, shown with its name above it. An ok file is checked
//!   like a snippet. A later file with the same name replaces the earlier one.
//! - ```` ```output ````: what running a command prints (stdout and stderr in
//!   the order they were written). `run="CMD"` gives the command (default:
//!   `ok` and the last ok file), `exit=N` the expected exit code (default 0),
//!   `browser=no` hides the Run button for commands the WebAssembly build
//!   cannot do (`run`, `fetch`).
//!
//! Files belong to the section they are in: every `#` or `##` heading starts
//! with no files.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use pulldown_cmark::{CodeBlockKind, CowStr, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

pub const SITE: &str = "https://okrun.dev";
pub const REPO_BLOB: &str = "https://github.com/xtr-dev/ok/blob/main/";
const REPO_EDIT: &str = "https://github.com/xtr-dev/ok/edit/main/docs/";

/// A group of pages in the sidebar, from a `# Title` line in `docs/pages.txt`.
pub struct Section {
    pub title: String,
    pub slugs: Vec<String>,
}

pub struct Page {
    /// File name without `.md`; `index` is the guide's front page.
    pub slug: String,
    pub title: String,
    pub markdown: String,
}

impl Page {
    /// The page's address below the site root, such as `/docs/pipelines/`.
    pub fn path(&self) -> String {
        page_path(&self.slug)
    }
}

fn page_path(slug: &str) -> String {
    if slug == "index" {
        "/docs/".into()
    } else {
        format!("/docs/{slug}/")
    }
}

/// Read `docs/pages.txt` and every page it lists, in that order.
pub fn load(docs: &Path) -> Result<(Vec<Section>, Vec<Page>), String> {
    let list = std::fs::read_to_string(docs.join("pages.txt"))
        .map_err(|e| format!("docs/pages.txt: {e}"))?;
    let mut sections: Vec<Section> = Vec::new();
    let mut pages = Vec::new();
    for line in list.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if let Some(title) = line.strip_prefix("# ") {
            sections.push(Section {
                title: title.trim().into(),
                slugs: Vec::new(),
            });
            continue;
        }
        let section = sections
            .last_mut()
            .ok_or("docs/pages.txt must start with a `# Section` line")?;
        let markdown = std::fs::read_to_string(docs.join(format!("{line}.md")))
            .map_err(|e| format!("docs/{line}.md: {e}"))?;
        let title = markdown
            .lines()
            .find_map(|l| l.strip_prefix("# "))
            .ok_or_else(|| format!("docs/{line}.md has no `# Title` line"))?
            .trim()
            .to_string();
        section.slugs.push(line.into());
        pages.push(Page {
            slug: line.into(),
            title,
            markdown,
        });
    }
    for entry in std::fs::read_dir(docs).map_err(|e| e.to_string())? {
        let name = entry
            .map_err(|e| e.to_string())?
            .file_name()
            .to_string_lossy()
            .into_owned();
        if let Some(slug) = name.strip_suffix(".md")
            && slug != "README"
            && !pages.iter().any(|p| p.slug == slug)
        {
            return Err(format!("docs/{name} is not listed in docs/pages.txt"));
        }
    }
    Ok((sections, pages))
}

/// The parsed info string of a code fence: the language, then words that are
/// either `key=value` (the value may be in double quotes) or bare flags.
#[derive(Debug, Default)]
pub struct Fence {
    pub lang: String,
    pub attrs: BTreeMap<String, String>,
    pub flags: Vec<String>,
}

impl Fence {
    pub fn parse(info: &str) -> Fence {
        let mut words = Vec::new();
        let mut word = String::new();
        let mut quoted = false;
        for ch in info.chars() {
            match ch {
                '"' => quoted = !quoted,
                c if c.is_whitespace() && !quoted => {
                    if !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                }
                c => word.push(c),
            }
        }
        if !word.is_empty() {
            words.push(word);
        }
        let mut fence = Fence::default();
        let mut words = words.into_iter();
        fence.lang = words.next().unwrap_or_default();
        for w in words {
            match w.split_once('=') {
                Some((k, v)) => {
                    fence.attrs.insert(k.into(), v.into());
                }
                None => fence.flags.push(w),
            }
        }
        fence
    }

    fn file(&self) -> Option<&str> {
        self.attrs.get("file").map(String::as_str)
    }
}

/// A code sample the tests check.
#[derive(Debug)]
pub enum Sample {
    /// ok code that must parse and be canonically formatted.
    Code { line: usize, code: String },
    /// A command to run with the section's files, and what it must print.
    Run {
        line: usize,
        files: Vec<(String, String)>,
        command: String,
        exit: i32,
        expected: String,
    },
}

/// The command an `output` block runs when it has no `run=`.
fn default_command(files: &[(String, String)]) -> Option<String> {
    files
        .iter()
        .rev()
        .find(|(name, _)| name.ends_with(".ok"))
        .map(|(name, _)| format!("ok {name}"))
}

/// Every sample in a page, in order. Fails on an `output` block that has
/// nothing to run.
pub fn samples(markdown: &str) -> Result<Vec<Sample>, String> {
    let line_of = |offset: usize| markdown[..offset].matches('\n').count() + 1;
    let mut out = Vec::new();
    let mut files: Vec<(String, String)> = Vec::new();
    let mut open: Option<(Fence, usize, String)> = None;
    for (event, range) in Parser::new_ext(markdown, options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1 | HeadingLevel::H2,
                ..
            }) => files.clear(),
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                open = Some((Fence::parse(&info), line_of(range.start), String::new()));
            }
            Event::Text(text) => {
                if let Some((_, _, body)) = &mut open {
                    body.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                let Some((fence, line, body)) = open.take() else {
                    continue;
                };
                if fence.lang == "output" {
                    let command = match fence.attrs.get("run") {
                        Some(c) => c.clone(),
                        None => default_command(&files)
                            .ok_or_else(|| format!("line {line}: an output block needs run=\"...\" or an ok file before it"))?,
                    };
                    let exit = match fence.attrs.get("exit") {
                        Some(e) => e
                            .parse()
                            .map_err(|_| format!("line {line}: exit={e} is not a number"))?,
                        None => 0,
                    };
                    out.push(Sample::Run {
                        line,
                        files: files.clone(),
                        command,
                        exit,
                        expected: body,
                    });
                    continue;
                }
                let is_ok = fence.lang == "ok" || fence.file().is_some_and(|f| f.ends_with(".ok"));
                if is_ok && !fence.flags.iter().any(|f| f == "nocheck") {
                    out.push(Sample::Code {
                        line,
                        code: body.clone(),
                    });
                }
                if let Some(name) = fence.file() {
                    files.retain(|(n, _)| n != name);
                    files.push((name.into(), body));
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

fn options() -> Options {
    Options::ENABLE_TABLES | Options::ENABLE_SMART_PUNCTUATION
}

/// Turn ok source into HTML with the landing page's highlight classes:
/// `k` keywords, `s` strings, `c` comments, `f` function names in calls.
/// Code that does not lex is shown plain.
pub fn highlight(src: &str) -> String {
    let mut classes: Vec<Option<&'static str>> = vec![None; src.len()];
    if let Ok(lexed) = okrun::lexer::lex(0, src) {
        paint(src, &lexed.tokens, &mut classes);
        for c in &lexed.comments {
            set(
                &mut classes,
                c.span.start as usize,
                c.span.end as usize,
                Some("c"),
            );
        }
    }
    let mut html = String::new();
    let mut start = 0;
    for i in 1..=src.len() {
        let boundary = i == src.len() || classes[i] != classes[start];
        if boundary && src.is_char_boundary(i) {
            let text = escape(&src[start..i]);
            match classes[start] {
                Some(class) => {
                    let _ = write!(html, "<span class=\"{class}\">{text}</span>");
                }
                None => html.push_str(&text),
            }
            start = i;
        }
    }
    html
}

fn set(
    classes: &mut [Option<&'static str>],
    start: usize,
    end: usize,
    class: Option<&'static str>,
) {
    let end = end.min(classes.len());
    if start < end {
        classes[start..end].fill(class);
    }
}

fn paint(src: &str, tokens: &[okrun::lexer::Token], classes: &mut [Option<&'static str>]) {
    use okrun::lexer::{StrPart, Tok, WordPart};
    for (i, t) in tokens.iter().enumerate() {
        let (start, end) = (t.span.start as usize, t.span.end as usize);
        match &t.tok {
            Tok::Kw(_) | Tok::RunFn { .. } => set(classes, start, end, Some("k")),
            Tok::Ident(_) => {
                if tokens
                    .get(i + 1)
                    .is_some_and(|n| n.tok == Tok::LParen && !n.space_before)
                {
                    set(classes, start, end, Some("f"));
                }
            }
            Tok::Str(parts) => {
                set(classes, start, end, Some("s"));
                for part in parts {
                    if let StrPart::Interp { tokens, span, .. } = part {
                        set(classes, span.start as usize, span.end as usize, None);
                        paint(src, tokens, classes);
                    }
                }
            }
            Tok::Command(cmd) => {
                let keyword = if cmd.bang { "run!" } else { "run" };
                if src[start..].starts_with(keyword) {
                    set(classes, start, start + keyword.len(), Some("k"));
                }
                let words = cmd
                    .words
                    .iter()
                    .chain(cmd.redirects.iter().map(|r| &r.target));
                for word in words {
                    for part in &word.parts {
                        if let WordPart::Interp { tokens, .. } | WordPart::Spread { tokens, .. } =
                            part
                        {
                            paint(src, tokens, classes);
                        }
                    }
                }
                if let Some(with) = &cmd.with {
                    paint(src, with, classes);
                }
            }
            _ => {}
        }
    }
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Heading text to a fragment id, by the rule GitHub uses for Markdown
/// headings, so a link such as `tasks.md#parallel-runs-and---jobs` works
/// both on the site and when the files are read on GitHub: lower case,
/// every space becomes `-`, and every other character that is not a
/// letter, a digit, `-` or `_` is dropped. Runs of `-` are kept.
fn slugify(text: &str) -> String {
    let mut id = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() || ch == '-' || ch == '_' {
            id.extend(ch.to_lowercase());
        } else if ch == ' ' {
            id.push('-');
        }
    }
    id
}

/// Where a Markdown link points on the site. Links between pages are written
/// as `pipelines.md#where` so they also work when reading the files on
/// GitHub; links out of `docs/` (`../SPEC.md`) go to the repository.
fn rewrite_link(url: &str) -> String {
    if url.contains("://")
        || url.starts_with('#')
        || url.starts_with("mailto:")
        || url.starts_with('/')
    {
        return url.into();
    }
    let (path, frag) = match url.split_once('#') {
        Some((p, f)) => (p, format!("#{f}")),
        None => (url, String::new()),
    };
    if let Some(rest) = path.strip_prefix("../") {
        return format!("{REPO_BLOB}{rest}{frag}");
    }
    match path.strip_suffix(".md") {
        Some(slug) => format!("{}{frag}", page_path(slug)),
        None => url.into(),
    }
}

/// The HTML of a page's content and its plain-text description (its first
/// paragraph), for the `<meta name="description">`.
pub fn render_content(markdown: &str) -> (String, String) {
    let mut html = String::new();
    let mut description = String::new();
    let mut in_first_para = None::<bool>;
    let mut group = 0usize;
    let mut fence: Option<(Fence, String)> = None;
    let mut files_in_group: Vec<String> = Vec::new();
    let mut heading: Option<(HeadingLevel, Vec<Event>)> = None;
    let mut events: Vec<Event> = Vec::new();

    for event in Parser::new_ext(markdown, options()) {
        // Collect the description from the first paragraph.
        match &event {
            Event::Start(Tag::Paragraph) if in_first_para.is_none() => in_first_para = Some(true),
            Event::End(TagEnd::Paragraph) if in_first_para == Some(true) => {
                in_first_para = Some(false)
            }
            Event::Text(t) | Event::Code(t) if in_first_para == Some(true) => {
                description.push_str(t)
            }
            Event::SoftBreak if in_first_para == Some(true) => description.push(' '),
            _ => {}
        }
        if let Some((level, inner)) = &mut heading {
            if let Event::End(TagEnd::Heading(_)) = event {
                let text: String = inner
                    .iter()
                    .filter_map(|e| match e {
                        Event::Text(t) | Event::Code(t) => Some(t.to_string()),
                        _ => None,
                    })
                    .collect();
                let mut inner_html = String::new();
                pulldown_cmark::html::push_html(&mut inner_html, std::mem::take(inner).into_iter());
                let n = *level as usize;
                if n == 1 {
                    let _ = writeln!(events_html(&mut events, &mut html), "<h1>{inner_html}</h1>");
                } else {
                    let id = slugify(&text);
                    let _ = writeln!(
                        events_html(&mut events, &mut html),
                        "<h{n} id=\"{id}\"><a class=\"anchor\" href=\"#{id}\" aria-hidden=\"true\">#</a>{inner_html}</h{n}>"
                    );
                }
                heading = None;
            } else {
                inner.push(event);
            }
            continue;
        }
        if let Some((_, body)) = &mut fence {
            match event {
                Event::Text(t) => body.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let (f, body) = fence.take().unwrap();
                    let out = events_html(&mut events, &mut html);
                    render_block(out, &f, &body, group, &mut files_in_group);
                }
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                if level <= HeadingLevel::H2 {
                    group += 1;
                    files_in_group.clear();
                }
                heading = Some((level, Vec::new()));
            }
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                fence = Some((Fence::parse(&info), String::new()))
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => {
                let dest_url = CowStr::from(rewrite_link(&dest_url));
                events.push(Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                }));
            }
            other => events.push(other),
        }
    }
    events_html(&mut events, &mut html);
    let description = description.split_whitespace().collect::<Vec<_>>().join(" ");
    (html, description)
}

/// Flush the buffered Markdown events into `html` and return it, so custom
/// HTML can follow in the right place.
fn events_html<'a>(events: &mut Vec<Event>, html: &'a mut String) -> &'a mut String {
    pulldown_cmark::html::push_html(html, events.drain(..));
    html
}

fn render_block(
    out: &mut String,
    fence: &Fence,
    body: &str,
    group: usize,
    files: &mut Vec<String>,
) {
    let is_ok = fence.lang == "ok" || fence.file().is_some_and(|f| f.ends_with(".ok"));
    let code = if is_ok { highlight(body) } else { escape(body) };
    if fence.lang == "output" {
        let command = fence.attrs.get("run").cloned().or_else(|| {
            files
                .iter()
                .rev()
                .find(|f| f.ends_with(".ok"))
                .map(|f| format!("ok {f}"))
        });
        let command = command.unwrap_or_default();
        let runnable = fence.attrs.get("browser").is_none_or(|b| b != "no");
        let _ = writeln!(
            out,
            "<figure class=\"sample-out\" data-group=\"g{group}\" data-run=\"{}\"{}><figcaption><span class=\"prompt\">$</span> {}</figcaption><pre class=\"out\">{code}</pre></figure>",
            escape(&command),
            if runnable { "" } else { " data-browser=\"no\"" },
            escape(&command),
        );
        return;
    }
    match fence.file() {
        Some(name) => {
            files.retain(|f| f != name);
            files.push(name.into());
            let _ = writeln!(
                out,
                "<figure class=\"sample-file\" data-group=\"g{group}\" data-file=\"{0}\"><figcaption>{0}</figcaption><pre><code>{code}</code></pre></figure>",
                escape(name)
            );
        }
        None => {
            let lang = if fence.lang.is_empty() {
                String::new()
            } else {
                format!(" class=\"lang-{}\"", escape(&fence.lang))
            };
            let _ = writeln!(out, "<pre{lang}><code>{code}</code></pre>");
        }
    }
}

/// A whole HTML page: header, sidebar, content, previous and next links.
pub fn render_page(sections: &[Section], pages: &[Page], index: usize) -> String {
    let page = &pages[index];
    let (content, description) = render_content(&page.markdown);
    let title = if page.slug == "index" {
        "ok documentation".to_string()
    } else {
        format!("{} · ok documentation", page.title)
    };
    let mut nav = String::new();
    for section in sections {
        let _ = write!(
            nav,
            "<p class=\"nav-section\">{}</p><ul>",
            escape(&section.title)
        );
        for slug in &section.slugs {
            let p = pages
                .iter()
                .find(|p| &p.slug == slug)
                .expect("listed page is loaded");
            let current = if p.slug == page.slug {
                " aria-current=\"page\""
            } else {
                ""
            };
            let _ = write!(
                nav,
                "<li><a href=\"{}\"{current}>{}</a></li>",
                p.path(),
                escape(&p.title)
            );
        }
        nav.push_str("</ul>");
    }
    let mut pager = String::new();
    if let Some(prev) = index.checked_sub(1).map(|i| &pages[i]) {
        let _ = write!(
            pager,
            "<a class=\"prev\" href=\"{}\"><span>Previous</span>{}</a>",
            prev.path(),
            escape(&prev.title)
        );
    }
    if let Some(next) = pages.get(index + 1) {
        let _ = write!(
            pager,
            "<a class=\"next\" href=\"{}\"><span>Next</span>{}</a>",
            next.path(),
            escape(&next.title)
        );
    }
    TEMPLATE
        .replace("{{title}}", &escape(&title))
        .replace("{{description}}", &escape(&truncate(&description, 160)))
        .replace("{{canonical}}", &format!("{SITE}{}", page.path()))
        .replace("{{page_title}}", &escape(&page.title))
        .replace("{{nav}}", &nav)
        .replace("{{edit}}", &format!("{REPO_EDIT}{}.md", page.slug))
        .replace("{{pager}}", &pager)
        .replace("{{content}}", &content)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.into();
    }
    let cut: String = s.chars().take(max - 1).collect();
    let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head);
    format!("{cut}…")
}

const TEMPLATE: &str = include_str!("template.html");
