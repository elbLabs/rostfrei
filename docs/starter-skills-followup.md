# Published starter follow-up

The current generated project deliberately pins published `v0.0.5-alpha` at
`c9f5516ddc324265916b60e1eb7300dad9acac9d`, with `supports_skills = false`.
Its connection/drain skeleton is unchanged. A clean new composition may still
compile with version `0.0.5-alpha`; that does not make it the published starter.

After a **genuinely published skills-enabled revision** is available:

1. Record its real tag, full commit, archive checksum and clean CLI/kit identity.
   Confirm the published source contains setup support; never invent a pin or
   point the public starter to this unpublished local integration branch.
2. Update `crates/rostfrei-structure/src/scaffold.rs`'s published tag/revision and
   qualified skills-capability flag together. Review its starter tests and the
   corresponding expectations in `scripts/check_agent_onboarding.py`.
3. Confirm `crates/rostfrei-structure/templates/new/Cargo.toml.template` resolves
   every framework dependency at that same published commit. Preserve the
   connection skeleton unless a separate host-scaffold task is authorized.
4. Run native scaffold/unit/installer tests and the repository's external
   generated-project check against the **unpatched** public pin. Create a new
   external project, lock/build it, inspect all resolved `rostfrei*` packages and
   both installed skill metadata files plus managed baseline. Check staged atomic
   creation, default setup and idempotency/conflict behavior.
5. Run an explicit dependency upgrade plus `skills update` in a disposable existing
   app, retain the matching native cache and verify source-free empty-PATH routing.
6. Update new-project documentation/release claims only after those results. Keep
   the historical pre-kit refusal test truthful for an explicitly old dependency
   rather than deleting unsupported-release coverage.

The old generated-project check proves the published connection skeleton.
The new external bike-rental shared-host setup in `release-01-kit.md` proves a
different experience: application handlers/fixtures, fresh split authority,
installed skills and actual NATS Test execution. Neither substitutes for the other.
The parent/release owner decides whether this post-publication follow-up is needed
for a day-one new-project claim; this local checkpoint does not publish it.
