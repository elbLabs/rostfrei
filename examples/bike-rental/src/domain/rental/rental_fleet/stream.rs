use rostfrei::{Aggregate, StreamAggregateId, StreamAggregateType, StreamId};

use super::RentalFleetAggregate;

pub fn stream_id(aggregate_id: &str) -> Result<StreamId, &'static str> {
    let aggregate_type = StreamAggregateType::new(RentalFleetAggregate::aggregate_type())
        .map_err(|_| "invalid rental fleet aggregate type")?;
    let aggregate_id =
        StreamAggregateId::new(aggregate_id).map_err(|_| "invalid rental fleet ID")?;
    Ok(StreamId::new(aggregate_type, aggregate_id))
}
