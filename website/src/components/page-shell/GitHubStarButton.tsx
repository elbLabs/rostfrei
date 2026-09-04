import { useEffect, useState } from "react"

const REPOSITORY_URL = "https://github.com/elbLabs/rostfrei"
const REPOSITORY_API_URL = "https://api.github.com/repos/elbLabs/rostfrei"
const numberFormatter = new Intl.NumberFormat("en-US")

let starCountRequest: Promise<number> | undefined

function getStarCount() {
  starCountRequest ??= fetch(REPOSITORY_API_URL, {
    headers: {
      Accept: "application/vnd.github+json",
    },
  }).then(async (response) => {
    if (!response.ok) {
      throw new Error(`GitHub returned ${response.status}`)
    }

    const data: unknown = await response.json()

    if (
      typeof data !== "object" ||
      data === null ||
      !("stargazers_count" in data) ||
      typeof data.stargazers_count !== "number"
    ) {
      throw new Error("GitHub returned an invalid star count")
    }

    return data.stargazers_count
  })

  return starCountRequest
}

export function GitHubStarButton() {
  const [starCount, setStarCount] = useState<number | null>(null)
  const [isUnavailable, setIsUnavailable] = useState(false)

  useEffect(() => {
    let isCurrent = true

    void getStarCount().then(
      (count) => {
        if (isCurrent) {
          setStarCount(count)
        }
      },
      () => {
        if (isCurrent) {
          setIsUnavailable(true)
        }
      }
    )

    return () => {
      isCurrent = false
    }
  }, [])

  const formattedCount =
    starCount === null
      ? isUnavailable
        ? "—"
        : "···"
      : numberFormatter.format(starCount)
  const accessibleLabel =
    starCount === null
      ? "Star Rostfrei on GitHub"
      : `Star Rostfrei on GitHub. ${formattedCount} stars.`

  return (
    <a
      aria-label={accessibleLabel}
      className="inline-flex h-9 items-center gap-2 rounded-lg border border-white/20 bg-[#302e3c] px-2.5 text-sm font-medium text-white shadow-sm transition-colors hover:bg-[#3a3748]"
      href={REPOSITORY_URL}
      target="_blank"
      rel="noreferrer"
    >
      <svg
        aria-hidden="true"
        className="size-5 shrink-0 fill-current"
        viewBox="0 0 24 24"
      >
        <path d="M12 1C5.923 1 1 5.923 1 12c0 4.867 3.149 8.979 7.521 10.436.55.096.756-.233.756-.522 0-.262-.013-1.128-.013-2.049-3.064.566-3.857-.742-4.101-1.432-.139-.358-.734-1.432-1.258-1.72-.428-.231-1.039-.8-.013-.814.96-.014 1.646.883 1.875 1.247 1.098 1.845 2.852 1.322 3.551 1.005.105-.8.428-1.323.777-1.625-2.446-.276-5.005-1.224-5.005-5.432 0-1.197.428-2.188 1.126-2.96-.113-.277-.488-1.398.107-2.918 0 0 .918-.295 3.009 1.13A10.6 10.6 0 0 1 12 6.58c.938.004 1.876.128 2.75.374 2.091-1.438 3.01-1.13 3.01-1.13.594 1.52.219 2.64.106 2.918.699.772 1.126 1.763 1.126 2.96 0 4.222-2.573 5.156-5.019 5.432.44.386.821 1.13.821 2.291 0 1.653-.013 2.982-.013 3.394 0 .316.206.687.762.522C19.851 20.979 23 16.854 23 12c0-6.077-4.922-11-11-11Z" />
      </svg>
      <span aria-live="polite" className="min-w-[2ch] tabular-nums">
        {formattedCount}
      </span>
    </a>
  )
}
