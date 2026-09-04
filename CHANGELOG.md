# Changelog

All notable changes to this project are documented in this file.

## [0.2.0] - 2026-09-04

### Changed

- **Breaking:** registered HTTP queries now use `POST` with an
  `application/json` body at the existing
  `/contexts/{context}/queries/{query}/schemas/{schema_version}` route. GET
  query routes and query-string payload decoding have been removed.
- Registered queries remain safe and idempotent by application contract despite
  using POST. Command endpoints are unchanged.
