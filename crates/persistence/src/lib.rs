use std::{error::Error as StdError, fmt};

use domain::IdentifierMapError;
use sqlx::{postgres::PgPoolOptions, PgPool};

pub mod instrumentation;
pub mod models;
pub mod repos;

pub use instrumentation::NegRiskPersistenceInstrumentation;
pub use models::{
    AdoptableTargetRevisionRow, CandidateAdoptionProvenanceRow, CandidateTargetSetRow,
    OperatorTargetAdoptionHistoryRow, RunSessionRow, RunSessionState, RuntimeProgressRow,
    StoredOrder,
};
pub use repos::{
    append_shadow_execution_batch, persist_discovery_snapshot, reconcile_current_family_view,
    ApprovalRepo, CandidateAdoptionRepo, CandidateArtifactRepo, ExecutionAttemptRepo,
    IdentifierRepo, InventoryRepo, JournalRepo, LiveArtifactRepo, LiveSubmissionRepo,
    NegRiskFamilyRepo, OperatorTargetAdoptionHistoryRepo, OrderRepo, PendingReconcileRepo,
    ResolutionRepo, RuntimeProgressRepo, ShadowArtifactRepo, SnapshotPublicationRepo,
};

pub type Result<T> = std::result::Result<T, PersistenceError>;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

#[derive(Debug)]
pub enum PersistenceError {
    MissingDatabaseUrl,
    Sqlx(sqlx::Error),
    Migration(sqlx::migrate::MigrateError),
    InvalidValue {
        kind: &'static str,
        value: String,
    },
    IncompleteSignedOrderIdentity,
    DuplicateSignedOrderHash {
        signed_order_hash: String,
        existing_order_id: String,
        attempted_order_id: String,
    },
    IdentifierConflict(IdentifierMapError),
    InvalidOrderIdentifierLinkage {
        market_id: String,
        condition_id: String,
        token_id: String,
    },
    ImmutableOrderConflict {
        order_id: String,
    },
    MissingDiscoverySnapshot {
        discovery_revision: i64,
    },
    DuplicateExecutionAttempt {
        attempt_id: String,
    },
    ConflictingCandidateTargetSet {
        candidate_revision: String,
    },
    ConflictingAdoptableTargetRevision {
        adoptable_revision: String,
    },
    ConflictingCandidateAdoptionProvenance {
        operator_target_revision: String,
    },
    MissingCandidateAdoptionLink {
        operator_target_revision: String,
    },
    DuplicatePendingReconcile {
        pending_ref: String,
    },
    ConflictingLiveSubmissionRecord {
        submission_ref: String,
    },
    LiveSubmissionRequiresLiveAttempt {
        submission_ref: String,
        attempt_id: String,
    },
    ShadowArtifactRequiresShadowAttempt {
        attempt_id: String,
    },
    LiveArtifactRequiresLiveAttempt {
        attempt_id: String,
    },
    ConflictingLiveArtifactPayload {
        attempt_id: String,
        stream: String,
    },
}

impl PersistenceError {
    pub fn invalid_value(kind: &'static str, value: impl Into<String>) -> Self {
        Self::InvalidValue {
            kind,
            value: value.into(),
        }
    }
}

impl fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingDatabaseUrl => write!(f, "DATABASE_URL is not set"),
            Self::Sqlx(err) => write!(f, "{err}"),
            Self::Migration(err) => write!(f, "{err}"),
            Self::InvalidValue { kind, value } => {
                write!(f, "invalid {kind} value: {value}")
            }
            Self::IncompleteSignedOrderIdentity => {
                write!(
                    f,
                    "signed order identity must include hash, salt, nonce, and signature"
                )
            }
            Self::DuplicateSignedOrderHash {
                signed_order_hash,
                existing_order_id,
                attempted_order_id,
            } => write!(
                f,
                "signed order hash {signed_order_hash} already belongs to order {existing_order_id}; attempted order {attempted_order_id}"
            ),
            Self::IdentifierConflict(err) => write!(f, "{err:?}"),
            Self::InvalidOrderIdentifierLinkage {
                market_id,
                condition_id,
                token_id,
            } => write!(
                f,
                "order identifiers do not resolve to a single mapping: market={market_id} condition={condition_id} token={token_id}"
            ),
            Self::ImmutableOrderConflict { order_id } => {
                write!(f, "order {order_id} already exists with immutable submitted fields")
            }
            Self::MissingDiscoverySnapshot { discovery_revision } => write!(
                f,
                "missing neg-risk discovery snapshot for revision {discovery_revision}"
            ),
            Self::DuplicateExecutionAttempt { attempt_id } => {
                write!(f, "execution attempt {attempt_id} already exists")
            }
            Self::ConflictingCandidateTargetSet { candidate_revision } => write!(
                f,
                "candidate target set {candidate_revision} already exists with different fields"
            ),
            Self::ConflictingAdoptableTargetRevision { adoptable_revision } => write!(
                f,
                "adoptable target revision {adoptable_revision} already exists with different fields"
            ),
            Self::ConflictingCandidateAdoptionProvenance {
                operator_target_revision,
            } => write!(
                f,
                "candidate adoption provenance {operator_target_revision} already exists with different linkage"
            ),
            Self::MissingCandidateAdoptionLink {
                operator_target_revision,
            } => write!(
                f,
                "operator target revision {operator_target_revision} could not be linked back to a candidate adoption provenance chain"
            ),
            Self::DuplicatePendingReconcile { pending_ref } => {
                write!(f, "pending reconcile item {pending_ref} already exists")
            }
            Self::ConflictingLiveSubmissionRecord { submission_ref } => {
                write!(
                    f,
                    "live submission record {submission_ref} already exists with a different payload"
                )
            }
            Self::LiveSubmissionRequiresLiveAttempt {
                submission_ref,
                attempt_id,
            } => write!(
                f,
                "live submission record {submission_ref} requires a live execution attempt for attempt_id {attempt_id}"
            ),
            Self::ShadowArtifactRequiresShadowAttempt { attempt_id } => write!(
                f,
                "shadow artifact attempt {attempt_id} must reference an existing shadow execution attempt"
            ),
            Self::LiveArtifactRequiresLiveAttempt { attempt_id } => write!(
                f,
                "live artifact attempt {attempt_id} must reference an existing live execution attempt"
            ),
            Self::ConflictingLiveArtifactPayload { attempt_id, stream } => write!(
                f,
                "live artifact ({attempt_id}, {stream}) already exists with a different payload"
            ),
        }
    }
}

impl StdError for PersistenceError {}

impl From<sqlx::Error> for PersistenceError {
    fn from(value: sqlx::Error) -> Self {
        Self::Sqlx(value)
    }
}

impl From<sqlx::migrate::MigrateError> for PersistenceError {
    fn from(value: sqlx::migrate::MigrateError) -> Self {
        Self::Migration(value)
    }
}

impl From<IdentifierMapError> for PersistenceError {
    fn from(value: IdentifierMapError) -> Self {
        Self::IdentifierConflict(value)
    }
}

pub async fn connect_pool(database_url: &str) -> Result<PgPool> {
    PgPoolOptions::new()
        .max_connections(8)
        .connect(database_url)
        .await
        .map_err(Into::into)
}

pub async fn connect_pool_from_env() -> Result<PgPool> {
    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| PersistenceError::MissingDatabaseUrl)?;

    connect_pool(&database_url).await
}

pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    MIGRATOR.run(pool).await?;
    Ok(())
}
