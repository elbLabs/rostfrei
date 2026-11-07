import { MAX_PAYLOAD_MATCHES, payloadMatchProblem } from "@/lib/correlation-api"
import type { PayloadMatch } from "@/lib/types"

export interface MatchRow extends PayloadMatch {
  key: number
}

const STORED_MATCHES_KEY = "rostfrei-tracer-studio-correlation-matches-v1"
let nextKey = 0

export function matchRows(matches: PayloadMatch[]): MatchRow[] {
  return matches.map((match) => ({ ...match, key: ++nextKey }))
}

export function blankRow(context = ""): MatchRow {
  return { context, eventType: "", field: "", value: "", key: ++nextKey }
}

export function isBlank(row: PayloadMatch): boolean {
  return !row.context && !row.eventType && !row.field && !row.value
}

export function rowMatches(rows: MatchRow[]): PayloadMatch[] {
  return rows
    .filter((row) => !isBlank(row))
    .map(({ context, eventType, field, value }) => ({
      context: context.trim(),
      eventType: eventType.trim(),
      field: field.trim(),
      value,
    }))
}

export function readStoredMatches(aggregateType: string): PayloadMatch[] {
  try {
    const stored: unknown = JSON.parse(
      localStorage.getItem(STORED_MATCHES_KEY) ?? "{}"
    )
    const matches =
      typeof stored === "object" && stored !== null
        ? Reflect.get(stored, aggregateType)
        : undefined
    return Array.isArray(matches)
      ? matches
          .filter(
            (match): match is PayloadMatch =>
              ["context", "eventType", "field", "value"].every(
                (key) => typeof match?.[key] === "string"
              ) && !payloadMatchProblem(match)
          )
          .slice(0, MAX_PAYLOAD_MATCHES)
      : []
  } catch {
    return []
  }
}

export function storeMatches(aggregateType: string, matches: PayloadMatch[]) {
  try {
    const stored: unknown = JSON.parse(
      localStorage.getItem(STORED_MATCHES_KEY) ?? "{}"
    )
    const all =
      typeof stored === "object" && stored !== null && !Array.isArray(stored)
        ? (stored as Record<string, unknown>)
        : {}
    if (matches.length) all[aggregateType] = matches
    else delete all[aggregateType]
    localStorage.setItem(STORED_MATCHES_KEY, JSON.stringify(all))
  } catch {
    // Matches still apply when browser storage is unavailable.
  }
}
