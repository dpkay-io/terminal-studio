//! Markdown preview renderer.
//!
//! Parsing is delegated to `pulldown-cmark` (CommonMark + GFM tables, task
//! lists, strikethrough, alerts, footnotes). The event stream is folded into a
//! small block tree ([`Block`]) which is then laid out with egui.
//!
//! Every run of inline text (a paragraph, heading, list item or table cell) is
//! rendered as a *single* [`LayoutJob`] whose sections carry the styling. That
//! lets egui word-wrap across bold / code / link boundaries and keeps text
//! selectable, instead of emitting one widget per styled fragment.

use std::sync::Arc;

use egui::text::LayoutJob;
use egui::{Align, Color32, FontId, Layout, Sense, Stroke, TextFormat, Vec2};
use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

use crate::syntax;
use crate::theme;

// ── Document model ───────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct SpanStyle {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    /// Link target. An empty string marks a non-clickable reference (footnote).
    link: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Span {
    text: String,
    style: SpanStyle,
}

type Inline = Vec<Span>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ListItem {
    checked: Option<bool>,
    blocks: Vec<Block>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Block {
    Heading {
        level: u8,
        text: Inline,
    },
    Paragraph(Inline),
    Code {
        lang: String,
        code: String,
    },
    List {
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    Quote {
        kind: Option<BlockQuoteKind>,
        blocks: Vec<Block>,
    },
    Table {
        aligns: Vec<ColAlign>,
        header: Vec<Inline>,
        rows: Vec<Vec<Inline>>,
    },
    Footnote {
        label: String,
        blocks: Vec<Block>,
    },
    /// Raw HTML block, reduced to its visible text.
    Html(String),
    Rule,
}

// ── Parsing ──────────────────────────────────────────────────────────────────

fn parse(src: &str) -> Vec<Block> {
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS;
    let mut b = Builder::default();
    for ev in Parser::new_ext(src, opts) {
        b.event(ev);
    }
    b.finish()
}

enum Container {
    Quote {
        kind: Option<BlockQuoteKind>,
        blocks: Vec<Block>,
    },
    List {
        start: Option<u64>,
        items: Vec<ListItem>,
    },
    Item {
        checked: Option<bool>,
        blocks: Vec<Block>,
    },
    Footnote {
        label: String,
        blocks: Vec<Block>,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InlineKind {
    Paragraph,
    Heading(u8),
    Cell,
}

#[derive(Default)]
struct TableBuild {
    aligns: Vec<ColAlign>,
    header: Vec<Inline>,
    rows: Vec<Vec<Inline>>,
    row: Vec<Inline>,
}

#[derive(Default)]
struct Builder {
    root: Vec<Block>,
    stack: Vec<Container>,
    inline: Option<(InlineKind, Inline)>,
    bold: u32,
    italic: u32,
    strike: u32,
    links: Vec<String>,
    /// Char count of the open inline run when each image started, so an image
    /// without alt text can fall back to showing its URL.
    images: Vec<usize>,
    code: Option<(String, String)>,
    html: Option<String>,
    table: Option<TableBuild>,
    in_metadata: bool,
}

impl Builder {
    fn style(&self) -> SpanStyle {
        SpanStyle {
            bold: self.bold > 0,
            italic: self.italic > 0,
            strike: self.strike > 0,
            code: false,
            link: self.links.last().cloned(),
        }
    }

    fn push_text(&mut self, text: &str, style: SpanStyle) {
        if text.is_empty() {
            return;
        }
        let (_, spans) = self
            .inline
            .get_or_insert_with(|| (InlineKind::Paragraph, Vec::new()));
        match spans.last_mut() {
            Some(last) if last.style == style => last.text.push_str(text),
            _ => spans.push(Span {
                text: text.to_string(),
                style,
            }),
        }
    }

    fn inline_char_len(&self) -> usize {
        self.inline
            .as_ref()
            .map(|(_, s)| s.iter().map(|sp| sp.text.chars().count()).sum())
            .unwrap_or(0)
    }

    fn push_block(&mut self, block: Block) {
        match self.stack.last_mut() {
            Some(Container::Quote { blocks, .. })
            | Some(Container::Item { blocks, .. })
            | Some(Container::Footnote { blocks, .. }) => blocks.push(block),
            // Blocks never appear directly inside a list (only inside items),
            // but keep the content rather than dropping it if they do.
            Some(Container::List { items, .. }) => items.push(ListItem {
                checked: None,
                blocks: vec![block],
            }),
            None => self.root.push(block),
        }
    }

    /// Close the open inline run (paragraph / heading / implicit tight-list text).
    fn flush_inline(&mut self) {
        let Some((kind, mut spans)) = self.inline.take() else {
            return;
        };
        trim_inline(&mut spans);
        match kind {
            InlineKind::Heading(level) => self.push_block(Block::Heading { level, text: spans }),
            InlineKind::Paragraph => {
                if !spans.is_empty() {
                    self.push_block(Block::Paragraph(spans));
                }
            }
            InlineKind::Cell => {
                if let Some(t) = self.table.as_mut() {
                    t.row.push(spans);
                }
            }
        }
    }

    fn pop_container(&mut self) {
        self.flush_inline();
        let Some(c) = self.stack.pop() else {
            return;
        };
        match c {
            Container::Quote { kind, blocks } => self.push_block(Block::Quote { kind, blocks }),
            Container::List { start, items } => self.push_block(Block::List { start, items }),
            Container::Footnote { label, blocks } => {
                self.push_block(Block::Footnote { label, blocks })
            }
            Container::Item { checked, blocks } => match self.stack.last_mut() {
                Some(Container::List { items, .. }) => items.push(ListItem { checked, blocks }),
                _ => {
                    for b in blocks {
                        self.push_block(b);
                    }
                }
            },
        }
    }

    fn event(&mut self, ev: Event<'_>) {
        if self.in_metadata {
            if let Event::End(TagEnd::MetadataBlock(_)) = ev {
                self.in_metadata = false;
            }
            return;
        }
        if let Some((_, buf)) = self.code.as_mut() {
            match ev {
                Event::Text(t) => buf.push_str(&t),
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, mut code) = self.code.take().unwrap_or_default();
                    while code.ends_with('\n') {
                        code.pop();
                    }
                    self.push_block(Block::Code { lang, code });
                }
                _ => {}
            }
            return;
        }
        if let Some(buf) = self.html.as_mut() {
            match ev {
                Event::Html(t) | Event::Text(t) => buf.push_str(&t),
                Event::End(TagEnd::HtmlBlock) => {
                    let raw = self.html.take().unwrap_or_default();
                    let text = html_visible_text(&raw);
                    if !text.is_empty() {
                        self.push_block(Block::Html(text));
                    }
                }
                _ => {}
            }
            return;
        }

        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) => {
                let style = self.style();
                self.push_text(&t, style);
            }
            Event::Code(t) | Event::InlineMath(t) | Event::DisplayMath(t) => {
                let mut style = self.style();
                style.code = true;
                self.push_text(&t, style);
            }
            Event::InlineHtml(t) => {
                let tag = t.trim().to_ascii_lowercase();
                if tag.starts_with("<br") {
                    let style = self.style();
                    self.push_text("\n", style);
                }
                // Other inline tags (<kbd>, <sup>, ...) are dropped; their text
                // content still arrives as ordinary Text events.
            }
            Event::Html(t) => {
                // Stray HTML outside an HTML block: keep any visible text.
                let text = html_visible_text(&t);
                if !text.is_empty() {
                    let style = self.style();
                    self.push_text(&text, style);
                }
            }
            Event::FootnoteReference(label) => {
                let style = SpanStyle {
                    link: Some(String::new()),
                    ..self.style()
                };
                self.push_text(&format!("[{label}]"), style);
            }
            Event::SoftBreak => {
                let style = self.style();
                self.push_text(" ", style);
            }
            Event::HardBreak => {
                let style = self.style();
                self.push_text("\n", style);
            }
            Event::Rule => {
                self.flush_inline();
                self.push_block(Block::Rule);
            }
            Event::TaskListMarker(checked) => {
                if let Some(Container::Item { checked: c, .. }) = self.stack.last_mut() {
                    *c = Some(checked);
                }
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                self.flush_inline();
                self.inline = Some((InlineKind::Paragraph, Vec::new()));
            }
            Tag::Heading { level, .. } => {
                self.flush_inline();
                self.inline = Some((InlineKind::Heading(heading_level(level)), Vec::new()));
            }
            Tag::BlockQuote(kind) => {
                self.flush_inline();
                self.stack.push(Container::Quote {
                    kind,
                    blocks: Vec::new(),
                });
            }
            Tag::CodeBlock(kind) => {
                self.flush_inline();
                let lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        info.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::HtmlBlock => {
                self.flush_inline();
                self.html = Some(String::new());
            }
            Tag::List(start) => {
                self.flush_inline();
                self.stack.push(Container::List {
                    start,
                    items: Vec::new(),
                });
            }
            Tag::Item => {
                self.flush_inline();
                self.stack.push(Container::Item {
                    checked: None,
                    blocks: Vec::new(),
                });
            }
            Tag::FootnoteDefinition(label) => {
                self.flush_inline();
                self.stack.push(Container::Footnote {
                    label: label.to_string(),
                    blocks: Vec::new(),
                });
            }
            Tag::Table(aligns) => {
                self.flush_inline();
                self.table = Some(TableBuild {
                    aligns: aligns
                        .into_iter()
                        .map(|a| match a {
                            Alignment::Center => ColAlign::Center,
                            Alignment::Right => ColAlign::Right,
                            Alignment::Left | Alignment::None => ColAlign::Left,
                        })
                        .collect(),
                    ..Default::default()
                });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    t.row.clear();
                }
            }
            Tag::TableCell => {
                self.inline = Some((InlineKind::Cell, Vec::new()));
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::Strikethrough => self.strike += 1,
            Tag::Link { dest_url, .. } => self.links.push(dest_url.to_string()),
            Tag::Image { dest_url, .. } => {
                self.images.push(self.inline_char_len());
                self.links.push(dest_url.to_string());
            }
            Tag::MetadataBlock(_) => {
                self.flush_inline();
                self.in_metadata = true;
            }
            Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {
                self.flush_inline();
            }
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Heading(_) => self.flush_inline(),
            TagEnd::BlockQuote(_) | TagEnd::List(_) | TagEnd::Item | TagEnd::FootnoteDefinition => {
                self.pop_container()
            }
            TagEnd::TableCell => self.flush_inline(),
            TagEnd::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.header = std::mem::take(&mut t.row);
                }
            }
            TagEnd::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.row);
                    t.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.push_block(Block::Table {
                        aligns: t.aligns,
                        header: t.header,
                        rows: t.rows,
                    });
                }
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::Strikethrough => self.strike = self.strike.saturating_sub(1),
            TagEnd::Link => {
                self.links.pop();
            }
            TagEnd::Image => {
                let start = self.images.pop().unwrap_or(0);
                if self.inline_char_len() == start {
                    let url = self.links.last().cloned().unwrap_or_default();
                    let style = self.style();
                    self.push_text(&format!("[image: {url}]"), style);
                }
                self.links.pop();
            }
            TagEnd::CodeBlock
            | TagEnd::HtmlBlock
            | TagEnd::MetadataBlock(_)
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition => self.flush_inline(),
        }
    }

    fn finish(mut self) -> Vec<Block> {
        self.flush_inline();
        while !self.stack.is_empty() {
            self.pop_container();
        }
        self.root
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// Drop leading/trailing whitespace of an inline run and any spans left empty.
fn trim_inline(spans: &mut Inline) {
    if let Some(first) = spans.first_mut() {
        let t = first.text.trim_start().to_string();
        first.text = t;
    }
    if let Some(last) = spans.last_mut() {
        let t = last.text.trim_end().to_string();
        last.text = t;
    }
    spans.retain(|s| !s.text.is_empty());
}

/// Reduce an HTML fragment to its visible text: comments and tags are removed,
/// whitespace is collapsed.
fn html_visible_text(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        rest = &rest[lt..];
        let end = if rest.starts_with("<!--") {
            rest.find("-->").map(|e| e + 3)
        } else {
            rest.find('>').map(|e| e + 1)
        };
        match end {
            Some(e) => {
                rest = &rest[e..];
                out.push(' ');
            }
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ── Rendering ────────────────────────────────────────────────────────────────

/// Widest the document column grows; wider panes get side margin instead of
/// ever-longer lines.
const MAX_DOC_WIDTH: f32 = 980.0;
/// Line height relative to font size for prose.
const LINE_HEIGHT: f32 = 1.45;

pub(super) fn render_markdown(ui: &mut egui::Ui, content: &str) {
    let blocks = cached_parse(ui, content);

    let avail = ui.available_width().max(0.0);
    let side = if avail > 600.0 {
        theme::SP_6
    } else {
        theme::SP_2
    };
    let doc_w = (avail - side * 2.0).clamp(0.0, MAX_DOC_WIDTH);

    let body = ui
        .style()
        .text_styles
        .get(&egui::TextStyle::Body)
        .map(|f| f.size)
        .unwrap_or(theme::FONT_MD)
        .max(theme::FONT_MD);

    let th = theme::active();
    let mut rc = RenderCtx {
        body,
        widget_idx: 0,
    };
    let tone = Tone {
        color: th.subtext1,
        strong: th.text,
        italic: false,
        bold: false,
    };

    egui::Frame::none()
        .inner_margin(egui::Margin {
            left: side,
            right: side,
            top: theme::SP_4,
            bottom: theme::SP_6 * 2.0,
        })
        .show(ui, |ui| {
            ui.set_max_width(doc_w);
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            render_blocks(ui, &blocks, &mut rc, tone);
        });
}

/// Re-parse only when the content changes. The cache slot is keyed by the
/// calling `Ui`, so each preview keeps its own entry.
fn cached_parse(ui: &egui::Ui, content: &str) -> Arc<Vec<Block>> {
    type Slot = (u64, Arc<Vec<Block>>);
    let hash = egui::util::hash(content);
    let id = ui.id().with("md_parse_cache");
    if let Some((h, blocks)) = ui.ctx().data(|d| d.get_temp::<Slot>(id)) {
        if h == hash {
            return blocks;
        }
    }
    let blocks = Arc::new(parse(content));
    ui.ctx()
        .data_mut(|d| d.insert_temp::<Slot>(id, (hash, blocks.clone())));
    blocks
}

struct RenderCtx {
    body: f32,
    /// Running counter used to give scroll areas stable, unique ids.
    widget_idx: usize,
}

impl RenderCtx {
    fn next_id(&mut self) -> usize {
        self.widget_idx += 1;
        self.widget_idx
    }
    fn block_gap(&self) -> f32 {
        (self.body * 0.8).round()
    }
}

/// Inherited text appearance for a subtree (blockquotes recolour their content).
#[derive(Clone, Copy)]
struct Tone {
    color: Color32,
    strong: Color32,
    italic: bool,
    bold: bool,
}

fn render_blocks(ui: &mut egui::Ui, blocks: &[Block], rc: &mut RenderCtx, tone: Tone) {
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            let gap = match block {
                Block::Heading { level, .. } if *level <= 2 => rc.block_gap() * 2.0,
                Block::Heading { .. } => rc.block_gap() * 1.5,
                _ => rc.block_gap(),
            };
            ui.add_space(gap);
        }
        render_block(ui, block, rc, tone);
    }
}

fn render_block(ui: &mut egui::Ui, block: &Block, rc: &mut RenderCtx, tone: Tone) {
    let th = theme::active();
    match block {
        Block::Heading { level, text } => {
            let size = match level {
                1 => theme::FONT_HEADING_1 + 4.0,
                2 => theme::FONT_HEADING_1,
                3 => theme::FONT_HEADING_2,
                4 => theme::FONT_LG,
                _ => rc.body + 1.0,
            };
            let color = if *level >= 6 {
                th.subtext0
            } else {
                th.md_heading
            };
            let style = InlineStyle {
                size,
                line_height: 1.25,
                tone: Tone {
                    color,
                    strong: color,
                    italic: false,
                    bold: true,
                },
            };
            inline_label(ui, text, &style);
            if *level <= 2 {
                ui.add_space(theme::SP_3);
                let w = ui.available_width();
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(w, theme::STROKE_THIN), Sense::hover());
                ui.painter().rect_filled(rect, 0.0, th.md_heading_rule);
            }
        }
        Block::Paragraph(text) => {
            inline_label(ui, text, &InlineStyle::prose(rc, tone));
        }
        Block::Html(text) => {
            let spans = vec![Span {
                text: text.clone(),
                style: SpanStyle::default(),
            }];
            let tone = Tone {
                color: th.fg_muted,
                ..tone
            };
            inline_label(ui, &spans, &InlineStyle::prose(rc, tone));
        }
        Block::Code { lang, code } => render_code(ui, lang, code, rc),
        Block::List { start, items } => render_list(ui, *start, items, rc, tone, 0),
        Block::Quote { kind, blocks } => render_quote(ui, *kind, blocks, rc, tone),
        Block::Table {
            aligns,
            header,
            rows,
        } => render_table(ui, aligns, header, rows, rc, tone),
        Block::Footnote { label, blocks } => {
            let tone = Tone {
                color: th.fg_muted,
                ..tone
            };
            ui.horizontal_top(|ui| {
                let style = InlineStyle::prose(rc, tone);
                let marker = vec![Span {
                    text: format!("{label}."),
                    style: SpanStyle::default(),
                }];
                inline_label(ui, &marker, &style);
                ui.add_space(theme::SP_3);
                ui.vertical(|ui| render_blocks(ui, blocks, rc, tone));
            });
        }
        Block::Rule => {
            ui.add_space(theme::SP_2);
            let w = ui.available_width();
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(w, theme::STROKE_THIN), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, th.md_heading_rule);
            ui.add_space(theme::SP_2);
        }
    }
}

// ── Inline text ──────────────────────────────────────────────────────────────

struct InlineStyle {
    size: f32,
    line_height: f32,
    tone: Tone,
}

impl InlineStyle {
    fn prose(rc: &RenderCtx, tone: Tone) -> Self {
        Self {
            size: rc.body,
            line_height: LINE_HEIGHT,
            tone,
        }
    }
}

fn span_format(span: &SpanStyle, style: &InlineStyle) -> TextFormat {
    let th = theme::active();
    let bold = span.bold || style.tone.bold;
    let mut color = if bold {
        style.tone.strong
    } else {
        style.tone.color
    };
    let mut fmt = TextFormat {
        font_id: FontId::proportional(style.size),
        line_height: Some((style.size * style.line_height).round()),
        italics: span.italic || style.tone.italic,
        valign: Align::Center,
        ..Default::default()
    };
    if span.code {
        fmt.font_id = FontId::monospace((style.size * 0.9).round());
        fmt.background = th.md_inline_code_bg;
        color = th.md_inline_code;
    }
    if span.link.is_some() {
        color = th.md_link;
    }
    if span.strike {
        fmt.strikethrough = Stroke::new(theme::STROKE_THIN, color);
    }
    fmt.color = color;
    fmt
}

/// Build one layout job for a run of spans; also returns the char ranges that
/// belong to links so clicks can be mapped back to a URL.
fn inline_job(spans: &Inline, style: &InlineStyle) -> (LayoutJob, Vec<(usize, usize, String)>) {
    let mut job = LayoutJob::default();
    let mut links = Vec::new();
    let mut chars = 0usize;
    for span in spans {
        let n = span.text.chars().count();
        if let Some(url) = &span.style.link {
            if !url.is_empty() {
                links.push((chars, chars + n, url.clone()));
            }
        }
        job.append(&span.text, 0.0, span_format(&span.style, style));
        chars += n;
    }
    if job.text.is_empty() {
        job.append(" ", 0.0, span_format(&SpanStyle::default(), style));
    }
    (job, links)
}

/// Render a wrapped, selectable run of inline text. Links are clickable.
fn inline_label(ui: &mut egui::Ui, spans: &Inline, style: &InlineStyle) {
    let (mut job, links) = inline_job(spans, style);
    job.wrap.max_width = ui.available_width().max(1.0);
    let galley = ui.fonts(|f| f.layout_job(job));
    let resp = ui.add(egui::Label::new(galley.clone()));

    if links.is_empty() {
        return;
    }
    let Some(pos) = resp.hover_pos() else {
        return;
    };
    let idx = galley.cursor_from_pos(pos - resp.rect.min).ccursor.index;
    let Some((_, _, url)) = links.iter().find(|(s, e, _)| idx >= *s && idx < *e) else {
        return;
    };
    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    let resp = resp.on_hover_text(url.as_str());
    if resp.clicked() && is_external_url(url) {
        let _ = open::that(url);
    }
}

fn is_external_url(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mailto:")
}

// ── Code blocks ──────────────────────────────────────────────────────────────

fn render_code(ui: &mut egui::Ui, lang: &str, code: &str, rc: &mut RenderCtx) {
    let th = theme::active();
    let id = rc.next_id();
    let font = FontId::monospace(theme::FONT_SM);
    let bg = [th.md_code_bg.r(), th.md_code_bg.g(), th.md_code_bg.b()];
    let plain_fg = theme::ensure_readable([th.md_code.r(), th.md_code.g(), th.md_code.b()], bg);
    let line_height = Some((theme::FONT_SM * 1.4).round());

    let mut job = LayoutJob::default();
    let syn = if lang.is_empty() {
        None
    } else {
        syntax::find_syntax_for_language(lang)
    };
    match syn {
        Some(syn) => {
            for (i, spans) in syntax::highlighted_lines(code, syn).iter().enumerate() {
                if i > 0 {
                    job.append("\n", 0.0, TextFormat::simple(font.clone(), plain_fg));
                }
                for (color, text) in spans {
                    let fg = theme::ensure_readable([color.r(), color.g(), color.b()], bg);
                    let mut fmt = TextFormat::simple(font.clone(), fg);
                    fmt.line_height = line_height;
                    job.append(text.trim_end_matches('\n'), 0.0, fmt);
                }
            }
        }
        None => {
            let mut fmt = TextFormat::simple(font.clone(), plain_fg);
            fmt.line_height = line_height;
            job.append(code, 0.0, fmt);
        }
    }
    if job.text.is_empty() {
        job.append(" ", 0.0, TextFormat::simple(font, plain_fg));
    }
    job.wrap.max_width = f32::INFINITY;

    egui::Frame::none()
        .fill(th.md_code_bg)
        .stroke(Stroke::new(theme::STROKE_THIN, th.md_code_border))
        .inner_margin(egui::Margin::symmetric(theme::SP_5, theme::SP_4))
        .rounding(egui::Rounding::same(theme::R_MD))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !lang.is_empty() {
                ui.label(
                    egui::RichText::new(lang)
                        .size(theme::FONT_XS)
                        .color(th.fg_muted),
                );
                ui.add_space(theme::SP_2);
            }
            ui.spacing_mut().scroll.floating_allocated_width = 0.0;
            egui::ScrollArea::horizontal()
                .id_source(("md_code_scroll", id))
                .show(ui, |ui| {
                    let galley = ui.fonts(|f| f.layout_job(job));
                    ui.add(egui::Label::new(galley).wrap_mode(egui::TextWrapMode::Extend));
                });
        });
}

// ── Lists ────────────────────────────────────────────────────────────────────

fn render_list(
    ui: &mut egui::Ui,
    start: Option<u64>,
    items: &[ListItem],
    rc: &mut RenderCtx,
    tone: Tone,
    depth: usize,
) {
    let th = theme::active();
    let prose = InlineStyle::prose(rc, tone);
    let line_h = (prose.size * prose.line_height).round();

    // Marker column: wide enough for the largest ordinal.
    let marker_w = match start {
        Some(s) => {
            let last = s + items.len().saturating_sub(1) as u64;
            let digits = last.to_string().len() as f32;
            (digits + 1.0) * rc.body * 0.6 + theme::SP_2
        }
        None => rc.body * 1.3,
    };
    let any_task = items.iter().any(|it| it.checked.is_some());

    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            ui.add_space(theme::SP_2);
        }
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            let (rect, _) = ui.allocate_exact_size(egui::vec2(marker_w, line_h), Sense::hover());
            let painter = ui.painter();
            match (start, item.checked) {
                (_, Some(checked)) => paint_checkbox(painter, rect, checked, rc.body),
                (Some(s), None) => {
                    let n = s + i as u64;
                    let galley = ui.fonts(|f| {
                        f.layout_no_wrap(
                            format!("{n}."),
                            FontId::proportional(rc.body),
                            th.md_bullet,
                        )
                    });
                    let pos = egui::pos2(
                        rect.right() - galley.size().x - theme::SP_3,
                        rect.center().y - galley.size().y / 2.0,
                    );
                    painter.galley(pos, galley, th.md_bullet);
                }
                (None, None) => {
                    if !any_task {
                        paint_bullet(painter, rect, depth, rc.body, th.md_bullet);
                    }
                }
            }
            ui.vertical(|ui| {
                for (bi, block) in item.blocks.iter().enumerate() {
                    if bi > 0 {
                        ui.add_space(theme::SP_3);
                    }
                    match block {
                        Block::List { start, items } => {
                            render_list(ui, *start, items, rc, tone, depth + 1)
                        }
                        other => render_block(ui, other, rc, tone),
                    }
                }
            });
        });
    }
}

fn paint_bullet(
    painter: &egui::Painter,
    rect: egui::Rect,
    depth: usize,
    body: f32,
    color: Color32,
) {
    let r = (body * 0.17).max(2.0);
    let c = egui::pos2(rect.left() + rect.width() * 0.45, rect.center().y);
    match depth % 3 {
        0 => {
            painter.circle_filled(c, r, color);
        }
        1 => {
            painter.circle_stroke(c, r, Stroke::new(theme::STROKE_THIN, color));
        }
        _ => {
            let sq = egui::Rect::from_center_size(c, egui::vec2(r * 1.8, r * 1.8));
            painter.rect_filled(sq, 0.0, color);
        }
    }
}

fn paint_checkbox(painter: &egui::Painter, rect: egui::Rect, checked: bool, body: f32) {
    let th = theme::active();
    let s = (body * 0.95).round();
    let b = egui::Rect::from_center_size(
        egui::pos2(rect.left() + s / 2.0 + 1.0, rect.center().y),
        egui::vec2(s, s),
    );
    if checked {
        painter.rect_filled(b, theme::R_SM, th.accent);
        let stroke = Stroke::new(theme::STROKE_BOLD, th.base);
        let p1 = egui::pos2(b.left() + s * 0.22, b.center().y);
        let p2 = egui::pos2(b.left() + s * 0.42, b.bottom() - s * 0.25);
        let p3 = egui::pos2(b.right() - s * 0.2, b.top() + s * 0.25);
        painter.line_segment([p1, p2], stroke);
        painter.line_segment([p2, p3], stroke);
    } else {
        painter.rect_stroke(
            b,
            theme::R_SM,
            Stroke::new(theme::STROKE_MEDIUM, th.overlay0),
        );
    }
}

// ── Blockquotes / alerts ─────────────────────────────────────────────────────

fn render_quote(
    ui: &mut egui::Ui,
    kind: Option<BlockQuoteKind>,
    blocks: &[Block],
    rc: &mut RenderCtx,
    tone: Tone,
) {
    let th = theme::active();
    let (bar, title) = match kind {
        None => (th.accent, None),
        Some(BlockQuoteKind::Note) => (th.blue, Some("Note")),
        Some(BlockQuoteKind::Tip) => (th.green, Some("Tip")),
        Some(BlockQuoteKind::Important) => (th.mauve, Some("Important")),
        Some(BlockQuoteKind::Warning) => (th.yellow, Some("Warning")),
        Some(BlockQuoteKind::Caution) => (th.red, Some("Caution")),
    };
    let bar_w = theme::TAB_COLOR_STRIP_W;
    let inner_tone = Tone {
        color: th.md_blockquote,
        ..tone
    };

    let resp = egui::Frame::none()
        .fill(th.md_blockquote_bg)
        .rounding(egui::Rounding {
            nw: 0.0,
            sw: 0.0,
            ne: theme::R_MD,
            se: theme::R_MD,
        })
        .inner_margin(egui::Margin {
            left: bar_w + theme::SP_5,
            right: theme::SP_5,
            top: theme::SP_4,
            bottom: theme::SP_4,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(title) = title {
                let spans = vec![Span {
                    text: title.to_string(),
                    style: SpanStyle::default(),
                }];
                let style = InlineStyle {
                    tone: Tone {
                        color: bar,
                        strong: bar,
                        italic: false,
                        bold: true,
                    },
                    ..InlineStyle::prose(rc, inner_tone)
                };
                inline_label(ui, &spans, &style);
                ui.add_space(theme::SP_2);
            }
            render_blocks(ui, blocks, rc, inner_tone);
        });
    let outer = resp.response.rect;
    let bar_rect = egui::Rect::from_min_size(outer.left_top(), egui::vec2(bar_w, outer.height()));
    ui.painter().rect_filled(bar_rect, 0.0, bar);
}

// ── Tables ───────────────────────────────────────────────────────────────────

/// Fit column widths into `avail`.
///
/// Columns keep their natural (unwrapped) width when everything fits. Otherwise
/// each column gets at least its minimum (longest word) and the remaining space
/// is shared in proportion to how much each column would still like to grow.
/// If even the minimums don't fit, the minimums are returned and the caller
/// scrolls horizontally.
fn fit_columns(natural: &[f32], min: &[f32], avail: f32) -> Vec<f32> {
    let total: f32 = natural.iter().sum();
    if total <= avail {
        return natural.to_vec();
    }
    let min_total: f32 = min.iter().sum();
    if min_total >= avail {
        return min.to_vec();
    }
    let slack: Vec<f32> = natural
        .iter()
        .zip(min)
        .map(|(n, m)| (n - m).max(0.0))
        .collect();
    let slack_total: f32 = slack.iter().sum();
    let extra = avail - min_total;
    min.iter()
        .zip(&slack)
        .map(|(m, s)| {
            if slack_total > 0.0 {
                m + s * extra / slack_total
            } else {
                *m
            }
        })
        .collect()
}

/// (natural width, longest-word width) of a cell, measured glyph by glyph.
fn measure_cell(fonts: &egui::text::Fonts, spans: &Inline, style: &InlineStyle) -> (f32, f32) {
    let mut natural = 0.0f32;
    let mut line = 0.0f32;
    let mut longest_word = 0.0f32;
    let mut word = 0.0f32;
    for span in spans {
        let font = span_format(&span.style, style).font_id;
        for c in span.text.chars() {
            if c == '\n' {
                natural = natural.max(line);
                longest_word = longest_word.max(word);
                line = 0.0;
                word = 0.0;
                continue;
            }
            let w = fonts.glyph_width(&font, c);
            line += w;
            if c.is_whitespace() {
                longest_word = longest_word.max(word);
                word = 0.0;
            } else {
                word += w;
            }
        }
    }
    (
        natural.max(line).ceil() + 1.0,
        longest_word.max(word).ceil() + 1.0,
    )
}

fn render_table(
    ui: &mut egui::Ui,
    aligns: &[ColAlign],
    header: &[Inline],
    rows: &[Vec<Inline>],
    rc: &mut RenderCtx,
    tone: Tone,
) {
    let th = theme::active();
    let ncols = rows
        .iter()
        .map(|r| r.len())
        .chain(std::iter::once(header.len()))
        .max()
        .unwrap_or(0);
    if ncols == 0 {
        return;
    }
    let id = rc.next_id();
    let pad_x = theme::SP_5;
    let pad_y = theme::SP_3;
    let border = theme::STROKE_THIN;

    let body_style = InlineStyle {
        line_height: 1.35,
        ..InlineStyle::prose(rc, tone)
    };
    let head_style = InlineStyle {
        tone: Tone { bold: true, ..tone },
        ..InlineStyle {
            line_height: 1.35,
            ..InlineStyle::prose(rc, tone)
        }
    };

    // Measure.
    let mut natural = vec![0.0f32; ncols];
    let mut min = vec![0.0f32; ncols];
    ui.fonts(|f| {
        let all = std::iter::once((header, &head_style))
            .chain(rows.iter().map(|r| (r.as_slice(), &body_style)));
        for (row, style) in all {
            for (c, cell) in row.iter().enumerate() {
                let (n, m) = measure_cell(f, cell, style);
                natural[c] = natural[c].max(n);
                min[c] = min[c].max(m);
            }
        }
    });
    // Never squeeze a column below a readable floor (unless it is naturally narrower).
    let floor = rc.body * 4.0;
    for (m, n) in min.iter_mut().zip(&natural) {
        *m = m.max(n.min(floor));
    }

    let avail = ui.available_width();
    let chrome = ncols as f32 * pad_x * 2.0 + border * 2.0;
    let widths = fit_columns(&natural, &min, (avail - chrome).max(0.0));
    let table_w: f32 = widths.iter().sum::<f32>() + chrome;

    let draw = |ui: &mut egui::Ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        let top = ui.cursor().min;
        let n_rows = rows.len() + 1;
        let mut col_x = Vec::with_capacity(ncols);
        let empty: Inline = Vec::new();

        for r in 0..n_rows {
            let (cells, style) = if r == 0 {
                (header, &head_style)
            } else {
                (rows[r - 1].as_slice(), &body_style)
            };
            let bg_idx = ui.painter().add(egui::Shape::Noop);
            let row = ui.horizontal_top(|ui| {
                ui.add_space(border);
                for (c, &w) in widths.iter().enumerate() {
                    if r == 0 {
                        col_x.push(ui.cursor().min.x);
                    }
                    let align = match aligns.get(c) {
                        Some(ColAlign::Center) => Align::Center,
                        Some(ColAlign::Right) => Align::Max,
                        _ => Align::Min,
                    };
                    let cell = cells.get(c).unwrap_or(&empty);
                    ui.allocate_ui_with_layout(
                        egui::vec2(w + pad_x * 2.0, 0.0),
                        Layout::top_down(align),
                        |ui| {
                            ui.set_width(w + pad_x * 2.0);
                            egui::Frame::none()
                                .inner_margin(egui::Margin::symmetric(pad_x, pad_y))
                                .show(ui, |ui| {
                                    ui.set_max_width(w);
                                    inline_label(ui, cell, style);
                                });
                        },
                    );
                }
                ui.add_space(border);
            });
            let rect = row.response.rect;
            let fill = if r == 0 {
                th.md_table_header_bg
            } else if r % 2 == 0 {
                th.md_table_row_alt_bg
            } else {
                Color32::TRANSPARENT
            };
            let rounding = egui::Rounding {
                nw: if r == 0 { theme::R_MD } else { 0.0 },
                ne: if r == 0 { theme::R_MD } else { 0.0 },
                sw: if r == n_rows - 1 { theme::R_MD } else { 0.0 },
                se: if r == n_rows - 1 { theme::R_MD } else { 0.0 },
            };
            ui.painter()
                .set(bg_idx, egui::Shape::rect_filled(rect, rounding, fill));
            if r < n_rows - 1 {
                let color = if r == 0 {
                    th.md_table_border
                } else {
                    th.md_table_border.linear_multiply(0.5)
                };
                ui.painter()
                    .hline(rect.x_range(), rect.bottom(), Stroke::new(border, color));
            }
        }

        let bottom = ui.cursor().min.y;
        let outer = egui::Rect::from_min_max(top, egui::pos2(top.x + table_w, bottom));
        let sep = Stroke::new(border, th.md_table_border.linear_multiply(0.5));
        for x in col_x.iter().skip(1) {
            ui.painter().vline(*x, outer.y_range(), sep);
        }
        ui.painter()
            .rect_stroke(outer, theme::R_MD, Stroke::new(border, th.md_table_border));
    };

    if table_w > avail + 0.5 {
        ui.spacing_mut().scroll.floating_allocated_width = 0.0;
        egui::ScrollArea::horizontal()
            .id_source(("md_table_scroll", id))
            .show(ui, |ui| {
                draw(ui);
                ui.add_space(theme::SP_4);
            });
    } else {
        draw(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(spans: &Inline) -> String {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn soft_wrapped_lines_join_into_one_paragraph() {
        let blocks = parse("first line\nsecond line\n\nnext para");
        assert_eq!(blocks.len(), 2);
        match &blocks[0] {
            Block::Paragraph(s) => assert_eq!(text(s), "first line second line"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn inline_code_in_heading_has_no_backticks() {
        let blocks = parse("## Generic (`ObjectId` + `sObjectTypeId`)");
        let Block::Heading { level, text: t } = &blocks[0] else {
            panic!("expected heading");
        };
        assert_eq!(*level, 2);
        assert_eq!(text(t), "Generic (ObjectId + sObjectTypeId)");
        assert!(t.iter().any(|s| s.style.code && s.text == "ObjectId"));
    }

    #[test]
    fn code_nested_in_bold_keeps_both_styles() {
        let blocks = parse("**Why `int` for it:** rest");
        let Block::Paragraph(spans) = &blocks[0] else {
            panic!("expected paragraph");
        };
        let code = spans.iter().find(|s| s.text == "int").expect("code span");
        assert!(code.style.code && code.style.bold);
        assert!(spans.iter().any(|s| s.text == " rest" && !s.style.bold));
    }

    #[test]
    fn adjacent_same_style_text_is_merged() {
        let blocks = parse("a\nb c");
        let Block::Paragraph(spans) = &blocks[0] else {
            panic!("expected paragraph");
        };
        assert_eq!(spans.len(), 1);
    }

    #[test]
    fn table_with_code_cells_and_alignment() {
        let md = "| Col | `Type` |\n|:---|---:|\n| `ObjectId` | int |\n| b |\n";
        let blocks = parse(md);
        let Block::Table {
            aligns,
            header,
            rows,
        } = &blocks[0]
        else {
            panic!("expected table");
        };
        assert_eq!(aligns, &vec![ColAlign::Left, ColAlign::Right]);
        assert_eq!(text(&header[1]), "Type");
        assert!(header[1][0].style.code);
        assert_eq!(rows.len(), 2);
        assert_eq!(text(&rows[0][0]), "ObjectId");
        assert!(rows[0][0][0].style.code);
    }

    #[test]
    fn tight_and_nested_lists() {
        let md = "1. one\n2. two\n   - nested\n3. three\n";
        let blocks = parse(md);
        let Block::List { start, items } = &blocks[0] else {
            panic!("expected list");
        };
        assert_eq!(*start, Some(1));
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[0].blocks[0], Block::Paragraph(s) if text(s) == "one"));
        assert!(matches!(
            &items[1].blocks[1],
            Block::List { start: None, .. }
        ));
    }

    #[test]
    fn task_list_markers() {
        let blocks = parse("- [x] done\n- [ ] todo\n");
        let Block::List { items, .. } = &blocks[0] else {
            panic!("expected list");
        };
        assert_eq!(items[0].checked, Some(true));
        assert_eq!(items[1].checked, Some(false));
    }

    #[test]
    fn fenced_code_keeps_language_and_content() {
        let blocks = parse("```rust title=x\nfn main() {}\n```\n");
        assert_eq!(
            blocks[0],
            Block::Code {
                lang: "rust".into(),
                code: "fn main() {}".into()
            }
        );
    }

    #[test]
    fn links_and_blockquote_alerts() {
        let blocks = parse("> [!WARNING]\n> see [docs](https://x.y)\n");
        let Block::Quote { kind, blocks } = &blocks[0] else {
            panic!("expected quote");
        };
        assert_eq!(*kind, Some(BlockQuoteKind::Warning));
        let Block::Paragraph(spans) = &blocks[0] else {
            panic!("expected paragraph");
        };
        let link = spans.iter().find(|s| s.text == "docs").expect("link");
        assert_eq!(link.style.link.as_deref(), Some("https://x.y"));
    }

    #[test]
    fn front_matter_and_html_comments_are_hidden() {
        let blocks = parse("---\ntitle: x\n---\n\n<!-- hidden -->\n\n# Title\n");
        assert_eq!(blocks.len(), 1);
        assert!(matches!(&blocks[0], Block::Heading { level: 1, .. }));
    }

    #[test]
    fn html_visible_text_strips_tags() {
        assert_eq!(html_visible_text("<p align=\"center\"><img src=x></p>"), "");
        assert_eq!(
            html_visible_text("<details><summary>Click  me</summary>"),
            "Click me"
        );
        assert_eq!(html_visible_text("a <!-- c --> b"), "a b");
    }

    #[test]
    fn fit_columns_keeps_natural_when_room() {
        assert_eq!(
            fit_columns(&[10.0, 20.0], &[5.0, 5.0], 100.0),
            vec![10.0, 20.0]
        );
    }

    #[test]
    fn fit_columns_shrinks_by_slack() {
        let w = fit_columns(&[100.0, 300.0], &[50.0, 50.0], 200.0);
        assert!((w.iter().sum::<f32>() - 200.0).abs() < 0.01);
        assert!(w[0] >= 50.0 && w[1] >= 50.0);
        assert!(w[1] > w[0]);
    }

    #[test]
    fn fit_columns_falls_back_to_minimums() {
        assert_eq!(
            fit_columns(&[100.0, 300.0], &[80.0, 90.0], 100.0),
            vec![80.0, 90.0]
        );
    }

    #[test]
    fn external_url_detection() {
        assert!(is_external_url("https://example.com"));
        assert!(is_external_url("MAILTO:a@b.c"));
        assert!(!is_external_url("#anchor"));
        assert!(!is_external_url("docs/readme.md"));
    }
}
