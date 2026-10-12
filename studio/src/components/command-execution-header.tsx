import { CircleDot, LoaderCircle, Pencil, TriangleAlert } from "lucide-react"
import { Button } from "@/components/ui/button"
import type { CommandExecutionRequest } from "@/components/command-panel"
import type { CommandExecutionResult, MessageGraphNode } from "@/lib/types"

export interface CommandView {
  request: CommandExecutionRequest
  result?: CommandExecutionResult
  error?: string
}

export function CommandExecutionHeader({
  execution,
  root,
  running,
  onEdit,
}: {
  execution: CommandView
  root?: MessageGraphNode
  running: boolean
  onEdit: () => void
}) {
  const { request, result, error } = execution
  const resultValue = result?.operation.result
  const decision =
    typeof resultValue === "object" && resultValue !== null
      ? Reflect.get(resultValue, "decision")
      : undefined
  const status = running
    ? "running"
    : (root?.status ??
      (result?.operation.status === "completed"
        ? decision === "accepted" || decision === "rejected"
          ? decision
          : "indeterminate"
        : result?.operation.status) ??
      "unavailable")
  const preview = request.mode === "preview"
  const title =
    status === "running" || status === "queued"
      ? "Execution in progress"
      : status === "accepted"
        ? "Command accepted"
        : status === "rejected"
          ? "Command rejected"
          : status === "indeterminate"
            ? "Outcome unconfirmed"
            : "Result unavailable"
  return (
    <section
      className="execution-header command-execution-header"
      aria-label="Command execution summary"
    >
      <div className="execution-title-row">
        <div>
          <p className="eyebrow">
            Command execution <span>/</span>{" "}
            {preview ? "Preview" : "Isolated Test"}
          </p>
          <h1>
            {request.commandLabel} · {preview ? "Preview" : "Test"}
          </h1>
        </div>
        <Button variant="outline" onClick={onEdit}>
          <Pencil /> Edit command
        </Button>
      </div>
      <div className="execution-result" data-status={status} role="status">
        {running ? (
          <LoaderCircle size={16} className="animate-spin" />
        ) : status === "indeterminate" || status === "failed" ? (
          <TriangleAlert size={16} />
        ) : (
          <CircleDot size={16} />
        )}
        <strong>{title}</strong>
        <span>
          {preview
            ? "Read-only preview · no messages published"
            : "Isolated Test command bus"}
        </span>
      </div>
      {(error || result?.inspectionError) && (
        <p className="execution-error" role="alert">
          {error ?? result?.inspectionError}
        </p>
      )}
    </section>
  )
}
