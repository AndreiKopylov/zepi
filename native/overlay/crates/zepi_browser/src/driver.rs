// CDP driver on chromiumoxide; replaces the Playwright driver layer.
use anyhow::{Context, Result};
use chromiumoxide::cdp::browser_protocol::{browser as cdp_browser, input, page as cdp_page};
use chromiumoxide::cdp::js_protocol::runtime;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use serde_json::Value;

pub const SNAPSHOT_JS: &str = include_str!("snapshot.js");

pub fn chrome_path() -> Option<std::path::PathBuf> {
    if let Ok(p) = std::env::var("ZEPI_BROWSER_PATH") {
        let p = std::path::PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    let candidates = [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
    ];
    candidates.iter().map(std::path::PathBuf::from).find(|p| p.exists())
}

fn pi_settings() -> Value {
    let dir = std::env::var("ZEPI_PI_CONFIG_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var("LOCALAPPDATA").ok().map(|l| std::path::PathBuf::from(l).join("Zepi").join("pi")));
    if let Some(d) = dir {
        if let Ok(s) = std::fs::read_to_string(d.join("settings.json")) {
            return serde_json::from_str(&s).unwrap_or(Value::Null);
        }
    }
    Value::Null
}

pub async fn launch() -> Result<(Browser, Page)> {
    // A dedicated profile dir: with the default profile a running Chrome hijacks the
    // launch and the new process exits before the debug port opens.
    let profile = std::env::temp_dir().join(format!(
        "zepi-browser-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&profile).context("create profile dir")?;
    let settings = pi_settings();
    let b = &settings["zepi"]["browser"];
    let window = std::env::var("ZEPI_BROWSER_WINDOW")
        .ok()
        .filter(|w| ["normal", "maximized", "fullscreen"].contains(&w.as_str()))
        .or_else(|| b["window"].as_str().map(String::from))
        .unwrap_or_else(|| "fullscreen".into());
    let headless = std::env::var("ZEPI_BROWSER_HEADLESS").is_ok();
    // viewport(None): page follows the real window size; the default 800x600
    // emulation squeezed the layout to one side on a maximized/fullscreen window.
    let mut builder = BrowserConfig::builder()
        .no_sandbox()
        .user_data_dir(profile)
        .viewport(None);
    // --window-size conflicts with --start-maximized; only size the normal window.
    if window == "normal" {
        builder = builder.window_size(1120, 780);
    }
    builder = if headless { builder.new_headless_mode() } else { builder.with_head() };
    builder = match window.as_str() {
        "maximized" => builder.arg("--start-maximized"),
        "fullscreen" => builder.arg("--start-fullscreen"),
        _ => builder,
    };
    let exe = b["executablePath"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .filter(|p| p.exists())
        .or_else(chrome_path);
    if let Some(p) = exe {
        builder = builder.chrome_executable(p);
    }
    let config = builder.build().map_err(|e| anyhow::anyhow!(e))?;
    let (browser, mut handler) = Browser::launch(config).await.context("launch chrome")?;
    tokio::spawn(async move { while handler.next().await.is_some() {} });
    apply_window_state(&browser, &window).await;
    let page = browser.new_page("about:blank").await.context("new page")?;
    Ok((browser, page))
}

// Set the state before the first page paints so the window opens already sized.
async fn apply_window_state(browser: &Browser, mode: &str) {
    let state = match mode {
        "maximized" => cdp_browser::WindowState::Maximized,
        "fullscreen" => cdp_browser::WindowState::Fullscreen,
        _ => cdp_browser::WindowState::Normal,
    };
    let mut bounds = cdp_browser::Bounds::builder().window_state(state);
    if mode == "normal" {
        bounds = bounds.width(1120).height(780);
    }
    let Ok(win) = browser
        .execute(cdp_browser::GetWindowForTargetParams::builder().build())
        .await
    else { return };
    let _ = browser
        .execute(cdp_browser::SetWindowBoundsParams::builder().window_id(win.result.window_id).bounds(bounds.build()).build().unwrap())
        .await;
}

pub async fn eval(page: &Page, expression: &str) -> Result<Value> {
    let resp = page
        .execute(
            runtime::EvaluateParams::builder()
                .expression(expression)
                .return_by_value(true)
                .build()
                .unwrap(),
        )
        .await
        .context("Runtime.evaluate")?;
    if let Some(exc) = resp.result.exception_details {
        anyhow::bail!("page exception: {}", exc.text);
    }
    Ok(resp.result.result.value.unwrap_or(Value::Null))
}

pub async fn navigate(page: &Page, url: &str) -> Result<()> {
    page.goto(url).await.context("navigate")?;
    for _ in 0..100 {
        if eval(page, "document.readyState").await? == Value::String("complete".into()) {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    Ok(())
}

pub async fn click(page: &Page, x: f64, y: f64) -> Result<()> {
    for ty in [input::DispatchMouseEventType::MousePressed, input::DispatchMouseEventType::MouseReleased] {
        page.execute(
            input::DispatchMouseEventParams::builder()
                .r#type(ty)
                .x(x)
                .y(y)
                .button(input::MouseButton::Left)
                .click_count(1)
                .build()
                .unwrap(),
        )
        .await
        .context("dispatch mouse")?;
    }
    Ok(())
}

pub async fn scroll(page: &Page, delta_y: f64) -> Result<()> {
    page.execute(
        input::DispatchMouseEventParams::builder()
            .r#type(input::DispatchMouseEventType::MouseWheel)
            .x(550.0)
            .y(400.0)
            .delta_x(0.0)
            .delta_y(delta_y)
            .build()
            .unwrap(),
    )
    .await
    .context("dispatch wheel")?;
    Ok(())
}

pub async fn type_text(page: &Page, text: &str) -> Result<()> {
    page.execute(
        input::InsertTextParams::builder().text(text).build().unwrap(),
    )
    .await
    .context("insert text")?;
    Ok(())
}

pub async fn eval_promise(page: &Page, expression: &str) -> Result<Value> {
    let resp = page
        .execute(
            runtime::EvaluateParams::builder()
                .expression(expression)
                .return_by_value(true)
                .await_promise(true)
                .build()
                .unwrap(),
        )
        .await
        .context("Runtime.evaluate")?;
    if let Some(exc) = resp.result.exception_details {
        anyhow::bail!("page exception: {}", exc.text);
    }
    Ok(resp.result.result.value.unwrap_or(Value::Null))
}

// Resolve an indexed snapshot node to a visible, unoccluded click point.
pub async fn resolve_target(page: &Page, action: &Value) -> Result<Option<(f64, f64)>> {
    let v = eval(page, &format!("{}{})", crate::scripts::TARGET, action)).await?;
    Ok(match (v["x"].as_f64(), v["y"].as_f64()) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    })
}

pub async fn act_node(page: &Page, kind: &str, node: i64, value: &str, text: &str) -> Result<()> {
    let action = serde_json::json!({"kind": kind, "node": node, "value": value});
    let Some((x, y)) = resolve_target(page, &action).await? else {
        anyhow::bail!("target changed or is covered; re-observe");
    };
    match kind {
        "select" => {}
        "click" => click(page, x, y).await?,
        "fill" => {
            click(page, x, y).await?;
            for ty in [input::DispatchKeyEventType::KeyDown, input::DispatchKeyEventType::KeyUp] {
                page.execute(
                    input::DispatchKeyEventParams::builder()
                        .r#type(ty)
                        .key("a")
                        .code("KeyA")
                        .modifiers(2)
                        .windows_virtual_key_code(65)
                        .build()
                        .unwrap(),
                )
                .await
                .context("select all")?;
            }
            type_text(page, text).await?;
        }
        other => anyhow::bail!("unknown node action {other}"),
    }
    let _ = eval_promise(page, &format!("{}{})", crate::scripts::AFTER_INPUT, action)).await;
    Ok(())
}

pub async fn press_key(page: &Page, key: &str) -> Result<()> {
    let (k, code, vk, text) = match key {
        "Enter" => ("Enter", "Enter", 13, "\r"),
        "Tab" => ("Tab", "Tab", 9, "\t"),
        "Escape" => ("Escape", "Escape", 27, ""),
        "Backspace" => ("Backspace", "Backspace", 8, ""),
        "Delete" => ("Delete", "Delete", 46, ""),
        "ArrowUp" => ("ArrowUp", "ArrowUp", 38, ""),
        "ArrowDown" => ("ArrowDown", "ArrowDown", 40, ""),
        "ArrowLeft" => ("ArrowLeft", "ArrowLeft", 37, ""),
        "ArrowRight" => ("ArrowRight", "ArrowRight", 39, ""),
        "Home" => ("Home", "Home", 36, ""),
        "End" => ("End", "End", 35, ""),
        other => anyhow::bail!("unsupported key {other}"),
    };
    for (ty, t) in [(input::DispatchKeyEventType::KeyDown, text), (input::DispatchKeyEventType::KeyUp, "")] {
        page.execute(
            input::DispatchKeyEventParams::builder()
                .r#type(ty)
                .key(k)
                .code(code)
                .windows_virtual_key_code(vk)
                .text(t)
                .build()
                .unwrap(),
        )
        .await
        .context("dispatch key")?;
    }
    Ok(())
}

pub async fn screenshot(page: &Page) -> Result<Vec<u8>> {
    let resp = page
        .execute(
            cdp_page::CaptureScreenshotParams::builder()
                .format(cdp_page::CaptureScreenshotFormat::Jpeg)
                .quality(72)
                .build(),
        )
        .await
        .context("screenshot")?;
    // Binary holds the base64 string; decode to real JPEG bytes.
    let b64 = AsRef::<[u8]>::as_ref(&resp.data);
    base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).context("decode screenshot")
}