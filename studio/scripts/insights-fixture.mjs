import assert from "node:assert/strict"

export async function installInsightsScenario(page, diagnostics) {
  const requests = [
    {
      id: "access-42",
      identity: {
        issuer: "https://identity.example/realms/support",
        subject: "support-engineer-42",
        clientId: "studio",
      },
      selection: {
        source: "people",
        resource: {
          kind: "aggregate",
          type: "identity/user",
          schemaVersion: 1,
        },
        aggregateType: "identity/user",
        aggregateId: "user-42",
      },
      fields: ["/age", "/name", "/postaddress"],
      filterFields: [],
      purpose: "support",
      records: [{ id: "user-42", version: "event-7:7" }],
      status: "pending",
      expiresAtMs: 1900000000000,
    },
  ]
  let revision = 1
  let policy = {
    types: [
      {
        source: "people",
        resource: requests[0].selection.resource,
        record: "requestLess",
        fields: {
          "/age": "requestLess",
          "/name": "requestOnly",
          "/postaddress": "requestOnly",
        },
      },
    ],
  }
  await page.setRequestInterception(true)
  page.on("request", (request) => {
    void (async () => {
      const url = new URL(request.url())
      if (url.pathname === "/favicon.ico") {
        await request.respond({ status: 204 })
        return
      }
      if (!url.pathname.startsWith("/api/")) {
        await request.continue()
        return
      }
      const path = url.pathname.slice(4)
      const body = request.postData()
        ? JSON.parse(request.postData())
        : undefined
      let response
      if (path === "/insights/query") {
        const fields = { "/age": 34 }
        if (body.requestId && requests[0].status === "approved")
          fields["/name"] = "Approved example name"
        response = {
          records: [
            { reference: { id: "user-42", version: "event-7:7" }, fields },
          ],
          status: body.requestId ? "complete" : "partial",
          requestId:
            body.requestMissing || body.requestId ? "access-42" : undefined,
          auditId: "audit-123",
          policyRevision: revision,
        }
      } else if (path === "/insights/access-requests") response = requests
      else if (path === "/insights/access-requests/access-42")
        response = requests[0]
      else if (path.endsWith("/decision")) {
        assert.equal(body.fields.includes("/name"), true)
        requests[0].status = body.approve ? "approved" : "rejected"
        response = null
      } else if (path.endsWith("/revoke")) {
        requests[0].status = "revoked"
        response = null
      } else if (path === "/insights/policies") {
        if (request.method() === "PUT") {
          assert.equal(body.expectedRevision, revision)
          revision += 1
          policy = body.policy
          response = revision
        } else response = { revision, policy }
      } else if (path === "/insights/audit") {
        response = {
          entries: [
            {
              id: "audit-123",
              identity: requests[0].identity,
              timestampMs: 1800000000000,
              action: "query.disclosure",
              outcome: "admitted; delivery unconfirmed",
              disclosures: [
                { reference: requests[0].records[0], fields: ["/age"] },
              ],
            },
          ],
          nextBefore: null,
        }
      } else {
        diagnostics.unexpectedRequests.push(path)
        await request.respond({
          status: 404,
          contentType: "application/json",
          body: "{}",
        })
        return
      }
      await request.respond({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify(response),
      })
    })().catch((error) => {
      diagnostics.pageErrors.push(error.message)
      void request.abort()
    })
  })
}

async function click(page, label) {
  await page.locator(`::-p-aria(${label})`).click()
}

export async function prepareInsightsScenario(page, scene) {
  await page.waitForSelector("#insights-query")
  if (scene === "insights-approval") {
    await click(page, "Access requests")
    await click(page, "Refresh requests")
    await page.waitForSelector(".insights-request")
    return
  }
  const query = {
    selection: {
      source: "people",
      resource: { kind: "aggregate", type: "identity/user", schemaVersion: 1 },
      aggregateType: "identity/user",
      aggregateId: "user-42",
    },
    fields: ["/age", "/name", "/postaddress"],
    purpose: "support",
  }
  await page.type("#insights-query", JSON.stringify(query, null, 2))
  await page.$eval("#insights-query", (input) => {
    input.scrollTop = 0
  })
  await click(page, "Request protected fields")
  await page.waitForSelector(".insights-result")
  assert.equal(
    await page.evaluate(() =>
      document
        .querySelector(".insights-result")
        .textContent.includes("Approved example name")
    ),
    false
  )
  await page.$eval(".insights-result", (result) =>
    result.scrollIntoView({ block: "center" })
  )
}

export async function exerciseInsights(page) {
  await click(page, "Access requests")
  await click(page, "Refresh requests")
  await page.waitForSelector(".insights-request")
  await click(page, "Approve selected fields")
  await page.waitForFunction(() =>
    document
      .querySelector(".insights-request h3")
      .textContent.includes("approved")
  )
  await click(page, "Query")
  await click(page, "Check approval")
  await click(page, "Retrieve approved fields")
  await page.waitForFunction(() =>
    document
      .querySelector(".insights-result")
      ?.textContent.includes("Approved example name")
  )
  assert.equal(
    await page.evaluate(() =>
      localStorage.getItem("rostfrei-tracer-studio-runs-v1")
    ),
    null
  )
  await click(page, "Policies")
  await click(page, "Load current policy")
  await page.waitForFunction(() =>
    document.querySelector("#insights-policy").value.includes("requestOnly")
  )
  await click(page, "Save policy revision")
  await page.waitForFunction(() =>
    document.body.textContent.includes("Policy revision 2 saved")
  )
  await click(page, "Access journal")
  await click(page, "Load recent accesses")
  await page.waitForFunction(() =>
    document.body.textContent.includes("query.disclosure")
  )
}

export async function checkInsights(browser, url, observePage) {
  const context = await browser.createBrowserContext()
  try {
    const page = await context.newPage()
    const diagnostics = observePage(page)
    await installInsightsScenario(page, diagnostics)
    await page.goto(new URL("/scripts/insights-fixture.html", url).href, {
      waitUntil: "networkidle0",
    })
    await prepareInsightsScenario(page, "insights")
    await exerciseInsights(page)
    for (const key of [
      "pageErrors",
      "console",
      "failedRequests",
      "httpErrors",
      "unexpectedRequests",
    ])
      assert.deepEqual(
        diagnostics[key],
        [],
        `Insights browser diagnostics: ${key}`
      )
    console.log(
      "Verified governed query, approval, retrieval, policy update and access journal (mocked API)"
    )
  } finally {
    await context.close()
  }
}
