// Bounded JSON-text prefix, preserving native raw-number lexemes. Never clones
// local core values or builds an unbounded payload string before truncation.
export function jsonPrefix(value, limit = 3072) {
  let text = ""
  let truncated = false
  let visited = 0
  function append(part) {
    const remaining = limit - text.length
    text += part.slice(0, Math.max(0, remaining))
    if (part.length > remaining) truncated = true
  }
  function visit(item, depth) {
    if (truncated) return
    if (++visited > 256 || depth > 16) { truncated = true; return }
    if (JSON.isRawJSON?.(item)) { append(item.rawJSON); return }
    if (typeof item === "string") {
      append('"')
      for (const char of item) {
        if (truncated) break
        // ASCII JSON escapes keep control/bidi/wide characters as inspectable
        // data, so a 40-character native detail row also fits 40 terminal cells.
        append(char.codePointAt(0) >= 127
          ? char.split("").map((unit) => `\\u${unit.charCodeAt(0).toString(16).padStart(4, "0")}`).join("")
          : JSON.stringify(char).slice(1, -1))
      }
      if (!truncated) append('"')
    } else if (item === null || typeof item !== "object") {
      append(JSON.stringify(item))
    } else {
      const array = Array.isArray(item)
      append(array ? "[" : "{")
      let first = true
      for (const key in item) {
        if (!Object.hasOwn(item, key)) continue
        if (!first) append(",")
        first = false
        if (!array) { visit(key, depth + 1); append(":") }
        visit(item[key], depth + 1)
        if (truncated) break
      }
      if (!truncated) append(array ? "]" : "}")
    }
  }
  visit(value, 0)
  return { text, truncated, limitCharacters: limit, maxDepth: 16, maxValues: 256 }
}

export function short(value, limit = 44) {
  const text = String(value).replace(/[\x00-\x1f\x7f-\x9f\u202a-\u202e\u2066-\u2069]/g, "?")
  return text.length <= limit ? text : `${text.slice(0, limit - 1)}…`
}

export const PAGE_SIZE = 160
export function page(text, offset, size = PAGE_SIZE) {
  if (!Number.isSafeInteger(offset) || offset < 0 || offset > text.length) throw new Error("Invalid detail offset")
  return { text: text.slice(offset, offset + size), offset, nextOffset: offset + size < text.length ? offset + size : null, totalCharacters: text.length }
}
