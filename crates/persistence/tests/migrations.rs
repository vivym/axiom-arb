use std::sync::atomic::{AtomicU64, Ordering};

use chrono::Utc;
use domain::{
    ApprovalState, ApprovalStatus, ConditionId, DisputeState, IdentifierRecord, InventoryBucket,
    MarketId, MarketRoute, Order, OrderId, ResolutionState, ResolutionStatus, SettlementState,
    SignatureType, SignedOrderIdentity, SubmissionState, TokenId, VenueOrderState, WalletRoute,
};
use persistence::{
    models::{InventoryBucketRow, JournalEntryInput, NewOrderRow, PendingReconcileRow},
    run_migrations, ApprovalRepo, IdentifierRepo, InventoryRepo, JournalRepo,
    OperatorTargetAdoptionHistoryRepo, OrderRepo, PendingReconcileRepo, PersistenceError,
    ResolutionRepo,
};
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

struct TestDatabase {
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
}

impl TestDatabase {
    async fn new() -> Self {
        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for persistence tests");

        let admin_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("test database should connect");

        let schema = format!(
            "persistence_test_{}_{}",
            std::process::id(),
            NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)
        );
        let create_schema = format!(r#"CREATE SCHEMA "{schema}""#);

        sqlx::query(&create_schema)
            .execute(&admin_pool)
            .await
            .expect("test schema should create");

        let search_path_sql = format!(r#"SET search_path TO "{schema}""#);
        let pool = PgPoolOptions::new()
            .max_connections(4)
            .after_connect(move |conn, _meta| {
                let search_path_sql = search_path_sql.clone();
                Box::pin(async move {
                    sqlx::query(&search_path_sql).execute(conn).await?;
                    Ok(())
                })
            })
            .connect(&database_url)
            .await
            .expect("isolated test pool should connect");

        Self {
            admin_pool,
            pool,
            schema,
        }
    }

    async fn cleanup(self) {
        self.pool.close().await;

        let drop_schema = format!(
            r#"DROP SCHEMA IF EXISTS "{schema}" CASCADE"#,
            schema = self.schema
        );
        sqlx::query(&drop_schema)
            .execute(&self.admin_pool)
            .await
            .expect("test schema should drop");

        self.admin_pool.close().await;
    }
}

async fn table_exists(pool: &PgPool, table_name: &str) -> bool {
    sqlx::query(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM information_schema.tables
            WHERE table_schema = current_schema() AND table_name = $1
        ) AS exists
        "#,
    )
    .bind(table_name)
    .fetch_one(pool)
    .await
    .expect("table lookup should succeed")
    .get("exists")
}

async fn column_exists(pool: &PgPool, table_name: &str, column_name: &str) -> bool {
    sqlx::query(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM information_schema.columns
            WHERE table_schema = current_schema() AND table_name = $1 AND column_name = $2
        ) AS exists
        "#,
    )
    .bind(table_name)
    .bind(column_name)
    .fetch_one(pool)
    .await
    .expect("column lookup should succeed")
    .get("exists")
}

async fn assert_has_columns(pool: &PgPool, table_name: &str, column_names: &[&str]) {
    for column_name in column_names {
        assert!(
            column_exists(pool, table_name, column_name).await,
            "expected column {column_name} on {table_name}"
        );
    }
}

fn identifier_record(
    market_id: &str,
    condition_id: &str,
    token_id: &str,
    outcome_label: &str,
) -> IdentifierRecord {
    IdentifierRecord {
        event_id: "event-1".into(),
        event_family_id: "family-1".into(),
        market_id: market_id.into(),
        condition_id: condition_id.into(),
        token_id: token_id.into(),
        outcome_label: outcome_label.to_owned(),
        route: MarketRoute::Standard,
    }
}

fn signed_order(
    order_id: &str,
    market_id: &str,
    condition_id: &str,
    token_id: &str,
    signed_order_hash: &str,
    price_hundredths: i64,
    settlement_state: SettlementState,
) -> Order {
    Order {
        order_id: OrderId::from(order_id),
        market_id: MarketId::from(market_id),
        condition_id: ConditionId::from(condition_id),
        token_id: TokenId::from(token_id),
        quantity: Decimal::new(10, 0),
        price: Decimal::new(price_hundredths, 2),
        submission_state: SubmissionState::Signed,
        venue_state: VenueOrderState::Live,
        settlement_state,
        signed_order: Some(SignedOrderIdentity {
            signed_order_hash: signed_order_hash.to_owned(),
            salt: format!("salt-{order_id}"),
            nonce: format!("nonce-{order_id}"),
            signature: format!("sig-{order_id}"),
        }),
    }
}

async fn seed_identifier_graph(pool: &PgPool) {
    IdentifierRepo
        .upsert_record(
            pool,
            &identifier_record("market-1", "condition-1", "token-yes", "YES"),
        )
        .await
        .unwrap();
}

async fn seed_partial_market_graph(pool: &PgPool, market_id: &str, condition_id: &str) {
    sqlx::query(
        r#"
        INSERT INTO event_families (event_family_id, name)
        VALUES ('family-1', 'family-1')
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO events (event_id, event_family_id, name)
        VALUES ('event-1', 'family-1', 'event-1')
        "#,
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO conditions (condition_id, event_id)
        VALUES ($1, 'event-1')
        "#,
    )
    .bind(condition_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO markets (market_id, condition_id, event_id, route)
        VALUES ($1, $2, 'event-1', 'standard')
        "#,
    )
    .bind(market_id)
    .bind(condition_id)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn migrations_create_signed_order_and_resolution_tables() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    assert!(table_exists(&db.pool, "orders").await);
    assert!(column_exists(&db.pool, "orders", "signed_order_hash").await);
    assert!(table_exists(&db.pool, "resolution_states").await);
    assert!(
        column_exists(
            &db.pool,
            "runtime_apply_progress",
            "operator_target_revision"
        )
        .await
    );

    db.cleanup().await;
}

#[tokio::test]
async fn migrations_create_candidate_and_adoption_tables() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    assert!(table_exists(&db.pool, "candidate_target_sets").await);
    assert!(table_exists(&db.pool, "adoptable_target_revisions").await);
    assert!(table_exists(&db.pool, "candidate_adoption_provenance").await);
    assert!(
        column_exists(
            &db.pool,
            "candidate_adoption_provenance",
            "operator_target_revision"
        )
        .await
    );

    db.cleanup().await;
}

#[tokio::test]
async fn migrations_create_operator_target_adoption_history_table() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    assert!(table_exists(&db.pool, "operator_target_adoption_history").await);
    assert_has_columns(
        &db.pool,
        "operator_target_adoption_history",
        &[
            "history_seq",
            "adoption_id",
            "action_kind",
            "operator_target_revision",
            "previous_operator_target_revision",
            "adoptable_revision",
            "candidate_revision",
            "adopted_at",
        ],
    )
    .await;

    db.cleanup().await;
}

#[tokio::test]
async fn migration_0013_backfills_history_seq_using_legacy_latest_order() {
    let db = TestDatabase::new().await;
    apply_migration_file(&db.pool, "0012_operator_target_adoption_history.sql")
        .await
        .unwrap();

    insert_legacy_adoption_history_row(
        &db.pool,
        "z-first",
        "adopt",
        "targets-rev-10",
        Some("targets-rev-9"),
        Some("adoptable-10"),
        Some("candidate-10"),
        "2026-03-30T10:00:00Z",
    )
    .await;
    insert_legacy_adoption_history_row(
        &db.pool,
        "a-second",
        "adopt",
        "targets-rev-11",
        Some("targets-rev-10"),
        Some("adoptable-11"),
        Some("candidate-11"),
        "2026-03-30T10:00:00Z",
    )
    .await;

    apply_migration_file(
        &db.pool,
        "0013_operator_target_adoption_history_constraints.sql",
    )
    .await
    .unwrap();

    let latest = OperatorTargetAdoptionHistoryRepo
        .latest(&db.pool)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.adoption_id, "z-first");
    assert_eq!(latest.operator_target_revision, "targets-rev-10");

    db.cleanup().await;
}

#[tokio::test]
async fn migration_0013_backfills_history_seq_using_legacy_previous_distinct_order() {
    let db = TestDatabase::new().await;
    apply_migration_file(&db.pool, "0012_operator_target_adoption_history.sql")
        .await
        .unwrap();

    insert_legacy_adoption_history_row(
        &db.pool,
        "z-first",
        "adopt",
        "targets-rev-12",
        Some("targets-rev-10"),
        Some("adoptable-12-a"),
        Some("candidate-12-a"),
        "2026-03-30T10:10:00Z",
    )
    .await;
    insert_legacy_adoption_history_row(
        &db.pool,
        "a-second",
        "adopt",
        "targets-rev-12",
        Some("targets-rev-11"),
        Some("adoptable-12-b"),
        Some("candidate-12-b"),
        "2026-03-30T10:10:00Z",
    )
    .await;

    apply_migration_file(
        &db.pool,
        "0013_operator_target_adoption_history_constraints.sql",
    )
    .await
    .unwrap();

    let previous = OperatorTargetAdoptionHistoryRepo
        .previous_distinct_revision(&db.pool, "targets-rev-12")
        .await
        .unwrap();
    assert_eq!(previous.as_deref(), Some("targets-rev-10"));

    db.cleanup().await;
}

#[tokio::test]
async fn persistence_repos_round_trip_runtime_foundation() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let identifiers = IdentifierRepo;
    let orders = OrderRepo;
    let approvals = ApprovalRepo;
    let inventory = InventoryRepo;
    let resolutions = ResolutionRepo;
    let journal = JournalRepo;

    identifiers
        .upsert_record(
            &db.pool,
            &identifier_record("market-1", "condition-1", "token-yes", "YES"),
        )
        .await
        .unwrap();

    let original_order = signed_order(
        "order-1",
        "market-1",
        "condition-1",
        "token-yes",
        "hash-1",
        55,
        SettlementState::Unknown,
    );
    orders
        .insert_signed_order(&db.pool, NewOrderRow::from_domain(&original_order, None))
        .await
        .unwrap();

    let retry_order = signed_order(
        "order-2",
        "market-1",
        "condition-1",
        "token-yes",
        "hash-2",
        54,
        SettlementState::Retrying,
    );
    orders
        .insert_signed_order(
            &db.pool,
            NewOrderRow::from_domain(&retry_order, Some(&original_order.order_id)),
        )
        .await
        .unwrap();

    approvals
        .upsert_state(
            &db.pool,
            &ApprovalState {
                token_id: TokenId::from("token-yes"),
                spender: "0xspender".to_owned(),
                owner_address: "0xowner".to_owned(),
                funder_address: "0xfunder".to_owned(),
                wallet_route: WalletRoute::Proxy,
                signature_type: SignatureType::Proxy,
                allowance: Decimal::new(100, 0),
                required_min_allowance: Decimal::new(50, 0),
                last_checked_at: Utc::now(),
                approval_status: ApprovalStatus::Approved,
            },
        )
        .await
        .unwrap();

    inventory
        .upsert_bucket(
            &db.pool,
            &InventoryBucketRow {
                linked_order_id: Some(original_order.order_id.as_str().to_owned()),
                ..InventoryBucketRow::new(
                    "token-yes",
                    "0xowner",
                    InventoryBucket::ReservedForOrder,
                    Decimal::new(10, 0),
                )
            },
        )
        .await
        .unwrap();

    resolutions
        .upsert_state(
            &db.pool,
            &ResolutionState {
                condition_id: ConditionId::from("condition-1"),
                resolution_status: ResolutionStatus::Resolved,
                payout_vector: vec![Decimal::new(1, 0), Decimal::new(0, 0)],
                resolved_at: Some(Utc::now()),
                dispute_state: DisputeState::None,
                redeemable_at: Some(Utc::now()),
            },
        )
        .await
        .unwrap();

    let journal_row = journal
        .append(
            &db.pool,
            &JournalEntryInput {
                stream: "runtime".to_owned(),
                source_kind: "test".to_owned(),
                source_session_id: "session-1".to_owned(),
                source_event_id: "event-1".to_owned(),
                dedupe_key: "dedupe-1".to_owned(),
                causal_parent_id: None,
                event_type: "order_submitted".to_owned(),
                event_ts: Utc::now(),
                payload: json!({"order_id": "order-1"}),
            },
        )
        .await
        .unwrap();

    assert_eq!(identifiers.list_records(&db.pool).await.unwrap().len(), 1);

    let stored_retry_order = orders
        .get_order(&db.pool, &retry_order.order_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        stored_retry_order
            .order
            .signed_order
            .unwrap()
            .signed_order_hash,
        "hash-2"
    );
    assert_eq!(
        stored_retry_order.retry_of_order_id,
        Some(original_order.order_id.clone())
    );

    assert_eq!(
        approvals
            .get_state(&db.pool, "token-yes", "0xspender", "0xowner")
            .await
            .unwrap()
            .unwrap()
            .wallet_route,
        WalletRoute::Proxy
    );
    assert_eq!(
        inventory
            .list_by_owner(&db.pool, "0xowner")
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        resolutions
            .get_state(&db.pool, &ConditionId::from("condition-1"))
            .await
            .unwrap()
            .unwrap()
            .payout_vector,
        vec![Decimal::new(1, 0), Decimal::new(0, 0)]
    );
    assert_eq!(journal_row.journal_seq, 1);
    assert_eq!(journal.list_after(&db.pool, 0, 10).await.unwrap().len(), 1);

    db.cleanup().await;
}

#[tokio::test]
async fn journal_repo_list_since_filters_rows_in_sql() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();
    let journal = JournalRepo;
    let old_ts = Utc::now() - chrono::Duration::hours(2);
    let recent_ts = Utc::now() - chrono::Duration::minutes(5);
    let cutoff = Utc::now() - chrono::Duration::minutes(30);

    sqlx::query(
        r#"
        INSERT INTO event_journal (
            stream,
            source_kind,
            source_session_id,
            source_event_id,
            dedupe_key,
            causal_parent_id,
            event_type,
            event_ts,
            payload,
            ingested_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind("verify")
    .bind("test")
    .bind("session-old")
    .bind("event-old")
    .bind("event-old")
    .bind(None::<i64>)
    .bind("old")
    .bind(old_ts)
    .bind(json!({ "kind": "old" }))
    .bind(old_ts)
    .execute(&db.pool)
    .await
    .unwrap();
    sqlx::query(
        r#"
        INSERT INTO event_journal (
            stream,
            source_kind,
            source_session_id,
            source_event_id,
            dedupe_key,
            causal_parent_id,
            event_type,
            event_ts,
            payload,
            ingested_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        "#,
    )
    .bind("verify")
    .bind("test")
    .bind("session-recent")
    .bind("event-recent")
    .bind("event-recent")
    .bind(None::<i64>)
    .bind("recent")
    .bind(recent_ts)
    .bind(json!({ "kind": "recent" }))
    .bind(recent_ts)
    .execute(&db.pool)
    .await
    .unwrap();

    let rows = journal.list_since(&db.pool, cutoff).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].event_type, "recent");

    db.cleanup().await;
}

#[tokio::test]
async fn live_submission_migration_preserves_existing_live_artifacts_pending_reconcile_anchors_and_blocks_mode_drift(
) {
    let db = TestDatabase::new().await;

    apply_migration_file(&db.pool, "0005_unified_runtime_backbone.sql")
        .await
        .unwrap();
    apply_migration_file(&db.pool, "0006_phase3b_negrisk_live.sql")
        .await
        .unwrap();
    apply_migration_file(&db.pool, "0007_phase3b_negrisk_live_followup.sql")
        .await
        .unwrap();
    apply_migration_file(&db.pool, "0008_execution_attempt_audit_anchor.sql")
        .await
        .unwrap();

    let pending_reconcile = PendingReconcileRow {
        pending_ref: "pending-reconcile-0009-1".to_owned(),
        scope_kind: "family".to_owned(),
        scope_id: "family-a".to_owned(),
        reason: "awaiting_resolve".to_owned(),
        payload: json!({
            "submission_ref": "submission-ref-0009-1",
            "family_id": "family-a",
            "route": "neg-risk",
            "reason": "awaiting_resolve",
        }),
    };

    sqlx::query(
        r#"
        INSERT INTO execution_attempts (
            attempt_id,
            plan_id,
            snapshot_id,
            execution_mode,
            attempt_no,
            idempotency_key
        )
        VALUES ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind("attempt-live-0009-1")
    .bind("request-bound:5:req-1:negrisk-submit-family:family-a")
    .bind("snapshot-legacy")
    .bind("live")
    .bind(1_i32)
    .bind("idem-legacy-0009-1")
    .execute(&db.pool)
    .await
    .unwrap();

    PendingReconcileRepo
        .append(&db.pool, &pending_reconcile)
        .await
        .unwrap();

    sqlx::query(
        r#"
        INSERT INTO live_execution_artifacts (attempt_id, stream, payload)
        VALUES ($1, $2, $3)
        "#,
    )
    .bind("attempt-live-0009-1")
    .bind("live.execution")
    .bind(json!({ "kind": "planned_order", "seq": 1 }))
    .execute(&db.pool)
    .await
    .unwrap();

    apply_migration_file(&db.pool, "0009_phase3c_negrisk_live_submit_closure.sql")
        .await
        .unwrap();

    assert!(table_exists(&db.pool, "live_submission_records").await);

    let pending_rows = PendingReconcileRepo.list_all(&db.pool).await.unwrap();
    assert_eq!(pending_rows, vec![pending_reconcile]);

    let payloads: Vec<serde_json::Value> = sqlx::query_scalar(
        r#"
        SELECT payload
        FROM live_execution_artifacts
        WHERE attempt_id = $1 AND stream = $2
        "#,
    )
    .bind("attempt-live-0009-1")
    .bind("live.execution")
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(payloads, vec![json!({ "kind": "planned_order", "seq": 1 })]);

    sqlx::query(
        r#"
        INSERT INTO live_submission_records (
            submission_ref,
            attempt_id,
            route,
            scope,
            provider,
            state,
            payload
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind("submission-ref-0009-1")
    .bind("attempt-live-0009-1")
    .bind("neg-risk")
    .bind("family-a")
    .bind("venue-polymarket")
    .bind("pending_reconcile")
    .bind(json!({
        "submission_ref": "submission-ref-0009-1",
        "family_id": "family-a",
        "route": "neg-risk",
        "reason": "awaiting_resolve",
    }))
    .execute(&db.pool)
    .await
    .unwrap();

    let err = sqlx::query(
        r#"
        UPDATE execution_attempts
        SET execution_mode = 'shadow'
        WHERE attempt_id = $1
        "#,
    )
    .bind("attempt-live-0009-1")
    .execute(&db.pool)
    .await
    .unwrap_err();

    let message = err.to_string();
    assert!(
        message.contains("live submission records")
            && message.contains("cannot change away from live"),
        "unexpected database error: {message}"
    );

    db.cleanup().await;
}

#[tokio::test]
async fn duplicate_signed_payloads_are_rejected() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();
    seed_identifier_graph(&db.pool).await;

    let orders = OrderRepo;
    let first_order = signed_order(
        "order-1",
        "market-1",
        "condition-1",
        "token-yes",
        "same-hash",
        55,
        SettlementState::Unknown,
    );
    orders
        .insert_signed_order(&db.pool, NewOrderRow::from_domain(&first_order, None))
        .await
        .unwrap();

    let duplicate_order = signed_order(
        "order-2",
        "market-1",
        "condition-1",
        "token-yes",
        "same-hash",
        56,
        SettlementState::Retrying,
    );
    let err = orders
        .insert_signed_order(&db.pool, NewOrderRow::from_domain(&duplicate_order, None))
        .await
        .expect_err("duplicate signed payload should be rejected");

    match err {
        PersistenceError::DuplicateSignedOrderHash {
            signed_order_hash,
            existing_order_id,
            attempted_order_id,
        } => {
            assert_eq!(signed_order_hash, "same-hash");
            assert_eq!(existing_order_id, "order-1");
            assert_eq!(attempted_order_id, "order-2");
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let order_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(order_count, 1);

    db.cleanup().await;
}

#[tokio::test]
async fn conflicting_identifier_mappings_are_rejected() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let repo = IdentifierRepo;
    repo.upsert_record(
        &db.pool,
        &identifier_record("market-1", "condition-1", "token-yes", "YES"),
    )
    .await
    .unwrap();

    let err = repo
        .upsert_record(
            &db.pool,
            &identifier_record("market-2", "condition-2", "token-yes", "YES"),
        )
        .await
        .expect_err("conflicting token mapping should be rejected");

    match err {
        PersistenceError::IdentifierConflict(conflict) => {
            assert!(matches!(
                conflict,
                domain::IdentifierMapError::ConflictingTokenCondition { .. }
            ));
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let stored_records = repo.list_records(&db.pool).await.unwrap();
    assert_eq!(
        stored_records,
        vec![identifier_record(
            "market-1",
            "condition-1",
            "token-yes",
            "YES"
        )]
    );

    db.cleanup().await;
}

#[tokio::test]
async fn market_uniqueness_race_is_reported_as_identifier_conflict() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();
    seed_partial_market_graph(&db.pool, "market-existing", "condition-1").await;

    let repo = IdentifierRepo;
    let err = repo
        .upsert_record(
            &db.pool,
            &identifier_record("market-new", "condition-1", "token-yes", "YES"),
        )
        .await
        .expect_err("market uniqueness conflict should normalize to IdentifierConflict");

    match err {
        PersistenceError::IdentifierConflict(conflict) => {
            assert!(matches!(
                conflict,
                domain::IdentifierMapError::ConflictingConditionMetadata { .. }
            ));
        }
        other => panic!("unexpected error: {other:?}"),
    }

    db.cleanup().await;
}

#[tokio::test]
async fn conflicting_identifier_metadata_rewrites_are_rejected() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let repo = IdentifierRepo;
    repo.upsert_record(
        &db.pool,
        &identifier_record("market-1", "condition-1", "token-yes", "YES"),
    )
    .await
    .unwrap();

    let err = repo
        .upsert_record(
            &db.pool,
            &IdentifierRecord {
                outcome_label: "MAYBE".to_owned(),
                ..identifier_record("market-1", "condition-1", "token-yes", "YES")
            },
        )
        .await
        .expect_err("conflicting identifier metadata rewrite should be rejected");

    match err {
        PersistenceError::IdentifierConflict(conflict) => {
            assert!(matches!(
                conflict,
                domain::IdentifierMapError::ConflictingTokenMetadata { .. }
            ));
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let stored_records = repo.list_records(&db.pool).await.unwrap();
    assert_eq!(
        stored_records,
        vec![identifier_record(
            "market-1",
            "condition-1",
            "token-yes",
            "YES"
        )]
    );

    db.cleanup().await;
}

#[tokio::test]
async fn duplicate_condition_outcome_is_reported_as_identifier_conflict() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let repo = IdentifierRepo;
    repo.upsert_record(
        &db.pool,
        &identifier_record("market-1", "condition-1", "token-yes", "YES"),
    )
    .await
    .unwrap();

    let err = repo
        .upsert_record(
            &db.pool,
            &identifier_record("market-1", "condition-1", "token-yes-2", "YES"),
        )
        .await
        .expect_err("duplicate condition/outcome should normalize to IdentifierConflict");

    match err {
        PersistenceError::IdentifierConflict(conflict) => {
            assert!(matches!(
                conflict,
                domain::IdentifierMapError::ConflictingTokenMetadata { .. }
            ));
        }
        other => panic!("unexpected error: {other:?}"),
    }

    db.cleanup().await;
}

#[tokio::test]
async fn inconsistent_order_identifier_linkage_is_rejected() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();

    let identifiers = IdentifierRepo;
    let orders = OrderRepo;

    identifiers
        .upsert_record(
            &db.pool,
            &identifier_record("market-1", "condition-1", "token-yes", "YES"),
        )
        .await
        .unwrap();
    identifiers
        .upsert_record(
            &db.pool,
            &identifier_record("market-2", "condition-2", "token-no", "NO"),
        )
        .await
        .unwrap();

    let err = orders
        .insert_signed_order(
            &db.pool,
            NewOrderRow::from_domain(
                &signed_order(
                    "order-3",
                    "market-1",
                    "condition-1",
                    "token-no",
                    "hash-3",
                    52,
                    SettlementState::Unknown,
                ),
                None,
            ),
        )
        .await
        .expect_err("inconsistent market/condition/token combination should be rejected");

    match err {
        PersistenceError::InvalidOrderIdentifierLinkage {
            market_id,
            condition_id,
            token_id,
        } => {
            assert_eq!(market_id, "market-1");
            assert_eq!(condition_id, "condition-1");
            assert_eq!(token_id, "token-no");
        }
        other => panic!("unexpected error: {other:?}"),
    }

    db.cleanup().await;
}

#[tokio::test]
async fn orders_table_rejects_invalid_submission_state() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();
    seed_identifier_graph(&db.pool).await;

    let err = sqlx::query(
        r#"
        INSERT INTO orders (
            order_id,
            market_id,
            condition_id,
            token_id,
            quantity,
            price,
            submission_state,
            venue_state,
            settlement_state
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#,
    )
    .bind("invalid-state-order")
    .bind("market-1")
    .bind("condition-1")
    .bind("token-yes")
    .bind(Decimal::new(10, 0))
    .bind(Decimal::new(55, 2))
    .bind("not-a-real-state")
    .bind("live")
    .bind("unknown")
    .execute(&db.pool)
    .await
    .expect_err("invalid submission_state should be rejected");

    let err_text = err.to_string();
    assert!(err_text.contains("orders_submission_state_valid"));

    db.cleanup().await;
}

#[tokio::test]
async fn reusing_order_id_with_different_payload_is_rejected() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();
    seed_identifier_graph(&db.pool).await;

    let orders = OrderRepo;
    let original = signed_order(
        "order-1",
        "market-1",
        "condition-1",
        "token-yes",
        "hash-1",
        55,
        SettlementState::Unknown,
    );
    orders
        .insert_signed_order(&db.pool, NewOrderRow::from_domain(&original, None))
        .await
        .unwrap();

    let conflicting = signed_order(
        "order-1",
        "market-1",
        "condition-1",
        "token-yes",
        "hash-1b",
        56,
        SettlementState::Retrying,
    );
    let err = orders
        .insert_signed_order(&db.pool, NewOrderRow::from_domain(&conflicting, None))
        .await
        .expect_err("reusing an existing order_id with different payload should be rejected");

    match err {
        PersistenceError::ImmutableOrderConflict { order_id } => {
            assert_eq!(order_id, "order-1");
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let stored = orders
        .get_order(&db.pool, &OrderId::from("order-1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.order.price, Decimal::new(55, 2));
    assert_eq!(
        stored.order.signed_order.unwrap().signed_order_hash,
        "hash-1"
    );

    db.cleanup().await;
}

#[tokio::test]
async fn replaying_identical_signed_order_is_idempotent() {
    let db = TestDatabase::new().await;
    run_migrations(&db.pool).await.unwrap();
    seed_identifier_graph(&db.pool).await;

    let orders = OrderRepo;
    let original = signed_order(
        "order-1",
        "market-1",
        "condition-1",
        "token-yes",
        "hash-1",
        55,
        SettlementState::Unknown,
    );
    let row = NewOrderRow::from_domain(&original, None);

    orders
        .insert_signed_order(&db.pool, row.clone())
        .await
        .unwrap();
    orders.insert_signed_order(&db.pool, row).await.unwrap();

    let order_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM orders")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(order_count, 1);

    let stored = orders
        .get_order(&db.pool, &OrderId::from("order-1"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.order.price, Decimal::new(55, 2));
    assert_eq!(
        stored.order.signed_order.unwrap().signed_order_hash,
        "hash-1"
    );

    db.cleanup().await;
}

fn migration_file(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../migrations")
        .join(name)
}

async fn apply_migration_file(pool: &PgPool, file_name: &str) -> Result<(), sqlx::Error> {
    let sql = std::fs::read_to_string(migration_file(file_name))
        .expect("migration file should exist for migration tests");
    sqlx::raw_sql(&sql).execute(pool).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn insert_legacy_adoption_history_row(
    pool: &PgPool,
    adoption_id: &str,
    action_kind: &str,
    operator_target_revision: &str,
    previous_operator_target_revision: Option<&str>,
    adoptable_revision: Option<&str>,
    candidate_revision: Option<&str>,
    adopted_at: &str,
) {
    sqlx::query(
        r#"
        INSERT INTO operator_target_adoption_history (
            adoption_id,
            action_kind,
            operator_target_revision,
            previous_operator_target_revision,
            adoptable_revision,
            candidate_revision,
            adopted_at
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7::timestamptz)
        "#,
    )
    .bind(adoption_id)
    .bind(action_kind)
    .bind(operator_target_revision)
    .bind(previous_operator_target_revision)
    .bind(adoptable_revision)
    .bind(candidate_revision)
    .bind(adopted_at)
    .execute(pool)
    .await
    .unwrap();
}
