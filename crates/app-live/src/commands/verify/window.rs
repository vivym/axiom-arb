use chrono::{DateTime, Duration, Utc};
use std::{error::Error, fmt};

use super::model::VerifyScenario;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyWindowSelection {
    LatestForScenario,
    ExplicitAttemptId(String),
    ExplicitSeqRange { from_seq: i64, to_seq: Option<i64> },
    ExplicitSince(DateTime<Utc>),
}

impl VerifyWindowSelection {
    pub fn from_args(
        from_seq: Option<i64>,
        to_seq: Option<i64>,
        attempt_id: Option<String>,
        since: Option<String>,
        _scenario: VerifyScenario,
    ) -> Result<Self, VerifyWindowSelectionError> {
        let has_seq_range = from_seq.is_some() || to_seq.is_some();
        let has_since = since.is_some();

        if attempt_id.is_some() && (has_seq_range || has_since) {
            return Err(VerifyWindowSelectionError::new(
                "attempt-id cannot be combined with seq range or since",
            ));
        }

        if has_seq_range && has_since {
            return Err(VerifyWindowSelectionError::new(
                "seq range cannot be combined with since",
            ));
        }

        if let Some(attempt_id) = attempt_id {
            return Ok(Self::ExplicitAttemptId(attempt_id));
        }

        if let Some(from_seq) = from_seq {
            return Ok(Self::ExplicitSeqRange { from_seq, to_seq });
        }

        if let Some(since) = since {
            let duration = parse_since(&since)?;
            return Ok(Self::ExplicitSince(Utc::now() - duration));
        }

        if to_seq.is_some() {
            return Err(VerifyWindowSelectionError::new(
                "to-seq requires from-seq",
            ));
        }

        Ok(Self::LatestForScenario)
    }

    pub fn is_historical_explicit(&self) -> bool {
        matches!(
            self,
            Self::ExplicitAttemptId(_) | Self::ExplicitSeqRange { .. } | Self::ExplicitSince(_)
        )
    }
}

pub fn parse_since(value: &str) -> Result<Duration, VerifyWindowSelectionError> {
    let value = value.trim();
    let (amount, unit) = value
        .chars()
        .last()
        .map(|unit| (&value[..value.len().saturating_sub(unit.len_utf8())], unit))
        .ok_or_else(|| VerifyWindowSelectionError::new("since value is empty"))?;

    let amount = amount
        .parse::<i64>()
        .map_err(|_| VerifyWindowSelectionError::new(format!("invalid since value: {value}")))?;

    if amount < 0 {
        return Err(VerifyWindowSelectionError::new(
            "since value must not be negative",
        ));
    }

    let duration = match unit {
        's' => Duration::seconds(amount),
        'm' => Duration::minutes(amount),
        'h' => Duration::hours(amount),
        'd' => Duration::days(amount),
        _ => {
            return Err(VerifyWindowSelectionError::new(format!(
                "unsupported since suffix: {unit}"
            )))
        }
    };

    Ok(duration)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyWindowSelectionError {
    message: String,
}

impl VerifyWindowSelectionError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for VerifyWindowSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for VerifyWindowSelectionError {}

#[cfg(test)]
mod tests {
    use super::{parse_since, VerifyScenario, VerifyWindowSelection};

    #[test]
    fn parses_since_shorthand_values() {
        assert_eq!(parse_since("10m").unwrap().num_minutes(), 10);
        assert_eq!(parse_since("2h").unwrap().num_hours(), 2);
    }

    #[test]
    fn explicit_history_is_marked_historical() {
        let selection = VerifyWindowSelection::from_args(
            Some(100),
            Some(200),
            None,
            None,
            VerifyScenario::Live,
        )
        .unwrap();
        assert!(selection.is_historical_explicit());
    }

    #[test]
    fn attempt_id_conflicts_with_seq_range() {
        let error = VerifyWindowSelection::from_args(
            Some(100),
            None,
            Some("attempt-1".to_owned()),
            None,
            VerifyScenario::Live,
        )
        .unwrap_err();
        assert!(error.to_string().contains("cannot be combined"));
    }
}
