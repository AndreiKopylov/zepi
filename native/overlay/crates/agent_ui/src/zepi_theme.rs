//! pi TUI theme -> agent panel palette.
//! Loads the pi theme JSON selected in pi settings (ZEPI_PI_CONFIG_DIR/themes/<name>.json)
//! so the GUI agent thread mirrors pi interactive mode.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Instant, UNIX_EPOCH};

use gpui::{App, HighlightStyle, Hsla, Rgba, TextStyleRefinement, relative};
use markdown::{HeadingLevelStyles, MarkdownStyle};
use theme::{ActiveTheme, Appearance, SyntaxTheme, SystemAppearance};

#[derive(Clone, Default)]
pub struct PiPalette {
    pub text: Option<Hsla>,
    pub muted: Option<Hsla>,
    pub faint: Option<Hsla>,
    pub primary: Option<Hsla>,
    pub border: Option<Hsla>,
    pub user_bg: Option<Hsla>,
    pub card_bg: Option<Hsla>,
    pub selected_bg: Option<Hsla>,
    pub tool_pending_bg: Option<Hsla>,
    pub md_heading: Option<Hsla>,
    pub md_link: Option<Hsla>,
    pub md_code: Option<Hsla>,
    pub md_code_block_border: Option<Hsla>,
    pub md_quote: Option<Hsla>,
    pub md_quote_border: Option<Hsla>,
    pub md_hr: Option<Hsla>,
    pub syntax_comment: Option<Hsla>,
    pub syntax_keyword: Option<Hsla>,
    pub syntax_function: Option<Hsla>,
    pub syntax_variable: Option<Hsla>,
    pub syntax_string: Option<Hsla>,
    pub syntax_number: Option<Hsla>,
    pub syntax_type: Option<Hsla>,
    pub syntax_operator: Option<Hsla>,
    pub syntax_punctuation: Option<Hsla>,
}

struct CacheEntry {
    checked: Instant,
    settings_stamp: u64,
    theme_stamp: u64,
    palette: Option<Arc<PiPalette>>,
}

static CACHE: Mutex<Option<CacheEntry>> = Mutex::new(None);

fn file_stamp(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs().wrapping_mul(1_000_000_000).wrapping_add(d.subsec_nanos() as u64))
        .unwrap_or(0)
}

fn parse_hex(raw: &str) -> Option<Hsla> {
    let s = raw.trim().trim_start_matches('#');
    let v = u32::from_str_radix(s, 16).ok()?;
    let (r, g, b) = match s.len() {
        6 => ((v >> 16) & 255, (v >> 8) & 255, v & 255),
        3 => (((v >> 8) & 15) * 17, ((v >> 4) & 15) * 17, (v & 15) * 17),
        _ => return None,
    };
    Some(Hsla::from(Rgba { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }))
}

fn theme_name(settings: &Path, cx: &App) -> Option<String> {
    let raw = std::fs::read_to_string(settings).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let name = value.get("theme")?.as_str()?.trim();
    if name.is_empty() || name == "system" {
        return None;
    }
    // pi stores automatic mode as "light/dark"; follow the Zed appearance.
    if let Some((light, dark)) = name.split_once('/') {
        return Some(match SystemAppearance::global(cx).0 {
            Appearance::Dark => dark.trim().to_string(),
            Appearance::Light => light.trim().to_string(),
        });
    }
    Some(name.to_string())
}

fn load_palette(path: &Path) -> Option<Arc<PiPalette>> {
    let raw = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let vars: HashMap<String, String> = value
        .get("vars")?
        .as_object()?
        .iter()
        .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
        .collect();
    let colors = value.get("colors")?.as_object()?;
    let resolve = |key: &str| -> Option<Hsla> {
        let raw = colors.get(key)?.as_str()?;
        if let Some(hex) = vars.get(raw) {
            return parse_hex(hex);
        }
        parse_hex(raw)
    };
    let export_card = value
        .get("export")
        .and_then(|e| e.get("cardBg"))
        .and_then(|c| c.as_str())
        .and_then(parse_hex);
    let palette = PiPalette {
        text: resolve("text"),
        muted: resolve("muted"),
        faint: resolve("dim").or(resolve("faint")),
        primary: vars.get("primary").and_then(|v| parse_hex(v)),
        border: resolve("border"),
        user_bg: resolve("userMessageBg"),
        card_bg: resolve("userMessageBg").or(export_card),
        selected_bg: resolve("selectedBg"),
        tool_pending_bg: resolve("toolPendingBg"),
        md_heading: resolve("mdHeading"),
        md_link: resolve("mdLink"),
        md_code: resolve("mdCode"),
        md_code_block_border: resolve("mdCodeBlockBorder"),
        md_quote: resolve("mdQuote"),
        md_quote_border: resolve("mdQuoteBorder"),
        md_hr: resolve("mdHr"),
        syntax_comment: resolve("syntaxComment"),
        syntax_keyword: resolve("syntaxKeyword"),
        syntax_function: resolve("syntaxFunction"),
        syntax_variable: resolve("syntaxVariable"),
        syntax_string: resolve("syntaxString"),
        syntax_number: resolve("syntaxNumber"),
        syntax_type: resolve("syntaxType"),
        syntax_operator: resolve("syntaxOperator"),
        syntax_punctuation: resolve("syntaxPunctuation"),
    };
    Some(Arc::new(palette))
}

pub fn palette(cx: &App) -> Option<Arc<PiPalette>> {
    let dir = std::env::var_os("ZEPI_PI_CONFIG_DIR")?;
    let dir = std::path::PathBuf::from(dir);
    let settings = dir.join("settings.json");
    let s_stamp = file_stamp(&settings);
    let mut cache = CACHE.lock().unwrap_or_else(|p| p.into_inner());
    let fresh = cache
        .as_ref()
        .is_some_and(|c| c.checked.elapsed().as_millis() < 1000 && c.settings_stamp == s_stamp);
    if fresh {
        return cache.as_ref().and_then(|c| c.palette.clone());
    }
    let name = theme_name(&settings, cx);
    let theme_path = name.map(|n| dir.join("themes").join(format!("{n}.json")));
    let t_stamp = theme_path.as_ref().map(|p| file_stamp(p)).unwrap_or(0);
    let reuse = cache
        .as_ref()
        .filter(|c| c.settings_stamp == s_stamp && c.theme_stamp == t_stamp)
        .and_then(|c| c.palette.clone());
    let palette = reuse.or_else(|| theme_path.as_ref().and_then(|p| load_palette(p)));
    *cache = Some(CacheEntry {
        checked: Instant::now(),
        settings_stamp: s_stamp,
        theme_stamp: t_stamp,
        palette: palette.clone(),
    });
    palette
}

/// GUI knobs live in Zed settings under "zepi", next to the officeViewer knobs.
fn zed_knob_f(key: &str) -> Option<f32> {
    use std::sync::Mutex;
    static KNOB_CACHE: Mutex<Option<(u64, Option<f32>)>> = Mutex::new(None);
    let mtime = std::fs::metadata(paths::settings_file())
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as u64);
    let mut guard = KNOB_CACHE.lock().unwrap_or_else(|p| p.into_inner());
    if guard.as_ref().map_or(true, |(cached, _)| *cached != mtime) {
        let value = std::fs::read_to_string(paths::settings_file())
            .ok()
            .and_then(|raw| serde_json_lenient::from_str::<serde_json_lenient::Value>(&raw).ok())
            .and_then(|root| root["zepi"][key].as_f64())
            .map(|v| v as f32);
        *guard = Some((mtime, value));
    }
    guard.as_ref().and_then(|(_, value)| *value)
}

/// Thread markdown line spacing; override via "zepi": {"threadLineHeight": ...} in Zed settings.
pub fn thread_line_height() -> f32 {
    zed_knob_f("threadLineHeight").unwrap_or(1.5)
}

fn pi_style_for(p: &PiPalette, name: &str) -> Option<HighlightStyle> {
    let color = if name == "comment" || name.starts_with("comment.") {
        p.syntax_comment
    } else if matches!(name, "keyword" | "emphasis" | "preproc" | "label") {
        p.syntax_keyword
    } else if matches!(name, "function" | "title" | "constructor") {
        p.syntax_function
    } else if name == "variable" || name.starts_with("variable.") {
        p.syntax_variable
    } else if name == "string" || name.starts_with("string.") || name == "text.literal" {
        p.syntax_string
    } else if matches!(name, "number" | "constant") {
        p.syntax_number
    } else if matches!(name, "type" | "tag" | "attribute") {
        p.syntax_type
    } else if name == "operator" {
        p.syntax_operator
    } else if name == "punctuation" || name.starts_with("punctuation.") {
        p.syntax_punctuation
    } else if matches!(name, "link_text" | "link_uri") {
        p.md_link
    } else {
        None
    };
    color.map(|color| HighlightStyle { color: Some(color), ..Default::default() })
}

// Code block highlight ids are resolved against the global theme and cached per text, so the
// agent syntax theme must reuse the global index order or the colors land on wrong tokens.
static SYNTAX_CACHE: Mutex<Option<(usize, usize, Arc<SyntaxTheme>)>> = Mutex::new(None);

pub fn agent_syntax(cx: &App) -> Option<Arc<SyntaxTheme>> {
    let palette = palette(cx)?;
    let global = cx.theme().syntax().clone();
    let key = (Arc::as_ptr(&global) as usize, Arc::as_ptr(&palette) as usize);
    if let Some((g, pl, theme)) = SYNTAX_CACHE.lock().unwrap_or_else(|p| p.into_inner()).clone() {
        if (g, pl) == key {
            return Some(theme);
        }
    }
    let mut pairs = Vec::new();
    let mut idx = 0usize;
    while let Some(name) = global.get_capture_name(idx) {
        let style = pi_style_for(&palette, name)
            .or_else(|| global.get(idx).cloned())
            .unwrap_or_default();
        pairs.push((name.to_string(), style));
        idx += 1;
    }
    let theme = Arc::new(SyntaxTheme::new(pairs));
    *SYNTAX_CACHE.lock().unwrap_or_else(|p| p.into_inner()) = Some((key.0, key.1, theme.clone()));
    Some(theme)
}

pub fn style_markdown(mut style: MarkdownStyle, cx: &App) -> MarkdownStyle {
    style.paragraph_line_height = relative(thread_line_height()).into();
    let Some(p) = palette(cx) else { return style };
    if let Some(c) = p.text {
        style.base_text_style.color = c;
    }
    if let Some(c) = p.selected_bg {
        style.selection_background_color = c;
    }
    if let Some(c) = p.md_link {
        style.link.color = Some(c);
        if let Some(underline) = style.link.underline.as_mut() {
            underline.color = Some(c.opacity(0.5));
        }
    }
    if let Some(c) = p.md_code {
        style.inline_code.color = Some(c);
    }
    if let Some(c) = p.card_bg {
        style.inline_code.background_color = Some(c);
        style.code_block.background = Some(c.into());
    }
    if let Some(c) = p.md_code_block_border {
        style.code_block.border_color = Some(c);
    }
    if let Some(c) = p.md_quote {
        style.block_quote.color = Some(c);
    }
    if let Some(c) = p.md_quote_border {
        style.block_quote_border_color = c;
    }
    if let Some(c) = p.md_hr {
        style.rule_color = c;
    }
    if let Some(c) = p.md_heading {
        let refinement = || {
            TextStyleRefinement {
                color: Some(c),
                ..Default::default()
            }
        };
        style.heading_level_styles = Some(HeadingLevelStyles {
            h1: Some(refinement()),
            h2: Some(refinement()),
            h3: Some(refinement()),
            h4: Some(refinement()),
            h5: Some(refinement()),
            h6: Some(refinement()),
        });
    }
    if let Some(syntax) = agent_syntax(cx) { style.syntax = syntax; }
    style
}

pub fn style_markdown_muted(style: MarkdownStyle, cx: &App) -> MarkdownStyle {
    let mut style = style_markdown(style, cx);
    if let Some(c) = palette(cx).and_then(|p| p.muted) {
        style.base_text_style.color = c;
    }
    style
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colors() {
        let c = parse_hex("#d97757").expect("hex");
        assert!(c.l > 0.0 && c.l < 1.0);
        assert!(parse_hex("nope").is_none());
        assert!(parse_hex("#12345").is_none());
    }
}