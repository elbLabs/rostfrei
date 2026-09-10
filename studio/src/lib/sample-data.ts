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
    id: "add-first-bicycle",
    name: "Add the first bicycle to a new fleet",
    revision: "local-demo",
    definitionHref: "/tests/add-first-bicycle",
    runHref: "/tests/add-first-bicycle/runs",
  },
  {
    id: "reject-duplicate-bicycle",
    name: "Reject a bicycle already in the fleet",
    revision: "local-demo",
    definitionHref: "/tests/reject-duplicate-bicycle",
    runHref: "/tests/reject-duplicate-bicycle/runs",
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
      schemaVersion: 2,
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
                schemaVersion: 2,
                context: "bike-rental",
                payload: { fleet_id: "city-fleet", bicycle_id: "bike-42" },
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
      schemaVersion: 2,
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
                schemaVersion: 2,
                context: "bike-rental",
                payload: { fleet_id: "city-fleet", bicycle_id: "bike-99" },
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
  "add-first-bicycle": {
    revision: "local-demo",
    definition: {
      schemaVersion: 2,
      id: "add-first-bicycle",
      name: "Add the first bicycle to a new fleet",
      setup: { fixture: "no-rental-fleets" },
      expected: {
        within: "35s",
        settleFor: "1s",
        graphs: [
          {
            nodes: [
              {
                kind: "command",
                key: "subject",
                name: "add-bicycle",
                schemaVersion: 2,
                context: "bike-rental",
                payload: {
                  fleet_id: "city-fleet",
                  bicycle_id: "bike-01",
                  condition: "serviceable",
                },
                outcome: "accepted",
              },
              {
                kind: "domain-event",
                key: "bicycle-added",
                parentKey: "subject",
                name: "bicycle-added",
                schemaVersion: 1,
                payload: {
                  fleet_id: "city-fleet",
                  bicycle_id: "bike-01",
                  condition: "serviceable",
                },
              },
            ],
          },
        ],
      },
    },
  },
  "reject-duplicate-bicycle": {
    revision: "local-demo",
    definition: {
      schemaVersion: 2,
      id: "reject-duplicate-bicycle",
      name: "Reject a bicycle already in the fleet",
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
                name: "add-bicycle",
                schemaVersion: 2,
                context: "bike-rental",
                payload: {
                  fleet_id: "city-fleet",
                  bicycle_id: "bike-42",
                  condition: "serviceable",
                },
                outcome: {
                  rejected: {
                    code: "BICYCLE_ALREADY_IN_FLEET",
                    payload: { bicycle_id: "bike-42" },
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
      schemaVersion: 2,
      id: "return-rented-bicycle",
      name: "Return a rented bicycle",
      setup: { fixture: "rented-demo-fleet" },
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
                schemaVersion: 2,
                context: "bike-rental",
                payload: { fleet_id: "city-fleet", bicycle_id: "bike-42" },
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
  revision: "2",
  messages: [
    {
      kind: "domain-event",
      messageId: "demo-bike-42-added",
      correlationId: "fixture:demo-fleet:2:bike-42",
      name: "bicycle-added",
      schemaVersion: 1,
      aggregate: {
        type: "bike-rental/rental-fleet",
        id: "city-fleet",
      },
      streamVersion: 1,
      payload: {
        fleet_id: "city-fleet",
        bicycle_id: "bike-42",
        condition: "serviceable",
      },
    },
    {
      kind: "domain-event",
      messageId: "demo-bike-99-added",
      correlationId: "fixture:demo-fleet:2:bike-99",
      name: "bicycle-added",
      schemaVersion: 1,
      aggregate: {
        type: "bike-rental/rental-fleet",
        id: "city-fleet",
      },
      streamVersion: 2,
      payload: {
        fleet_id: "city-fleet",
        bicycle_id: "bike-99",
        condition: "maintenance-required",
      },
    },
  ],
}

export const SAMPLE_FIXTURES: Record<string, Fixture> = {
  "demo-fleet": SAMPLE_FIXTURE,
  "no-rental-fleets": {
    schemaVersion: 1,
    id: "no-rental-fleets",
    revision: "1",
    messages: [],
  },
  "rented-demo-fleet": {
    schemaVersion: 1,
    id: "rented-demo-fleet",
    revision: "3",
    messages: [
      {
        kind: "domain-event",
        messageId: "rented-demo-bike-42-added",
        correlationId: "fixture:rented-demo-fleet:3:bike-42",
        name: "bicycle-added",
        schemaVersion: 1,
        aggregate: {
          type: "bike-rental/rental-fleet",
          id: "city-fleet",
        },
        streamVersion: 1,
        payload: {
          fleet_id: "city-fleet",
          bicycle_id: "bike-42",
          condition: "serviceable",
        },
      },
      {
        kind: "domain-event",
        messageId: "rented-demo-bike-99-added",
        correlationId: "fixture:rented-demo-fleet:3:bike-99",
        name: "bicycle-added",
        schemaVersion: 1,
        aggregate: {
          type: "bike-rental/rental-fleet",
          id: "city-fleet",
        },
        streamVersion: 2,
        payload: {
          fleet_id: "city-fleet",
          bicycle_id: "bike-99",
          condition: "maintenance-required",
        },
      },
      {
        kind: "domain-event",
        messageId: "fixture-bike-42-rented",
        correlationId: "fixture:rented-demo-fleet:3:rent",
        name: "bicycle-rented",
        schemaVersion: 1,
        aggregate: {
          type: "bike-rental/rental-fleet",
          id: "city-fleet",
        },
        streamVersion: 3,
        payload: {
          fleet_id: "city-fleet",
          bicycle_id: "bike-42",
        },
      },
    ],
  },
}

export const SAMPLE_GRAPH: MessageGraphNode[] = [
  {
    id: "fixture-event-0-demo-bike-42-added",
    kind: "domain-event",
    name: "bicycle-added",
    schemaVersion: 1,
    payload: SAMPLE_FIXTURE.messages[0].payload,
    messageId: "demo-bike-42-added",
    aggregateType: "bike-rental/rental-fleet",
    aggregateId: "city-fleet",
    streamVersion: 1,
    context: "fixture",
    status: "accepted",
  },
  {
    id: "fixture-event-0-demo-bike-99-added",
    parentId: "fixture-event-0-demo-bike-42-added",
    edgeFidelity: "exact",
    edgeRelationship: "stream-order",
    kind: "domain-event",
    name: "bicycle-added",
    schemaVersion: 1,
    payload: SAMPLE_FIXTURE.messages[1].payload,
    messageId: "demo-bike-99-added",
    aggregateType: "bike-rental/rental-fleet",
    aggregateId: "city-fleet",
    streamVersion: 2,
    context: "fixture",
    status: "accepted",
  },
  {
    id: "command-rent",
    parentId: "fixture-event-0-demo-bike-99-added",
    edgeRelationship: "context",
    kind: "command",
    name: "rent-bicycle",
    schemaVersion: 2,
    payload: { fleet_id: "city-fleet", bicycle_id: "bike-42" },
    response: { status: "accepted", value: null },
    messageId: "cmd_01HZX8B7T7",
    boundedContext: "bike-rental",
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
]
