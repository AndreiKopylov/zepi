// CDP driver on chromiumoxide; replaces the Playwright driver layer.
use anyhow::{Context, Result};
use chromiumoxide::cdp::browser_protocol::{browser as cdp_browser, input, page as cdp_page};
use chromiumoxide::cdp::js_protocol::runtime;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use serde_json::Value;

pub const SNAPSHOT_JS: &str = include_str!("dom.js");

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

pub async fn launch() -> Result<(Browser, Page, std::path::PathBuf)> {
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
        .user_data_dir(profile.clone())
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
    Ok((browser, page, profile))
}

// Kill only this session's Chrome (matched by its unique profile dir), never the user's browser.
pub fn kill_profile(profile: &std::path::Path) {
    let Some(marker) = profile.file_name().map(|m| m.to_string_lossy().into_owned()) else { return };
    #[cfg(windows)]
    {
        let cmd = format!(
            "Get-CimInstance Win32_Process | Where-Object {{ $_.CommandLine -like '*{}*' }} | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}",
            marker
        );
        let _ = std::process::Command::new("powershell").args(["-NoProfile", "-Command", &cmd]).output();
    }
    #[cfg(not(windows))]
    {
        let _ = std::process::Command::new("pkill").arg("-f").arg(&marker).output();
    }
    let _ = std::fs::remove_dir_all(profile);
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
        let desc = exc.exception.as_ref().and_then(|e| e.description.clone()).unwrap_or_default();
        anyhow::bail!("page exception: {} {}", exc.text, desc);
    }
    Ok(resp.result.result.value.unwrap_or(Value::Null))
}

pub async fn navigate(page: &Page, url: &str) -> Result<()> {
    page.goto(url).await.context("navigate")?;
    for _ in 0..100 {
        if eval(page, "document.readyState").await? == Value::String("complete".into()) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    // SPA settle: let the framework mount and fire initial requests before the first snapshot.
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    Ok(())
}

// Shared single-action dispatch (used by the standard interaction tools).
pub async fn dispatch_act(page: &Page, kind: &str, node: Option<i64>, x: f64, y: f64, text: &str, delta: f64, key: &str, value: &str) -> Result<()> {
    if let Some(n) = node {
        let k = if kind == "type" { "fill" } else { kind };
        return act_node(page, k, n, value, text).await;
    }
    match kind {
        "click" => click(page, x, y).await,
        "type" => type_text(page, text).await,
        "scroll" => scroll(page, x, y, delta).await,
        "press_key" => press_key(page, key).await,
        other => anyhow::bail!("unknown action {other}"),
    }
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

pub async fn scroll(page: &Page, x: f64, y: f64, delta_y: f64) -> Result<()> {
    page.execute(
        input::DispatchMouseEventParams::builder()
            .r#type(input::DispatchMouseEventType::MouseWheel)
            .x(x)
            .y(y)
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
        let desc = exc.exception.as_ref().and_then(|e| e.description.clone()).unwrap_or_default();
        anyhow::bail!("page exception: {} {}", exc.text, desc);
    }
    Ok(resp.result.result.value.unwrap_or(Value::Null))
}

// Resolve an indexed snapshot node to a visible, unoccluded click point.
pub async fn resolve_target(page: &Page, action: &Value) -> Result<Option<(f64, f64)>> {
    let v = eval(page, &format!("{}{})", crate::scripts::TARGET, action)).await?;
    Ok(match (v["x"].as_f64(), v["y"].as_f64()) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => {
            crate::log::log(&format!("target node={} kind={} why={}", action["node"], action["kind"], v["why"].as_str().unwrap_or("?")));
            None
        }
    })
}

pub async fn act_node(page: &Page, kind: &str, node: i64, value: &str, text: &str) -> Result<()> {
    crate::log::log(&format!("act kind={kind} node={node} value={value:?} text={text:?}"));
    let action = serde_json::json!({"kind": kind, "node": node, "value": value});
    if kind == "select" {
        let js = format!(
            "(() => {{ const e = (window.__zepiResolve ? window.__zepiResolve({node}) : window.__zepiBrowser?.nodes.get({node})); \
             if (!e || e.tagName !== 'SELECT') return false; const want = {want}; \
             const opt = [...e.options].find(o => o.value === want) || \
               [...e.options].find(o => o.label === want || o.textContent.trim() === want); \
             if (!opt) return false; e.value = opt.value; \
             e.dispatchEvent(new Event('input', {{bubbles:true}})); \
             e.dispatchEvent(new Event('change', {{bubbles:true}})); return true; }})()",
            node = node,
            want = serde_json::to_string(value)?
        );
        let ok = eval(page, &js).await?;
        crate::log::log(&format!("select node={node} ok={ok}"));
        anyhow::ensure!(ok.as_bool().unwrap_or(false), "select option not found; re-observe");
        return Ok(());
    }
    // Actionability auto-wait: the page may still be settling (SPA re-render); retry briefly.
    let mut target = resolve_target(page, &action).await?;
    for _ in 0..3 {
        if target.is_some() { break; }
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        target = resolve_target(page, &action).await?;
    }
    match kind {
        "click" => {
            if let Some((x, y)) = target {
                click(page, x, y).await?;
            } else {
                // Occluded/offscreen (e.g. popup option): fall back to a DOM click.
                let js = format!(
                    "(() => {{ let e = (window.__zepiResolve ? window.__zepiResolve({node}) : window.__zepiBrowser?.nodes.get({node})); \
                     if (!e) return false; e.scrollIntoView({{block:'center'}}); \
                     e = (window.__zepiResolve ? window.__zepiResolve({node}) : e) || e; \
                     if (!e.isConnected) return false; e.click(); return true; }})()",
                    node = node
                );
                let mut ok = eval(page, &js).await?.as_bool().unwrap_or(false);
                // Virtualized menus render only visible options; scroll the list to reveal the target.
                for _ in 0..12 {
                    if ok { break; }
                    let s = eval(page, SNAPSHOT_JS).await?;
                    let r = s["elements"].as_array()
                        .and_then(|a| a.iter().find(|e| e["role"].as_str() == Some("listbox") || e["role"].as_str() == Some("option")))
                        .and_then(|e| e["rect"].as_object().cloned());
                    let (x, y) = match &r {
                        Some(r) => (r["x"].as_f64().unwrap_or(550.0) + r["w"].as_f64().unwrap_or(0.0) / 2.0,
                                    r["y"].as_f64().unwrap_or(400.0) + r["h"].as_f64().unwrap_or(0.0) / 2.0),
                        None => (550.0, 400.0),
                    };
                    let _ = scroll(page, x, y, 300.0).await;
                    ok = eval(page, &js).await?.as_bool().unwrap_or(false);
                }
                crate::log::log(&format!("click dom-fallback node={node} ok={ok}"));
                anyhow::ensure!(ok, "target changed or is covered; re-observe");
            }
        }
        "fill" => {
            let Some((x, y)) = target else {
                // DOM fill fallback (iframe / covered fields): set value + events.
                let js = format!(
                    "(() => {{ const e = window.__zepiResolve ? window.__zepiResolve({node}) : null; \
                     if (!e || !('value' in e)) return false; e.value = {want}; \
                     e.dispatchEvent(new Event('input', {{bubbles:true}})); \
                     e.dispatchEvent(new Event('change', {{bubbles:true}})); return true; }})()",
                    node = node,
                    want = serde_json::to_string(text)?
                );
                anyhow::ensure!(eval(page, &js).await?.as_bool().unwrap_or(false), "target changed or is covered; re-observe");
                let _ = eval_promise(page, &format!("{}{})", crate::scripts::AFTER_INPUT, action)).await;
                return Ok(());
            };
            // Date-like inputs don't accept keystrokes; set the value and fire events.
            let js = format!(
                "(() => {{ const e = window.__zepiBrowser?.nodes.get({node}); if (!e) return false; \
                 if (!['date','time','datetime-local','month','week'].includes(e.type)) return false; \
                 e.value = {want}; e.dispatchEvent(new Event('input', {{bubbles:true}})); \
                 e.dispatchEvent(new Event('change', {{bubbles:true}})); return true; }})()",
                node = node,
                want = serde_json::to_string(text)?
            );
            if eval(page, &js).await?.as_bool().unwrap_or(false) {
                crate::log::log(&format!("fill date-set node={node}"));
                let _ = eval_promise(page, &format!("{}{})", crate::scripts::AFTER_INPUT, action)).await;
                return Ok(());
            }
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

pub async fn resolve_arg_node(page: &Page, node: Option<i64>, element: &Option<String>) -> Result<Option<i64>> {
    if let Some(n) = node { return Ok(Some(n)); }
    if let Some(el) = element {
        let v = eval(page, &format!(
            "(window.__zepiFind ? window.__zepiFind({n}, '') : null)",
            n = serde_json::to_string(el)?
        )).await?;
        return Ok(v.as_i64());
    }
    Ok(None)
}

async fn field_display(page: &Page, node: i64) -> String {
    eval(page, &format!(
        "(() => {{ const e = window.__zepiResolve ? window.__zepiResolve({node}) : null; \
         if (!e) return ''; return e.value !== undefined ? String(e.value) : (e.textContent || '').trim(); }})()",
        node = node
    )).await.ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

fn find_option(s: &Value, want: &str) -> Option<i64> {
    s["elements"].as_array()?.iter()
        .find(|e| e["role"].as_str() == Some("option") && e["name"].as_str().map(|n| n.contains(want)).unwrap_or(false))
        .and_then(|e| e["index"].as_i64())
}

// Universal dropdown setter: open, click the matching option; on failure fall back to
// keyboard (type-to-filter + ArrowDown + Enter); verify the field value; retry once.
pub async fn choose_option(page: &Page, node: i64, want: &str) -> Result<Value> {
    crate::log::log(&format!("choose node={node} want={want:?}"));
    // Native <select>: set value + events directly (Playwright select_option semantics).
    let native = format!(
        "(() => {{ const e = window.__zepiResolve ? window.__zepiResolve({node}) : null; \
         if (!e || e.tagName !== 'SELECT') return false; const want = {want}; \
         const opt = [...e.options].find(o => o.value === want) || \
           [...e.options].find(o => o.label === want || o.textContent.trim() === want); \
         if (!opt) return false; e.value = opt.value; \
         e.dispatchEvent(new Event('input', {{bubbles:true}})); \
         e.dispatchEvent(new Event('change', {{bubbles:true}})); return true; }})()",
        node = node,
        want = serde_json::to_string(want)?
    );
    if eval(page, &native).await?.as_bool().unwrap_or(false) {
        let s2 = eval(page, SNAPSHOT_JS).await?;
        crate::log::log(&format!("choose node={node} ok via native select"));
        return Ok(s2);
    }
    let _ = act_node(page, "click", node, "", "").await;
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    for attempt in 0..2 {
        let mut s = eval(page, SNAPSHOT_JS).await?;
        let mut opt = find_option(&s, want);
        if opt.is_none() {
            // Some menus (MUI) open on keyboard rather than the synthetic click.
            let _ = press_key(page, "Enter").await;
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            s = eval(page, SNAPSHOT_JS).await?;
            opt = find_option(&s, want);
        }
        // Virtual-scroll menus render only visible options; scroll the list to reveal the target.
        for _ in 0..12 {
            if opt.is_some() { break; }
            let r = s["elements"].as_array()
                .and_then(|a| a.iter().find(|e| e["role"].as_str() == Some("listbox") || e["role"].as_str() == Some("option")))
                .and_then(|e| e["rect"].as_object().cloned());
            let (x, y) = match &r {
                Some(r) => (r["x"].as_f64().unwrap_or(550.0) + r["w"].as_f64().unwrap_or(0.0) / 2.0,
                            r["y"].as_f64().unwrap_or(400.0) + r["h"].as_f64().unwrap_or(0.0) / 2.0),
                None => (550.0, 400.0),
            };
            let _ = scroll(page, x, y, 300.0).await;
            s = eval(page, SNAPSHOT_JS).await?;
            opt = find_option(&s, want);
        }
        if let Some(opt) = opt {
            if act_node(page, "click", opt, "", "").await.is_ok() {
                let s2 = eval(page, SNAPSHOT_JS).await?;
                if field_display(page, node).await.contains(want) {
                    crate::log::log(&format!("choose node={node} ok via click"));
                    return Ok(s2);
                }
            }
        }
        crate::log::log(&format!("choose node={node} attempt={attempt} fallback keyboard"));
        // Searchable dropdown: type to filter, wait for async options, click the match.
        let _ = act_node(page, "fill", node, "", want).await;
        let mut s2 = eval(page, SNAPSHOT_JS).await?;
        let mut o = find_option(&s2, want);
        for _ in 0..8 {
            if o.is_some() { break; }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            s2 = eval(page, SNAPSHOT_JS).await?;
            o = find_option(&s2, want);
        }
        if let Some(o) = o {
            if act_node(page, "click", o, "", "").await.is_ok() {
                let s3 = eval(page, SNAPSHOT_JS).await?;
                if field_display(page, node).await.contains(want) {
                    crate::log::log(&format!("choose node={node} ok via search+click"));
                    return Ok(s3);
                }
            }
        }
        let _ = press_key(page, "ArrowDown").await;
        let _ = press_key(page, "Enter").await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let s3 = eval(page, SNAPSHOT_JS).await?;
        if field_display(page, node).await.contains(want) {
            crate::log::log(&format!("choose node={node} ok via keyboard"));
            return Ok(s3);
        }
    }
    anyhow::bail!("could not select {want:?}; re-observe")
}

// Clicking an option is semantically selecting it: route through the robust select flow
// (handles searchable/async/virtualized menus) instead of a raw coordinate click.
pub async fn click_or_select(page: &Page, node: i64) -> Result<Value> {
    let meta = eval(page, &format!(
        "(() => {{ const m = window.__zepiBrowser && window.__zepiBrowser.meta && window.__zepiBrowser.meta.get({node}); return m ? {{role:m.role, text:m.text}} : null; }})()",
        node = node
    )).await?;
    if meta["role"].as_str() == Some("option") {
        // A currently-visible option in an open menu: click it directly.
        let action = serde_json::json!({"kind": "click", "node": node});
        if resolve_target(page, &action).await?.is_some() {
            act_node(page, "click", node, "", "").await?;
            return eval(page, SNAPSHOT_JS).await;
        }
        // Stale/unrendered option: route through the robust select flow on the owning field.
        let s = eval(page, SNAPSHOT_JS).await?;
        let els = s["elements"].as_array();
        let field = els.and_then(|a| a.iter().find(|e| e["role"].as_str() == Some("combobox") && e["expanded"].as_str() == Some("true")))
            .or_else(|| els.and_then(|a| a.iter().find(|e| e["role"].as_str() == Some("combobox"))))
            .and_then(|e| e["index"].as_i64());
        if let Some(f) = field {
            let text = meta["text"].as_str().unwrap_or("");
            return choose_option(page, f, text).await;
        }
    }
    act_node(page, "click", node, "", "").await?;
    eval(page, SNAPSHOT_JS).await
}