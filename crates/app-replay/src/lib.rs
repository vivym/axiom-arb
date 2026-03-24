use std::{error::Error as StdError, fmt};

use journal::{replay_entries, JournalEntry, SourceKind};
use persistence::{connect_pool_from_env, models::JournalEntryRow, JournalRepo, PersistenceError};

pub trait ReplayConsumer {
    type Error;

    fn consume(&mut self, entry: JournalEntry) -> Result<(), Self::Error>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayRange {
    pub after_seq: i64,
    pub limit: Option<i64>,
}

impl ReplayRange {
    pub const DEFAULT_LIMIT: i64 = 1_000;

    pub const fn new(after_seq: i64, limit: Option<i64>) -> Self {
        Self { after_seq, limit }
    }

    pub const fn effective_limit(self) -> i64 {
        match self.limit {
            Some(limit) => limit,
            None => Self::DEFAULT_LIMIT,
        }
    }
}

pub trait ReplaySource {
    type Error;

    fn list(&self, range: ReplayRange) -> Result<Vec<JournalEntry>, Self::Error>;
}

pub fn replay_journal<I, C>(entries: I, consumer: &mut C) -> Result<(), C::Error>
where
    I: IntoIterator<Item = JournalEntry>,
    C: ReplayConsumer,
{
    for entry in replay_entries(entries) {
        consumer.consume(entry)?;
    }

    Ok(())
}

pub fn replay_from_source<S, C>(
    source: &S,
    range: ReplayRange,
    consumer: &mut C,
) -> Result<(), ReplayExecutionError<S::Error, C::Error>>
where
    S: ReplaySource,
    C: ReplayConsumer,
{
    let entries = source.list(range).map_err(ReplayExecutionError::Source)?;
    replay_journal(entries, consumer).map_err(ReplayExecutionError::Consumer)
}

pub fn parse_args<I, S>(args: I) -> Result<ReplayRange, ReplayArgsError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into);
    let _program = args.next();
    let mut after_seq = None;
    let mut limit = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--from-seq" => {
                let value = args
                    .next()
                    .ok_or(ReplayArgsError::MissingValue("--from-seq"))?;
                after_seq = Some(parse_i64("--from-seq", &value)?);
            }
            "--limit" => {
                let value = args
                    .next()
                    .ok_or(ReplayArgsError::MissingValue("--limit"))?;
                limit = Some(parse_i64("--limit", &value)?);
            }
            other => return Err(ReplayArgsError::UnknownArg(other.to_owned())),
        }
    }

    Ok(ReplayRange::new(
        after_seq.ok_or(ReplayArgsError::MissingRequired("--from-seq"))?,
        limit,
    ))
}

pub async fn replay_event_journal_from_env<C>(
    range: ReplayRange,
    consumer: &mut C,
) -> Result<(), ReplayRunError<C::Error>>
where
    C: ReplayConsumer,
{
    let pool = connect_pool_from_env()
        .await
        .map_err(ReplayRunError::Persistence)?;
    let rows = JournalRepo
        .list_after(&pool, range.after_seq, range.effective_limit())
        .await
        .map_err(ReplayRunError::Persistence)?;
    let entries = rows
        .into_iter()
        .map(|row| map_row(row).map_err(ReplayRunError::InvalidSourceKind))
        .collect::<Result<Vec<_>, _>>()?;

    replay_journal(entries, consumer).map_err(ReplayRunError::Consumer)
}

#[derive(Debug, Default)]
pub struct NoopReplayConsumer;

impl ReplayConsumer for NoopReplayConsumer {
    type Error = std::convert::Infallible;

    fn consume(&mut self, _entry: JournalEntry) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayArgsError {
    MissingRequired(&'static str),
    MissingValue(&'static str),
    InvalidNumber { flag: &'static str, value: String },
    UnknownArg(String),
}

impl fmt::Display for ReplayArgsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingRequired(flag) => write!(f, "missing required argument {flag}"),
            Self::MissingValue(flag) => write!(f, "missing value for {flag}"),
            Self::InvalidNumber { flag, value } => {
                write!(f, "invalid integer for {flag}: {value}")
            }
            Self::UnknownArg(arg) => write!(f, "unknown argument {arg}"),
        }
    }
}

impl StdError for ReplayArgsError {}

#[derive(Debug)]
pub enum ReplayExecutionError<SE, CE> {
    Source(SE),
    Consumer(CE),
}

impl<SE, CE> fmt::Display for ReplayExecutionError<SE, CE>
where
    SE: fmt::Display,
    CE: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Source(error) => write!(f, "{error}"),
            Self::Consumer(error) => write!(f, "{error}"),
        }
    }
}

impl<SE, CE> StdError for ReplayExecutionError<SE, CE>
where
    SE: StdError + 'static,
    CE: StdError + 'static,
{
}

#[derive(Debug)]
pub enum ReplayRunError<E> {
    Persistence(PersistenceError),
    InvalidSourceKind(String),
    Consumer(E),
}

impl<E> fmt::Display for ReplayRunError<E>
where
    E: fmt::Display,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Persistence(error) => write!(f, "{error}"),
            Self::InvalidSourceKind(value) => write!(f, "invalid journal source kind: {value}"),
            Self::Consumer(error) => write!(f, "{error}"),
        }
    }
}

impl<E> StdError for ReplayRunError<E> where E: StdError + 'static {}

fn parse_i64(flag: &'static str, value: &str) -> Result<i64, ReplayArgsError> {
    value
        .parse::<i64>()
        .map_err(|_| ReplayArgsError::InvalidNumber {
            flag,
            value: value.to_owned(),
        })
}

fn map_row(row: JournalEntryRow) -> Result<JournalEntry, String> {
    Ok(JournalEntry {
        journal_seq: row.journal_seq,
        stream: row.stream,
        source_kind: parse_source_kind(&row.source_kind)?,
        source_session_id: row.source_session_id,
        source_event_id: row.source_event_id,
        dedupe_key: row.dedupe_key,
        causal_parent_id: row.causal_parent_id,
        event_type: row.event_type,
        event_ts: row.event_ts,
        payload: row.payload,
        ingested_at: row.ingested_at,
    })
}

fn parse_source_kind(value: &str) -> Result<SourceKind, String> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "ws_market" | "wsmarket" => Ok(SourceKind::WsMarket),
        "ws_user" | "wsuser" => Ok(SourceKind::WsUser),
        "rest_heartbeat" | "restheartbeat" => Ok(SourceKind::RestHeartbeat),
        "internal" | "test" => Ok(SourceKind::Internal),
        _ => Err(value.to_owned()),
    }
}
