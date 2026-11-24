//! `zepi office ...`: document rendering for scripts and the agent, without the GUI.
//!
//! Uses the same renderer as the built-in viewers, so the produced images match what the
//! user sees in Zepi and need neither LibreOffice nor Microsoft Office.

use anyhow::{bail, Context as _, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use zepi_ui::office_page_indices;

const MAX_PIXELS: f64 = 6_000_000.0;
/// Renders older than this are removed on the next run, so the cache cannot grow forever.
const RENDER_TTL_SECS: u64 = 7 * 24 * 60 * 60;
const USAGE: &str =
    "usage: zepi office render <file> [--out DIR] [--dpi N] [--pages 1-3,5] [--cells B10:E40] | zepi office help";

/// Returns the arguments of an "zepi office ..." invocation, or None for the GUI.
pub fn requested() -> Option<Vec<String>> {
    let mut args = std::env::args_os()
        .skip(1)
        .map(|arg| arg.to_string_lossy().into_owned());
    (args.next().as_deref() == Some("office")).then(|| args.collect())
}

/// Runs the subcommand and returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    wordcraft_fonts::FontDb::global().load_system_fonts();
    let (command, rest) = match args.split_first() {
        Some((command, rest)) => (command.as_str(), rest),
        None => {
            print(&json!({"ok": false, "error": USAGE}));
            return 2;
        }
    };
    if matches!(command, "help" | "--help" | "-h") {
        print(&json!({"ok": true, "usage": USAGE}));
        return 0;
    }
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
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(json!({"usage": USAGE}));
    }
    let mut file: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut dpi = 150.0_f64;
    let mut pages = None;
    let mut cells: Option<zepi_ui::sheet_raster::Cells> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--out" | "-o" => out = Some(PathBuf::from(next(args, &mut index)?)),
            "--dpi" => {
                dpi = next(args, &mut index)?
                    .parse()
                    .context("--dpi expects a number, for example 150")?
            }
            "--pages" => pages = Some(next(args, &mut index)?),
            "--cells" => {
                let spec = next(args, &mut index)?;
                cells = Some(zepi_ui::sheet_raster::Cells::parse(&spec).with_context(|| {
                    format!("--cells expects a cell or a cell box, for example B10:E40, not {spec}")
                })?)
            }
            other if other.starts_with('-') => bail!("unknown option: {other}"),
            other => file = Some(PathBuf::from(other)),
        }
        index += 1;
    }
    let file = file.context("missing file to render")?;
    // like a script the caller writes: the working directory, unless --out names another one
    let out = out.unwrap_or_else(|| PathBuf::from("."));
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
        "docx" | "docm" | "doc" => render_docx(&bytes, &out, scale, pages.as_deref())?,
        "xlsx" | "xlsm" => render_xlsx(&bytes, &out, dpi, pages.as_deref(), cells.as_ref())?,
        other => bail!("unsupported format: {other}"),
    };
    if produced.is_empty() {
        bail!("no images were rendered");
    }
    Ok(json!({
        "file": file.display().to_string(),
        "out": out.display().to_string(),
        "pages": produced.len(),
        "files": produced,
    }))
}

/// A sheet's geometry is pixels at 96 dpi, so `--dpi` acts as a zoom over it.
fn render_xlsx(
    bytes: &[u8],
    out: &Path,
    dpi: f64,
    pages: Option<&str>,
    cells: Option<&zepi_ui::sheet_raster::Cells>,
) -> Result<Vec<String>> {
    let sheets = zepi_ui::sheet_raster::sheet_pngs(bytes, dpi / 96.0, pages, cells)?;
    let mut files = Vec::new();
    for (name, png) in sheets {
        std::fs::write(out.join(&name), png).with_context(|| format!("writing {name}"))?;
        files.push(name);
    }
    Ok(files)
}

fn next(args: &[String], index: &mut usize) -> Result<String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .with_context(|| format!("{} expects a value", args[*index - 1]))
}

fn render_pptx(
    bytes: &[u8],
    out: &Path,
    scale: f64,
    dpi: f64,
    pages: Option<&str>,
) -> Result<Vec<String>> {
    let (presentation, _warnings) =
        deckcraft_pptx::import_checked(bytes).context("reading the presentation")?;
    let mut files = Vec::new();
    for page in office_page_indices(pages, presentation.slides.len())? {
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
    pages: Option<&str>,
) -> Result<Vec<String>> {
    let document = if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        wordcraft_docbin::read(bytes).map_err(|error| anyhow::anyhow!("doc import: {error:?}"))?
    } else if zepi_ui::html_doc::looks_like_html(bytes) {
        zepi_ui::html_doc::read(bytes)?
    } else {
        wordcraft_docx::read(bytes).map_err(|error| anyhow::anyhow!("docx import: {error:?}"))?
    };
    let mut cache = wordcraft_layout::LayoutCache::new();
    let opts = wordcraft_layout::LayoutOptions::default();
    let layout = wordcraft_layout::layout(&document, &mut cache, &opts);
    let mut files = Vec::new();
    for page in office_page_indices(pages, layout.pages.len())? {
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

/// State root shared with the viewers: temp images live here.
pub fn state_root() -> PathBuf {
    match std::env::var_os("LOCALAPPDATA") {
        Some(dir) => PathBuf::from(dir).join("Zepi"),
        None => std::env::temp_dir().join("zepi"),
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
