use domain::{InventoryBucket, TokenId};
use rust_decimal::Decimal;
use state::StateStore;

#[test]
fn state_store_exposes_inventory_snapshot_without_mutable_access() {
    let mut store = StateStore::new();
    store.record_local_inventory(
        TokenId::from("token-a"),
        InventoryBucket::Free,
        Decimal::new(2, 0),
    );

    assert_eq!(store.inventory_snapshot().len(), 1);
}

#[test]
fn inventory_snapshot_ordering_is_deterministic() {
    let mut store = StateStore::new();
    store.record_local_inventory(
        TokenId::from("token-b"),
        InventoryBucket::MatchedUnsettled,
        Decimal::new(3, 0),
    );
    store.record_local_inventory(
        TokenId::from("token-a"),
        InventoryBucket::Free,
        Decimal::new(1, 0),
    );
    store.record_local_inventory(
        TokenId::from("token-a"),
        InventoryBucket::MatchedUnsettled,
        Decimal::new(2, 0),
    );

    let snapshot = store.inventory_snapshot();

    assert_eq!(
        snapshot
            .into_iter()
            .map(|row| (row.token_id, row.bucket, row.quantity))
            .collect::<Vec<_>>(),
        vec![
            (
                TokenId::from("token-a"),
                InventoryBucket::Free,
                Decimal::new(1, 0)
            ),
            (
                TokenId::from("token-a"),
                InventoryBucket::MatchedUnsettled,
                Decimal::new(2, 0)
            ),
            (
                TokenId::from("token-b"),
                InventoryBucket::MatchedUnsettled,
                Decimal::new(3, 0)
            ),
        ]
    );
}
