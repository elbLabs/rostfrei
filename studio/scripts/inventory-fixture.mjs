export const infrastructurePath = "/fixture/broker-snapshot"
export const domainPath = "/fixture/domain-snapshot"
export const serverPath = "/fixture/server-health"
export const workloadPath = "/fixture/workload-memory"

export const inventoryScenarios = {
  infrastructure:
    "Sampled account, stream and KV budgets with partial evidence",
  domain:
    "Rental fleet and its declarations with Bicycle collection requested selected",
  "read-models":
    "Read-model discovery with explicit and unknown storage mappings",
  "infrastructure-details":
    "Ledger stream and consumer readings in an Infrastructure overlay",
  "read-model-details": "Declared Read model policy and mapped cgroup readings",
}

export const inventoryCapabilities = {
  infrastructure: { snapshotHref: infrastructurePath },
  domainInventory: { snapshotHref: domainPath },
  servers: { snapshotHref: serverPath },
  workloads: { snapshotHref: workloadPath },
}

export const serverSnapshot = {
  servers: [
    {
      label: "Rental node A",
      health: { status: "available", value: "healthy" },
      metrics: {
        status: "available",
        value: {
          version: "2.12.15",
          uptime: "14h19m6s",
          residentMemoryBytes: 36_700_160,
          jetStreamLimits: { maxMemoryBytes: 134_217_728 },
        },
      },
    },
    {
      label: "Rental node B",
      health: { status: "unavailable", reason: "timedOut" },
      metrics: { status: "unavailable", reason: "timedOut" },
    },
  ],
}
export const workloadSnapshot = {
  workloads: [
    {
      label: "Projection worker",
      memory: {
        currentBytes: 33_554_432,
        limit: { status: "limited", bytes: 134_217_728 },
      },
    },
  ],
}

const capacity = (usedBytes, limitBytes) => ({ usedBytes, limitBytes })
const items = (entries, status = "complete") => ({ status, items: entries })

export const brokerSnapshot = {
  sources: [
    {
      label: "Bike rental · Production",
      connection: "connected",
      account: {
        status: "complete",
        usage: {
          memory: capacity(4_194_304, 16_777_216),
          storage: capacity(69_206_016, 83_886_080),
          streamCount: 6,
          consumerCount: 3,
          tiers: [
            {
              name: "R3",
              memory: capacity(0, null),
              storage: capacity(35_651_584, 83_886_080),
            },
          ],
        },
      },
      streams: items(
        [
          {
            name: "BIKE_RENTAL_EVENTS",
            storage: "file",
            replicas: 3,
            messages: 42_560,
            capacity: capacity(35_651_584, 41_943_040),
            consumerCount: 2,
            consumers: items([
              {
                name: "rental-projection",
                kind: "pull",
                durable: true,
                paused: false,
                liveness: "pullWaiting",
                pending: 12,
                ackPending: 2,
                redelivered: 1,
                waiting: 1,
                deliveredStreamSequence: "9007199254740993",
                ackFloorStreamSequence: "9007199254740991",
              },
              {
                name: "availability-projection",
                kind: "pull",
                durable: true,
                paused: false,
                liveness: "unknown",
                pending: 29,
                ackPending: 0,
                redelivered: 0,
                waiting: 0,
                deliveredStreamSequence: "42531",
                ackFloorStreamSequence: "42531",
              },
            ]),
          },
          {
            name: "KV_active-rentals",
            storage: "file",
            replicas: 3,
            messages: 8,
            capacity: capacity(12_288, null),
            consumerCount: 0,
            consumers: items([]),
          },
        ],
        "truncated"
      ),
      kvBuckets: items([
        {
          name: "active-rentals",
          stream: "KV_active-rentals",
          storage: "file",
          capacity: capacity(12_288, null),
        },
        {
          name: "reservations",
          stream: "KV_reservations",
          storage: "file",
          capacity: capacity(4_096, null),
        },
      ]),
    },
    {
      label: "Bike rental · Test",
      connection: "connected",
      account: { status: "unavailable", usage: null },
      streams: items([], "timedOut"),
      kvBuckets: items([]),
    },
  ],
}

// A hand-built inspection-only backend. Synthetic routes differ from the
// server's real routes so the browser test detects hard-coded snapshot paths.
export function inventoryBackend({ catalog = {}, reply } = {}) {
  const requests = []
  const response = (pathname) => {
    const override = reply?.(pathname)
    if (override) return override
    if (pathname === "/catalog")
      return [
        200,
        {
          catalogVersion: 1,
          contexts: [],
          ...catalog,
          ...inventoryCapabilities,
        },
      ]
    if (pathname === infrastructurePath) return [200, brokerSnapshot]
    if (pathname === domainPath) return [200, domainSnapshot]
    if (pathname === serverPath) return [200, serverSnapshot]
    if (pathname === workloadPath) return [200, workloadSnapshot]
    return [404, { message: "Unmocked inventory fixture request" }]
  }
  return {
    requests,
    configureServer(server) {
      server.middlewares.use(async (req, res, next) => {
        const pathname = new URL(req.url, "http://studio.invalid").pathname
        if (!pathname.startsWith("/api/")) return next()
        const path = pathname.slice(4)
        const inspection =
          req.headers.authorization === "Bearer inventory-inspection"
        requests.push({ method: req.method, path, inspection })
        const [status, body] =
          req.method === "GET" && inspection ? await response(path) : [403, {}]
        if (res.destroyed) return
        res.writeHead(status, { "content-type": "application/json" })
        res.end(JSON.stringify(body))
      })
    },
  }
}

const fleet = { context: "bike-rental", local: "rental-fleet" }
const root = { aggregate: fleet, local: "rental-fleet" }
const bicycle = { aggregate: fleet, local: "bicycle" }
const rateCard = { context: "billing", local: "rate-card" }
const rate = { aggregate: rateCard, local: "rate" }
const identifierField = (name, id, label) => ({
  name,
  value: {
    kind: "scalar",
    scalar: {
      kind: "semantic",
      id,
      label,
      representation: "string",
    },
  },
})
const bicycleField = identifierField("bicycle_id", "bicycle-id", "Bicycle ID")
export const fleetNodeId = "aggregate:bike-rental/rental-fleet"
export const collectionRequestedNodeId =
  "event:bike-rental/rental-fleet/bicycle-collection-requested"
export const collectionRequestedLabel = "Bicycle collection requested"
export const domainSnapshot = {
  compiledModelAvailable: true,
  boundedContexts: [
    { id: "bike-rental", label: "Bike rental" },
    { id: "billing", label: "Billing" },
  ],
  aggregates: [
    {
      id: fleet,
      root,
      label: "Rental fleet",
      anchor: true,
      aggregateType: "bike-rental/rental-fleet",
    },
    { id: rateCard, label: "Rate card", root: rate, anchor: false },
  ],
  entities: [
    { id: root, label: "Fleet", identity: { owner: root }, fields: [] },
    {
      id: bicycle,
      label: "Bicycle",
      identity: { owner: bicycle },
      fields: [
        bicycleField,
        {
          name: "category",
          declaredType: "Option<BicycleCategory>",
          value: { kind: "optional", value: { kind: "opaque" } },
        },
        {
          name: "rate_card",
          value: {
            kind: "optional",
            value: { kind: "aggregateReference", aggregate: rateCard },
          },
        },
      ],
    },
    { id: rate, label: "Rate", identity: { owner: rate }, fields: [] },
  ],
  domainIdentities: [{ id: { owner: bicycle } }],
  valueObjects: [{ id: "rental-period", label: "Rental period" }],
  domainServices: [
    {
      id: { context: "billing", local: "price-rental" },
      label: "Price rental",
    },
  ],
  domainEvents: [
    {
      id: { aggregate: fleet, local: "bicycle-rented" },
      label: "Bicycle rented",
      schemaVersion: 2,
      fields: [bicycleField],
    },
    {
      id: { aggregate: fleet, local: "bicycle-collection-requested" },
      label: collectionRequestedLabel,
      schemaVersion: 1,
      fields: [
        identifierField("fleet_id", "fleet-id", "Fleet ID"),
        identifierField("reservation_id", "reservation-id", "Reservation ID"),
        bicycleField,
        {
          name: "payment_id",
          declaredType: "PaymentId",
          value: { kind: "opaque" },
        },
      ],
    },
  ],
  domainErrors: [],
  actions: [],
  policies: [],
  queries: [],
  invariants: [],
  registeredCommands: [
    {
      context: "bike-rental",
      name: "rent-bicycle",
      label: "Rent bicycle",
      schemaVersion: 2,
      fields: [
        {
          name: "fleet_id",
          value: { kind: "opaque" },
          declaredType: "FleetId",
        },
        {
          name: "bicycle_id",
          value: { kind: "opaque" },
          declaredType: "BicycleId",
        },
      ],
    },
  ],
  registeredQueries: [],
  readModels: [
    {
      context: "bike-rental",
      name: "active-rentals",
      schemaVersion: 1,
      brokerSource: "Bike rental · Production",
      workload: "Projection worker",
      storage: {
        kind: "natsKv",
        application: "bike-rental",
        scope: "production",
        bucketName: "active-rentals",
        streamName: "KV_active-rentals",
        maxBytes: "67108864",
        maxValueBytes: 65536,
        storage: "file",
        replicas: 3,
        history: 1,
        ttlNanos: "0",
      },
    },
    {
      context: "bike-rental",
      name: "bicycle-availability-projection-with-an-intentionally-long-identifier",
      schemaVersion: 2,
    },
  ],
}

export async function prepareInventoryScenario(page, scene) {
  await page.waitForFunction(
    () => {
      const view = document
        .querySelector("#studio-workspace")
        ?.getAttribute("data-workspace")
      if (!["infrastructure", "domain", "read-models"].includes(view))
        return Boolean(view)
      return Boolean(
        document.querySelector(
          `[data-inventory-view="${view}"]:not([data-inventory-state="loading"])`
        )
      )
    },
    { timeout: 5_000 }
  )
  const view =
    scene === "infrastructure-details"
      ? "infrastructure"
      : scene === "read-model-details"
        ? "read-models"
        : scene
  const tab = `[data-workspace-tab="${view}"]`
  if (!(await page.$(`${tab}[aria-selected="true"]`))) await page.click(tab)
  await page.waitForSelector(
    `[data-inventory-view="${view}"][data-inventory-state="ready"]`,
    { timeout: 5_000 }
  )
  const clickResource = async (label) => {
    const selector = `button[aria-label="${label}"]`
    await page.waitForFunction(
      (selector) => {
        const element = document.querySelector(selector)
        if (!element) return false
        const { x, y, width, height } = element.getBoundingClientRect()
        return element.contains(
          document.elementFromPoint(x + width / 2, y + height / 2)
        )
      },
      { timeout: 5_000 },
      selector
    )
    await page.click(selector)
  }
  if (scene === "infrastructure-details") {
    await clickResource("Explore broker source Bike rental · Production")
    await clickResource("Explore stream BIKE_RENTAL_EVENTS")
    await clickResource("Inspect consumer rental-projection")
  } else if (scene === "read-model-details") {
    await clickResource("Inspect read model active-rentals")
    await page.waitForFunction(() =>
      document
        .querySelector('[aria-label="Container memory"]')
        ?.textContent.includes("32 MiB")
    )
  }
}
