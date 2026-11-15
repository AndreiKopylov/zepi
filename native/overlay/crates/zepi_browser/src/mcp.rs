// MCP server (stdio) exposing the direct browser tools; the pi agent decides actions.
use crate::driver;
use anyhow::Result;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

struct Session {
    _browser: chromiumoxide::Browser,
    page: chromiumoxide::Page,
    profile: std::path::PathBuf,
}

#[derive(Clone)]
pub struct BrowserServer {
    tool_router: ToolRouter<Self>,
    session: Arc<Mutex<Option<Session>>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct OpenArgs {
    /// URL to open in the task browser.
    url: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ActArgs {
    #[serde(default)]
    kind: String,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    key: Option<String>,
    /// Indexed element id from browser_snapshot; preferred over x/y.
    #[serde(default)]
    node: Option<i64>,
    /// Alias for node.
    #[serde(default)]
    index: Option<i64>,
    /// Human-readable element name/label; resolved live at action time (preferred over node).
    #[serde(default)]
    element: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct WaitArgs {
    /// Wait until this text appears on the page.
    #[serde(default)]
    text: Option<String>,
    /// Wait until this text disappears.
    #[serde(default)]
    text_gone: Option<String>,
    /// Wait a fixed number of seconds.
    #[serde(default)]
    time: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SelectOptionArgs {
    /// node id of the select/combobox field from the snapshot.
    #[serde(default)]
    node: Option<i64>,
    /// Visible text (or substring) of the option to select.
    #[serde(default)]
    value: Option<String>,
    /// Playwright-style values array (first entry used).
    #[serde(default)]
    values: Option<Vec<String>>,
    /// Field name/label; resolved live at action time (preferred over node).
    #[serde(default)]
    element: Option<String>,
}

fn ok_text(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![Content::text(v.to_string())]))
}

#[tool_router]
impl BrowserServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
            session: Arc::new(Mutex::new(None)),
        }
    }

    #[tool(description = "Navigate the task browser to a URL and return the observed page state.")]
    async fn browser_navigate(&self, Parameters(a): Parameters<OpenArgs>) -> Result<CallToolResult, McpError> {
        crate::log::log(&format!("tool browser_navigate url={}", a.url));
        let (browser, page, profile) = driver::launch().await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        driver::navigate(&page, &a.url).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let state = driver::eval(&page, driver::SNAPSHOT_JS).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let mut guard = self.session.lock().await;
        if let Some(old) = guard.take() {
            driver::kill_profile(&old.profile);
        }
        *guard = Some(Session { _browser: browser, page, profile });
        ok_text(state)
    }

    #[tool(description = "Re-observe the current page as an accessibility tree. Interactive elements carry an 'index' (use it as node in browser_click/browser_type/browser_select_option); also returns text and scroll state.")]
    async fn browser_snapshot(&self) -> Result<CallToolResult, McpError> {
        let guard = self.session.lock().await;
        let s = guard.as_ref().ok_or_else(|| McpError::internal_error("browser not open", None))?;
        let state = driver::eval(&s.page, driver::SNAPSHOT_JS).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        crate::log::log(&format!("snapshot url={} text_len={}", state["url"].as_str().unwrap_or("?"), state["text"].as_str().map(|t| t.len()).unwrap_or(0)));
        ok_text(state)
    }

    #[tool(description = "Capture the viewport to a JPEG file and return its path. The image is NOT sent to the model (text models reject it); open the file to view.")]
    async fn browser_take_screenshot(&self) -> Result<CallToolResult, McpError> {
        let guard = self.session.lock().await;
        let s = guard.as_ref().ok_or_else(|| McpError::internal_error("browser not open", None))?;
        let bytes = driver::screenshot(&s.page).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let dir = std::env::var("LOCALAPPDATA")
            .map(|l| std::path::PathBuf::from(l).join("Zepi").join("temp"))
            .unwrap_or_else(|_| std::env::temp_dir());
        std::fs::create_dir_all(&dir).map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let path = dir.join(format!("zepi-browser-{}.jpg", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()));
        std::fs::write(&path, bytes).map_err(|e| McpError::internal_error(e.to_string(), None))?;
        ok_text(json!({"path": path.to_string_lossy(), "format": "jpeg"}))
    }

    #[tool(description = "Select option(s) in a <select>, combobox or searchable/async dropdown (region, address, city). Like Playwright browser_select_option: opens the field, scrolls virtualized lists, clicks the matching option, falls back to keyboard (type-to-filter + ArrowDown + Enter) and verifies the value. Use this for ALL dropdowns instead of clicking options manually. RETURNS the updated state.")]
    async fn browser_select_option(&self, Parameters(a): Parameters<SelectOptionArgs>) -> Result<CallToolResult, McpError> {
        let want = a.value.or_else(|| a.values.and_then(|v| v.into_iter().next())).unwrap_or_default();
        let guard = self.session.lock().await;
        let s = guard.as_ref().ok_or_else(|| McpError::internal_error("browser not open", None))?;
        let n = driver::resolve_arg_node(&s.page, a.node, &a.element).await.map_err(|e| McpError::internal_error(e.to_string(), None))?
            .ok_or_else(|| McpError::internal_error("element not found", None))?;
        crate::log::log(&format!("tool browser_select_option node={} value={}", n, want));
        let state = driver::choose_option(&s.page, n, &want).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        ok_text(state)
    }

    #[tool(description = "Click an element by node (from browser_snapshot) or by x/y coordinates. RETURNS the updated state.")]
    async fn browser_click(&self, Parameters(a): Parameters<ActArgs>) -> Result<CallToolResult, McpError> {
        let guard = self.session.lock().await;
        let s = guard.as_ref().ok_or_else(|| McpError::internal_error("browser not open", None))?;
        let n = driver::resolve_arg_node(&s.page, a.node.or(a.index), &a.element).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let state = if let Some(n) = n {
            driver::click_or_select(&s.page, n).await.map_err(|e| McpError::internal_error(e.to_string(), None))?
        } else {
            driver::click(&s.page, a.x.unwrap_or(0.0), a.y.unwrap_or(0.0)).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
            driver::eval(&s.page, driver::SNAPSHOT_JS).await.map_err(|e| McpError::internal_error(e.to_string(), None))?
        };
        ok_text(state)
    }

    #[tool(description = "Type text into an input/textarea/combobox by node. For searchable dropdowns this filters the options. RETURNS the updated state.")]
    async fn browser_type(&self, Parameters(a): Parameters<ActArgs>) -> Result<CallToolResult, McpError> {
        let guard = self.session.lock().await;
        let s = guard.as_ref().ok_or_else(|| McpError::internal_error("browser not open", None))?;
        let n = driver::resolve_arg_node(&s.page, a.node.or(a.index), &a.element).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        driver::dispatch_act(&s.page, "type", n, 0.0, 0.0, &a.text.clone().unwrap_or_default(), 0.0, "", "").await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let state = driver::eval(&s.page, driver::SNAPSHOT_JS).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        ok_text(state)
    }

    #[tool(description = "Press a keyboard key (Enter, Tab, Escape, ArrowDown, ArrowUp, Backspace, Delete, Home, End). RETURNS the updated state.")]
    async fn browser_press_key(&self, Parameters(a): Parameters<ActArgs>) -> Result<CallToolResult, McpError> {
        let guard = self.session.lock().await;
        let s = guard.as_ref().ok_or_else(|| McpError::internal_error("browser not open", None))?;
        driver::dispatch_act(&s.page, "press_key", None, 0.0, 0.0, "", 0.0, &a.key.clone().unwrap_or_default(), "").await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let state = driver::eval(&s.page, driver::SNAPSHOT_JS).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        ok_text(state)
    }

    #[tool(description = "Wait for text to appear (text) or disappear (text_gone), or a fixed time in seconds. Use after actions that load data asynchronously.")]
    async fn browser_wait_for(&self, Parameters(a): Parameters<WaitArgs>) -> Result<CallToolResult, McpError> {
        let guard = self.session.lock().await;
        let s = guard.as_ref().ok_or_else(|| McpError::internal_error("browser not open", None))?;
        if let Some(t) = a.time {
            tokio::time::sleep(std::time::Duration::from_secs_f64(t.min(30.0))).await;
        } else {
            for _ in 0..40 {
                let state = driver::eval(&s.page, driver::SNAPSHOT_JS).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
                let txt = state["text"].as_str().unwrap_or("");
                let ok = match (&a.text, &a.text_gone) {
                    (Some(w), _) => txt.contains(w.as_str()),
                    (_, Some(w)) => !txt.contains(w.as_str()),
                    _ => true,
                };
                if ok { break; }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        }
        let state = driver::eval(&s.page, driver::SNAPSHOT_JS).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        ok_text(state)
    }


    #[tool(description = "Close the task browser session.")]
    async fn browser_close(&self) -> Result<CallToolResult, McpError> {
        if let Some(s) = self.session.lock().await.take() {
            driver::kill_profile(&s.profile);
        }
        ok_text(json!({"closed": true}))
    }
}

#[tool_handler]
impl ServerHandler for BrowserServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

pub async fn serve() -> Result<()> {
    let service = BrowserServer::new().serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}