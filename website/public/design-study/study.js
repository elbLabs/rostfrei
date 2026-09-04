const directions = {
  foundry: {
    title: "01 / Foundry",
    caption: "warm paper, oxide, precise typography.",
    heading: "Domain logic.<br /><em>In plain sight.</em>",
    description: "An earlier exploration: warm paper, iron-toned neutrals, oxide, and ruled layouts. The website takes the form of a technical manual. Retained for comparison with the selected Reference ’15 direction.",
    type: "IBM Plex Sans + IBM Plex Mono. Small corners. No shadows. Rust used sparingly.",
    references: [
      ["IBM Plex", "https://www.ibm.com/plex/", "An engineered type family with human detail."],
      ["Teenage Engineering TX–6", "https://teenage.engineering/products/tx-6", "Precise labels and a material-led instrument identity."],
    ],
  },
  instrument: {
    title: "02 / Instrument",
    caption: "cool steel, burnt orange, an open workbench.",
    heading: "Model. Execute.<br /><em>Inspect.</em>",
    description: "The foundation for the 2010s explorations. Cool steel surfaces offset the warm rust accent. A wider, diagram-led homepage and compact Studio share square controls, measured spacing, and strong alignment. Reference ’15 develops this into the selected documentation-led direction.",
    type: "IBM Plex Sans + IBM Plex Mono. Square edges. Wide composition. Compact tool chrome.",
    references: [
      ["Oxide / Pentagram", "https://www.pentagram.com/work/oxide", "One technical visual language across product and website."],
      ["Carbon Design System", "https://carbondesignsystem.com/", "Structured grids and productive interface density."],
    ],
  },
  fieldnotes: {
    title: "03 / Fieldnotes",
    caption: "chalk, aubergine, terracotta, an editorial voice.",
    heading: "Good systems start<br />with a <em>clear model.</em>",
    description: "The most distinctive and editorial option. A serif headline, terracotta annotations, and long horizontal rules give the site the character of a field manual. Studio keeps its data in sans and mono; the serif is reserved for orientation.",
    type: "Source Serif 4 for headlines; IBM Plex Sans + Mono for the working interface. Content first.",
    references: [
      ["Ink & Switch", "https://www.inkandswitch.com/essay/local-first/", "Technical evidence integrated with editorial explanation."],
      ["Tufte CSS", "https://edwardtufte.github.io/tufte-css/", "Ruled figures, restrained color, and useful annotations."],
    ],
  },
  workshop: {
    title: "04 / Workshop ’12",
    era: true,
    caption: "the early-2010s project site: steel chrome, useful panels, rust details.",
    heading: "Domain models, events,<br />and messages. <em>In Rust.</em>",
    description: "A project homepage with the confidence of a useful piece of software. A centered canvas, visible documentation, compact Helvetica-style type, restrained button depth, and framed code. Studio becomes a docked inspector with proper tabs. Inspired by early-2010s interfaces; the dark theme is a contemporary adaptation.",
    type: "Arial / Helvetica + system monospace. 34px headline, 14px body. Shallow control gradients, 3px corners, dense navigation.",
    references: [
      ["GitHub, September 2012", "https://github.blog/news-insights/how-we-keep-github-fast/", "Period repository and trace screenshots: framed controls, tabs, and dense diagnostic rows."],
      ["Redis.io, May 2012", "https://github.com/redis/redis-io/tree/3f230d2b974ced753a3c4a5057e006a01d7058b7", "Versioned source: 900px canvas, 26px headings, runnable example, resource sidebar."],
      ["Bootstrap 2.0.4, June 2012", "https://getbootstrap.com/2.0.4/components.html", "Preserved components: joined buttons, explicit tabs, shallow gradients."],
    ],
  },
  reference: {
    title: "05 / Reference ’15",
    era: true,
    caption: "the mid-2010s reference site: navigation first, content at working scale.",
    heading: "A Rust framework for <br /><em>your domain model.</em>",
    description: "The selected direction pairs the supplied rust-orange duck with a documentation-led front door. The full mascot accompanies the introduction; a head crop identifies the website and Studio. Permanent navigation, modest headings, and real source keep the product immediately useful. Both themes share flat, high-contrast surfaces.",
    type: "System sans + monospace. 32px headline, 14px body. Original duck artwork with a fine silhouette keyline for dark surfaces. Compact navigation, tables, and square panels.",
    references: [
      ["Rust, May 2015", "https://github.com/rust-lang/prev.rust-lang.org/tree/77e65109c281aa56216a8967136f495170825227", "Period source: a small product statement, install controls, and a runnable example."],
      ["GitLab 8.0, September 2015", "https://github.com/gitlabhq/gitlabhq/tree/v8.0.0/app/assets/stylesheets", "Steel-tinted working surfaces, a 230px project sidebar, and 15px UI type."],
      ["Read the Docs 0.1.9, 2015", "https://github.com/readthedocs/sphinx_rtd_theme/tree/0.1.9", "Persistent contents, breadcrumbs, and a bounded reading surface."],
    ],
  },
}

const commands = {
  rent: {
    title: "Rent bicycle",
    type: "RentBicycle",
    fixture: "demo-fleet",
    source: "rent-available-bicycle.json",
    nodes: [
      { kind: "Command", name: "rent-bicycle", schema: 2, icon: "□", description: "Requests a rental for bike-42 in city-fleet." },
      { kind: "Domain event", name: "bicycle-rented", schema: 1, icon: "◇", description: "Raised when the rental action accepts an available bicycle." },
      { kind: "Integration event", name: "bicycle-rental-started", schema: 1, icon: "↗", description: "Published from the committed bicycle-rented domain event." },
    ],
  },
  return: {
    title: "Return bicycle",
    type: "ReturnBicycle",
    fixture: "rented-demo-fleet",
    source: "return-rented-bicycle.json",
    nodes: [
      { kind: "Command", name: "return-bicycle", schema: 2, icon: "□", description: "Requests the return of bike-42 in city-fleet." },
      { kind: "Domain event", name: "bicycle-returned", schema: 1, icon: "◇", description: "Raised when the return action accepts a rented bicycle." },
    ],
  },
}

const modes = {
  simulate: { note: "Read-only · no persisted events", environment: "Simulate / read-only", button: "Preview simulation" },
  test: { note: "Isolated state · resettable fixtures", environment: "Test / isolated", button: "Preview test" },
  dispatch: { note: "Production · separate authorization required", environment: "Dispatch / production", button: "Dispatch unavailable" },
}

const root = document.documentElement
const media = window.matchMedia("(prefers-color-scheme: dark)")
const params = new URLSearchParams(window.location.search)
const state = {
  direction: Object.hasOwn(directions, params.get("direction")) ? params.get("direction") : "reference",
  theme: ["light", "dark"].includes(params.get("theme")) ? params.get("theme") : media.matches ? "dark" : "light",
  view: params.get("view") === "studio" ? "studio" : "website",
  command: "rent",
  mode: "test",
  node: 1,
}

const element = (id) => document.getElementById(id)

function syncPressed(attribute, value) {
  document.querySelectorAll(`button[data-${attribute}]`).forEach((button) => {
    button.setAttribute("aria-pressed", String(button.dataset[attribute] === value))
  })
}

function updateAddress() {
  const url = new URL(window.location.href)
  for (const key of ["direction", "theme", "view"]) url.searchParams.set(key, state[key])
  window.history.replaceState(null, "", url)
}

function renderPalette() {
  const styles = getComputedStyle(root)
  element("palette-heading").textContent = `${state.theme === "light" ? "Light" : "Dark"} palette`
  const tokens = [["Ground", "bg"], ["Surface", "surface"], ["Text", "ink"], ["Secondary", "muted"], ["Rust", "accent"], ["Success", "success"]]
  element("palette").replaceChildren(...tokens.map(([label, token]) => {
    const value = styles.getPropertyValue(`--${token}`).trim()
    const item = document.createElement("div")
    item.className = "swatch"
    const chip = document.createElement("span")
    chip.className = "swatch-chip"
    chip.style.backgroundColor = value
    chip.setAttribute("aria-hidden", "true")
    const code = document.createElement("code")
    code.textContent = value.toUpperCase()
    item.append(chip, label, code)
    return item
  }))
  document.querySelector('meta[name="theme-color"]').content = styles.getPropertyValue("--bg").trim()
}

function renderDirection() {
  const design = directions[state.direction]
  root.dataset.direction = state.direction
  root.dataset.theme = state.theme
  root.dataset.view = state.view
  root.dataset.era = String(Boolean(design.era))
  document.querySelectorAll("[data-dark-asset]").forEach((source) => {
    source.media = state.theme === "dark" ? "all" : "not all"
  })
  syncPressed("direction", state.direction)
  syncPressed("view", state.view)
  element("website-preview").hidden = state.view !== "website"
  element("studio-preview").hidden = state.view !== "studio"
  element("direction-caption").textContent = `${design.title} — ${design.caption}`
  element("hero-heading").innerHTML = design.heading
  element("notes-heading").textContent = design.title
  element("design-description").textContent = design.description
  element("design-type").textContent = design.type
  element("references").replaceChildren(...design.references.map(([label, url, description]) => {
    const reference = document.createElement("div")
    reference.className = "reference"
    const link = document.createElement("a")
    link.href = url
    link.textContent = `${label} ↗`
    const text = document.createElement("span")
    text.textContent = description
    reference.append(link, text)
    return reference
  }))
  const nextTheme = state.theme === "light" ? "Dark" : "Light"
  element("theme-toggle").innerHTML = `${nextTheme} <span aria-hidden="true">◐</span>`
  element("theme-toggle").setAttribute("aria-label", `Use ${nextTheme.toLowerCase()} theme`)
  document.title = `rostfrei — ${design.title.slice(5)} / ${state.view === "studio" ? "Studio" : "Website"} / ${state.theme}`
  renderPalette()
  updateAddress()
}

function renderInspector() {
  const node = commands[state.command].nodes[state.node]
  element("inspector-kind").textContent = node.kind
  element("inspector-name").textContent = node.name
  element("inspector-description").textContent = node.description
  element("message-schema").textContent = `schema v${node.schema}`
  document.querySelectorAll("[data-node]").forEach((button) => {
    button.setAttribute("aria-pressed", String(Number(button.dataset.node) === state.node))
  })
}

function renderStudio() {
  const command = commands[state.command]
  const mode = modes[state.mode]
  syncPressed("command", state.command)
  syncPressed("mode", state.mode)
  element("breadcrumb-command").textContent = command.type
  element("command-title").innerHTML = `${command.title}<span>.</span>`
  element("fixture-name").textContent = command.fixture
  element("mode-note").textContent = mode.note
  element("trace-environment").textContent = mode.environment
  element("sample-run").innerHTML = `${mode.button} <span aria-hidden="true">→</span>`
  element("sample-run").disabled = state.mode === "dispatch"
  element("run-note").textContent = state.mode === "dispatch"
    ? "This design sample has no connection or production authorization."
    : state.mode === "simulate"
      ? "Illustrative proposal only. No writes or integration-event delivery."
      : "Illustrative result from the repository fixture. No runtime request."
  // Simulation previews the command decision and proposed domain event only.
  const nodes = state.mode === "simulate" ? command.nodes.slice(0, 2) : command.nodes
  if (state.node >= nodes.length) state.node = 1
  element("trace-title").textContent = state.mode === "simulate" ? "Proposed domain events" : "Expected message flow"
  element("trace-count").textContent = state.mode === "simulate" ? "01 proposed event" : `${String(nodes.length).padStart(2, "0")} messages`
  element("trace-footnote").textContent = state.mode === "simulate"
    ? "Illustrative decision and proposed event. Nothing is committed or published."
    : `Expected messages from ${command.source}; not a live trace.`
  element("fixture-source").href = `https://github.com/elbLabs/rostfrei/blob/main/examples/bike-rental/tests/tracer/${command.source}`
  element("trace-nodes").replaceChildren(...nodes.map((node, index) => {
    const button = document.createElement("button")
    button.className = "trace-node"
    button.dataset.node = String(index)
    button.setAttribute("aria-label", `Inspect ${node.kind.toLowerCase()}: ${node.name}`)
    button.innerHTML = `<span class="trace-icon" aria-hidden="true">${node.icon}</span><span class="trace-node-content"><span>${node.kind}${state.mode === "simulate" && index === 1 ? " / proposed" : ""} · v${node.schema}</span><strong>${node.name}</strong></span>`
    button.addEventListener("click", () => {
      state.node = index
      renderInspector()
    })
    return button
  }))
  element("sample-status").textContent = ""
  renderInspector()
}

document.querySelectorAll("button[data-direction]").forEach((button) => {
  button.addEventListener("click", () => {
    state.direction = button.dataset.direction
    renderDirection()
    // Return keyboard focus to the disclosure before hiding an earlier-study button.
    const disclosure = button.closest("details")
    if (disclosure) disclosure.querySelector("summary").focus()
    document.querySelector(".earlier-studies").open = false
  })
})

function setView(view) {
  state.view = view
  renderDirection()
}

document.querySelectorAll("button[data-view]").forEach((button) => {
  button.addEventListener("click", () => setView(button.dataset.view))
})

document.querySelectorAll("[data-open-studio], [data-open-website]").forEach((button) => {
  button.addEventListener("click", () => {
    setView(button.hasAttribute("data-open-studio") ? "studio" : "website")
    element("preview").focus({ preventScroll: true })
    window.scrollTo({ top: 0, behavior: "instant" })
  })
})

element("theme-toggle").addEventListener("click", () => {
  state.theme = state.theme === "light" ? "dark" : "light"
  renderDirection()
})

document.querySelectorAll("button[data-command]").forEach((button) => {
  button.addEventListener("click", () => {
    state.command = button.dataset.command
    state.node = 1
    renderStudio()
  })
})

document.querySelectorAll("button[data-mode]").forEach((button) => {
  button.addEventListener("click", () => {
    state.mode = button.dataset.mode
    renderStudio()
  })
})

element("sample-run").addEventListener("click", () => {
  element("sample-status").textContent = state.mode === "simulate"
    ? "Sample simulation: accepted · one proposed domain event · no writes."
    : `Sample test: accepted · ${commands[state.command].nodes.length} expected messages · no runtime contacted.`
})

element("copy-command").addEventListener("click", async () => {
  const command = "cargo run --locked -p bike-rental --bin bike-rental-model"
  try {
    await navigator.clipboard.writeText(command)
    element("copy-status").textContent = "Command copied. Run it from the repository root."
  } catch {
    element("copy-status").textContent = "Select the command above to copy it."
  }
})

renderDirection()
renderStudio()
