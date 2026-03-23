use std::{path::Path, sync::OnceLock};

use chrono::Utc;
use domain::{
    ApprovalState, ApprovalStatus, ConditionId, DisputeState, IdentifierRecord, MarketId,
    MarketRoute, Order, OrderId, ResolutionState, ResolutionStatus, SettlementState, SignatureType,
    SignedOrderIdentity, SubmissionState, TokenId, VenueOrderState, WalletRoute,
};
use persistence::{
    models::{InventoryBucketRow, JournalEntryInput, NewOrderRow},
    ApprovalRepo, IdentifierRepo, InventoryRepo, JournalRepo, OrderRepo, ResolutionRepo,
};
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use tokio::sync::{Mutex, MutexGuard};

fn test_lock() -> &'static Mutex<()> {
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    TEST_LOCK.get_or_init(|| Mutex::new(()))
}

async fn test_db_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for persistence tests");

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await
        .expect("test database should connect");

    sqlx::query("DROP SCHEMA IF EXISTS public CASCADE")
        .execute(&pool)
        .await
        .expect("public schema should drop");
    sqlx::query("CREATE SCHEMA public")
        .execute(&pool)
        .await
        .expect("public schema should recreate");

    pool
}

async fn isolated_test_db_pool() -> (MutexGuard<'static, ()>, PgPool) {
    let guard = test_lock().lock().await;
    let pool = test_db_pool().await;
    (guard, pool)
}

async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    let migrations_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations");
    let migrator = sqlx::migrate::Migrator::new(migrations_dir.as_path()).await?;

    migrator.run(pool).await
}

async fn table_exists(pool: &PgPool, table_name: &str) -> bool {
    sqlx::query(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM information_schema.tables
            WHERE table_schema = 'public' AND table_name = $1
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
            WHERE table_schema = 'public' AND table_name = $1 AND column_name = $2
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

#[tokio::test]
async fn migrations_create_signed_order_and_resolution_tables() {
    let (_guard, pool) = isolated_test_db_pool().await;
    run_migrations(&pool).await.unwrap();

    assert!(table_exists(&pool, "orders").await);
    assert!(column_exists(&pool, "orders", "signed_order_hash").await);
    assert!(table_exists(&pool, "resolution_states").await);
}

#[tokio::test]
async fn persistence_repos_round_trip_runtime_foundation() {
    let (_guard, pool) = isolated_test_db_pool().await;
    run_migrations(&pool).await.unwrap();

    let identifiers = IdentifierRepo;
    let orders = OrderRepo;
    let approvals = ApprovalRepo;
    let inventory = InventoryRepo;
    let resolutions = ResolutionRepo;
    let journal = JournalRepo;

    identifiers
        .upsert_record(
            &pool,
            &IdentifierRecord {
                event_id: "event-1".into(),
                event_family_id: "family-1".into(),
                market_id: "market-1".into(),
                condition_id: "condition-1".into(),
                token_id: "token-yes".into(),
                outcome_label: "YES".to_owned(),
                route: MarketRoute::Standard,
            },
        )
        .await
        .unwrap();

    let original_order = Order {
        order_id: OrderId::from("order-1"),
        market_id: MarketId::from("market-1"),
        condition_id: ConditionId::from("condition-1"),
        token_id: TokenId::from("token-yes"),
        quantity: Decimal::new(10, 0),
        price: Decimal::new(55, 2),
        submission_state: SubmissionState::Signed,
        venue_state: VenueOrderState::Live,
        settlement_state: SettlementState::Unknown,
        signed_order: Some(SignedOrderIdentity {
            signed_order_hash: "hash-1".to_owned(),
            salt: "salt-1".to_owned(),
            nonce: "nonce-1".to_owned(),
            signature: "sig-1".to_owned(),
        }),
    };

    orders
        .insert_signed_order(&pool, NewOrderRow::from_domain(&original_order, None))
        .await
        .unwrap();

    let retry_order = Order {
        order_id: OrderId::from("order-2"),
        market_id: MarketId::from("market-1"),
        condition_id: ConditionId::from("condition-1"),
        token_id: TokenId::from("token-yes"),
        quantity: Decimal::new(10, 0),
        price: Decimal::new(54, 2),
        submission_state: SubmissionState::Submitted,
        venue_state: VenueOrderState::Unknown,
        settlement_state: SettlementState::Retrying,
        signed_order: Some(SignedOrderIdentity {
            signed_order_hash: "hash-2".to_owned(),
            salt: "salt-2".to_owned(),
            nonce: "nonce-2".to_owned(),
            signature: "sig-2".to_owned(),
        }),
    };

    orders
        .insert_signed_order(
            &pool,
            NewOrderRow::from_domain(&retry_order, Some(&original_order.order_id)),
        )
        .await
        .unwrap();

    approvals
        .upsert_state(
            &pool,
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
            &pool,
            &InventoryBucketRow {
                linked_order_id: Some(original_order.order_id.as_str().to_owned()),
                ..InventoryBucketRow::new(
                    "token-yes",
                    "0xowner",
                    domain::InventoryBucket::ReservedForOrder,
                    Decimal::new(10, 0),
                )
            },
        )
        .await
        .unwrap();

    resolutions
        .upsert_state(
            &pool,
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
            &pool,
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

    assert_eq!(identifiers.list_records(&pool).await.unwrap().len(), 1);
    assert_eq!(
        orders
            .get_order(&pool, &retry_order.order_id)
            .await
            .unwrap()
            .unwrap()
            .signed_order
            .unwrap()
            .signed_order_hash,
        "hash-2"
    );
    assert_eq!(
        approvals
            .get_state(&pool, "token-yes", "0xspender", "0xowner")
            .await
            .unwrap()
            .unwrap()
            .wallet_route,
        WalletRoute::Proxy
    );
    assert_eq!(
        inventory
            .list_by_owner(&pool, "0xowner")
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        resolutions
            .get_state(&pool, &ConditionId::from("condition-1"))
            .await
            .unwrap()
            .unwrap()
            .payout_vector,
        vec![Decimal::new(1, 0), Decimal::new(0, 0)]
    );
    assert_eq!(journal_row.journal_seq, 1);
    assert_eq!(journal.list_after(&pool, 0, 10).await.unwrap().len(), 1);
}
