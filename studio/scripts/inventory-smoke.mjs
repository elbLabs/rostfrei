import assert from "node:assert/strict"
import { pathToFileURL } from "node:url"
import {
  closeStudioBrowser,
  desktopViewport,
  launchStudioBrowser,
  observePage,
  settlePage,
  startStudioServer,
} from "./browser.mjs"
import {
  brokerSnapshot,
  collectionRequestedLabel,
  collectionRequestedNodeId,
  domainPath,
  fleetNodeId,
  infrastructurePath,
  inventoryBackend,
  inventoryCapabilities,
  prepareInventoryScenario,
  serverPath,
  workloadPath,
} from "./inventory-fixture.mjs"

export async function checkInventory({ domainOnly = false, resourcesOnly = false } = {}) {
  let nextSnapshot
  let nextDomain
  let advertiseInfrastructure = true
  const backend = inventoryBackend({
    reply: (pathname) => {
      if (pathname === "/catalog" && !advertiseInfrastructure) {
        return [
          200,
          {
            catalogVersion: 1,
            contexts: [],
            domainInventory: inventoryCapabilities.domainInventory,
          },
        ]
      }
      if (pathname === infrastructurePath) return nextSnapshot
      if (pathname === domainPath) return nextDomain
    },
  })
  const { server, url } = await startStudioServer({
    configureServer: backend.configureServer,
    define: {
      "import.meta.env.VITE_TRACER_API_URL": JSON.stringify("/api"),
      "import.meta.env.VITE_TRACER_TOKEN": JSON.stringify(""),
      "import.meta.env.VITE_TRACER_INSPECTION_TOKEN": JSON.stringify(
        "inventory-inspection"
      ),
    },
  })
  let browser
  try {
    browser = await launchStudioBrowser()
    const page = await browser.newPage()
    const diagnostics = observePage(page)
    await page.setViewport(desktopViewport)
    await page.goto(url, { waitUntil: "networkidle0" })
    await page.waitForSelector('[data-workspace-tab="infrastructure"]', {
      visible: true,
      timeout: 3_000,
    })
    if (!resourcesOnly) await checkDomain(page)
    if (domainOnly) {
      assert.deepEqual(diagnostics.pageErrors, [])
      return
    }
    await page.setViewport(desktopViewport)
    await prepareInventoryScenario(page, "infrastructure")
    const root = '[data-inventory-view="infrastructure"]'
    const canvas = '[aria-label="Infrastructure canvas"]'
    const overlay = '[aria-label="Inventory details"]'
    const select = (label) => clickVisible(page, `${root} button[aria-label="${label}"]`)
    const pane = `${canvas} .react-flow__pane`
    await checkResourceCanvas(page, canvas)
    const text = (selector = root) =>
      page.$eval(selector, (element) => element.innerText)
    assert.equal(await page.$(overlay), null)
    await select("Inspect nats node Rental node A")
    assert.match(await text(overlay), /Process RAM \(RSS\)\s+35 MiB/)
    assert.match(await text(overlay), /JetStream server memory limit\s+128 MiB/)
    await page.keyboard.press("Escape")
    await select("Inspect nats node Rental node B")
    assert.match(await text(overlay), /Health: Timed out/)
    await page.keyboard.press("Escape")
    const readsBeforeNavigation = backend.requests.length
    await select("Explore broker source Bike rental · Test")
    await select("Inspect broker source Bike rental · Test")
    assert.match(await text(overlay), /Timed out/)
    assert.match(await text(overlay), /No KV buckets reported/)
    await select("All sources")
    await select("Explore broker source Bike rental · Production")
    const camera = () => page.$eval(canvas, (element) => ({
      transform: element.querySelector(".react-flow__viewport").style.transform,
      bounds: element.querySelector(".react-flow__pane").getBoundingClientRect().toJSON(),
    }))
    const before = await camera()
    await select("Inspect broker source Bike rental · Production")
    assert.match(await text(overlay), /66 MiB.*80 MiB/)
    assert.match(await text(overlay), /Truncated/)
    await page.click(`${overlay} summary`)
    assert.match(await text(overlay), /No finite limit reported/)
    assert.deepEqual(await camera(), before)
    await select("Close details")
    assert.deepEqual(await camera(), before)
    assert.equal(await page.$(overlay), null)
    await page.waitForSelector(`${canvas} [data-inventory-edge="backs-bucket"]`, visible)
    assert.equal(await page.$$eval(`${canvas} [data-inventory-edge="backs-bucket"]`, edges => edges.length), 1)
    await page.waitForSelector(`${canvas} [data-inventory-edge="reports-bucket"]`, visible)
    assert.equal(await page.$(`${canvas} [data-inventory-stream="KV_reservations"]`), null, "A reference cannot invent a sampled stream")
    await select("Inspect kv bucket active-rentals")
    assert.match(await text(overlay), /12 KiB used.*No finite limit reported/)
    await page.keyboard.press("Escape")
    await select("Explore stream BIKE_RENTAL_EVENTS")
    assert.match(await text(), /Liveness unknown/)
    await select("Inspect stream BIKE_RENTAL_EVENTS")
    assert.match(await text(overlay), /34 MiB used.*40 MiB limit/)
    assert.match(await text(overlay), /2 returned.*2 reported/)
    await page.keyboard.press("Escape")
    await select("Inspect consumer rental-projection")
    assert.match(await text(overlay), /Pull request waiting/)
    assert.match(await text(overlay), /Pending delivery\s+12/)
    assert.match(await text(overlay), /Delivered stream sequence\s+9007199254740993/)
    assert.equal(backend.requests.length, readsBeforeNavigation, "Navigation and disclosure must not reread snapshots")
    assert.equal(backend.requests.filter(({path}) => path === serverPath).length, 1)
    await select("Zoom in")
    await page.waitForFunction((selector) => document.querySelector(selector)?.textContent.includes("120%"), {}, `${canvas} .inventory-camera`)
    const zoomed = await camera()
    const dragStart = await page.$eval(pane, (element) => {
      const {left, bottom} = element.getBoundingClientRect()
      return {x: left + 100, y: bottom - 120}
    })
    await page.mouse.move(dragStart.x, dragStart.y)
    await page.mouse.down()
    await page.mouse.move(dragStart.x + 75, dragStart.y - 40, { steps: 8 })
    await page.mouse.up()
    assert.notEqual((await camera()).transform, zoomed.transform, "Drag pans even while details are open")

    let finishRefresh
    nextSnapshot = new Promise((resolve) => {
      finishRefresh = resolve
    })
    const refresh = `${root} button[aria-label="Refresh snapshot"]`
    await page.click(refresh)
    await page.waitForSelector(`${root}[data-inventory-state="loading"]`)
    finishRefresh([
      503,
      { message: "Snapshot service temporarily unavailable" },
    ])
    await page.waitForSelector(`${root} [role="alert"]`)
    assert.match(await text(), /Showing the previous snapshot/)
    assert.match(await text(), /Snapshot service temporarily unavailable/)
    assert.match(await text(), /9007199254740993/)
    for (const [status, message] of [[401, /authentication failed/], [403, /does not have permission/]]) {
      nextSnapshot = [status, {}]
      await page.click(refresh)
      await page.waitForSelector(`${root}[data-inventory-state="error"]`)
      assert.match(await text(), message)
      assert.doesNotMatch(await text(), /Showing the previous snapshot|BIKE_RENTAL_EVENTS/)
      nextSnapshot = undefined
      await page.click(refresh)
      await page.waitForSelector(`${root}[data-inventory-state="ready"]`)
    }
    nextSnapshot = [200, { ...brokerSnapshot, sources: [] }]
    await page.click(refresh)
    await page.waitForSelector(`${root}[data-inventory-state="ready"]`)
    assert.match(await text(), /No broker sources reported/)
    assert.doesNotMatch(await text(), /BIKE_RENTAL_EVENTS/)

    await page.setViewport({ width: 390, height: 844 })
    nextSnapshot = undefined
    const paths = [infrastructurePath, domainPath, workloadPath]
    const counts = () => Object.fromEntries(paths.map(path => [path, backend.requests.filter(req => req.path === path).length]))
    const beforeModels = counts()
    await prepareInventoryScenario(page, "read-models")
    const readModels = '[data-inventory-view="read-models"]'
    const readCanvas = '[aria-label="Read models canvas"]'
    const inspectModel = (label) => clickVisible(page, `${readCanvas} button[aria-label="${label}"]`)
    await checkResourceCanvas(page, readCanvas)
    await page.waitForSelector(`${readCanvas} [data-inventory-edge="declares-store"] path`, { visible: true, timeout: 5_000 })
    assert.equal(await page.$$eval(`${readCanvas} [data-inventory-edge]`, edges => edges.length), 1, "Only explicit policy gets a link")
    assert.equal(await page.$(overlay), null)
    await inspectModel("Inspect read model active-rentals")
    await page.waitForFunction(() =>
      document
        .querySelector('[aria-label="Container memory"]')
        ?.textContent.includes("32 MiB")
    )
    const models = await text(overlay)
    const beforeDetails = counts()
    for (const path of paths) assert.equal(beforeDetails[path] - beforeModels[path], 1, `${path} is sampled once`)
    assert.match(models, /Limit: 128 MiB/)
    assert.match(models, /KV_active-rentals/)
    assert.doesNotMatch(models, /Sampled backing stream bytes/)
    await page.keyboard.press("Escape")
    const unmapped = `${readCanvas} [data-inventory-resource="model"]:not([aria-label="Inspect read model active-rentals"])`
    await clickVisible(page, unmapped)
    assert.match(await text(overlay), /Storage mapping not declared/)
    await page.keyboard.press("Escape")
    await inspectModel("Inspect declared kv store active-rentals")
    assert.match(await text(overlay), /Sampled backing stream bytes\s+12 KiB used/)
    assert.match(await text(overlay), /Broker sample received/)
    assert.deepEqual(counts(), beforeDetails)
    nextDomain = [503, { message: "Projection declarations temporarily unavailable" }]
    await page.click(`${readModels} button[aria-label="Refresh snapshot"]`)
    await page.waitForSelector(`${readModels}[data-inventory-state="error"]`)
    assert.match(await text(readModels), /Showing the previous snapshot/)
    assert.match(await text(overlay), /Sampled backing stream bytes\s+12 KiB used/)
    assert.equal(backend.requests.filter(req => req.path === workloadPath).length, beforeDetails[workloadPath], "A failed declarations refresh does not resample the previous workload")
    await page.keyboard.press("Escape")
    await inspectModel("Inspect read model active-rentals")
    assert.match(await text(overlay), /Usage: 32 MiB · Limit: 128 MiB/)
    assert.ok(
      await page.$eval(
        readModels,
        (root) => root.scrollWidth <= root.clientWidth
      )
    )
    advertiseInfrastructure = false
    const beforeUnavailable = backend.requests.filter(
      ({ path }) => path === infrastructurePath
    ).length
    await page.click('[data-workspace-tab="infrastructure"]')
    await page.waitForSelector(`${root}[data-inventory-state="unavailable"]`)
    assert.match(await text(), /not advertised by Tracer for inspection/)
    assert.equal(
      backend.requests.filter(({ path }) => path === infrastructurePath).length,
      beforeUnavailable
    )

    const snapshots = backend.requests.filter(({ path }) =>
      [infrastructurePath, domainPath].includes(path)
    )
    assert.ok(snapshots.some(({ path }) => path === infrastructurePath))
    assert.ok(snapshots.some(({ path }) => path === domainPath))
    assert.ok(
      snapshots.every(
        ({ method, inspection }) => method === "GET" && inspection
      )
    )
    assert.ok(
      backend.requests.some(
        ({ path, inspection }) => path === "/catalog" && inspection
      )
    )
    assert.deepEqual(diagnostics.pageErrors, [])
  } finally {
    try {
      await closeStudioBrowser(browser)
    } finally {
      await server.close()
    }
  }
}

async function checkResourceCanvas(page, selector) {
  await page.waitForSelector(`${selector} .react-flow__pane`, visible)
  const { canvas, workspace, actions } = await page.$eval(selector, (root) => ({
    canvas: root.querySelector(".react-flow__pane").getBoundingClientRect().toJSON(),
    workspace: root.closest("#studio-workspace").getBoundingClientRect().toJSON(),
    actions: root.querySelector(".inventory-actions").getBoundingClientRect().toJSON(),
  }))
  assert.ok(Math.abs(canvas.top - workspace.top) <= 2 && canvas.height >= workspace.height * 0.95, "Canvas starts directly under tabs and fills workspace")
  assert.ok(actions.top - canvas.top < 20 && actions.left - canvas.left < 20, "Actions float at top-left")
}

const domainCanvas = '[aria-label="Domain canvas"]'
const details = '[aria-label="Domain details"]'
const allContexts = 'button[aria-label="Back to Domain model"]'
const graphNode = (id) => `${domainCanvas} button[data-domain-node="${id}"]`
const visible = { visible: true, timeout: 3_000 }

async function checkDomain(page) {
  await prepareInventoryScenario(page, "domain")
  await checkDomainCanvas(page)
  await page.setViewport({ width: 390, height: 844 })
  await page.reload({ waitUntil: "networkidle0" })
  await prepareInventoryScenario(page, "domain")
  await checkDomainCanvas(page)
}

async function checkDomainCanvas(page) {
  const width = page.viewport().width
  const assertUnselected = async () => {
    assert.equal(await page.$(details), null, "No details without selection")
    assert.equal(
      await page.$(`${domainCanvas} [data-domain-node][aria-pressed="true"]`),
      null
    )
  }
  await page.waitForSelector(domainCanvas, visible)
  await settlePage(page)
  const { canvas, workspace } = await page.$eval(domainCanvas, (root) => ({
    canvas: root
      .querySelector(".react-flow__pane")
      .getBoundingClientRect()
      .toJSON(),
    workspace: root
      .closest("#studio-workspace")
      .getBoundingClientRect()
      .toJSON(),
  }))
  assert.ok(
    Math.abs(canvas.left - workspace.left) <= 4 &&
      Math.abs(canvas.top - workspace.top) <= 2 &&
      Math.abs(canvas.right - workspace.right) <= 4 &&
      canvas.height >= workspace.height * 0.75,
    `Domain must fill the workspace at ${width}px: ${JSON.stringify({ canvas, workspace })}`
  )
  assert.equal(
    await page.$('[aria-label="Domain view"], [data-inventory-context]'),
    null,
    "There is no alternate Domain list or view switch"
  )
  await assertUnselected()
  assert.equal(
    await page.$(graphNode(collectionRequestedNodeId)),
    null,
    "Overview must not fit every aggregate's children"
  )
  if (width === desktopViewport.width) {
    await clickVisible(page, graphNode("context:bike-rental"))
    const commands = await page.waitForSelector(`${details} summary`, visible)
    await commands.click()
    const rent = await page.waitForSelector(`${details} [data-domain-command="rent-bicycle"] summary`, visible)
    await rent.click()
    assert.match(await page.$eval(details, el => el.innerText), /fleet_id\s+FleetId\s+bicycle_id\s+BicycleId/)
    await page.keyboard.press("Escape")
  }
  await clickVisible(page, graphNode(fleetNodeId))
  assert.ok(await page.$(graphNode(collectionRequestedNodeId)), "Opening the fleet must show its events immediately")
  assert.ok(await page.$(graphNode("entity:bike-rental/rental-fleet/bicycle")), "Entities share the same canvas as events")
  await page.waitForSelector(allContexts, visible)
  await assertUnselected()
  await settlePage(page)
  const aggregateGeometry = await domainGeometry(page)

  // In its own scope, clicking the aggregate inspects rather than drilling again.
  await clickVisible(page, graphNode(fleetNodeId))
  await page.waitForSelector(details, visible)
  assert.match(
    await page.$eval(details, (element) => element.innerText),
    /Rental fleet/
  )
  const close = `${details} button[aria-label="Close details"]`
  await page.click(close)
  await assertUnselected()
  assert.deepEqual(
    await domainGeometry(page),
    aggregateGeometry,
    "Scoped inspection must not renavigate"
  )

  const event = graphNode(collectionRequestedNodeId)
  const fit = `${domainCanvas} button[aria-label="Fit domain to view"]`
  await page.click(fit)
  await waitForHitTarget(page, event)
  await settlePage(page)
  if (width === desktopViewport.width) await checkDomainEdges(page)
  const before = await domainGeometry(page)

  for (const dismiss of [
    () => page.click(close),
    () => page.keyboard.press("Escape"),
  ]) {
    await clickVisible(page, event)
    await page.waitForSelector(details, visible)
    await settlePage(page)
    assert.equal(
      await page.$eval(event, (node) => node.getAttribute("aria-pressed")),
      "true"
    )
    const content = await page.$eval(details, (element) => element.innerText)
    assert.ok(content.includes(collectionRequestedLabel))
    assert.match(content, /\b(?:Schema(?: version)?\s+v?1|v1)\b/i)
    for (const [field, label] of [
      ["fleet_id", "Fleet ID"],
      ["reservation_id", "Reservation ID"],
      ["bicycle_id", "Bicycle ID"],
    ]) {
      assert.match(content, new RegExp(`${field}\\s+${label} \\(string\\)`))
    }
    assert.match(
      content,
      /\bpayment_id\s+PaymentId\b/,
      "Opaque semantics do not hide a declared payload type"
    )
    assert.doesNotMatch(
      content,
      /\bOpaque\b|Raw descriptor|"(?:kind|schemaVersion)"\s*:/i
    )

    const overlay = await page.$eval(details, (node) =>
      node.getBoundingClientRect().toJSON()
    )
    const pane = before.canvas
    assert.ok(
      overlay.width > 0 &&
        overlay.height > 0 &&
        overlay.left >= pane.left - 1 &&
        overlay.right <= pane.right + 1 &&
        overlay.top >= pane.top - 1 &&
        overlay.bottom <= pane.bottom + 1,
      `Domain details must overlay the actual canvas at ${width}px: ${JSON.stringify({ overlay, pane })}`
    )
    assert.deepEqual(
      await domainGeometry(page),
      before,
      "Opening details must not resize, pan or zoom the graph"
    )
    assert.ok(
      await page.$$eval(`${domainCanvas} [data-domain-node]`, (nodes) =>
        nodes.some((node) => {
          const { x, y, width, height } = node.getBoundingClientRect()
          return node.contains(document.elementFromPoint(x + width / 2, y + height / 2))
        })
      ),
      `At least one graph card must remain visible outside details at ${width}px`
    )
    await dismiss()
    await assertUnselected()
    await settlePage(page)
    assert.deepEqual(
      await domainGeometry(page),
      before,
      "Dismissing details must not move the graph"
    )
  }

  await clickVisible(page, event)
  await page.waitForSelector(details, visible)
  await checkDomainCamera(page)
  await page.keyboard.press("Escape")
  await page.click(allContexts)
  await waitForHitTarget(page, graphNode(fleetNodeId))
  assert.equal(await page.$(event), null)
  await assertUnselected()
}

async function checkDomainEdges(page) {
  await page.waitForSelector(`${domainCanvas} [data-domain-edge="declares-event"]`)
  const leaf = graphNode(collectionRequestedNodeId)
  const traced = `${domainCanvas} [data-edge-traced="true"]`
  await page.hover(leaf)
  await page.waitForSelector(`${traced}[data-target-id="${collectionRequestedNodeId}"]`, visible)
  assert.equal(await page.$(details), null, "Tracing a relationship does not open details")
  assert.deepEqual(await page.$$eval(traced, paths => paths.map(p => p.dataset.sourceId)), [fleetNodeId])
  const edges = await page.$$eval(
    `${domainCanvas} [data-domain-edge]`,
    (edges) =>
      edges.map((edge) => [
        edge.dataset.sourceId,
        edge.dataset.targetId,
        edge.dataset.domainEdge,
      ])
  )
  assert.deepEqual(edges.filter(e => e[2] === 'declares-event').sort(), [
    [fleetNodeId, "event:bike-rental/rental-fleet/bicycle-rented", "declares-event"],
    [fleetNodeId, collectionRequestedNodeId, "declares-event"],
  ].sort(), "Event cards belong to their declared aggregate")
  const { cards, obscured, missing } = await page.$eval(domainCanvas, (root) => {
    const cards = [...root.querySelectorAll("[data-domain-node]")].map(
      (node) => {
        const bounds = node.getBoundingClientRect().toJSON()
        return {
          id: node.dataset.domainNode,
          ...bounds,
        }
      }
    )
    const obscured = []
    const missing = []
    for (const edge of root.querySelectorAll("[data-domain-edge]")) {
      const paths = edge.querySelectorAll("path.domain-edge")
      const link = `${edge.dataset.sourceId} → ${edge.dataset.targetId}`
      if (!paths.length) missing.push(link)
      for (const path of paths) {
        const matrix = path.getScreenCTM()
        const length = path.getTotalLength()
        for (let distance = 0; distance < length; distance += 2) {
          const { x, y } = path.getPointAtLength(distance).matrixTransform(matrix)
          const hit = cards.find(
            (card) => x > card.left + 1 && x < card.right - 1 && y > card.top + 1 && y < card.bottom - 1
          )
          if (hit) {
            obscured.push(`${link} through ${hit.id}`)
            break
          }
        }
      }
    }
    return { cards, obscured, missing }
  })
  for (const [i, a] of cards.entries()) {
    for (const b of cards.slice(i + 1)) {
      assert.ok(a.right <= b.left || b.right <= a.left || a.bottom <= b.top || b.bottom <= a.top, `${a.id} overlaps ${b.id}`)
    }
  }
  assert.deepEqual(missing, [], "Each explicit link must have a visible stroke")
  assert.deepEqual(obscured, [], "Links must not pass through cards")
}

async function checkDomainCamera(page) {
  const camera = `${domainCanvas} .react-flow__viewport`
  const readCamera = () =>
    page.$eval(camera, (element) => {
      const matrix = new DOMMatrixReadOnly(getComputedStyle(element).transform)
      return { x: matrix.e, y: matrix.f, scale: matrix.a }
    })
  const { scale } = await readCamera()
  await clickVisible(page, `${domainCanvas} button[aria-label="Zoom in"]`)
  await page.waitForFunction(
    (selector, before) =>
      new DOMMatrixReadOnly(
        getComputedStyle(document.querySelector(selector)).transform
      ).a > before,
    { timeout: 3_000 },
    camera,
    scale
  )
  await settlePage(page)
  const before = await readCamera()
  const point = await page.$eval(
    `${domainCanvas} .react-flow__pane`,
    (pane) => {
      const { left, top, right, bottom } = pane.getBoundingClientRect()
      // Use exposed background, not an arbitrary coordinate that could be a node or overlay.
      for (let y = top + 12; y < bottom - 42; y += 24) {
        for (let x = left + 12; x < right - 72; x += 24) {
          if (
            [0, 0.5, 1].every(
              (t) => document.elementFromPoint(x + 60 * t, y + 30 * t) === pane
            )
          ) {
            return { x, y }
          }
        }
      }
      return null
    }
  )
  assert.ok(point, "The details overlay must leave room to drag the canvas")
  await page.mouse.move(point.x, point.y)
  await page.mouse.down()
  await page.mouse.move(point.x + 60, point.y + 30, { steps: 6 })
  await page.mouse.up()
  const after = await readCamera()
  assert.ok(
    after.x > before.x && after.y > before.y,
    "Dragging the canvas should pan the viewport"
  )
}

async function domainGeometry(page) {
  return page.$eval(domainCanvas, (root) => ({
    canvas: root
      .querySelector(".react-flow__pane")
      .getBoundingClientRect()
      .toJSON(),
    transform: root.querySelector(".react-flow__viewport").style.transform,
    nodes: [...root.querySelectorAll("[data-domain-node]")].map((node) => ({
      id: node.dataset.domainNode,
      bounds: node.getBoundingClientRect().toJSON(),
    })),
  }))
}

async function waitForHitTarget(page, selector) {
  await page.waitForFunction(
    (selector) => {
      const target = document.querySelector(selector)
      if (!target) return false
      const { x, y, width, height } = target.getBoundingClientRect()
      return (
        width > 0 &&
        height > 0 &&
        target.contains(
          document.elementFromPoint(x + width / 2, y + height / 2)
        )
      )
    },
    { timeout: 3_000 },
    selector
  )
}

async function clickVisible(page, selector) {
  await waitForHitTarget(page, selector)
  await page.click(selector)
}

if (
  process.argv[1] &&
  import.meta.url === pathToFileURL(process.argv[1]).href
) {
  const domainOnly = process.argv.includes("--domain-only")
  await checkInventory({ domainOnly, resourcesOnly: process.argv.includes("--resources-only") })
  console.log(
    `${domainOnly ? "Domain inventory" : "Inventory"} browser checks passed`
  )
}
