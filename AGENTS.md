# NATS implementation

- Prefer native NATS/JetStream features and official client APIs for batching, ordering, retries, and recovery.
- Check native capabilities before building custom machinery; briefly document any concrete gap that requires it.
- Preserve Rostfrei's domain validation and consistency guarantees when adopting native features.
