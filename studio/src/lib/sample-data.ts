import type {
  Fixture,
  MessageGraphNode,
  TestDefinitionRevision,
  TestDefinitionSummary,
} from "@/lib/types"

export const SAMPLE_TESTS: TestDefinitionSummary[] = [
  {
    id: "rent-available-bicycle",
    name: "Rent an available bicycle",
    revision: "local-demo",
    definitionHref: "/tests/rent-available-bicycle",
    runHref: "/tests/rent-available-bicycle/runs",
  },
  {
    id: "reject-unavailable-bicycle",
    name: "Reject a maintenance-required bicycle",
    revision: "local-demo",
    definitionHref: "/tests/reject-unavailable-bicycle",
    runHref: "/tests/reject-unavailable-bicycle/runs",
  },
  {
    id: "return-rented-bicycle",
    name: "Return a rented bicycle",
    revision: "local-demo",
    definitionHref: "/tests/return-rented-bicycle",
    runHref: "/tests/return-rented-bicycle/runs",
  },
]

export const SAMPLE_DEFINITIONS: Record<string, TestDefinitionRevision> = {
  "rent-available-bicycle": {
    revision: "local-demo",
    definition: {
      schemaVersion: 1,
      id: "rent-available-bicycle",
      name: "Rent an available bicycle",
      setup: { fixture: "demo-fleet" },
      expected: {
        within: "35s",
        settleFor: "1s",
        graphs: [
          {
            nodes: [
              {
                kind: "command",
                key: "subject",
                name: "rent-bicycle",
                schemaVersion: 1,
                aggregate: {
                  type: "bike-rental/rental-fleet",
                  id: "city-fleet",
                },
                payload: { bicycle_id: "bike-42" },
                outcome: "accepted",
              },
              {
                kind: "domain-event",
                key: "bicycle-rented",
                parentKey: "subject",
                name: "bicycle-rented",
                schemaVersion: 1,
                payload: {
                  fleet_id: "city-fleet",
                  bicycle_id: "bike-42",
                },
              },
              {
                kind: "integration-event",
                key: "rental-started",
                parentKey: "bicycle-rented",
                name: "bicycle-rental-started",
                schemaVersion: 1,
                payload: {
                  fleet_id: "city-fleet",
                  bicycle_id: "bike-42",
                },
              },
            ],
          },
        ],
      },
    },
  },
  "reject-unavailable-bicycle": {
    revision: "local-demo",
    definition: {
      schemaVersion: 1,
      id: "reject-unavailable-bicycle",
      name: "Reject a maintenance-required bicycle",
      setup: { fixture: "demo-fleet" },
      expected: {
        within: "35s",
        settleFor: "1s",
        graphs: [
          {
            nodes: [
              {
                kind: "command",
                key: "subject",
                name: "rent-bicycle",
                schemaVersion: 1,
                aggregate: {
                  type: "bike-rental/rental-fleet",
                  id: "city-fleet",
                },
                payload: { bicycle_id: "bike-99" },
                outcome: {
                  rejected: {
                    code: "BICYCLE_UNAVAILABLE",
                    payload: { bicycle_id: "bike-99" },
                  },
                },
              },
            ],
          },
        ],
      },
    },
  },
  "return-rented-bicycle": {
    revision: "local-demo",
    definition: {
      schemaVersion: 1,
      id: "return-rented-bicycle",
      name: "Return a rented bicycle",
      setup: { fixture: "demo-fleet" },
      expected: {
        within: "35s",
        settleFor: "1s",
        graphs: [
          {
            nodes: [
              {
                kind: "command",
                key: "subject",
                name: "return-bicycle",
                schemaVersion: 1,
                aggregate: {
                  type: "bike-rental/rental-fleet",
                  id: "city-fleet",
                },
                payload: { bicycle_id: "bike-42" },
                outcome: "accepted",
              },
              {
                kind: "domain-event",
                key: "bicycle-returned",
                parentKey: "subject",
                name: "bicycle-returned",
                schemaVersion: 1,
                payload: {
                  fleet_id: "city-fleet",
                  bicycle_id: "bike-42",
                },
              },
            ],
          },
        ],
      },
    },
  },
}

export const SAMPLE_FIXTURE: Fixture = {
  schemaVersion: 1,
  id: "demo-fleet",
  revision: "1",
  messages: [
    {
      kind: "domain-event",
      messageId: "demo-fleet-imported",
      correlationId: "fixture:demo-fleet:1",
      name: "rental-fleet-imported",
      schemaVersion: 1,
      aggregate: {
        type: "bike-rental/rental-fleet",
        id: "city-fleet",
      },
      streamVersion: 1,
      payload: {
        fleet_id: "city-fleet",
        bicycles: [
          {
            bicycle_id: "bike-42",
            status: "available",
            condition: "serviceable",
          },
          {
            bicycle_id: "bike-99",
            status: "available",
            condition: "maintenance-required",
          },
        ],
      },
    },
  ],
}

export const SAMPLE_GRAPH: MessageGraphNode[] = [
  {
    id: "fixture-event-0-demo-fleet-imported",
    kind: "domain-event",
    name: "rental-fleet-imported",
    schemaVersion: 1,
    payload: SAMPLE_FIXTURE.messages[0].payload,
    messageId: "demo-fleet-imported",
    aggregateType: "bike-rental/rental-fleet",
    aggregateId: "city-fleet",
    streamVersion: 1,
    context: "fixture",
    status: "accepted",
  },
  {
    id: "command-rent",
    parentId: "fixture-event-0-demo-fleet-imported",
    edgeRelationship: "context",
    kind: "command",
    name: "rent-bicycle",
    schemaVersion: 1,
    payload: { bicycle_id: "bike-42" },
    response: { status: "accepted", value: null },
    messageId: "cmd_01HZX8B7T7",
    aggregateType: "bike-rental/rental-fleet",
    aggregateId: "city-fleet",
    status: "accepted",
  },
  {
    id: "event-rented",
    parentId: "command-rent",
    edgeFidelity: "exact",
    kind: "domain-event",
    name: "bicycle-rented",
    schemaVersion: 1,
    payload: { fleet_id: "city-fleet", bicycle_id: "bike-42" },
    messageId: "evt_01HZX8B8A2",
    causationId: "cmd_01HZX8B7T7",
  },
  {
    id: "event-audit",
    parentId: "command-rent",
    edgeFidelity: "exact",
    kind: "domain-event",
    name: "rental-attempt-recorded",
    schemaVersion: 1,
    payload: { outcome: "accepted", station: "central" },
    messageId: "evt_01HZX8B8E4",
    causationId: "cmd_01HZX8B7T7",
  },
  {
    id: "integration-started",
    parentId: "event-rented",
    edgeFidelity: "exact",
    kind: "integration-event",
    name: "bicycle-rental-started",
    schemaVersion: 1,
    payload: { fleet_id: "city-fleet", bicycle_id: "bike-42" },
    messageId: "msg_01HZX8B9G1",
    causationId: "evt_01HZX8B8A2",
  },
  {
    id: "integration-availability",
    parentId: "event-rented",
    edgeFidelity: "exact",
    kind: "integration-event",
    name: "fleet-availability-changed",
    schemaVersion: 1,
    payload: { available: 11, rented: 4 },
    messageId: "msg_01HZX8B9K9",
    causationId: "evt_01HZX8B8A2",
  },
  {
    id: "integration-audit",
    parentId: "event-audit",
    edgeFidelity: "exact",
    kind: "integration-event",
    name: "rental-audit-indexed",
    schemaVersion: 1,
    payload: { index: "rental-audit", result: "stored" },
    messageId: "msg_01HZX8BA12",
    causationId: "evt_01HZX8B8E4",
  },
]
