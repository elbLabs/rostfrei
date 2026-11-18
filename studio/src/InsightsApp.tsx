import { useEffect, useState } from "react"
import { Brand } from "@rostfrei/ui/brand"
import { ThemeToggle } from "@rostfrei/ui/theme"
import { EvidenceDisclosure } from "@rostfrei/ui/evidence"
import { Button } from "@/components/ui/button"
import {
  insightsSession,
  signInToInsights,
  type AccessRequest,
  type InsightQuery,
  type InsightResult,
  type InsightsSession,
} from "@/lib/insights"

export default function InsightsApp() {
  const [session, setSession] = useState<InsightsSession>()
  const [error, setError] = useState("")
  const [loading, setLoading] = useState(true)
  useEffect(() => {
    let active = true
    insightsSession()
      .then((session) => {
        if (active) setSession(session)
      })
      .catch((error: unknown) => {
        if (active) setError(message(error))
      })
      .finally(() => {
        if (active) setLoading(false)
      })
    return () => {
      active = false
    }
  }, [])
  if (session) return <InsightsWorkspace session={session} />
  return (
    <main className="insights-shell">
      <header className="insights-header">
        <Brand />
        <ThemeToggle />
      </header>
      <h1>Governed insights</h1>
      <p>
        Inspect application evidence with field-level permissions and recorded
        access.
      </p>
      {error && <p role="alert">{error}</p>}
      <Button
        disabled={loading}
        onClick={() => {
          void signInToInsights().catch((error: unknown) =>
            setError(message(error))
          )
        }}
      >
        {loading ? "Connecting…" : "Sign in with Keycloak"}
      </Button>
    </main>
  )
}

export function InsightsWorkspace({ session }: { session: InsightsSession }) {
  const [tab, setTab] = useState("query")
  const [error, setError] = useState("")
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<InsightResult>()
  const [queryText, setQueryText] = useState("")
  const [submitted, setSubmitted] = useState<InsightQuery>()
  const [request, setRequest] = useState<AccessRequest>()
  const [requests, setRequests] = useState<AccessRequest[]>([])
  const [policy, setPolicy] = useState("")
  const [revision, setRevision] = useState<number>()
  const [audit, setAudit] = useState<unknown>()
  const [notice, setNotice] = useState("")

  async function perform(task: () => Promise<void>) {
    setBusy(true)
    setError("")
    setNotice("")
    try {
      await task()
    } catch (error) {
      setError(message(error))
    } finally {
      setBusy(false)
    }
  }

  async function runQuery(approve: boolean, grantId?: string) {
    setResult(undefined)
    const query =
      grantId && submitted ? submitted : (JSON.parse(queryText) as InsightQuery)
    const body = { ...query, requestMissing: approve, requestId: grantId }
    const result = await session.request<InsightResult>("/insights/query", body)
    setSubmitted(query)
    setResult(result)
    setRequest(undefined)
  }

  async function refreshRequests() {
    setRequests(
      await session.request<AccessRequest[]>("/insights/access-requests")
    )
  }

  const tabs = [
    ["query", "Query", "insights.read"],
    ["requests", "Access requests", "insights.approve"],
    ["policies", "Policies", "insights.policy.manage"],
    ["audit", "Access journal", "insights.audit.read"],
  ].filter(([, , role]) => session.roles.includes(role))
  const activeTab = tabs.some(([id]) => id === tab) ? tab : tabs[0]?.[0]

  return (
    <main className="insights-shell">
      <header className="insights-header">
        <Brand />
        <div className="insights-actions">
          <ThemeToggle />
          <Button
            variant="outline"
            onClick={() => {
              setResult(undefined)
              setAudit(undefined)
              session.logout()
            }}
          >
            Sign out
          </Button>
        </div>
      </header>
      <h1>Governed insights</h1>
      <p className="insights-identity">
        {session.subject} · {session.clientId}
      </p>
      <nav className="insights-tabs" aria-label="Insights workspace">
        {tabs.map(([id, label]) => (
          <Button
            key={id}
            variant={activeTab === id ? "default" : "ghost"}
            aria-current={activeTab === id ? "page" : undefined}
            onClick={() => {
              setTab(id)
              setError("")
              setNotice("")
            }}
          >
            {label}
          </Button>
        ))}
      </nav>
      {error && (
        <p role="alert" className="insights-error">
          {error}
        </p>
      )}
      {notice && <p role="status">{notice}</p>}
      {activeTab === "query" && (
        <section aria-label="Query evidence">
          <h2>Query evidence</h2>
          <p>
            Choose a source, registered type, fields, and purpose. Protected
            fields require approval. Every read is recorded.
          </p>
          <label htmlFor="insights-query">Query JSON</label>
          <textarea
            id="insights-query"
            value={queryText}
            onChange={(event) => {
              setQueryText(event.target.value)
              setResult(undefined)
              setSubmitted(undefined)
              setRequest(undefined)
            }}
            rows={8}
            spellCheck={false}
            placeholder={
              '{"selection":{"source":"people","resource":{"kind":"aggregate","type":"identity/user","schemaVersion":1},"aggregateType":"identity/user","aggregateId":"user-42"},"fields":["/age","/name"],"purpose":"support"}'
            }
          />
          <div className="insights-actions">
            <Button
              disabled={busy || !queryText.trim()}
              onClick={() => void perform(() => runQuery(false))}
            >
              Read available fields
            </Button>
            {session.roles.includes("insights.request") && (
              <Button
                variant="outline"
                disabled={busy || !queryText.trim()}
                onClick={() => void perform(() => runQuery(true))}
              >
                Request protected fields
              </Button>
            )}
          </div>
          {result && (
            <div className="insights-result">
              <h3>
                {result.status === "partial"
                  ? "Partial result"
                  : "Available evidence"}
              </h3>
              <p>
                Audit reference: <code>{result.auditId}</code> · Policy revision{" "}
                {result.policyRevision}
              </p>
              {result.status === "partial" && (
                <p>
                  Some requested data is restricted. An empty result does not
                  establish that protected records are absent.
                </p>
              )}
              <pre>{JSON.stringify(result.records, null, 2)}</pre>
              {result.requestId && (
                <div className="insights-actions">
                  <span>
                    Access request <code>{result.requestId}</code>
                    {request && ` · ${request.status}`}
                  </span>
                  <Button
                    variant="outline"
                    disabled={busy}
                    onClick={() =>
                      void perform(async () => {
                        setRequest(
                          await session.request<AccessRequest>(
                            `/insights/access-requests/${encodeURIComponent(result.requestId!)}`
                          )
                        )
                      })
                    }
                  >
                    Check approval
                  </Button>
                  {request?.status === "approved" && (
                    <Button
                      disabled={busy}
                      onClick={() =>
                        void perform(() => runQuery(false, request.id))
                      }
                    >
                      Retrieve approved fields
                    </Button>
                  )}
                </div>
              )}
            </div>
          )}
        </section>
      )}
      {activeTab === "requests" && (
        <section aria-label="Access requests">
          <h2>Access requests</h2>
          <p>
            Approve a frozen selection for up to 15 minutes. Requesters cannot
            approve their own access.
          </p>
          <Button disabled={busy} onClick={() => void perform(refreshRequests)}>
            Refresh requests
          </Button>
          {requests.map((request) => (
            <RequestCard
              key={request.id}
              request={request}
              busy={busy}
              decide={(body) =>
                void perform(async () => {
                  await session.request(
                    `/insights/access-requests/${encodeURIComponent(request.id)}/decision`,
                    body
                  )
                  await refreshRequests()
                })
              }
              revoke={() =>
                void perform(async () => {
                  await session.request(
                    `/insights/grants/${encodeURIComponent(request.id)}/revoke`,
                    {}
                  )
                  await refreshRequests()
                })
              }
            />
          ))}
        </section>
      )}
      {activeTab === "policies" && (
        <section aria-label="Data policies">
          <h2>Data policies</h2>
          <p>
            Set <code>requestLess</code>, <code>requestOnly</code>, or{" "}
            <code>deny</code> for registered types and JSON Pointer fields.
            Saving a revision revokes existing grants.
          </p>
          <Button
            variant="outline"
            disabled={busy}
            onClick={() =>
              void perform(async () => {
                const result = await session.request<{
                  revision: number
                  policy: unknown
                }>("/insights/policies")
                setRevision(result.revision)
                setPolicy(JSON.stringify(result.policy, null, 2))
              })
            }
          >
            Load current policy
          </Button>
          <label htmlFor="insights-policy">
            Policy JSON{revision === undefined ? "" : ` · revision ${revision}`}
          </label>
          <textarea
            id="insights-policy"
            value={policy}
            onChange={(event) => setPolicy(event.target.value)}
            rows={18}
            spellCheck={false}
          />
          <Button
            disabled={busy || revision === undefined || !policy.trim()}
            onClick={() =>
              void perform(async () => {
                const next = await session.request<number>(
                  "/insights/policies",
                  { expectedRevision: revision, policy: JSON.parse(policy) },
                  "PUT"
                )
                setRevision(next)
                setNotice(
                  `Policy revision ${next} saved. Previous grants are revoked.`
                )
              })
            }
          >
            Save policy revision
          </Button>
        </section>
      )}
      {activeTab === "audit" && (
        <section aria-label="Access journal">
          <h2>Access journal</h2>
          <p>
            Verified identities, admission timestamps, decisions, and disclosed
            field references. Delivery to a client does not establish human
            consumption.
          </p>
          <Button
            disabled={busy}
            onClick={() =>
              void perform(async () => {
                setAudit(await session.request("/insights/audit?limit=50"))
              })
            }
          >
            Load recent accesses
          </Button>
          {audit !== undefined && <pre>{JSON.stringify(audit, null, 2)}</pre>}
        </section>
      )}
      {!activeTab && <p>No Insights roles are assigned to this account.</p>}
      {busy && <p role="status">Working…</p>}
    </main>
  )
}

function RequestCard({
  request,
  busy,
  decide,
  revoke,
}: {
  request: AccessRequest
  busy: boolean
  decide(body: unknown): void
  revoke(): void
}) {
  const [seconds, setSeconds] = useState(300)
  const [fields, setFields] = useState([
    ...new Set([...request.fields, ...request.filterFields]),
  ])
  return (
    <article className="insights-request">
      <h3>
        {request.selection.resource.type} · {request.status}
      </h3>
      <p>
        <code>{request.id}</code>
      </p>
      <p>
        {request.identity.subject} via {request.identity.clientId}
      </p>
      <p>Purpose: {request.purpose}</p>
      <EvidenceDisclosure
        title={`Frozen record selection (${request.records.length})`}
      >
        <pre>{JSON.stringify(request.records, null, 2)}</pre>
      </EvidenceDisclosure>
      {request.status === "pending" && (
        <>
          <fieldset>
            <legend>Fields to approve</legend>
            {[...new Set([...request.fields, ...request.filterFields])].map(
              (field) => (
                <label key={field} className="insights-field">
                  <input
                    type="checkbox"
                    checked={fields.includes(field)}
                    onChange={(event) =>
                      setFields(
                        event.target.checked
                          ? [...fields, field]
                          : fields.filter((value) => value !== field)
                      )
                    }
                  />
                  <code>{field}</code>
                  {request.filterFields.includes(field) ? " (predicate)" : ""}
                </label>
              )
            )}
          </fieldset>
          <label>
            Grant lifetime (seconds)
            <input
              type="number"
              min={1}
              max={900}
              value={seconds}
              onChange={(event) => setSeconds(Number(event.target.value))}
            />
          </label>
          <div className="insights-actions">
            <Button
              disabled={
                busy ||
                !Number.isInteger(seconds) ||
                seconds < 1 ||
                seconds > 900
              }
              onClick={() =>
                decide({
                  approve: true,
                  durationSeconds: seconds,
                  fields,
                  records: null,
                })
              }
            >
              Approve selected fields
            </Button>
            <Button
              variant="outline"
              disabled={busy}
              onClick={() =>
                decide({
                  approve: false,
                  durationSeconds: 0,
                  fields: null,
                  records: null,
                })
              }
            >
              Reject request
            </Button>
          </div>
        </>
      )}
      {request.status === "approved" && (
        <Button variant="outline" disabled={busy} onClick={revoke}>
          Revoke grant
        </Button>
      )}
    </article>
  )
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : "Insights request failed."
}
