use domain::{
    ApprovalState, ConditionId, IdentifierMap, IdentifierRecord, OrderId, ResolutionState,
};
use sqlx::{postgres::PgRow, PgPool, Postgres, Row, Transaction};

use crate::{
    models::{
        ApprovalStateRow, IdentifierRecordRow, InventoryBucketRow, JournalEntryInput,
        JournalEntryRow, NewOrderRow, OrderRow, ResolutionStateRow, StoredOrder,
    },
    PersistenceError, Result,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct IdentifierRepo;

impl IdentifierRepo {
    pub async fn upsert_record(&self, pool: &PgPool, record: &IdentifierRecord) -> Result<()> {
        let row = IdentifierRecordRow::from_domain(record);
        let mut tx = pool.begin().await?;
        self.validate_record_in_tx(&mut tx, record).await?;

        self.insert_or_confirm_event_family(&mut tx, &row).await?;
        self.insert_or_confirm_event(&mut tx, &row).await?;
        self.insert_or_confirm_condition(&mut tx, &row).await?;
        self.insert_or_confirm_market(&mut tx, &row).await?;
        self.insert_or_confirm_token(&mut tx, &row).await?;
        self.insert_or_confirm_identifier_map(&mut tx, &row).await?;

        tx.commit().await?;
        Ok(())
    }

    async fn validate_record_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        record: &IdentifierRecord,
    ) -> Result<()> {
        let mut records = self.list_records_in_tx(tx).await?;
        records.push(record.clone());
        IdentifierMap::from_records(records)?;
        Ok(())
    }

    async fn list_records_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
    ) -> Result<Vec<IdentifierRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT event_id, event_family_id, market_id, condition_id, token_id, outcome_label, route
            FROM identifier_map
            ORDER BY event_family_id, event_id, market_id, token_id
            "#,
        )
        .fetch_all(&mut **tx)
        .await?;

        rows.into_iter()
            .map(map_identifier_record_row)
            .map(|row| row.and_then(IdentifierRecordRow::into_domain))
            .collect()
    }

    async fn insert_or_confirm_event_family(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        row: &IdentifierRecordRow,
    ) -> Result<()> {
        let result = sqlx::query(
            r#"
            INSERT INTO event_families (event_family_id, name)
            VALUES ($1, $2)
            ON CONFLICT (event_family_id) DO NOTHING
            "#,
        )
        .bind(&row.event_family_id)
        .bind(&row.event_family_id)
        .execute(&mut **tx)
        .await?;

        if result.rows_affected() == 0 {
            let existing_name: String =
                sqlx::query_scalar("SELECT name FROM event_families WHERE event_family_id = $1")
                    .bind(&row.event_family_id)
                    .fetch_one(&mut **tx)
                    .await?;

            if existing_name != row.event_family_id {
                return Err(PersistenceError::IdentifierConflict(
                    domain::IdentifierMapError::ConflictingConditionMetadata {
                        condition_id: row.condition_id.clone().into(),
                    },
                ));
            }
        }

        Ok(())
    }

    async fn insert_or_confirm_event(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        row: &IdentifierRecordRow,
    ) -> Result<()> {
        let result = sqlx::query(
            r#"
            INSERT INTO events (event_id, event_family_id, name)
            VALUES ($1, $2, $3)
            ON CONFLICT (event_id) DO NOTHING
            "#,
        )
        .bind(&row.event_id)
        .bind(&row.event_family_id)
        .bind(&row.event_id)
        .execute(&mut **tx)
        .await?;

        if result.rows_affected() == 0 {
            let existing =
                sqlx::query("SELECT event_family_id, name FROM events WHERE event_id = $1")
                    .bind(&row.event_id)
                    .fetch_one(&mut **tx)
                    .await?;

            let existing_family_id: String = existing.try_get("event_family_id")?;
            let existing_name: String = existing.try_get("name")?;
            if existing_family_id != row.event_family_id || existing_name != row.event_id {
                return Err(PersistenceError::IdentifierConflict(
                    domain::IdentifierMapError::ConflictingConditionMetadata {
                        condition_id: row.condition_id.clone().into(),
                    },
                ));
            }
        }

        Ok(())
    }

    async fn insert_or_confirm_condition(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        row: &IdentifierRecordRow,
    ) -> Result<()> {
        let result = sqlx::query(
            r#"
            INSERT INTO conditions (condition_id, event_id)
            VALUES ($1, $2)
            ON CONFLICT (condition_id) DO NOTHING
            "#,
        )
        .bind(&row.condition_id)
        .bind(&row.event_id)
        .execute(&mut **tx)
        .await?;

        if result.rows_affected() == 0 {
            let existing_event_id: String =
                sqlx::query_scalar("SELECT event_id FROM conditions WHERE condition_id = $1")
                    .bind(&row.condition_id)
                    .fetch_one(&mut **tx)
                    .await?;

            if existing_event_id != row.event_id {
                return Err(PersistenceError::IdentifierConflict(
                    domain::IdentifierMapError::ConflictingConditionMetadata {
                        condition_id: row.condition_id.clone().into(),
                    },
                ));
            }
        }

        Ok(())
    }

    async fn insert_or_confirm_market(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        row: &IdentifierRecordRow,
    ) -> Result<()> {
        let result = sqlx::query(
            r#"
            INSERT INTO markets (market_id, condition_id, event_id, route)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (market_id) DO NOTHING
            "#,
        )
        .bind(&row.market_id)
        .bind(&row.condition_id)
        .bind(&row.event_id)
        .bind(&row.route)
        .execute(&mut **tx)
        .await?;

        if result.rows_affected() == 0 {
            let existing = sqlx::query(
                "SELECT condition_id, event_id, route FROM markets WHERE market_id = $1",
            )
            .bind(&row.market_id)
            .fetch_one(&mut **tx)
            .await?;

            let existing_condition_id: String = existing.try_get("condition_id")?;
            let existing_event_id: String = existing.try_get("event_id")?;
            let existing_route: String = existing.try_get("route")?;

            if existing_condition_id != row.condition_id || existing_event_id != row.event_id {
                return Err(PersistenceError::IdentifierConflict(
                    domain::IdentifierMapError::ConflictingConditionMetadata {
                        condition_id: row.condition_id.clone().into(),
                    },
                ));
            }

            if existing_route != row.route {
                return Err(PersistenceError::IdentifierConflict(
                    domain::IdentifierMapError::ConflictingConditionRoute {
                        condition_id: row.condition_id.clone().into(),
                        existing_route: route_from_str(&existing_route)?,
                        new_route: route_from_str(&row.route)?,
                    },
                ));
            }
        }

        Ok(())
    }

    async fn insert_or_confirm_token(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        row: &IdentifierRecordRow,
    ) -> Result<()> {
        let result = sqlx::query(
            r#"
            INSERT INTO tokens (token_id, condition_id, outcome_label)
            VALUES ($1, $2, $3)
            ON CONFLICT (token_id) DO NOTHING
            "#,
        )
        .bind(&row.token_id)
        .bind(&row.condition_id)
        .bind(&row.outcome_label)
        .execute(&mut **tx)
        .await?;

        if result.rows_affected() == 0 {
            let existing =
                sqlx::query("SELECT condition_id, outcome_label FROM tokens WHERE token_id = $1")
                    .bind(&row.token_id)
                    .fetch_one(&mut **tx)
                    .await?;

            let existing_condition_id: String = existing.try_get("condition_id")?;
            let existing_outcome_label: String = existing.try_get("outcome_label")?;

            if existing_condition_id != row.condition_id {
                return Err(PersistenceError::IdentifierConflict(
                    domain::IdentifierMapError::ConflictingTokenCondition {
                        token_id: row.token_id.clone().into(),
                        existing_condition_id: existing_condition_id.into(),
                        new_condition_id: row.condition_id.clone().into(),
                    },
                ));
            }

            if existing_outcome_label != row.outcome_label {
                return Err(PersistenceError::IdentifierConflict(
                    domain::IdentifierMapError::ConflictingTokenMetadata {
                        token_id: row.token_id.clone().into(),
                    },
                ));
            }
        }

        Ok(())
    }

    async fn insert_or_confirm_identifier_map(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        row: &IdentifierRecordRow,
    ) -> Result<()> {
        let result = sqlx::query(
            r#"
            INSERT INTO identifier_map (
                token_id,
                condition_id,
                market_id,
                event_id,
                event_family_id,
                outcome_label,
                route
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (token_id) DO NOTHING
            "#,
        )
        .bind(&row.token_id)
        .bind(&row.condition_id)
        .bind(&row.market_id)
        .bind(&row.event_id)
        .bind(&row.event_family_id)
        .bind(&row.outcome_label)
        .bind(&row.route)
        .execute(&mut **tx)
        .await?;

        if result.rows_affected() == 0 {
            let existing_row = sqlx::query(
                r#"
                SELECT event_id, event_family_id, market_id, condition_id, token_id, outcome_label, route
                FROM identifier_map
                WHERE token_id = $1
                "#,
            )
            .bind(&row.token_id)
            .fetch_one(&mut **tx)
            .await?;

            let existing = map_identifier_record_row(existing_row)?.into_domain()?;
            let attempted = row.clone().into_domain()?;

            if existing != attempted {
                let mut records = self.list_records_in_tx(tx).await?;
                records.push(attempted);
                IdentifierMap::from_records(records)?;
                return Err(PersistenceError::IdentifierConflict(
                    domain::IdentifierMapError::ConflictingTokenMetadata {
                        token_id: row.token_id.clone().into(),
                    },
                ));
            }
        }

        Ok(())
    }

    pub async fn list_records(&self, pool: &PgPool) -> Result<Vec<IdentifierRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT event_id, event_family_id, market_id, condition_id, token_id, outcome_label, route
            FROM identifier_map
            ORDER BY event_family_id, event_id, market_id, token_id
            "#,
        )
        .fetch_all(pool)
        .await?;

        rows.into_iter()
            .map(map_identifier_record_row)
            .map(|row| row.and_then(IdentifierRecordRow::into_domain))
            .collect()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct OrderRepo;

impl OrderRepo {
    pub async fn insert_signed_order(&self, pool: &PgPool, row: NewOrderRow) -> Result<()> {
        let signed_field_count = [
            row.signed_order_hash.is_some(),
            row.salt.is_some(),
            row.nonce.is_some(),
            row.signature.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count();

        if signed_field_count != 0 && signed_field_count != 4 {
            return Err(PersistenceError::IncompleteSignedOrderIdentity);
        }

        let mut tx = pool.begin().await?;

        if let Some(signed_order_hash) = row.signed_order_hash.as_deref() {
            if let Some(existing_order_id) = self
                .find_order_id_by_signed_hash_excluding(&mut tx, signed_order_hash, &row.order_id)
                .await?
            {
                return Err(PersistenceError::DuplicateSignedOrderHash {
                    signed_order_hash: signed_order_hash.to_owned(),
                    existing_order_id,
                    attempted_order_id: row.order_id.clone(),
                });
            }
        }

        let query_result = sqlx::query(
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
                settlement_state,
                signed_order_hash,
                salt,
                nonce,
                signature,
                retry_of_order_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            ON CONFLICT (order_id) DO NOTHING
            "#,
        )
        .bind(&row.order_id)
        .bind(&row.market_id)
        .bind(&row.condition_id)
        .bind(&row.token_id)
        .bind(row.quantity)
        .bind(row.price)
        .bind(submission_state(&row))
        .bind(venue_state(&row))
        .bind(settlement_state(&row))
        .bind(row.signed_order_hash.as_deref())
        .bind(row.salt.as_deref())
        .bind(row.nonce.as_deref())
        .bind(row.signature.as_deref())
        .bind(row.retry_of_order_id.as_deref())
        .execute(&mut *tx)
        .await;

        if let Ok(result) = &query_result {
            if result.rows_affected() == 0 {
                let existing = self
                    .get_order_row(&mut tx, &row.order_id)
                    .await?
                    .expect("existing order should load on order_id conflict");

                if order_row_matches_input(&existing, &row) {
                    tx.commit().await?;
                    return Ok(());
                }

                return Err(PersistenceError::ImmutableOrderConflict {
                    order_id: row.order_id.clone(),
                });
            }
        }

        if let Err(err) = query_result {
            if constraint_name(&err) == Some("orders_signed_order_hash_unique") {
                let signed_order_hash = row
                    .signed_order_hash
                    .clone()
                    .expect("duplicate hash constraint requires signed_order_hash");
                let existing_order_id = self
                    .find_order_id_by_signed_hash_in_pool(pool, &signed_order_hash)
                    .await?
                    .unwrap_or_else(|| "<unknown>".to_owned());

                return Err(PersistenceError::DuplicateSignedOrderHash {
                    signed_order_hash,
                    existing_order_id,
                    attempted_order_id: row.order_id.clone(),
                });
            }

            if constraint_name(&err) == Some("orders_identifier_map_link_valid") {
                return Err(PersistenceError::InvalidOrderIdentifierLinkage {
                    market_id: row.market_id.clone(),
                    condition_id: row.condition_id.clone(),
                    token_id: row.token_id.clone(),
                });
            }

            return Err(err.into());
        }

        tx.commit().await?;
        Ok(())
    }

    pub async fn get_order(
        &self,
        pool: &PgPool,
        order_id: &OrderId,
    ) -> Result<Option<StoredOrder>> {
        let row = sqlx::query(
            r#"
            SELECT
                order_id,
                market_id,
                condition_id,
                token_id,
                quantity,
                price,
                submission_state,
                venue_state,
                settlement_state,
                signed_order_hash,
                salt,
                nonce,
                signature,
                retry_of_order_id,
                created_at,
                updated_at
            FROM orders
            WHERE order_id = $1
            "#,
        )
        .bind(order_id.as_str())
        .fetch_optional(pool)
        .await?;

        row.map(map_order_row)
            .transpose()?
            .map(OrderRow::into_stored_order)
            .transpose()
    }

    async fn get_order_row(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        order_id: &str,
    ) -> Result<Option<OrderRow>> {
        let row = sqlx::query(
            r#"
            SELECT
                order_id,
                market_id,
                condition_id,
                token_id,
                quantity,
                price,
                submission_state,
                venue_state,
                settlement_state,
                signed_order_hash,
                salt,
                nonce,
                signature,
                retry_of_order_id,
                created_at,
                updated_at
            FROM orders
            WHERE order_id = $1
            "#,
        )
        .bind(order_id)
        .fetch_optional(&mut **tx)
        .await?;

        row.map(map_order_row).transpose()
    }

    async fn find_order_id_by_signed_hash_in_pool(
        &self,
        pool: &PgPool,
        signed_order_hash: &str,
    ) -> Result<Option<String>> {
        sqlx::query_scalar(
            r#"
            SELECT order_id
            FROM orders
            WHERE signed_order_hash = $1
            LIMIT 1
            "#,
        )
        .bind(signed_order_hash)
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
    }

    async fn find_order_id_by_signed_hash_excluding(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        signed_order_hash: &str,
        order_id: &str,
    ) -> Result<Option<String>> {
        sqlx::query_scalar(
            r#"
            SELECT order_id
            FROM orders
            WHERE signed_order_hash = $1 AND order_id <> $2
            LIMIT 1
            "#,
        )
        .bind(signed_order_hash)
        .bind(order_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(Into::into)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ApprovalRepo;

impl ApprovalRepo {
    pub async fn upsert_state(&self, pool: &PgPool, state: &ApprovalState) -> Result<()> {
        let row = ApprovalStateRow::from_domain(state);

        sqlx::query(
            r#"
            INSERT INTO approval_states (
                token_id,
                spender,
                owner_address,
                funder_address,
                wallet_route,
                signature_type,
                allowance,
                required_min_allowance,
                last_checked_at,
                approval_status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (token_id, spender, owner_address) DO UPDATE
            SET funder_address = EXCLUDED.funder_address,
                wallet_route = EXCLUDED.wallet_route,
                signature_type = EXCLUDED.signature_type,
                allowance = EXCLUDED.allowance,
                required_min_allowance = EXCLUDED.required_min_allowance,
                last_checked_at = EXCLUDED.last_checked_at,
                approval_status = EXCLUDED.approval_status,
                updated_at = NOW()
            "#,
        )
        .bind(&row.token_id)
        .bind(&row.spender)
        .bind(&row.owner_address)
        .bind(&row.funder_address)
        .bind(&row.wallet_route)
        .bind(&row.signature_type)
        .bind(row.allowance)
        .bind(row.required_min_allowance)
        .bind(row.last_checked_at)
        .bind(&row.approval_status)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn get_state(
        &self,
        pool: &PgPool,
        token_id: &str,
        spender: &str,
        owner_address: &str,
    ) -> Result<Option<ApprovalState>> {
        let row = sqlx::query(
            r#"
            SELECT
                token_id,
                spender,
                owner_address,
                funder_address,
                wallet_route,
                signature_type,
                allowance,
                required_min_allowance,
                last_checked_at,
                approval_status,
                updated_at
            FROM approval_states
            WHERE token_id = $1 AND spender = $2 AND owner_address = $3
            "#,
        )
        .bind(token_id)
        .bind(spender)
        .bind(owner_address)
        .fetch_optional(pool)
        .await?;

        row.map(map_approval_state_row)
            .transpose()?
            .map(ApprovalStateRow::into_domain)
            .transpose()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct InventoryRepo;

impl InventoryRepo {
    pub async fn upsert_bucket(&self, pool: &PgPool, row: &InventoryBucketRow) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO inventory_buckets (
                token_id,
                owner_address,
                bucket,
                quantity,
                linked_order_id,
                ctf_operation_id,
                relayer_transaction_id
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (token_id, owner_address, bucket) DO UPDATE
            SET quantity = EXCLUDED.quantity,
                linked_order_id = EXCLUDED.linked_order_id,
                ctf_operation_id = EXCLUDED.ctf_operation_id,
                relayer_transaction_id = EXCLUDED.relayer_transaction_id,
                updated_at = NOW()
            "#,
        )
        .bind(&row.token_id)
        .bind(&row.owner_address)
        .bind(&row.bucket)
        .bind(row.quantity)
        .bind(row.linked_order_id.as_deref())
        .bind(row.ctf_operation_id.as_deref())
        .bind(row.relayer_transaction_id.as_deref())
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn list_by_owner(
        &self,
        pool: &PgPool,
        owner_address: &str,
    ) -> Result<Vec<InventoryBucketRow>> {
        let rows = sqlx::query(
            r#"
            SELECT
                token_id,
                owner_address,
                bucket,
                quantity,
                linked_order_id,
                ctf_operation_id,
                relayer_transaction_id,
                updated_at
            FROM inventory_buckets
            WHERE owner_address = $1
            ORDER BY token_id, bucket
            "#,
        )
        .bind(owner_address)
        .fetch_all(pool)
        .await?;

        rows.into_iter().map(map_inventory_bucket_row).collect()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ResolutionRepo;

impl ResolutionRepo {
    pub async fn upsert_state(&self, pool: &PgPool, state: &ResolutionState) -> Result<()> {
        let row = ResolutionStateRow::from_domain(state);

        sqlx::query(
            r#"
            INSERT INTO resolution_states (
                condition_id,
                resolution_status,
                payout_vector,
                resolved_at,
                dispute_state,
                redeemable_at
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (condition_id) DO UPDATE
            SET resolution_status = EXCLUDED.resolution_status,
                payout_vector = EXCLUDED.payout_vector,
                resolved_at = EXCLUDED.resolved_at,
                dispute_state = EXCLUDED.dispute_state,
                redeemable_at = EXCLUDED.redeemable_at,
                updated_at = NOW()
            "#,
        )
        .bind(&row.condition_id)
        .bind(&row.resolution_status)
        .bind(&row.payout_vector)
        .bind(row.resolved_at)
        .bind(&row.dispute_state)
        .bind(row.redeemable_at)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn get_state(
        &self,
        pool: &PgPool,
        condition_id: &ConditionId,
    ) -> Result<Option<ResolutionState>> {
        let row = sqlx::query(
            r#"
            SELECT
                condition_id,
                resolution_status,
                payout_vector,
                resolved_at,
                dispute_state,
                redeemable_at,
                updated_at
            FROM resolution_states
            WHERE condition_id = $1
            "#,
        )
        .bind(condition_id.as_str())
        .fetch_optional(pool)
        .await?;

        row.map(map_resolution_state_row)
            .transpose()?
            .map(ResolutionStateRow::into_domain)
            .transpose()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct JournalRepo;

impl JournalRepo {
    pub async fn append(
        &self,
        pool: &PgPool,
        entry: &JournalEntryInput,
    ) -> Result<JournalEntryRow> {
        let row = sqlx::query(
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
                payload
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING
                journal_seq,
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
            "#,
        )
        .bind(&entry.stream)
        .bind(&entry.source_kind)
        .bind(&entry.source_session_id)
        .bind(&entry.source_event_id)
        .bind(&entry.dedupe_key)
        .bind(entry.causal_parent_id)
        .bind(&entry.event_type)
        .bind(entry.event_ts)
        .bind(&entry.payload)
        .fetch_one(pool)
        .await?;

        map_journal_entry_row(row)
    }

    pub async fn list_after(
        &self,
        pool: &PgPool,
        after_seq: i64,
        limit: i64,
    ) -> Result<Vec<JournalEntryRow>> {
        let rows = sqlx::query(
            r#"
            SELECT
                journal_seq,
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
            FROM event_journal
            WHERE journal_seq > $1
            ORDER BY journal_seq
            LIMIT $2
            "#,
        )
        .bind(after_seq)
        .bind(limit)
        .fetch_all(pool)
        .await?;

        rows.into_iter().map(map_journal_entry_row).collect()
    }
}

fn map_identifier_record_row(row: PgRow) -> Result<IdentifierRecordRow> {
    Ok(IdentifierRecordRow {
        event_id: row.try_get("event_id")?,
        event_family_id: row.try_get("event_family_id")?,
        market_id: row.try_get("market_id")?,
        condition_id: row.try_get("condition_id")?,
        token_id: row.try_get("token_id")?,
        outcome_label: row.try_get("outcome_label")?,
        route: row.try_get("route")?,
    })
}

fn map_order_row(row: PgRow) -> Result<OrderRow> {
    Ok(OrderRow {
        order_id: row.try_get("order_id")?,
        market_id: row.try_get("market_id")?,
        condition_id: row.try_get("condition_id")?,
        token_id: row.try_get("token_id")?,
        quantity: row.try_get("quantity")?,
        price: row.try_get("price")?,
        submission_state: row.try_get("submission_state")?,
        venue_state: row.try_get("venue_state")?,
        settlement_state: row.try_get("settlement_state")?,
        signed_order_hash: row.try_get("signed_order_hash")?,
        salt: row.try_get("salt")?,
        nonce: row.try_get("nonce")?,
        signature: row.try_get("signature")?,
        retry_of_order_id: row.try_get("retry_of_order_id")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_approval_state_row(row: PgRow) -> Result<ApprovalStateRow> {
    Ok(ApprovalStateRow {
        token_id: row.try_get("token_id")?,
        spender: row.try_get("spender")?,
        owner_address: row.try_get("owner_address")?,
        funder_address: row.try_get("funder_address")?,
        wallet_route: row.try_get("wallet_route")?,
        signature_type: row.try_get("signature_type")?,
        allowance: row.try_get("allowance")?,
        required_min_allowance: row.try_get("required_min_allowance")?,
        last_checked_at: row.try_get("last_checked_at")?,
        approval_status: row.try_get("approval_status")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_inventory_bucket_row(row: PgRow) -> Result<InventoryBucketRow> {
    Ok(InventoryBucketRow {
        token_id: row.try_get("token_id")?,
        owner_address: row.try_get("owner_address")?,
        bucket: row.try_get("bucket")?,
        quantity: row.try_get("quantity")?,
        linked_order_id: row.try_get("linked_order_id")?,
        ctf_operation_id: row.try_get("ctf_operation_id")?,
        relayer_transaction_id: row.try_get("relayer_transaction_id")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_resolution_state_row(row: PgRow) -> Result<ResolutionStateRow> {
    Ok(ResolutionStateRow {
        condition_id: row.try_get("condition_id")?,
        resolution_status: row.try_get("resolution_status")?,
        payout_vector: row.try_get("payout_vector")?,
        resolved_at: row.try_get("resolved_at")?,
        dispute_state: row.try_get("dispute_state")?,
        redeemable_at: row.try_get("redeemable_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}

fn map_journal_entry_row(row: PgRow) -> Result<JournalEntryRow> {
    Ok(JournalEntryRow {
        journal_seq: row.try_get("journal_seq")?,
        stream: row.try_get("stream")?,
        source_kind: row.try_get("source_kind")?,
        source_session_id: row.try_get("source_session_id")?,
        source_event_id: row.try_get("source_event_id")?,
        dedupe_key: row.try_get("dedupe_key")?,
        causal_parent_id: row.try_get("causal_parent_id")?,
        event_type: row.try_get("event_type")?,
        event_ts: row.try_get("event_ts")?,
        payload: row.try_get("payload")?,
        ingested_at: row.try_get("ingested_at")?,
    })
}

fn submission_state(row: &NewOrderRow) -> &'static str {
    match row.submission_state {
        domain::SubmissionState::Draft => "draft",
        domain::SubmissionState::Planned => "planned",
        domain::SubmissionState::RiskApproved => "risk_approved",
        domain::SubmissionState::Signed => "signed",
        domain::SubmissionState::Submitted => "submitted",
        domain::SubmissionState::Acked => "acked",
        domain::SubmissionState::Rejected => "rejected",
        domain::SubmissionState::Unknown => "unknown",
    }
}

fn venue_state(row: &NewOrderRow) -> &'static str {
    match row.venue_state {
        domain::VenueOrderState::Live => "live",
        domain::VenueOrderState::Matched => "matched",
        domain::VenueOrderState::Delayed => "delayed",
        domain::VenueOrderState::Unmatched => "unmatched",
        domain::VenueOrderState::CancelPending => "cancel_pending",
        domain::VenueOrderState::Cancelled => "cancelled",
        domain::VenueOrderState::Expired => "expired",
        domain::VenueOrderState::Unknown => "unknown",
    }
}

fn settlement_state(row: &NewOrderRow) -> &'static str {
    match row.settlement_state {
        domain::SettlementState::Matched => "matched",
        domain::SettlementState::Mined => "mined",
        domain::SettlementState::Confirmed => "confirmed",
        domain::SettlementState::Retrying => "retrying",
        domain::SettlementState::Failed => "failed",
        domain::SettlementState::Unknown => "unknown",
    }
}

fn order_row_matches_input(existing: &OrderRow, incoming: &NewOrderRow) -> bool {
    existing.order_id == incoming.order_id
        && existing.market_id == incoming.market_id
        && existing.condition_id == incoming.condition_id
        && existing.token_id == incoming.token_id
        && existing.quantity == incoming.quantity
        && existing.price == incoming.price
        && existing.submission_state == submission_state(incoming)
        && existing.venue_state == venue_state(incoming)
        && existing.settlement_state == settlement_state(incoming)
        && existing.signed_order_hash.as_deref() == incoming.signed_order_hash.as_deref()
        && existing.salt.as_deref() == incoming.salt.as_deref()
        && existing.nonce.as_deref() == incoming.nonce.as_deref()
        && existing.signature.as_deref() == incoming.signature.as_deref()
        && existing.retry_of_order_id.as_deref() == incoming.retry_of_order_id.as_deref()
}

fn route_from_str(value: &str) -> Result<domain::MarketRoute> {
    match value {
        "standard" => Ok(domain::MarketRoute::Standard),
        "negrisk" => Ok(domain::MarketRoute::NegRisk),
        _ => Err(PersistenceError::invalid_value("market_route", value)),
    }
}

fn constraint_name(err: &sqlx::Error) -> Option<&str> {
    match err {
        sqlx::Error::Database(db_err) => db_err.constraint(),
        _ => None,
    }
}
