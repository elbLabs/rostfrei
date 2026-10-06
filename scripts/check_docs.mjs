import assert from "node:assert/strict"
import { readFileSync, readdirSync } from "node:fs"
import { resolve } from "node:path"
import { fileURLToPath } from "node:url"

import { docsPages } from "../website/src/docs/navigation.ts"
import { documentationHref, documentationSource } from "../website/docs-plugin.ts"

const root = fileURLToPath(new URL("../", import.meta.url))
const docs = resolve(root, "docs")
const dist = resolve(root, "website/dist")
const base = process.env.VITE_BASE_PATH ?? "/"
const files = readdirSync(docs, { recursive: true })
  .filter((file) => /\.(md|mdx)$/.test(file)).sort()

assert.equal(new Set(docsPages.map((page) => page.slug)).size, docsPages.length)
assert.deepEqual(docsPages.map((page) => page.file).sort(), files,
  "Every shared documentation page must be published exactly once")

const index = readFileSync(resolve(dist, "llms.txt"), "utf8")
for (const page of docsPages) {
  const document = resolve(docs, page.file)
  const source = readFileSync(document, "utf8")
  const published = readFileSync(resolve(dist, "docs/source", page.file), "utf8")
  assert.equal(published, documentationSource(source, document), page.file)
  assert.ok(index.includes(`](${base}docs/source/${page.file})`), page.file)
  const entry = readFileSync(resolve(dist, "docs", page.slug, "index.html"), "utf8")
  assert.ok(entry.includes("data-rostfrei-meta"), `Missing page entry: ${page.slug}`)
  assert.ok(!source.includes("<MacroExample"), "Code examples must be readable in source")
}

const document = resolve(docs, "read-models/nats.mdx")
assert.equal(documentationHref("index.mdx#declare-the-data", document),
  "/docs/read-models#declare-the-data")
assert.equal(documentationHref("../read-models.md#resource-policy-and-permissions", document),
  "/docs/read-models/storage#resource-policy-and-permissions")
assert.equal(documentationHref("../README.md", document), "/docs/reference")
assert.equal(documentationHref("index.mdx#declare-the-data", document, "source"),
  "index.mdx#declare-the-data")
assert.equal(documentationHref("../../Cargo.toml", document),
  "https://github.com/elbLabs/rostfrei/blob/main/Cargo.toml")
assert.equal(documentationHref("#scope", document), "#scope")
assert.throws(() => documentationHref("missing.md", document), /Broken documentation link/)
const fenced = "```md\n[example](missing.md)\n```\n"
assert.equal(documentationSource(fenced, document), fenced)

console.log(`Verified ${docsPages.length} shared pages, published sources, machine index, and link routing`)
