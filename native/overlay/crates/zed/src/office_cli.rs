//! `zepi office ...`: document rendering for scripts and the agent, without the GUI.
//!
//! Uses the same renderer as the built-in viewers, so the produced images match what the
//! user sees in Zepi and need neither LibreOffice nor Microsoft Office.

use anyhow::{bail, Context as _, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const MAX_PIXELS: f64 = 6_000_000.0;
/// Renders older than this are removed on the next run, so the cache cannot grow forever.
const RENDER_TTL_SECS: u64 = 7 * 24 * 60 * 60;

/// Returns the arguments of an "zepi office ..." invocation, or None for the GUI.
pub fn requested() -> Option<Vec<String>> {
    let mut args = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned());
    (args.next().as_deref() == Some("office")).then(|| args.collect())
}

/// Runs the subcommand and returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    let (command, rest) = match args.split_first() {
        Some((command, rest)) => (command.as_str(), rest),
        None => {
            print(
                &json!({"ok": false, "error": "usage: zepi office render <file> --out DIR [--dpi N] [--pages 1-3,5]"}),
            );
            return 2;
        }
    };
    let result = match command {
        "render" => render(rest),
        other => Err(anyhow::anyhow!("unknown command: {other}")),
    };
    match result {
        Ok(data) => {
            print(&json!({"ok": true, "data": data}));
            0
        }
        Err(error) => {
            print(&json!({"ok": false, "error": format!("{error:#}")}));
            1
        }
    }
}

fn print(value: &Value) {
    println!("{value}");
}

fn render(args: &[String]) -> Result<Value> {
    let mut file: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut dpi = 150.0_f64;
    let mut pages: Option<Vec<usize>> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" | "-o" => out = Some(PathBuf::from(next(args, &mut index)?)),
            "--dpi" => {
                dpi = next(args, &mut index)?
                    .parse()
                    .context("--dpi expects a number, for example 150")?
            }
            "--pages" => pages = Some(parse_pages(&next(args, &mut index)?)?),
            other if other.starts_with('-') => bail!("unknown option: {other}"),
            other => file = Some(PathBuf::from(other)),
        }
        index += 1;
    }
    let file = file.context("missing file to render")?;
    let cache = render_root();
    purge_old(&cache);
    let out = out.unwrap_or_else(|| out_for(&cache, &file));
    if !(30.0..=600.0).contains(&dpi) {
        bail!("--dpi must be between 30 and 600");
    }
    let scale = dpi / 72.0;
    let bytes = std::fs::read(&file).with_context(|| format!("reading {}", file.display()))?;
    std::fs::create_dir_all(&out).with_context(|| format!("creating {}", out.display()))?;
    let extension = file
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let produced = match extension.as_str() {
        "pptx" | "potx" => render_pptx(&bytes, &out, scale, dpi, pages.as_deref())?,
        "docx" | "docm" => render_docx(&bytes, &out, scale, pages.as_deref())?,
        "xlsx" | "xlsm" => {
            bail!("spreadsheets have no built-in renderer; export them with LibreOffice or Office")
        }
        other => bail!("unsupported format: {other}"),
    };
    Ok(json!({
        "file": file.display().to_string(),
        "out": out.display().to_string(),
        "pages": produced.len(),
        "files": produced,
    }))
}

fn next(args: &[String], index: &mut usize) -> Result<String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .with_context(|| format!("{} expects a value", args[*index - 1]))
}

/// Parses "1-3,5" into zero-based page indexes, keeping the written order.
fn parse_pages(spec: &str) -> Result<Vec<usize>> {
    let mut pages = Vec::new();
    for part in spec
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        match part.split_once('-') {
            Some((from, to)) => {
                let from: usize = from.trim().parse().context("--pages expects numbers")?;
                let to: usize = to.trim().parse().context("--pages expects numbers")?;
                if from == 0 || to < from {
                    bail!("--pages range {part} is empty; pages start at 1");
                }
                pages.extend((from..=to).map(|page| page - 1));
            }
            None => {
                let page: usize = part.parse().context("--pages expects numbers")?;
                if page == 0 {
                    bail!("--pages starts at 1");
                }
                pages.push(page - 1);
            }
        }
    }
    if pages.is_empty() {
        bail!("--pages got no page numbers");
    }
    Ok(pages)
}

fn selected(count: usize, pages: Option<&[usize]>) -> Vec<usize> {
    match pages {
        Some(pages) => pages.iter().copied().filter(|page| *page < count).collect(),
        None => (0..count).collect(),
    }
}

fn render_pptx(
    bytes: &[u8],
    out: &Path,
    scale: f64,
    dpi: f64,
    pages: Option<&[usize]>,
) -> Result<Vec<String>> {
    let (presentation, _warnings) =
        deckcraft_pptx::import_checked(bytes).context("reading the presentation")?;
    let count = presentation.slides.len();
    let mut files = Vec::new();
    for page in selected(count, pages) {
        let scale = fit_scale(
            scale,
            presentation.slide_size.width,
            presentation.slide_size.height,
        );
        let options = deckcraft_render::RenderOpts {
            scale,
            threads: 4,
            ..Default::default()
        };
        let image = deckcraft_render::render_slide(&presentation, page, &options);
        let name = format!("slide-{}.png", page + 1);
        let png = image.to_png_at(dpi);
        if png.is_empty() {
            bail!("slide {} did not render", page + 1);
        }
        std::fs::write(out.join(&name), png).with_context(|| format!("writing {name}"))?;
        files.push(name);
    }
    Ok(files)
}

fn render_docx(
    bytes: &[u8],
    out: &Path,
    scale: f64,
    pages: Option<&[usize]>,
) -> Result<Vec<String>> {
    let document =
        wordcraft_docx::read(bytes).map_err(|error| anyhow::anyhow!("docx import: {error:?}"))?;
    let mut cache = wordcraft_layout::LayoutCache::new();
    let layout = wordcraft_layout::layout(
        &document,
        &mut cache,
        &wordcraft_layout::LayoutOptions::default(),
    );
    let count = layout.pages.len();
    let mut files = Vec::new();
    for page in selected(count, pages) {
        let raster = &layout.pages[page];
        let scale = fit_scale(scale, raster.w as f64, raster.h as f64);
        let rendered = wordcraft_render::render_page(
            &document,
            raster,
            scale as f32,
            &wordcraft_render::RenderOptions::default(),
        );
        let name = format!("page-{}.png", page + 1);
        let png = rendered.to_png();
        if png.is_empty() {
            bail!("page {} did not render", page + 1);
        }
        std::fs::write(out.join(&name), png).with_context(|| format!("writing {name}"))?;
        files.push(name);
    }
    Ok(files)
}

/// Renders live in the user's profile, never next to the document or in the working directory.
/// State root shared with the viewers: renders and slide images live side by side.
pub fn state_root() -> PathBuf {
    match std::env::var_os("LOCALAPPDATA") {
        Some(dir) => PathBuf::from(dir).join("Zepi"),
        None => std::env::temp_dir().join("zepi"),
    }
}

fn render_root() -> PathBuf {
    state_root().join("render")
}

fn out_for(cache: &Path, file: &Path) -> PathBuf {
    let stem: String = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("document")
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    cache.join(if stem.is_empty() {
        "document".to_owned()
    } else {
        stem
    })
}

/// Drops renders nobody looked at for a week; a render in progress keeps its directory fresh.
fn purge_old(cache: &Path) {
    let Ok(entries) = std::fs::read_dir(cache) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_dir() {
            continue;
        }
        let stale = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age.as_secs() > RENDER_TTL_SECS);
        if stale {
            std::fs::remove_dir_all(entry.path()).ok();
        }
    }
}

/// Drops slide and page images the viewers left behind more than a week ago.
pub fn purge_temp(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let stale = metadata
            .modified()
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age.as_secs() > RENDER_TTL_SECS);
        if !stale {
            continue;
        }
        if metadata.is_dir() {
            std::fs::remove_dir_all(entry.path()).ok();
        } else {
            std::fs::remove_file(entry.path()).ok();
        }
    }
}

/// Keeps a raster under MAX_PIXELS so a large page does not exhaust memory.
fn fit_scale(scale: f64, width: f64, height: f64) -> f64 {
    let area = width * height * scale * scale;
    if area > MAX_PIXELS {
        scale * (MAX_PIXELS / area).sqrt()
    } else {
        scale
    }
}
