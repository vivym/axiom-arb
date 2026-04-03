use chrono::{DateTime, Duration, Utc};
use std::{error::Error, fmt};

use super::model::VerifyScenario;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyWindowSelection {
    LatestForScenario(VerifyScenario),
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
            return Err(VerifyWindowSelectionError::AttemptIdCannotBeCombinedWithWindow);
        }

        if has_seq_range && has_since {
            return Err(VerifyWindowSelectionError::SeqRangeCannotBeCombinedWithSince);
        }

        if let Some(attempt_id) = attempt_id {
            return Ok(Self::ExplicitAttemptId(attempt_id));
        }

        if let Some(from_seq) = from_seq {
            if let Some(to_seq) = to_seq {
                if to_seq < from_seq {
                    return Err(VerifyWindowSelectionError::descending_seq_range(
                        from_seq, to_seq,
                    ));
                }
            }
            return Ok(Self::ExplicitSeqRange { from_seq, to_seq });
        }

        if let Some(since) = since {
            let duration = parse_since(&since)?;
            let selected_at = Utc::now()
                .checked_sub_signed(duration)
                .ok_or_else(|| VerifyWindowSelectionError::since_window_overflows_now(since))?;
            return Ok(Self::ExplicitSince(selected_at));
        }

        if to_seq.is_some() {
            return Err(VerifyWindowSelectionError::ToSeqRequiresFromSeq);
        }

        Ok(Self::LatestForScenario(_scenario))
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
        's' => Duration::try_seconds(amount),
        'm' => Duration::try_minutes(amount),
        'h' => Duration::try_hours(amount),
        'd' => Duration::try_days(amount),
        _ => {
            return Err(VerifyWindowSelectionError::unsupported_since_suffix(unit));
        }
    }
    .ok_or_else(|| VerifyWindowSelectionError::since_value_overflow(value.to_owned(), unit))?;

    Ok(duration)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyWindowSelectionError {
    AttemptIdCannotBeCombinedWithWindow,
    SeqRangeCannotBeCombinedWithSince,
    ToSeqRequiresFromSeq,
    DescendingSeqRange { from_seq: i64, to_seq: i64 },
    InvalidSinceValue(String),
    UnsupportedSinceSuffix(char),
    SinceValueOverflow { value: String, unit: char },
    SinceWindowOverflowsNow(String),
}

impl VerifyWindowSelectionError {
    fn new(message: impl Into<String>) -> Self {
        Self::InvalidSinceValue(message.into())
    }

    fn descending_seq_range(from_seq: i64, to_seq: i64) -> Self {
        Self::DescendingSeqRange { from_seq, to_seq }
    }

    fn unsupported_since_suffix(unit: char) -> Self {
        Self::UnsupportedSinceSuffix(unit)
    }

    fn since_value_overflow(value: String, unit: char) -> Self {
        Self::SinceValueOverflow { value, unit }
    }

    fn since_window_overflows_now(value: String) -> Self {
        Self::SinceWindowOverflowsNow(value)
    }
}

impl fmt::Display for VerifyWindowSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AttemptIdCannotBeCombinedWithWindow => {
                f.write_str("attempt-id cannot be combined with seq range or since")
            }
            Self::SeqRangeCannotBeCombinedWithSince => {
                f.write_str("seq range cannot be combined with since")
            }
            Self::ToSeqRequiresFromSeq => f.write_str("to-seq requires from-seq"),
            Self::DescendingSeqRange { from_seq, to_seq } => write!(
                f,
                "descending seq range is invalid: from-seq {from_seq} must be <= to-seq {to_seq}"
            ),
            Self::InvalidSinceValue(value) => f.write_str(value),
            Self::UnsupportedSinceSuffix(unit) => {
                write!(f, "unsupported since suffix: {unit}")
            }
            Self::SinceValueOverflow { value, unit } => {
                write!(f, "since value overflows chrono duration: {value}{unit}")
            }
            Self::SinceWindowOverflowsNow(value) => {
                write!(f, "since window overflows current time: {value}")
            }
        }
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
    fn descending_seq_range_is_rejected() {
        let error = VerifyWindowSelection::from_args(
            Some(200),
            Some(100),
            None,
            None,
            VerifyScenario::Live,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            super::VerifyWindowSelectionError::DescendingSeqRange {
                from_seq: 200,
                to_seq: 100
            }
        ));
    }

    #[test]
    fn to_seq_without_from_seq_is_rejected() {
        let error =
            VerifyWindowSelection::from_args(None, Some(100), None, None, VerifyScenario::Live)
                .unwrap_err();
        assert!(matches!(
            error,
            super::VerifyWindowSelectionError::ToSeqRequiresFromSeq
        ));
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
        assert!(matches!(
            error,
            super::VerifyWindowSelectionError::AttemptIdCannotBeCombinedWithWindow
        ));
    }

    #[test]
    fn attempt_id_conflicts_with_since() {
        let error = VerifyWindowSelection::from_args(
            None,
            None,
            Some("attempt-1".to_owned()),
            Some("10m".to_owned()),
            VerifyScenario::Live,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            super::VerifyWindowSelectionError::AttemptIdCannotBeCombinedWithWindow
        ));
    }

    #[test]
    fn seq_range_conflicts_with_since() {
        let error = VerifyWindowSelection::from_args(
            Some(100),
            Some(200),
            None,
            Some("10m".to_owned()),
            VerifyScenario::Live,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            super::VerifyWindowSelectionError::SeqRangeCannotBeCombinedWithSince
        ));
    }

    #[test]
    fn parse_since_rejects_invalid_values() {
        assert!(matches!(
            parse_since("abcm").unwrap_err(),
            super::VerifyWindowSelectionError::InvalidSinceValue(_)
        ));
        assert!(matches!(
            parse_since("10x").unwrap_err(),
            super::VerifyWindowSelectionError::UnsupportedSinceSuffix('x')
        ));
    }

    #[test]
    fn parse_since_rejects_overflowing_values() {
        assert!(matches!(
            parse_since("9223372036854775807s").unwrap_err(),
            super::VerifyWindowSelectionError::SinceValueOverflow { .. }
        ));
    }

    #[test]
    fn from_args_rejects_overflowing_since_without_panicking() {
        let error = VerifyWindowSelection::from_args(
            None,
            None,
            None,
            Some("9223372036854775s".to_owned()),
            VerifyScenario::Live,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            super::VerifyWindowSelectionError::SinceWindowOverflowsNow(_)
        ));
    }
}
