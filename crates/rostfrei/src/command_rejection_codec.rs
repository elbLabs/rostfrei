use rostfrei_core::{
    EncodedRejection, EventCodecError, EventCodecErrorKind, JsonRejectionCodec, RejectionCodec,
};
use rostfrei_messaging_core::CommandRejection;

use crate::CommandRejectionMapper;

/// Explicit bidirectional bridge for a domain rejection and the processor's durable
/// mapped rejection format.
///
/// Use the same canonical execution metadata and this
/// bridge when direct generic execution must replay outcomes written by a processor.
/// The reverse mapping must retain readers for all previously stored domain codes.
#[derive(Clone, Debug)]
pub struct MappedCommandRejectionCodec<M, D> {
    mapper: M,
    decoder: D,
}

impl<M, D> MappedCommandRejectionCodec<M, D> {
    pub const fn new(mapper: M, decoder: D) -> Self {
        Self { mapper, decoder }
    }
}

impl<R, M, D> RejectionCodec<R> for MappedCommandRejectionCodec<M, D>
where
    M: CommandRejectionMapper<R>,
    D: Fn(&CommandRejection) -> Result<R, String> + Send + Sync,
{
    fn encode(&self, rejection: &R) -> Result<EncodedRejection, EventCodecError> {
        let mapped = self.mapper.map(rejection).map_err(|message| {
            EventCodecError::new(EventCodecErrorKind::EncodingFailed, message)
        })?;
        JsonRejectionCodec::new("rostfrei.command-rejection", 1).encode(&mapped)
    }

    fn decode(&self, rejection: &EncodedRejection) -> Result<R, EventCodecError> {
        let mapped: CommandRejection =
            JsonRejectionCodec::new("rostfrei.command-rejection", 1).decode(rejection)?;
        (self.decoder)(&mapped)
            .map_err(|message| EventCodecError::new(EventCodecErrorKind::MalformedPayload, message))
    }
}
