//! Read-only import of HTML-based .doc files (Confluence MHTML exports, plain HTML).
//! Honors inline CSS (font, size, color, background) the way Word's HTML import does.

use std::sync::Arc;

use base64::Engine as _;
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use wordcraft_doc::Block;
use wordcraft_doc::para::{Paragraph, Run};
use wordcraft_doc::props::{
    Border, BorderStyle, Borders, CharProps, LineSpacing, ParaProps, Rgb, TableProps, TextColor,
    Underline,
};
use wordcraft_doc::table::{Cell, Row, Table};
use wordcraft_doc::{para_block, Document};

pub fn looks_like_html(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_ascii_lowercase();
    let t = head.trim_start();
    t.starts_with("date:")
        || t.starts_with("mime-version:")
        || t.starts_with("from ")
        || t.contains("<html")
        || t.contains("<!doctype html")
}

pub fn read(bytes: &[u8]) -> anyhow::Result<Document> {
    let html = extract_html(bytes)?;
    let dom = parse_document(RcDom::default(), Default::default()).one(html.as_str());
    let body = find_body(&dom.document).unwrap_or_else(|| dom.document.clone());
    let mut conv = Conv::default();
    conv.base = Fmt {
        font: Some("Times New Roman".to_string()),
        size: Some(12.0),
        color: Some(Rgb(0, 0, 0)),
        ..Default::default()
    };
    let body_css = style_of(&body);
    if body_css.font.is_some() {
        conv.base.font = body_css.font;
    }
    if body_css.size.is_some() {
        conv.base.size = body_css.size;
    }
    if body_css.color.is_some() {
        conv.base.color = body_css.color;
    }
    conv.walk_children(&body, 0.0);
    conv.flush();
    let mut doc = Document::new();
    if !conv.blocks.is_empty() {
        doc.body = conv.blocks;
    }
    Ok(doc)
}

fn extract_html(bytes: &[u8]) -> anyhow::Result<String> {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let t = text.trim_start();
    if t.starts_with('<') {
        return Ok(text);
    }
    let (headers, body) = split_headers(&text);
    if let Some(boundary) = header_param(&headers, "boundary") {
        let delim = format!("--{boundary}");
        for part in body.split(delim.as_str()) {
            let (ph, pb) = split_headers(part);
            if ph.to_ascii_lowercase().contains("content-type: text/html") {
                return Ok(decode_transfer(&ph, pb));
            }
        }
    }
    if body.trim_start().starts_with('<') {
        return Ok(body.to_string());
    }
    anyhow::bail!("no HTML part in .doc")
}

fn split_headers(text: &str) -> (String, &str) {
    for (sep, skip) in [("\r\n\r\n", 4), ("\n\n", 2)] {
        if let Some(ix) = text.find(sep) {
            return (text[..ix].to_string(), &text[ix + skip..]);
        }
    }
    (String::new(), text)
}

fn header_param(headers: &str, key: &str) -> Option<String> {
    let lower = headers.to_ascii_lowercase();
    let ix = lower.find(&format!("{key}="))?;
    let rest = &headers[ix + key.len() + 1..];
    let rest = rest.trim_start();
    let value = if let Some(quoted) = rest.strip_prefix('"') {
        quoted.split('"').next().unwrap_or("")
    } else {
        rest.split(|c: char| c.is_whitespace() || c == ';').next().unwrap_or("")
    };
    (!value.is_empty()).then(|| value.to_string())
}

fn decode_transfer(headers: &str, body: &str) -> String {
    let lower = headers.to_ascii_lowercase();
    if lower.contains("content-transfer-encoding: base64") {
        let clean: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(clean) {
            return String::from_utf8_lossy(&bytes).into_owned();
        }
    }
    if lower.contains("content-transfer-encoding: quoted-printable") {
        return String::from_utf8_lossy(&decode_qp(body)).into_owned();
    }
    body.to_string()
}

fn decode_qp(text: &str) -> Vec<u8> {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'=' {
            if i + 2 < b.len() && b[i + 1] == b'\r' && b[i + 2] == b'\n' {
                i += 3;
                continue;
            }
            if i + 1 < b.len() && b[i + 1] == b'\n' {
                i += 2;
                continue;
            }
            if i + 2 < b.len() {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("");
                if let Ok(v) = u8::from_str_radix(hex, 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn find_body(node: &Handle) -> Option<Handle> {
    if let NodeData::Element { name, .. } = &node.data {
        if name.local.as_ref() == "body" {
            return Some(node.clone());
        }
    }
    for child in node.children.borrow().iter() {
        if let Some(found) = find_body(child) {
            return Some(found);
        }
    }
    None
}

#[derive(Clone, Default)]
struct Css {
    font: Option<String>,
    size: Option<f32>,
    color: Option<Rgb>,
    bg: Option<Rgb>,
    bold: Option<bool>,
    italic: Option<bool>,
    padding: f32,
    border: Option<Rgb>,
}

fn style_of(node: &Handle) -> Css {
    if let NodeData::Element { attrs, .. } = &node.data {
        for a in attrs.borrow().iter() {
            if a.name.local.as_ref() == "style" {
                return parse_style(&a.value);
            }
        }
    }
    Css::default()
}

fn parse_style(s: &str) -> Css {
    let mut c = Css::default();
    for decl in s.split(';') {
        let Some((prop, val)) = decl.split_once(':') else { continue };
        let prop = prop.trim().to_ascii_lowercase();
        let val = val.trim();
        match prop.as_str() {
            "font-family" => {
                c.font = val.split(',').next().map(|f| {
                    f.trim().trim_matches('"').trim_matches('\'').to_string()
                });
            }
            "font-size" => c.size = parse_size(val),
            "color" => c.color = parse_color(val),
            "background-color" | "background" => c.bg = parse_color(val),
            "font-weight" => {
                c.bold = Some(val.contains("bold") || val.parse::<u32>().is_ok_and(|w| w >= 600));
            }
            "font-style" => c.italic = Some(val.contains("italic")),
            "padding" => c.padding = parse_size(val).unwrap_or(0.0),
            "border" => {
                c.border = Some(parse_color(val).unwrap_or_else(|| {
                    val.find("rgb(").and_then(|i| parse_color(&val[i..])).unwrap_or(Rgb(60, 60, 60))
                }));
            }
            _ => {}
        }
    }
    c
}

fn parse_color(v: &str) -> Option<Rgb> {
    let v = v.trim();
    if let Some(rest) = v.strip_prefix("rgb(") {
        let n: Vec<u8> = rest
            .trim_end_matches(')')
            .split(',')
            .filter_map(|x| x.trim().parse::<u8>().ok())
            .collect();
        return (n.len() >= 3).then(|| Rgb(n[0], n[1], n[2]));
    }
    if let Some(hex) = v.strip_prefix('#') {
        return Rgb::parse(hex);
    }
    match v.to_ascii_lowercase().as_str() {
        "black" => Some(Rgb(0, 0, 0)),
        "white" => Some(Rgb(255, 255, 255)),
        "red" => Some(Rgb(255, 0, 0)),
        _ => None,
    }
}

fn parse_size(v: &str) -> Option<f32> {
    let v = v.trim();
    if let Some(px) = v.strip_suffix("px") {
        return px.trim().parse::<f32>().ok().map(|x| x * 0.75);
    }
    if let Some(pt) = v.strip_suffix("pt") {
        return pt.trim().parse().ok();
    }
    if let Some(em) = v.strip_suffix("em") {
        return em.trim().parse::<f32>().ok().map(|x| x * 12.0);
    }
    v.parse::<f32>().ok()
}

#[derive(Clone, Default, PartialEq)]
struct Fmt {
    bold: bool,
    italic: bool,
    underline: bool,
    mono: bool,
    link: bool,
    font: Option<String>,
    size: Option<f32>,
    color: Option<Rgb>,
}

impl Fmt {
    fn props(&self) -> CharProps {
        CharProps {
            bold: self.bold.then_some(true),
            italic: self.italic.then_some(true),
            underline: (self.underline || self.link).then_some(Underline::Single),
            font: self.font.clone().or_else(|| self.mono.then(|| "Consolas".to_string())),
            size: self.size,
            color: self.color.map(TextColor::Rgb),
            style: self.link.then(|| "Hyperlink".to_string()),
            ..Default::default()
        }
    }
}

#[derive(Default)]
struct Conv {
    blocks: Vec<Arc<Block>>,
    segments: Vec<(String, Fmt)>,
    style: Option<String>,
    indent: f32,
    base: Fmt,
    extra: ParaProps,
}

const BLOCK_TAGS: &[&str] = &[
    "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "ul", "ol", "li", "table", "pre",
    "blockquote", "section", "article", "main", "center", "hr",
];

impl Conv {
    fn start(&mut self, style: Option<&str>, indent: f32) {
        self.flush();
        self.style = style.map(|s| s.to_string());
        self.indent = indent;
        self.extra = ParaProps::default();
    }

    fn push(&mut self, text: &str, fmt: &Fmt) {
        if text.is_empty() {
            return;
        }
        let mut merged = self.base.clone();
        merged.bold |= fmt.bold;
        merged.italic |= fmt.italic;
        merged.underline |= fmt.underline;
        merged.mono |= fmt.mono;
        merged.link |= fmt.link;
        merged.font = fmt.font.clone().or(merged.font);
        merged.size = fmt.size.or(merged.size);
        merged.color = fmt.color.or(merged.color);
        let text = if text.starts_with(' ')
            && self.segments.last().is_some_and(|(s, _)| s.ends_with(' '))
        {
            &text[1..]
        } else {
            text
        };
        if let Some(last) = self.segments.last_mut() {
            if last.1 == merged {
                last.0.push_str(text);
                return;
            }
        }
        self.segments.push((text.to_string(), merged));
    }

    fn flush(&mut self) {
        if self.segments.is_empty() {
            return;
        }
        let mut segs = std::mem::take(&mut self.segments);
        if let Some(first) = segs.first_mut() {
            first.0 = first.0.trim_start().to_string();
        }
        if let Some(last) = segs.last_mut() {
            last.0 = last.0.trim_end().to_string();
        }
        segs.retain(|(s, _)| !s.is_empty());
        if segs.is_empty() {
            return;
        }
        let mut text = String::new();
        let mut runs = Vec::new();
        for (s, fmt) in segs {
            text.push_str(&s);
            runs.push(Run { len: s.len(), props: fmt.props() });
        }
        let mut para = Paragraph::new();
        para.text = text;
        para.runs = runs;
        para.props = ParaProps {
            style: self.style.clone(),
            indent_left: (self.indent > 0.0).then_some(self.indent),
            ..self.extra.clone()
        };
        self.blocks.push(para_block(para));
    }

    fn walk_children(&mut self, node: &Handle, indent: f32) {
        for child in node.children.borrow().iter() {
            self.walk(child, indent);
        }
    }

    fn walk(&mut self, node: &Handle, indent: f32) {
        match &node.data {
            NodeData::Text { contents } => {
                let s = collapse(&contents.borrow());
                if !s.trim().is_empty() {
                    self.start(None, indent);
                    self.push(&s, &Fmt::default());
                    self.flush();
                }
            }
            NodeData::Element { name, .. } => {
                let tag = name.local.as_ref();
                match tag {
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        let level = (tag.as_bytes()[1] - b'0') as usize;
                        let size = [24.0, 18.0, 14.0, 12.0, 10.0, 8.0][level - 1];
                        self.start(None, indent);
                        self.extra.space_before = Some(if level <= 2 { 12.0 } else { 8.0 });
                        self.extra.space_after = Some(6.0);
                        let hf = Fmt {
                            bold: true,
                            font: Some("Times New Roman".to_string()),
                            size: Some(size),
                            color: Some(Rgb(0, 0, 0)),
                            ..Default::default()
                        };
                        self.inline_children(node, &hf, false);
                        self.flush();
                    }
                    "p" => {
                        let css = style_of(node);
                        let f = Fmt {
                            font: css.font,
                            size: css.size,
                            color: css.color,
                            bold: css.bold.unwrap_or(false),
                            italic: css.italic.unwrap_or(false),
                            ..Default::default()
                        };
                        self.start(None, indent);
                        self.inline_children(node, &f, false);
                        self.flush();
                    }
                    "pre" => self.pre(node, indent),
                    "ul" | "ol" => {
                        let ordered = tag == "ol";
                        let mut n = 0;
                        for li in node.children.borrow().iter() {
                            let NodeData::Element { name, .. } = &li.data else { continue };
                            if name.local.as_ref() != "li" {
                                continue;
                            }
                            n += 1;
                            self.start(Some("ListParagraph"), indent + 18.0);
                            let prefix = if ordered { format!("{n}. ") } else { "\u{2022} ".to_string() };
                            self.push(&prefix, &Fmt::default());
                            for child in li.children.borrow().iter() {
                                if is_block(child) {
                                    self.flush();
                                    self.walk(child, indent + 18.0);
                                } else {
                                    self.inline(child, &Fmt::default(), false);
                                }
                            }
                            self.flush();
                        }
                    }
                    "table" => {
                        self.flush();
                        let table = self.table(node);
                        self.blocks.push(table);
                    }
                    "blockquote" => self.walk_children(node, indent + 24.0),
                    "hr" => {
                        self.flush();
                        let mut para = Paragraph::new();
                        para.props = ParaProps {
                            borders: Some(Borders {
                                bottom: Some(Border { style: BorderStyle::Single, width: 0.75, color: Some(Rgb(160, 160, 160)), space: 1.0 }),
                                ..Default::default()
                            }),
                            ..Default::default()
                        };
                        self.blocks.push(para_block(para));
                    }
                    "div" | "section" | "article" | "main" | "center" | "body" | "html" => {
                        if node.children.borrow().iter().any(is_block) {
                            self.walk_children(node, indent);
                        } else {
                            let css = style_of(node);
                            let f = Fmt { font: css.font, size: css.size, color: css.color, ..Default::default() };
                            self.start(None, indent);
                            self.inline_children(node, &f, false);
                            self.flush();
                        }
                    }
                    "script" | "style" | "head" | "meta" | "link" | "title" => {}
                    _ => {
                        self.start(None, indent);
                        self.inline_children(node, &Fmt::default(), false);
                        self.flush();
                    }
                }
            }
            _ => {}
        }
    }

    fn inline_children(&mut self, node: &Handle, fmt: &Fmt, pre: bool) {
        for child in node.children.borrow().iter() {
            self.inline(child, fmt, pre);
        }
    }

    fn inline(&mut self, node: &Handle, fmt: &Fmt, pre: bool) {
        match &node.data {
            NodeData::Text { contents } => {
                let raw = contents.borrow();
                let s = if pre { raw.to_string() } else { collapse(&raw) };
                self.push(&s, fmt);
            }
            NodeData::Element { name, .. } => {
                let css = style_of(node);
                let mut f = fmt.clone();
                if css.font.is_some() {
                    f.font = css.font;
                }
                if css.size.is_some() {
                    f.size = css.size;
                }
                if css.color.is_some() {
                    f.color = css.color;
                }
                if css.bold == Some(true) {
                    f.bold = true;
                }
                if css.italic == Some(true) {
                    f.italic = true;
                }
                match name.local.as_ref() {
                    "b" | "strong" => f.bold = true,
                    "i" | "em" => f.italic = true,
                    "u" | "ins" => f.underline = true,
                    "a" => f.link = true,
                    "code" | "tt" | "kbd" | "samp" => f.mono = true,
                    "br" => {
                        let (style, indent, extra) = (self.style.clone(), self.indent, self.extra.clone());
                        self.flush();
                        self.style = style;
                        self.indent = indent;
                        self.extra = extra;
                        return;
                    }
                    "script" | "style" | "img" => return,
                    _ => {}
                }
                self.inline_children(node, &f, pre);
            }
            _ => {}
        }
    }

    fn pre(&mut self, node: &Handle, indent: f32) {
        let css = style_of(node);
        let size = css.size.unwrap_or(9.75);
        let base = Fmt {
            font: Some(css.font.clone().unwrap_or_else(|| "Consolas".to_string())),
            size: Some(size),
            color: css.color.or_else(|| css.bg.map(|_| Rgb(220, 220, 220))),
            ..Default::default()
        };
        self.flush();
        let mut segs: Vec<(String, Fmt)> = Vec::new();
        self.pre_walk(node, &base, &mut segs);
        let mut text = String::new();
        let mut runs = Vec::new();
        for (t, fm) in segs {
            if t.is_empty() { continue; }
            text.push_str(&t);
            runs.push(Run { len: t.len(), props: fm.props() });
        }
        let mut para = Paragraph::new();
        para.text = text;
        para.runs = runs;
        para.props = ParaProps {
            shading: css.bg,
            indent_left: (indent > 0.0).then_some(indent),
            space_before: Some(0.0),
            space_after: Some(0.0),
            line_spacing: Some(LineSpacing::Exactly(size * 1.5)),
            borders: (css.padding > 0.0 || css.border.is_some()).then(|| {
                Borders::box_(Border {
                    style: if css.border.is_some() { BorderStyle::Single } else { BorderStyle::None },
                    width: 0.75,
                    color: css.border,
                    space: css.padding,
                })
            }),
            ..Default::default()
        };
        self.blocks.push(para_block(para));
    }

    fn pre_walk(&mut self, node: &Handle, fmt: &Fmt, runs: &mut Vec<(String, Fmt)>) {
        match &node.data {
            NodeData::Text { contents } => {
                let t = contents.borrow().replace('\r', "");
                if !t.is_empty() { runs.push((t, fmt.clone())); }
            }
            NodeData::Element { name, .. } => {
                let tag = name.local.as_ref();
                if tag == "br" { runs.push(("\n".to_string(), fmt.clone())); return; }
                if tag == "script" || tag == "style" { return; }
                let c = style_of(node);
                let mut fm = fmt.clone();
                if c.color.is_some() { fm.color = c.color; }
                if c.font.is_some() { fm.font = c.font; }
                if c.size.is_some() { fm.size = c.size; }
                if c.bold == Some(true) { fm.bold = true; }
                if c.italic == Some(true) { fm.italic = true; }
                for child in node.children.borrow().iter() { self.pre_walk(child, &fm, runs); }
            }
            _ => {}
        }
    }

    fn table(&mut self, node: &Handle) -> Arc<Block> {
        let mut rows: Vec<Row> = Vec::new();
        let mut cols = 0usize;
        let mut trs = Vec::new();
        collect_rows(node, &mut trs);
        for tr in trs.iter() {
            let mut cells = Vec::new();
            for td in tr.children.borrow().iter() {
                let NodeData::Element { name, .. } = &td.data else { continue };
                let tag = name.local.as_ref();
                if tag != "td" && tag != "th" {
                    continue;
                }
                let mut sub = Conv::default();
                sub.base = self.base.clone();
                if tag == "th" {
                    sub.base.bold = true;
                }
                sub.walk_children(td, 0.0);
                sub.flush();
                if sub.blocks.is_empty() {
                    sub.blocks.push(para_block(Paragraph::new()));
                }
                cells.push(Cell { props: Default::default(), blocks: sub.blocks, controls: Default::default() });
            }
            cols = cols.max(cells.len());
            if !cells.is_empty() {
                rows.push(Row { props: Default::default(), cells, controls: Default::default() });
            }
        }
        let cols = cols.max(1);
        let table = Table {
            props: TableProps { style: Some("TableGrid".to_string()), ..Default::default() },
            grid: vec![468.0 / cols as f32; cols],
            rows,
            controls: Default::default(),
        };
        Arc::new(Block::Table(table))
    }
}

fn collect_rows(node: &Handle, out: &mut Vec<Handle>) {
    for child in node.children.borrow().iter() {
        if let NodeData::Element { name, .. } = &child.data {
            let t = name.local.as_ref();
            if t == "tr" {
                out.push(child.clone());
            } else if t == "thead" || t == "tbody" || t == "tfoot" {
                collect_rows(child, out);
            }
        }
    }
}

fn is_block(node: &Handle) -> bool {
    match &node.data {
        NodeData::Element { name, .. } => BLOCK_TAGS.contains(&name.local.as_ref()),
        _ => false,
    }
}

fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = text.chars().next().is_some_and(|c| c.is_whitespace());
    for c in text.chars() {
        if c.is_whitespace() {
            space = true;
        } else {
            if space {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

fn raw_text(node: &Handle) -> String {
    let mut out = String::new();
    match &node.data {
        NodeData::Text { contents } => out.push_str(&contents.borrow()),
        NodeData::Element { name, .. } if name.local.as_ref() == "br" => out.push('\n'),
        _ => {
            for child in node.children.borrow().iter() {
                out.push_str(&raw_text(child));
            }
        }
    }
    out
}
