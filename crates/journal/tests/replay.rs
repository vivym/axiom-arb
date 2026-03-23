use journal::{replay_entries, JournalEvent, JournalWriter, SourceKind};
use serde_json::json;

#[tokio::test]
async fn journal_assigns_monotonic_seq_for_mixed_sources() {
    let writer = JournalWriter::in_memory();
    let a = writer.append(sample_market_event()).await.unwrap();
    let b = writer
        .append(sample_user_event(Some(a.journal_seq)))
        .await
        .unwrap();

    assert!(a.journal_seq < b.journal_seq);
}

#[tokio::test]
async fn replay_orders_by_journal_seq_and_preserves_replay_fields() {
    let writer = JournalWriter::in_memory();
    let parent = writer.append(sample_market_event()).await.unwrap();
    writer
        .append(sample_user_event(Some(parent.journal_seq)))
        .await
        .unwrap();

    let mut entries = writer.entries().unwrap();
    entries.reverse();

    let replay = replay_entries(entries);

    assert_eq!(replay.len(), 2);
    assert_eq!(replay[0].journal_seq, 1);
    assert_eq!(replay[0].stream, "market");
    assert_eq!(replay[0].source_event_id, "book-1");
    assert_eq!(replay[0].dedupe_key, "market:book-1");
    assert_eq!(replay[1].journal_seq, 2);
    assert_eq!(replay[1].stream, "user");
    assert_eq!(replay[1].source_session_id, "user-session-1");
    assert_eq!(replay[1].source_event_id, "trade-1");
    assert_eq!(replay[1].causal_parent_id, Some(parent.journal_seq));
}

#[tokio::test]
async fn journal_entries_match_durable_event_journal_contract() {
    let writer = JournalWriter::in_memory();
    let parent = writer.append(sample_market_event()).await.unwrap();
    let child = writer
        .append(sample_user_event(Some(parent.journal_seq)))
        .await
        .unwrap();

    assert_eq!(parent.source_event_id, "book-1");
    assert_eq!(parent.causal_parent_id, None);
    assert_eq!(child.source_event_id, "trade-1");
    assert_eq!(child.causal_parent_id, Some(1));
}

fn sample_market_event() -> JournalEvent {
    JournalEvent {
        stream: "market".to_owned(),
        source_kind: SourceKind::WsMarket,
        source_session_id: "market-session-1".to_owned(),
        source_event_id: "book-1".to_owned(),
        dedupe_key: "market:book-1".to_owned(),
        causal_parent_id: None,
        event_type: "orderbook_snapshot".to_owned(),
        event_ts: chrono::Utc::now(),
        payload: json!({
            "asset_id": "token-yes",
            "best_bid": "0.45",
        }),
    }
}

fn sample_user_event(causal_parent_id: Option<i64>) -> JournalEvent {
    JournalEvent {
        stream: "user".to_owned(),
        source_kind: SourceKind::WsUser,
        source_session_id: "user-session-1".to_owned(),
        source_event_id: "trade-1".to_owned(),
        dedupe_key: "user:trade-1".to_owned(),
        causal_parent_id,
        event_type: "trade".to_owned(),
        event_ts: chrono::Utc::now(),
        payload: json!({
            "order_id": "order-1",
            "status": "MATCHED",
        }),
    }
}
