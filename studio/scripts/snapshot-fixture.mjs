import {
  historicalCapability,
  historicalMessages,
  historicalPage,
} from "./historical-fixture.mjs"

export const snapshotCapability = {
  ...historicalCapability,
  aggregateHref: "/historical/test/aggregate",
  snapshotFixtureHref: "/historical/test/snapshot-fixture",
  snapshotAggregateTypes: historicalCapability.aggregateTypes,
}
export const snapshotHistory = {
  ...historicalPage([{ ...historicalMessages[0], streamVersion: "6" }]),
  generation: "snapshot-history",
}
export function snapshotReconstruction(useSnapshots = true, version = "6") {
  const target = Number(version)
  const base = useSnapshots && target >= 4 ? 4 : 0
  return {
    application: snapshotCapability.application,
    scope: "test",
    generation: "snapshot-history",
    aggregateType: "bike-rental/rental-fleet",
    aggregateId: "city-fleet",
    version,
    snapshotVersion: String(base),
    snapshotStatus: base ? "hit" : useSnapshots ? "miss" : "disabled",
    eventsRead: target - base,
    eventsApplied: target - base,
    state: { fleet_id: "city-fleet", event_count: target },
    snapshots: [
      {
        streamVersion: "4",
        replayRevision: "rental-fleet-v1",
        schemaVersion: 1,
        stateBytes: 46,
        valid: true,
        state: { fleet_id: "city-fleet", event_count: 4 },
      },
    ],
  }
}
