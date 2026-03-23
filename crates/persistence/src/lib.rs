use std::{
    error::Error as StdError,
    fmt,
    path::{Path, PathBuf},
};

use sqlx::{postgres::PgPoolOptions, PgPool};

pub mod models;
pub mod repos;

pub use repos::{
    ApprovalRepo, IdentifierRepo, InventoryRepo, JournalRepo, OrderRepo, ResolutionRepo,
};

pub type Result<T> = std::result::Result<T, PersistenceError>;

#[derive(Debug)]
pub enum PersistenceError {
    MissingDatabaseUrl,
    Sqlx(sqlx::Error),
    Migration(sqlx::migrate::MigrateError),
    InvalidValue { kind: &'static str, value: String },
    IncompleteSignedOrderIdentity,
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
    let migrator = sqlx::migrate::Migrator::new(migrations_dir().as_path()).await?;
    migrator.run(pool).await?;
    Ok(())
}

pub fn migrations_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../migrations")
}
