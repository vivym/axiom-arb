use crate::JournalEntry;

pub type ReplayEntry = JournalEntry;

pub fn replay_entries<I>(entries: I) -> Vec<ReplayEntry>
where
    I: IntoIterator<Item = JournalEntry>,
{
    let mut ordered: Vec<_> = entries.into_iter().collect();
    ordered.sort_by_key(|entry| entry.journal_seq);
    ordered
}
