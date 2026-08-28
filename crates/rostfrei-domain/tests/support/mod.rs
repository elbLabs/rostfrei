use std::{
    any::Any,
    fmt,
    panic::{AssertUnwindSafe, catch_unwind},
};

#[derive(Debug)]
pub enum ExpectedPanicError {
    DidNotPanic,
    UnsupportedPayload,
}

impl fmt::Display for ExpectedPanicError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DidNotPanic => formatter.write_str("expected the operation to panic"),
            Self::UnsupportedPayload => {
                formatter.write_str("panic payload was neither String nor &'static str")
            }
        }
    }
}

impl std::error::Error for ExpectedPanicError {}

pub fn panic_message(operation: impl FnOnce()) -> Result<String, ExpectedPanicError> {
    match catch_unwind(AssertUnwindSafe(operation)) {
        Ok(()) => Err(ExpectedPanicError::DidNotPanic),
        Err(payload) => panic_payload(payload),
    }
}

fn panic_payload(payload: Box<dyn Any + Send>) -> Result<String, ExpectedPanicError> {
    match payload.downcast::<String>() {
        Ok(message) => Ok(*message),
        Err(payload) => payload
            .downcast::<&'static str>()
            .map(|message| (*message).to_owned())
            .map_err(|_| ExpectedPanicError::UnsupportedPayload),
    }
}
