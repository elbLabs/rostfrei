//! Domain modeling, event sourcing, and typed messaging for Rust applications.
//!
//! Use `rostfrei` as your application's framework dependency. Domain macros,
//! command/query buses, read models, and the in-memory runtime are always available
//! at the crate root. Install the macro bridge once with [`install_macro_support!`].
//!
//! # Optional features
//!
//! No adapter features are enabled by default.
//!
//! | Feature | API |
//! | --- | --- |
//! | `nats` | `nats`: `JetStream` messaging, event stores, and read models |
//! | `http` | `http`: HTTP command and query routes |
//! | `tracer` | `tracer`: diagnostics, observation, behavioral tests, and Tracer HTTP |
//! | `fixtures` | `fixtures`: versioned aggregate fixtures |
//! | `testing` | `testing`: test harnesses; also enables `fixtures` |
//!
//! Combine `nats` and `tracer` for `rostfrei::tracer::NatsApplication` and the
//! managed application host. `tracer` also enables `fixtures`.
//!
//! Lower-level contracts are available through [`core`], [`messaging`], and
//! [`registry`]. These are the same types used by the standalone adapter crates.
//!
//! See the [getting-started guide](https://elblabs.github.io/rostfrei/docs/getting-started)
//! and [bike-rental example](https://github.com/elbLabs/rostfrei/tree/main/examples/bike-rental).

pub use rostfrei_runtime::*;

/// Event sourcing, execution, and persistence contracts.
pub use rostfrei_core as core;
/// Messaging envelopes, addresses, consumers, and transport contracts.
pub use rostfrei_messaging_core as messaging;
/// Command/query registration and domain discovery.
pub use rostfrei_registry as registry;

#[cfg(feature = "fixtures")]
pub use rostfrei_fixtures as fixtures;
#[cfg(feature = "http")]
pub use rostfrei_http as http;
#[cfg(feature = "nats")]
pub use rostfrei_nats as nats;
#[cfg(feature = "testing")]
pub use rostfrei_testing as testing;
#[cfg(feature = "tracer")]
pub use rostfrei_tracer as tracer;
