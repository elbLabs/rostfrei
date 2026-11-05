import { useId, useRef, useState } from "react"
import { Copy } from "lucide-react"
import { Button } from "@rostfrei/ui/components/button"

import { agentTutorialPrompt } from "@/agent-prompt"

export function AgentPrompt() {
  const promptRef = useRef<HTMLTextAreaElement>(null)
  const statusId = useId()
  const [status, setStatus] = useState("")

  async function copyPrompt() {
    try {
      await navigator.clipboard.writeText(agentTutorialPrompt)
      setStatus("Copied. Paste the prompt into your coding agent.")
    } catch {
      promptRef.current?.focus()
      promptRef.current?.select()
      setStatus("Clipboard unavailable. The prompt is selected; copy it manually.")
    }
  }

  return (
    <div className="agent-prompt" data-agent-prompt>
      <textarea
        aria-label="Agent tutorial prompt"
        aria-describedby={statusId}
        className="agent-prompt-text"
        readOnly
        ref={promptRef}
        rows={6}
        value={agentTutorialPrompt}
      />
      <div className="agent-prompt-actions">
        <Button
          aria-label="Copy agent prompt"
          onClick={copyPrompt}
          size="sm"
          type="button"
        >
          <Copy aria-hidden="true" /> Copy prompt
        </Button>
        <p id={statusId} role="status">
          {status}
        </p>
      </div>
    </div>
  )
}
