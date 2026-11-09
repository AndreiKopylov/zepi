use acp_thread::{ZepiTurnState, ZepiTurnStatus};
use gpui::App;
use serde::{Deserialize, Serialize};

const NAMESPACE: &str = "zepi-task-state-v1";

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Record {
    pub turn: ZepiTurnState,
    pub unread: bool,
    #[serde(default)]
    pub pending_review: usize,
    #[serde(default)]
    pub tokens: Option<u64>,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub usage: Option<acp_thread::TokenUsage>,
}

pub fn read(session: &str, cx: &App) -> Record {
    crate::zepi_session_store::read(NAMESPACE, session, cx).ok().flatten()
        .and_then(|payload| serde_json::from_str(&payload).ok()).unwrap_or_default()
}

impl Record {
    fn observe(&mut self, turn: ZepiTurnState, visible: bool, pending_review: usize) {
        // Replayed messages and restoring buffers can notify before ACP sends
        // the saved outcome. The native placeholder is not a new turn.
        if turn == ZepiTurnState::default() && self.turn != ZepiTurnState::default() {
            return;
        }
        if self.turn != turn && !matches!(turn.status, ZepiTurnStatus::Idle | ZepiTurnStatus::Running) {
            self.unread = true;
        }
        self.turn = turn;
        self.pending_review = pending_review;
        if visible { self.unread = false; }
    }

    pub fn restored_status(&self) -> ZepiTurnStatus {
        if self.turn.status == ZepiTurnStatus::Running { ZepiTurnStatus::Interrupted } else { self.turn.status }
    }
}

/// Acknowledging a visible conversation must not replace a saved outcome with
/// the default state of a thread whose history is still loading.
pub fn mark_read(session: &str, cx: &mut App) {
    let mut record = read(session, cx);
    if record.unread {
        record.unread = false;
        if let Ok(payload) = serde_json::to_string(&record) {
            let _ = crate::zepi_session_store::write(NAMESPACE, session.into(), payload, cx);
        }
    }
}

/// Token and duration labels are display state, so a restored thread reloads them.
pub fn remember_turn(session: &str, tokens: Option<u64>, duration: Option<std::time::Duration>, cx: &mut App) {
    let mut record = read(session, cx);
    let duration_ms = duration.map(|value| value.as_millis().min(u64::MAX as u128) as u64);
    if record.tokens == tokens && record.duration_ms == duration_ms { return; }
    record.tokens = tokens;
    record.duration_ms = duration_ms;
    if let Ok(payload) = serde_json::to_string(&record) {
        let _ = crate::zepi_session_store::write(NAMESPACE, session.into(), payload, cx);
    }
}

pub fn remember_usage(session: &str, usage: Option<acp_thread::TokenUsage>, cx: &mut App) {
    let Some(usage) = usage else { return };
    let mut record = read(session, cx);
    if record.usage.as_ref() == Some(&usage) { return; }
    record.usage = Some(usage);
    if let Ok(payload) = serde_json::to_string(&record) {
        let _ = crate::zepi_session_store::write(NAMESPACE, session.into(), payload, cx);
    }
}

pub fn observe(session: &str, turn: ZepiTurnState, visible: bool, pending_review: usize, cx: &mut App) {
    let previous = read(session, cx);
    let mut next = previous.clone();
    next.observe(turn, visible, pending_review);
    if next != previous {
        if let Ok(payload) = serde_json::to_string(&next) {
            let _ = crate::zepi_session_store::write(NAMESPACE, session.into(), payload, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[gpui::test]
    async fn test_zepi_task_reloads_unread_result_and_pending_review(cx: &mut gpui::TestAppContext) {
        crate::conversation_view::tests::init_test(cx);
        let state = ZepiTurnState {version: 1, id: Some("finished".into()), status: ZepiTurnStatus::Completed};
        cx.update(|cx| observe("persisted-task", state.clone(), false, 2, cx));
        cx.read(|cx| crate::zepi_session_store::flush(NAMESPACE, "persisted-task", cx)).await.unwrap();
        cx.update(|cx| {
            let payload = db::kvp::KeyValueStore::global(cx).scoped(NAMESPACE).read("persisted-task").unwrap().unwrap();
            let restored: Record = serde_json::from_str(&payload).unwrap();
            assert!(restored.unread);
            assert_eq!(restored.pending_review, 2);
            assert_eq!(restored.restored_status(), ZepiTurnStatus::Completed);
            mark_read("persisted-task", cx);
            assert_eq!(read("persisted-task", cx).turn, state);
            assert!(!read("persisted-task", cx).unread);
            assert_eq!(read("persisted-task", cx).pending_review, 2);
        });
        cx.read(|cx| crate::zepi_session_store::flush(NAMESPACE, "persisted-task", cx)).await.unwrap();
    }


    #[gpui::test]
    async fn test_zepi_task_keeps_last_turn_stats_for_a_restored_thread(cx: &mut gpui::TestAppContext) {
        crate::conversation_view::tests::init_test(cx);
        cx.update(|cx| remember_turn("persisted-stats", Some(1234), Some(std::time::Duration::from_millis(5000)), cx));
        cx.read(|cx| crate::zepi_session_store::flush(NAMESPACE, "persisted-stats", cx)).await.unwrap();
        cx.update(|cx| {
            let restored = read("persisted-stats", cx);
            assert_eq!(restored.tokens, Some(1234));
            assert_eq!(restored.duration_ms, Some(5000));
        });
    }
    #[test]
    fn test_zepi_task_outcomes_and_unread_are_independent_of_visibility() {
        let mut record = Record::default();
        let state = |id: &str, status| ZepiTurnState {version: 1, id: Some(id.into()), status};
        record.observe(state("one", ZepiTurnStatus::Running), false, 0);
        assert!(!record.unread);
        assert_eq!(record.restored_status(), ZepiTurnStatus::Interrupted);
        record.observe(state("one", ZepiTurnStatus::Failed), false, 0);
        assert!(record.unread);
        record.observe(state("one", ZepiTurnStatus::Failed), true, 0);
        assert!(!record.unread);
        record.observe(ZepiTurnState::default(), false, 0);
        assert_eq!(record.turn, state("one", ZepiTurnStatus::Failed));
        record.observe(state("one", ZepiTurnStatus::Failed), false, 0);
        assert!(!record.unread, "repaint must not make a read result unread");
        record.observe(state("two", ZepiTurnStatus::Cancelled), false, 0);
        assert!(record.unread);
        record.observe(state("three", ZepiTurnStatus::Completed), true, 0);
        assert!(!record.unread);
        assert_eq!(record.restored_status(), ZepiTurnStatus::Completed);
    }
}
