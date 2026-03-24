use app_replay::{replay_journal, NoopReplayConsumer};

fn main() {
    let mut consumer = NoopReplayConsumer;
    replay_journal(Vec::new(), &mut consumer).unwrap();
}
