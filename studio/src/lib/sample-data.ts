import type {
  MessageGraphNode,
  TestDefinitionRevision,
  TestDefinitionSummary,
} from "@/lib/types"

export const SAMPLE_TESTS: TestDefinitionSummary[] = [
  {
    id: "rent-available-bicycle",
    name: "Rent an available bicycle",
    revision: "local-demo",
    runHref: "/tests/rent-available-bicycle/runs",
  },
  {
    id: "reject-unavailable-bicycle",
    name: "Reject a maintenance bicycle",
    revision: "local-demo",
    runHref: "/tests/reject-unavailable-bicycle/runs",
  },
  {
    id: "return-rented-bicycle",
    name: "Return a rented bicycle",
    revision: "local-demo",
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
      given: { fixture: "demo-fleet" },
      when: {
        command: {
          name: "rent-bicycle",
          schemaVersion: 1,
          aggregate: {
            type: "bike-rental/rental-fleet",
            id: "city-fleet",
          },
          payload: { bicycle_id: "bike-42" },
        },
      },
      then: {
        outcome: "accepted",
        within: "35s",
        trace: {
          contains: [
            {
              kind: "domain-event",
              name: "bicycle-rented",
              schemaVersion: 1,
              payload: { fleet_id: "city-fleet", bicycle_id: "bike-42" },
            },
            {
              kind: "integration-event",
              name: "bicycle-rental-started",
              schemaVersion: 1,
              payload: { fleet_id: "city-fleet", bicycle_id: "bike-42" },
            },
          ],
        },
      },
    },
  },
  "reject-unavailable-bicycle": {
    revision: "local-demo",
    definition: {
      schemaVersion: 1,
      id: "reject-unavailable-bicycle",
      name: "Reject a maintenance bicycle",
      given: { fixture: "demo-fleet" },
      when: {
        command: {
          name: "rent-bicycle",
          schemaVersion: 1,
          aggregate: {
            type: "bike-rental/rental-fleet",
            id: "city-fleet",
          },
          payload: { bicycle_id: "bike-99" },
        },
      },
      then: {
        outcome: {
          rejected: {
            code: "BICYCLE_UNAVAILABLE",
            payload: { bicycle_id: "bike-99" },
          },
        },
        within: "35s",
      },
    },
  },
  "return-rented-bicycle": {
    revision: "local-demo",
    definition: {
      schemaVersion: 1,
      id: "return-rented-bicycle",
      name: "Return a rented bicycle",
      given: {
        fixture: "demo-fleet",
        commands: [
          {
            name: "rent-bicycle",
            schemaVersion: 1,
            aggregate: {
              type: "bike-rental/rental-fleet",
              id: "city-fleet",
            },
            payload: { bicycle_id: "bike-42" },
          },
        ],
      },
      when: {
        command: {
          name: "return-bicycle",
          schemaVersion: 1,
          aggregate: {
            type: "bike-rental/rental-fleet",
            id: "city-fleet",
          },
          payload: { bicycle_id: "bike-42" },
        },
      },
      then: {
        outcome: "accepted",
        within: "35s",
        trace: {
          contains: [
            {
              kind: "domain-event",
              name: "bicycle-returned",
              schemaVersion: 1,
              payload: { fleet_id: "city-fleet", bicycle_id: "bike-42" },
            },
          ],
        },
      },
    },
  },
}

export const SAMPLE_GRAPH: MessageGraphNode[] = [
  {
    id: "fixture-rent-available-bicycle",
    kind: "domain-event",
    name: "demo-fleet",
    schemaVersion: 1,
    payload: { fixture: "demo-fleet" },
    context: "fixture",
    status: "accepted",
  },
  {
    id: "command-rent",
    parentId: "fixture-rent-available-bicycle",
    hideIncomingEdge: true,
    kind: "command",
    name: "rent-bicycle",
    schemaVersion: 1,
    payload: { bicycle_id: "bike-42" },
    response: {
      decision: "accepted",
      published: true,
      duplicate: false,
      commandMessageId: "cmd_01HZX8B7T7",
      responseMessageId: "rsp_01HZX8B85C",
    },
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
