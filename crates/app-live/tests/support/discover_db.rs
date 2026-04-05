#![allow(dead_code)]

use std::{
    env,
    sync::atomic::{AtomicU64, Ordering},
};

use persistence::run_migrations;
use sqlx::{postgres::PgPoolOptions, PgPool};

use super::cli::default_test_database_url;

static NEXT_SCHEMA_ID: AtomicU64 = AtomicU64::new(1);

pub struct TestDatabase {
    runtime: tokio::runtime::Runtime,
    admin_pool: PgPool,
    pool: PgPool,
    schema: String,
    database_url: String,
}

impl TestDatabase {
    pub fn new() -> Self {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build");

        let (admin_pool, pool, schema, database_url) = runtime.block_on(async {
            let admin_database_url =
                env::var("DATABASE_URL").unwrap_or_else(|_| default_test_database_url().to_owned());
            let admin_pool = PgPoolOptions::new()
                .max_connections(8)
                .connect(&admin_database_url)
                .await
                .expect("test database should connect");
            let schema = format!(
                "app_live_discover_{}_{}",
                std::process::id(),
                NEXT_SCHEMA_ID.fetch_add(1, Ordering::Relaxed)
            );
            sqlx::query(&format!(r#"CREATE SCHEMA "{schema}""#))
                .execute(&admin_pool)
                .await
                .expect("test schema should create");

            let database_url = schema_scoped_database_url(&admin_database_url, &schema);
            let pool = PgPoolOptions::new()
                .max_connections(8)
                .connect(&database_url)
                .await
                .expect("schema-scoped test pool should connect");
            run_migrations(&pool)
                .await
                .expect("test migrations should run");

            (admin_pool, pool, schema, database_url)
        });

        Self {
            runtime,
            admin_pool,
            pool,
            schema,
            database_url,
        }
    }

    pub fn database_url(&self) -> &str {
        &self.database_url
    }

    pub fn has_candidate_rows(&self) -> bool {
        self.count_rows("candidate_target_sets") > 0
    }

    pub fn has_adoptable_rows(&self) -> bool {
        self.count_rows("adoptable_target_revisions") > 0
    }

    pub fn has_candidate_provenance_rows(&self) -> bool {
        self.count_rows("candidate_adoption_provenance") > 0
    }

    pub fn cleanup(self) {
        self.runtime.block_on(async {
            self.pool.close().await;
            let drop_schema = format!(
                r#"DROP SCHEMA IF EXISTS "{schema}" CASCADE"#,
                schema = self.schema
            );
            let _ = sqlx::query(&drop_schema).execute(&self.admin_pool).await;
            self.admin_pool.close().await;
        });
    }

    fn count_rows(&self, table: &str) -> i64 {
        self.runtime.block_on(async {
            sqlx::query_scalar::<_, i64>(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(&self.pool)
                .await
                .expect("row count should load")
        })
    }
}

fn schema_scoped_database_url(database_url: &str, schema: &str) -> String {
    let options = format!("options=-csearch_path%3D{schema}");
    if database_url.contains('?') {
        format!("{database_url}&{options}")
    } else {
        format!("{database_url}?{options}")
    }
}
