use journal::{replay_entries, JournalEntry};

pub trait ReplayConsumer {
    type Error;

    fn consume(&mut self, entry: JournalEntry) -> Result<(), Self::Error>;
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

#[derive(Debug, Default)]
pub struct NoopReplayConsumer;

impl ReplayConsumer for NoopReplayConsumer {
    type Error = std::convert::Infallible;

    fn consume(&mut self, _entry: JournalEntry) -> Result<(), Self::Error> {
        Ok(())
    }
}
