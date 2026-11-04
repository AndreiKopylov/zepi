//! File bytes for command inputs: preview and shell capture read the exact disk contents and,
//! for text files, the editor buffer on top of them.

use crate::AcpThread;
use anyhow::{Context as _, Result, ensure};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use gpui::{AppContext as _, Context, Task};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Command input exceeds this and the shell refuses to capture it.
const MAX_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ZepiFileSnapshot {
    /// What the command reads: the buffer for text files, the disk bytes otherwise (base64).
    pub content: String,
    /// Raw disk bytes (base64), absent when the file does not exist.
    pub disk: Option<String>,
    /// Editor buffer text, absent for binary files.
    pub buffer: Option<String>,
}

impl AcpThread {
    pub fn observe_file_bytes(&self, path: PathBuf, cx: &mut Context<Self>) -> Task<Result<ZepiFileSnapshot>> {
        cx.spawn(async move |this, cx| {
            let disk_path = path.clone();
            let disk = cx
                .background_executor()
                .spawn(async move { std::fs::read(&disk_path).ok() })
                .await;
            let binary = disk.as_ref().is_some_and(|bytes| bytes.contains(&0) || std::str::from_utf8(bytes).is_err());
            let buffer = if binary {
                None
            } else {
                Some(
                    this.update(cx, |thread, cx| thread.observe_text_file(path.clone(), cx))?
                        .await
                        .map_err(|error| anyhow::anyhow!("{error:?}"))?,
                )
            };
            let bytes = buffer.as_ref().map(String::as_bytes).or(disk.as_deref()).context("File is unavailable")?;
            ensure!(bytes.len() <= MAX_BYTES, "Command input exceeds 1 MiB");
            Ok(ZepiFileSnapshot {
                content: STANDARD.encode(bytes),
                disk: disk.as_deref().map(|bytes| STANDARD.encode(bytes)),
                buffer,
            })
        })
    }
}
