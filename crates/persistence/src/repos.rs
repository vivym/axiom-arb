use std::collections::BTreeMap;

use domain::{
    ApprovalState, ConditionId, IdentifierMap, IdentifierRecord, OrderId, ResolutionState,
};
use serde_json::{json, Value};
use sqlx::{postgres::PgRow, Executor, PgPool, Postgres, Row, Transaction};

use crate::{
    models::{
        execution_mode_from_str, execution_mode_to_str, ApprovalStateRow, ExecutionAttemptRow,
        FamilyHaltRow, IdentifierRecordRow, InventoryBucketRow, JournalEntryInput, JournalEntryRow,
        LiveExecutionArtifactRow, NegRiskDiscoverySnapshotInput, NegRiskFamilyMemberRow,
        NegRiskFamilyValidationRow, NewOrderRow, OrderRow, PendingReconcileRow, ResolutionStateRow,
        RuntimeProgressRow, ShadowExecutionArtifactRow, SnapshotPublicationRow, StoredOrder,
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
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(&row.market_id)
        .bind(&row.condition_id)
        .bind(&row.event_id)
        .bind(&row.route)
        .execute(&mut **tx)
        .await?;

        if result.rows_affected() == 0 {
            if let Some(existing) = sqlx::query(
                "SELECT condition_id, event_id, route FROM markets WHERE market_id = $1",
            )
            .bind(&row.market_id)
            .fetch_optional(&mut **tx)
            .await?
            {
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

                return Ok(());
            }

            if sqlx::query_scalar::<_, String>(
                "SELECT market_id FROM markets WHERE condition_id = $1 LIMIT 1",
            )
            .bind(&row.condition_id)
            .fetch_optional(&mut **tx)
            .await?
            .is_some()
            {
                return Err(PersistenceError::IdentifierConflict(
                    domain::IdentifierMapError::ConflictingConditionMetadata {
                        condition_id: row.condition_id.clone().into(),
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
            ON CONFLICT DO NOTHING
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
            if let Some(existing_row) = sqlx::query(
                r#"
                SELECT event_id, event_family_id, market_id, condition_id, token_id, outcome_label, route
                FROM identifier_map
                WHERE token_id = $1
                "#,
            )
            .bind(&row.token_id)
            .fetch_optional(&mut **tx)
            .await?
            {
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

                return Ok(());
            }

            if sqlx::query_scalar::<_, String>(
                "SELECT token_id FROM identifier_map WHERE condition_id = $1 AND outcome_label = $2 LIMIT 1",
            )
            .bind(&row.condition_id)
            .bind(&row.outcome_label)
            .fetch_optional(&mut **tx)
            .await?
            .is_some()
            {
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
pub struct NegRiskFamilyRepo;

impl NegRiskFamilyRepo {
    pub async fn upsert_validation(
        &self,
        pool: &PgPool,
        row: &NegRiskFamilyValidationRow,
    ) -> Result<()> {
        let mut tx = pool.begin().await?;
        let existing = sqlx::query(
            r#"
            SELECT
                event_family_id,
                validation_status,
                exclusion_reason,
                metadata_snapshot_hash,
                last_seen_discovery_revision,
                member_count,
                first_seen_at,
                last_seen_at,
                validated_at,
                updated_at
            FROM neg_risk_family_validations
            WHERE event_family_id = $1
            FOR UPDATE
            "#,
        )
        .bind(&row.event_family_id)
        .fetch_optional(&mut *tx)
        .await?
        .map(map_neg_risk_family_validation_row)
        .transpose()?;

        if existing
            .as_ref()
            .is_some_and(|existing| same_validation_state(existing, row))
        {
            tx.commit().await?;
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO neg_risk_family_validations (
                event_family_id,
                validation_status,
                exclusion_reason,
                metadata_snapshot_hash,
                last_seen_discovery_revision,
                member_count,
                first_seen_at,
                last_seen_at,
                validated_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (event_family_id) DO UPDATE
            SET validation_status = EXCLUDED.validation_status,
                exclusion_reason = EXCLUDED.exclusion_reason,
                metadata_snapshot_hash = EXCLUDED.metadata_snapshot_hash,
                last_seen_discovery_revision = EXCLUDED.last_seen_discovery_revision,
                member_count = EXCLUDED.member_count,
                first_seen_at = EXCLUDED.first_seen_at,
                last_seen_at = EXCLUDED.last_seen_at,
                validated_at = EXCLUDED.validated_at,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&row.event_family_id)
        .bind(&row.validation_status)
        .bind(row.exclusion_reason.as_deref())
        .bind(&row.metadata_snapshot_hash)
        .bind(row.last_seen_discovery_revision)
        .bind(row.member_count)
        .bind(row.first_seen_at)
        .bind(row.last_seen_at)
        .bind(row.validated_at)
        .bind(row.updated_at)
        .execute(&mut *tx)
        .await?;

        let entry = JournalEntryInput {
            stream: format!("neg_risk_family:{}", row.event_family_id),
            source_kind: row.source_kind.clone(),
            source_session_id: row.source_session_id.clone(),
            source_event_id: row.source_event_id.clone(),
            dedupe_key: format!(
                "family_validation:{}:{}:{}:{}",
                row.event_family_id,
                row.last_seen_discovery_revision,
                row.metadata_snapshot_hash,
                row.source_event_id
            ),
            causal_parent_id: None,
            event_type: "family_validation".to_owned(),
            event_ts: row.event_ts,
            payload: json!({
                "event_family_id": row.event_family_id,
                "validation_status": row.validation_status,
                "exclusion_reason": row.exclusion_reason,
                "metadata_snapshot_hash": row.metadata_snapshot_hash,
                "discovery_revision": row.last_seen_discovery_revision,
                "member_count": row.member_count,
                "first_seen_at": row.first_seen_at.to_rfc3339(),
                "last_seen_at": row.last_seen_at.to_rfc3339(),
                "validated_at": row.validated_at.to_rfc3339(),
                "member_vector": member_vector_json(&row.member_vector),
            }),
        };
        append_journal_entry(&mut *tx, &entry).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn list_validations(&self, pool: &PgPool) -> Result<Vec<NegRiskFamilyValidationRow>> {
        let rows = sqlx::query(
            r#"
            SELECT
                event_family_id,
                validation_status,
                exclusion_reason,
                metadata_snapshot_hash,
                last_seen_discovery_revision,
                member_count,
                first_seen_at,
                last_seen_at,
                validated_at,
                updated_at
            FROM neg_risk_family_validations
            ORDER BY event_family_id
            "#,
        )
        .fetch_all(pool)
        .await?;

        rows.into_iter()
            .map(map_neg_risk_family_validation_row)
            .collect()
    }

    pub async fn upsert_halt(&self, pool: &PgPool, row: &FamilyHaltRow) -> Result<()> {
        let mut tx = pool.begin().await?;
        let existing = sqlx::query(
            r#"
            SELECT
                event_family_id,
                halted,
                reason,
                blocks_new_risk,
                metadata_snapshot_hash,
                last_seen_discovery_revision,
                set_at,
                updated_at
            FROM family_halt_settings
            WHERE event_family_id = $1
            FOR UPDATE
            "#,
        )
        .bind(&row.event_family_id)
        .fetch_optional(&mut *tx)
        .await?
        .map(map_family_halt_row)
        .transpose()?;

        if existing
            .as_ref()
            .is_some_and(|existing| same_halt_state(existing, row))
        {
            tx.commit().await?;
            return Ok(());
        }

        sqlx::query(
            r#"
            INSERT INTO family_halt_settings (
                event_family_id,
                halted,
                reason,
                blocks_new_risk,
                metadata_snapshot_hash,
                last_seen_discovery_revision,
                set_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (event_family_id) DO UPDATE
            SET halted = EXCLUDED.halted,
                reason = EXCLUDED.reason,
                blocks_new_risk = EXCLUDED.blocks_new_risk,
                metadata_snapshot_hash = EXCLUDED.metadata_snapshot_hash,
                last_seen_discovery_revision = EXCLUDED.last_seen_discovery_revision,
                set_at = EXCLUDED.set_at,
                updated_at = EXCLUDED.updated_at
            "#,
        )
        .bind(&row.event_family_id)
        .bind(row.halted)
        .bind(row.reason.as_deref())
        .bind(row.blocks_new_risk)
        .bind(row.metadata_snapshot_hash.as_deref())
        .bind(row.last_seen_discovery_revision)
        .bind(row.set_at)
        .bind(row.updated_at)
        .execute(&mut *tx)
        .await?;

        let entry = JournalEntryInput {
            stream: format!("neg_risk_family:{}", row.event_family_id),
            source_kind: row.source_kind.clone(),
            source_session_id: row.source_session_id.clone(),
            source_event_id: row.source_event_id.clone(),
            dedupe_key: format!(
                "family_halt:{}:{}:{}:{}",
                row.event_family_id,
                row.last_seen_discovery_revision,
                row.metadata_snapshot_hash.as_deref().unwrap_or("none"),
                row.source_event_id
            ),
            causal_parent_id: None,
            event_type: "family_halt".to_owned(),
            event_ts: row.event_ts,
            payload: json!({
                "event_family_id": row.event_family_id,
                "halted": row.halted,
                "reason": row.reason,
                "blocks_new_risk": row.blocks_new_risk,
                "metadata_snapshot_hash": row.metadata_snapshot_hash,
                "discovery_revision": row.last_seen_discovery_revision,
                "set_at": row.set_at.to_rfc3339(),
                "member_vector": member_vector_json(&row.member_vector),
            }),
        };
        append_journal_entry(&mut *tx, &entry).await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn list_halts(&self, pool: &PgPool) -> Result<Vec<FamilyHaltRow>> {
        let rows = sqlx::query(
            r#"
            SELECT
                event_family_id,
                halted,
                reason,
                blocks_new_risk,
                metadata_snapshot_hash,
                last_seen_discovery_revision,
                set_at,
                updated_at
            FROM family_halt_settings
            ORDER BY event_family_id
            "#,
        )
        .fetch_all(pool)
        .await?;

        rows.into_iter().map(map_family_halt_row).collect()
    }
}

pub async fn persist_discovery_snapshot(
    pool: &PgPool,
    snapshot: NegRiskDiscoverySnapshotInput,
) -> Result<JournalEntryRow> {
    let mut payload = serde_json::Map::from_iter([
        (
            "discovery_revision".to_owned(),
            Value::Number(snapshot.discovery_revision.into()),
        ),
        (
            "metadata_snapshot_hash".to_owned(),
            Value::String(snapshot.metadata_snapshot_hash.clone()),
        ),
        (
            "discovered_family_count".to_owned(),
            Value::Number((snapshot.family_ids.len() as i64).into()),
        ),
        (
            "family_ids".to_owned(),
            Value::Array(
                snapshot
                    .family_ids
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        ),
        (
            "captured_at".to_owned(),
            Value::String(snapshot.captured_at.to_rfc3339()),
        ),
    ]);

    if let Value::Object(extra_payload) = snapshot.extra_payload {
        for (key, value) in extra_payload {
            if is_reserved_discovery_snapshot_key(&key) {
                continue;
            }
            payload.insert(key, value);
        }
    }

    JournalRepo
        .append(
            pool,
            &JournalEntryInput {
                stream: "neg_risk_discovery".to_owned(),
                source_kind: snapshot.source_kind,
                source_session_id: snapshot.source_session_id,
                source_event_id: snapshot.source_event_id,
                dedupe_key: snapshot.dedupe_key,
                causal_parent_id: None,
                event_type: "neg_risk_discovery_snapshot".to_owned(),
                event_ts: snapshot.captured_at,
                payload: Value::Object(payload),
            },
        )
        .await
}

pub async fn reconcile_current_family_view(pool: &PgPool, discovery_revision: i64) -> Result<()> {
    let _ = discovery_revision;

    let latest_snapshot = latest_discovery_snapshot(pool)
        .await?
        .ok_or(PersistenceError::MissingDiscoverySnapshot { discovery_revision })?;
    let family_ids = latest_snapshot.family_ids;

    let mut tx = pool.begin().await?;

    sqlx::query(
        r#"
        DELETE FROM neg_risk_family_validations
        WHERE NOT (event_family_id = ANY($1))
        "#,
    )
    .bind(&family_ids)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        r#"
        DELETE FROM family_halt_settings
        WHERE NOT (event_family_id = ANY($1))
        "#,
    )
    .bind(&family_ids)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Default, Clone, Copy)]
pub struct JournalRepo;

impl JournalRepo {
    pub async fn append(
        &self,
        pool: &PgPool,
        entry: &JournalEntryInput,
    ) -> Result<JournalEntryRow> {
        append_journal_entry(pool, entry).await
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

const RUNTIME_PROGRESS_KEY: &str = "default";

#[derive(Debug, Default, Clone, Copy)]
pub struct RuntimeProgressRepo;

impl RuntimeProgressRepo {
    pub async fn record_progress(
        &self,
        pool: &PgPool,
        last_journal_seq: i64,
        last_state_version: i64,
        last_snapshot_id: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO runtime_apply_progress (
                progress_key,
                last_journal_seq,
                last_state_version,
                last_snapshot_id
            )
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (progress_key) DO UPDATE
            SET last_journal_seq = EXCLUDED.last_journal_seq,
                last_state_version = EXCLUDED.last_state_version,
                last_snapshot_id = EXCLUDED.last_snapshot_id,
                updated_at = NOW()
            "#,
        )
        .bind(RUNTIME_PROGRESS_KEY)
        .bind(last_journal_seq)
        .bind(last_state_version)
        .bind(last_snapshot_id)
        .execute(pool)
        .await?;

        Ok(())
    }

    pub async fn current(&self, pool: &PgPool) -> Result<Option<RuntimeProgressRow>> {
        let row = sqlx::query(
            r#"
            SELECT last_journal_seq, last_state_version, last_snapshot_id
            FROM runtime_apply_progress
            WHERE progress_key = $1
            "#,
        )
        .bind(RUNTIME_PROGRESS_KEY)
        .fetch_optional(pool)
        .await?;

        row.map(map_runtime_progress_row).transpose()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SnapshotPublicationRepo;

impl SnapshotPublicationRepo {
    pub async fn upsert(&self, pool: &PgPool, row: &SnapshotPublicationRow) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO snapshot_publications (
                snapshot_id,
                state_version,
                committed_journal_seq,
                fullset_ready,
                negrisk_ready,
                metadata,
                published_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (snapshot_id) DO UPDATE
            SET state_version = EXCLUDED.state_version,
                committed_journal_seq = EXCLUDED.committed_journal_seq,
                fullset_ready = EXCLUDED.fullset_ready,
                negrisk_ready = EXCLUDED.negrisk_ready,
                metadata = EXCLUDED.metadata,
                published_at = EXCLUDED.published_at
            "#,
        )
        .bind(&row.snapshot_id)
        .bind(row.state_version)
        .bind(row.committed_journal_seq)
        .bind(row.fullset_ready)
        .bind(row.negrisk_ready)
        .bind(&row.metadata)
        .bind(row.published_at)
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ExecutionAttemptRepo;

impl ExecutionAttemptRepo {
    pub async fn append(&self, pool: &PgPool, row: &ExecutionAttemptRow) -> Result<()> {
        let result = sqlx::query(
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
        .bind(&row.attempt_id)
        .bind(&row.plan_id)
        .bind(&row.snapshot_id)
        .bind(execution_mode_to_str(row.execution_mode))
        .bind(row.attempt_no)
        .bind(&row.idempotency_key)
        .execute(pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(err) if constraint_name(&err) == Some("execution_attempts_pkey") => {
                Err(PersistenceError::DuplicateExecutionAttempt {
                    attempt_id: row.attempt_id.clone(),
                })
            }
            Err(err) => Err(err.into()),
        }
    }

    pub async fn list_live_attempts(&self, pool: &PgPool) -> Result<Vec<ExecutionAttemptRow>> {
        let rows = sqlx::query(
            r#"
            SELECT
                attempt_id,
                plan_id,
                snapshot_id,
                execution_mode,
                attempt_no,
                idempotency_key
            FROM execution_attempts
            WHERE execution_mode = $1
            ORDER BY created_at, attempt_id
            "#,
        )
        .bind(execution_mode_to_str(domain::ExecutionMode::Live))
        .fetch_all(pool)
        .await?;

        rows.into_iter().map(map_execution_attempt_row).collect()
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PendingReconcileRepo;

impl PendingReconcileRepo {
    pub async fn append(
        &self,
        pool: &PgPool,
        row: &PendingReconcileRow,
        payload: &Value,
    ) -> Result<()> {
        let result = sqlx::query(
            r#"
            INSERT INTO pending_reconcile_items (
                pending_ref,
                scope_kind,
                scope_id,
                reason,
                payload
            )
            VALUES ($1, $2, $3, $4, $5)
            "#,
        )
        .bind(&row.pending_ref)
        .bind(&row.scope_kind)
        .bind(&row.scope_id)
        .bind(&row.reason)
        .bind(payload)
        .execute(pool)
        .await;

        match result {
            Ok(_) => Ok(()),
            Err(err) if constraint_name(&err) == Some("pending_reconcile_items_pkey") => {
                Err(PersistenceError::DuplicatePendingReconcile {
                    pending_ref: row.pending_ref.clone(),
                })
            }
            Err(err) => Err(err.into()),
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ShadowArtifactRepo;

impl ShadowArtifactRepo {
    pub async fn append(&self, pool: &PgPool, row: ShadowExecutionArtifactRow) -> Result<()> {
        let result = sqlx::query(
            r#"
            INSERT INTO shadow_execution_artifacts (attempt_id, stream, payload)
            SELECT $1, $2, $3
            FROM execution_attempts
            WHERE attempt_id = $1 AND execution_mode = $4
            "#,
        )
        .bind(&row.attempt_id)
        .bind(&row.stream)
        .bind(&row.payload)
        .bind(execution_mode_to_str(domain::ExecutionMode::Shadow))
        .execute(pool)
        .await?;

        if result.rows_affected() == 1 {
            Ok(())
        } else {
            Err(PersistenceError::ShadowArtifactRequiresShadowAttempt {
                attempt_id: row.attempt_id,
            })
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct LiveArtifactRepo;

impl LiveArtifactRepo {
    pub async fn append(&self, pool: &PgPool, row: LiveExecutionArtifactRow) -> Result<()> {
        let result = sqlx::query(
            r#"
            INSERT INTO live_execution_artifacts (attempt_id, stream, payload)
            SELECT $1, $2, $3
            FROM execution_attempts
            WHERE attempt_id = $1 AND execution_mode = $4
            ON CONFLICT (attempt_id, stream) DO NOTHING
            "#,
        )
        .bind(&row.attempt_id)
        .bind(&row.stream)
        .bind(&row.payload)
        .bind(execution_mode_to_str(domain::ExecutionMode::Live))
        .execute(pool)
        .await?;

        if result.rows_affected() == 1 {
            return Ok(());
        }

        let existing_payload = sqlx::query_scalar::<_, Value>(
            r#"
            SELECT payload
            FROM live_execution_artifacts
            WHERE attempt_id = $1 AND stream = $2
            "#,
        )
        .bind(&row.attempt_id)
        .bind(&row.stream)
        .fetch_optional(pool)
        .await?;

        match existing_payload {
            Some(payload) if payload == row.payload => Ok(()),
            Some(_) => Err(PersistenceError::ConflictingLiveArtifactPayload {
                attempt_id: row.attempt_id,
                stream: row.stream,
            }),
            None => Err(PersistenceError::LiveArtifactRequiresLiveAttempt {
                attempt_id: row.attempt_id,
            }),
        }
    }

    pub async fn list_for_attempt(
        &self,
        pool: &PgPool,
        attempt_id: &str,
    ) -> Result<Vec<LiveExecutionArtifactRow>> {
        let rows = sqlx::query(
            r#"
            SELECT attempt_id, stream, payload
            FROM live_execution_artifacts
            WHERE attempt_id = $1
            ORDER BY stream
            "#,
        )
        .bind(attempt_id)
        .fetch_all(pool)
        .await?;

        rows.into_iter()
            .map(map_live_execution_artifact_row)
            .collect()
    }

    pub async fn list_for_attempts(
        &self,
        pool: &PgPool,
        attempt_ids: &[String],
    ) -> Result<BTreeMap<String, Vec<LiveExecutionArtifactRow>>> {
        if attempt_ids.is_empty() {
            return Ok(BTreeMap::new());
        }

        let rows = sqlx::query(
            r#"
            SELECT attempt_id, stream, payload
            FROM live_execution_artifacts
            WHERE attempt_id = ANY($1)
            ORDER BY attempt_id, stream
            "#,
        )
        .bind(attempt_ids)
        .fetch_all(pool)
        .await?;

        let mut artifacts = BTreeMap::<String, Vec<LiveExecutionArtifactRow>>::new();
        for row in rows {
            let artifact = map_live_execution_artifact_row(row)?;
            artifacts
                .entry(artifact.attempt_id.clone())
                .or_default()
                .push(artifact);
        }

        Ok(artifacts)
    }
}

fn same_validation_state(
    existing: &NegRiskFamilyValidationRow,
    candidate: &NegRiskFamilyValidationRow,
) -> bool {
    existing.event_family_id == candidate.event_family_id
        && existing.validation_status == candidate.validation_status
        && existing.exclusion_reason == candidate.exclusion_reason
        && existing.metadata_snapshot_hash == candidate.metadata_snapshot_hash
        && existing.last_seen_discovery_revision == candidate.last_seen_discovery_revision
        && existing.member_count == candidate.member_count
        && existing.first_seen_at == candidate.first_seen_at
        && existing.last_seen_at == candidate.last_seen_at
        && existing.validated_at == candidate.validated_at
}

fn same_halt_state(existing: &FamilyHaltRow, candidate: &FamilyHaltRow) -> bool {
    existing.event_family_id == candidate.event_family_id
        && existing.halted == candidate.halted
        && existing.reason == candidate.reason
        && existing.blocks_new_risk == candidate.blocks_new_risk
        && existing.metadata_snapshot_hash == candidate.metadata_snapshot_hash
        && existing.last_seen_discovery_revision == candidate.last_seen_discovery_revision
        && existing.set_at == candidate.set_at
}

fn member_vector_json(member_vector: &[NegRiskFamilyMemberRow]) -> Value {
    Value::Array(
        member_vector
            .iter()
            .map(|member| {
                json!({
                    "condition_id": member.condition_id,
                    "token_id": member.token_id,
                    "outcome_label": member.outcome_label,
                    "is_placeholder": member.is_placeholder,
                    "is_other": member.is_other,
                    "neg_risk_variant": member.neg_risk_variant,
                })
            })
            .collect(),
    )
}

fn is_reserved_discovery_snapshot_key(key: &str) -> bool {
    matches!(
        key,
        "discovery_revision"
            | "metadata_snapshot_hash"
            | "discovered_family_count"
            | "family_ids"
            | "captured_at"
    )
}

struct LatestDiscoverySnapshot {
    family_ids: Vec<String>,
}

async fn latest_discovery_snapshot(pool: &PgPool) -> Result<Option<LatestDiscoverySnapshot>> {
    let payload = sqlx::query_scalar(
        r#"
        SELECT payload
        FROM event_journal
        WHERE event_type = 'neg_risk_discovery_snapshot'
        ORDER BY journal_seq DESC
        LIMIT 1
        "#,
    )
    .fetch_optional(pool)
    .await?;

    payload
        .map(|payload: Value| {
            payload
                .get("discovery_revision")
                .and_then(Value::as_i64)
                .ok_or_else(|| {
                    PersistenceError::invalid_value(
                        "neg_risk_discovery_snapshot.discovery_revision",
                        payload.to_string(),
                    )
                })?;

            let family_ids = payload
                .get("family_ids")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    PersistenceError::invalid_value(
                        "neg_risk_discovery_snapshot.family_ids",
                        payload.to_string(),
                    )
                })?
                .iter()
                .map(|item| {
                    item.as_str().map(str::to_owned).ok_or_else(|| {
                        PersistenceError::invalid_value(
                            "neg_risk_discovery_snapshot.family_ids",
                            item.to_string(),
                        )
                    })
                })
                .collect::<Result<Vec<_>>>()?;

            Ok(LatestDiscoverySnapshot { family_ids })
        })
        .transpose()
}

async fn append_journal_entry<'e, E>(
    executor: E,
    entry: &JournalEntryInput,
) -> Result<JournalEntryRow>
where
    E: Executor<'e, Database = Postgres>,
{
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
    .fetch_one(executor)
    .await?;

    map_journal_entry_row(row)
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

fn map_runtime_progress_row(row: PgRow) -> Result<RuntimeProgressRow> {
    Ok(RuntimeProgressRow {
        last_journal_seq: row.try_get("last_journal_seq")?,
        last_state_version: row.try_get("last_state_version")?,
        last_snapshot_id: row.try_get("last_snapshot_id")?,
    })
}

fn map_execution_attempt_row(row: PgRow) -> Result<ExecutionAttemptRow> {
    Ok(ExecutionAttemptRow {
        attempt_id: row.try_get("attempt_id")?,
        plan_id: row.try_get("plan_id")?,
        snapshot_id: row.try_get("snapshot_id")?,
        execution_mode: execution_mode_from_str(&row.try_get::<String, _>("execution_mode")?)?,
        attempt_no: row.try_get("attempt_no")?,
        idempotency_key: row.try_get("idempotency_key")?,
    })
}

fn map_live_execution_artifact_row(row: PgRow) -> Result<LiveExecutionArtifactRow> {
    Ok(LiveExecutionArtifactRow {
        attempt_id: row.try_get("attempt_id")?,
        stream: row.try_get("stream")?,
        payload: row.try_get("payload")?,
    })
}

fn map_neg_risk_family_validation_row(row: PgRow) -> Result<NegRiskFamilyValidationRow> {
    Ok(NegRiskFamilyValidationRow {
        event_family_id: row.try_get("event_family_id")?,
        validation_status: row.try_get("validation_status")?,
        exclusion_reason: row.try_get("exclusion_reason")?,
        metadata_snapshot_hash: row.try_get("metadata_snapshot_hash")?,
        last_seen_discovery_revision: row.try_get("last_seen_discovery_revision")?,
        member_count: row.try_get("member_count")?,
        first_seen_at: row.try_get("first_seen_at")?,
        last_seen_at: row.try_get("last_seen_at")?,
        validated_at: row.try_get("validated_at")?,
        updated_at: row.try_get("updated_at")?,
        member_vector: Vec::new(),
        source_kind: String::new(),
        source_session_id: String::new(),
        source_event_id: String::new(),
        event_ts: row.try_get("validated_at")?,
    })
}

fn map_family_halt_row(row: PgRow) -> Result<FamilyHaltRow> {
    Ok(FamilyHaltRow {
        event_family_id: row.try_get("event_family_id")?,
        halted: row.try_get("halted")?,
        reason: row.try_get("reason")?,
        blocks_new_risk: row.try_get("blocks_new_risk")?,
        metadata_snapshot_hash: row.try_get("metadata_snapshot_hash")?,
        last_seen_discovery_revision: row.try_get("last_seen_discovery_revision")?,
        set_at: row.try_get("set_at")?,
        updated_at: row.try_get("updated_at")?,
        member_vector: Vec::new(),
        source_kind: String::new(),
        source_session_id: String::new(),
        source_event_id: String::new(),
        event_ts: row.try_get("set_at")?,
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
