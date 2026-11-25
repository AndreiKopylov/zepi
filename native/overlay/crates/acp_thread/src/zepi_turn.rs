use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ZepiTurnStatus {
    #[default]
    Idle,
    Running,
    Completed,
    Cancelled,
    Failed,
    Interrupted,
}

impl ZepiTurnStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Idle => "Ready",
            Self::Running => "Running",
            Self::Completed => "Run finished",
            Self::Cancelled => "Cancelled",
            Self::Failed => "Run failed",
            Self::Interrupted => "Outcome unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ZepiTurnState {
    pub version: u32,
    pub id: Option<String>,
    pub status: ZepiTurnStatus,
}

impl Default for ZepiTurnState {
    fn default() -> Self {
        Self {version: 1, id: None, status: ZepiTurnStatus::Idle}
    }
}

pub fn zepi_provider_error_message(reason: &str) -> Option<&'static str> {
    Some(match reason {
        "econnrefused" => "Could not reach the model provider: connection refused (ECONNREFUSED).",
        "econnreset" => "The model provider connection was reset (ECONNRESET).",
        "etimedout" => "The model provider connection timed out (ETIMEDOUT).",
        "enotfound" | "eai_again" => "Could not resolve the model provider's address.",
        "provider_unavailable" => "The model provider is temporarily unavailable.",
        "rate_limit" => "The model provider's rate limit was reached.",
        "billing_error" => "The model provider rejected the request due to billing or quota limits.",
        "auth_error" => "Model provider authentication failed. Check your API key or sign in again.",
        "provider_error" => "The model provider could not complete the request.",
        _ => return None,
    })
}
