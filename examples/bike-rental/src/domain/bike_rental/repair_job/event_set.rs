use rostfrei::AggregateEvents;

use super::RepairJobOpened;

#[derive(AggregateEvents)]
pub enum RepairJobEvent {
    RepairJobOpened(RepairJobOpened),
}
