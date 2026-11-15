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
    /// click | type | scroll | press_key
    kind: String,
    #[serde(default)]
    x: Option<f64>,
    #[serde(default)]
    y: Option<f64>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    delta: Option<f64>,
    #[serde(default)]
    key: Option<String>,
    /// Indexed element id from browser_snapshot; preferred over x/y.
    #[serde(default)]
    node: Option<i64>,
    /// Option value for kind=select.
    #[serde(default)]
    value: Option<String>,
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

    #[tool(description = "Open a URL in the task browser and return the observed page state.")]
    async fn browser_open(&self, Parameters(a): Parameters<OpenArgs>) -> Result<CallToolResult, McpError> {
        let (browser, page) = driver::launch().await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        driver::navigate(&page, &a.url).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        let state = driver::eval(&page, driver::SNAPSHOT_JS).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        *self.session.lock().await = Some(Session { _browser: browser, page });
        ok_text(state)
    }

    #[tool(description = "Re-observe the current page: indexed elements, text and scroll state.")]
    async fn browser_snapshot(&self) -> Result<CallToolResult, McpError> {
        let guard = self.session.lock().await;
        let s = guard.as_ref().ok_or_else(|| McpError::internal_error("browser not open", None))?;
        let state = driver::eval(&s.page, driver::SNAPSHOT_JS).await.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        ok_text(state)
    }

    #[tool(description = "Capture the viewport to a JPEG file and return its path. The image is NOT sent to the model (text models reject it); open the file to view.")]
    async fn browser_screenshot(&self) -> Result<CallToolResult, McpError> {
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

    #[tool(description = "Direct action. Prefer node (indexed element from browser_snapshot): click/fill(type)/select. Without node: click(x,y), type(text), scroll(delta). press_key: Enter/Tab/Escape/Backspace/Delete/arrows/Home/End.")]
    async fn browser_act(&self, Parameters(a): Parameters<ActArgs>) -> Result<CallToolResult, McpError> {
        let guard = self.session.lock().await;
        let s = guard.as_ref().ok_or_else(|| McpError::internal_error("browser not open", None))?;
        let r = if let Some(node) = a.node {
            let kind = if a.kind == "type" { "fill" } else { a.kind.as_str() };
            driver::act_node(&s.page, kind, node, &a.value.unwrap_or_default(), &a.text.unwrap_or_default()).await
        } else {
            match a.kind.as_str() {
                "click" => driver::click(&s.page, a.x.unwrap_or(0.0), a.y.unwrap_or(0.0)).await,
                "type" => driver::type_text(&s.page, &a.text.unwrap_or_default()).await,
                "scroll" => driver::scroll(&s.page, a.delta.unwrap_or(300.0)).await,
                "press_key" => driver::press_key(&s.page, &a.key.unwrap_or_default()).await,
                other => Err(anyhow::anyhow!("unknown action {other}")),
            }
        };
        r.map_err(|e| McpError::internal_error(e.to_string(), None))?;
        ok_text(json!({"executed": a.kind}))
    }

    #[tool(description = "Check browser availability and whether a session is open.")]
    async fn browser_status(&self) -> Result<CallToolResult, McpError> {
        let open = self.session.lock().await.is_some();
        ok_text(json!({"available": driver::chrome_path().is_some(), "open": open}))
    }

    #[tool(description = "Close the task browser session.")]
    async fn browser_close(&self) -> Result<CallToolResult, McpError> {
        *self.session.lock().await = None;
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