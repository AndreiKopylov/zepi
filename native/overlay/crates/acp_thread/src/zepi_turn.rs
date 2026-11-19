use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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

/// A binary shell output (xlsx, images) applied to the project, pending native keep/reject.
/// Binary files have no editor buffer, so these reviews live outside the buffer diff.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZepiBinaryReview {
    pub path: String,
    /// "modified" or "created".
    pub kind: String,
    /// Absolute path to the pre-command copy inside the agent run directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup: Option<PathBuf>,
}

/// A text review replayed after a session reload: agent edits live in editor buffers,
/// so both sides of the diff travel from the agent's journal.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZepiTextReview {
    /// Workspace-relative path, the same shape the shell reports.
    pub path: String,
    pub base: String,
    pub current: String,
    /// Disk mtime in epoch nanoseconds, as text: a float would lose the precision Zed checks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_mtime_ns: Option<String>,
}

/// A text review the agent still considers pending; content travels only on replay.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZepiTextReviewEntry {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_mtime_ns: Option<String>,
}

impl ZepiTextReviewEntry {
    pub fn into_review(self) -> Option<ZepiTextReview> {
        Some(ZepiTextReview {
            path: self.path,
            base: self.base?,
            current: self.current?,
            disk_mtime_ns: self.disk_mtime_ns,
        })
    }
}

impl Default for ZepiTurnState {
    fn default() -> Self {
        Self {version: 1, id: None, status: ZepiTurnStatus::Idle}
    }
}
