pub mod span_names {
    pub const APP_BOOTSTRAP: &str = "axiom.app.bootstrap";
    pub const APP_BOOTSTRAP_COMPLETE: &str = "axiom.app.bootstrap.complete";
    pub const REPLAY_RUN: &str = "axiom.app_replay.run";
    pub const REPLAY_SUMMARY: &str = "axiom.app_replay.summary";
}

pub mod field_keys {
    pub const SERVICE_NAME: &str = "service.name";
    pub const APP_MODE: &str = "app_mode";
    pub const RUNTIME_MODE: &str = "runtime_mode";
    pub const BOOTSTRAP_STATUS: &str = "bootstrap_status";
    pub const PROCESSED_COUNT: &str = "processed_count";
    pub const LAST_JOURNAL_SEQ: &str = "last_journal_seq";
}

pub mod metric_dimensions {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Channel {
        Market,
        User,
    }

    impl Channel {
        pub const fn as_pair(self) -> (&'static str, &'static str) {
            match self {
                Self::Market => ("channel", "market"),
                Self::User => ("channel", "user"),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum HaltScope {
        Global,
        Family,
        Market,
        Strategy,
    }

    impl HaltScope {
        pub const fn as_pair(self) -> (&'static str, &'static str) {
            match self {
                Self::Global => ("scope", "global"),
                Self::Family => ("scope", "family"),
                Self::Market => ("scope", "market"),
                Self::Strategy => ("scope", "strategy"),
            }
        }
    }
}
