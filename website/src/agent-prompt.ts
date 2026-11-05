import gettingStarted from "../../docs/getting-started.mdx?raw"

// The shared guide stays readable in a checkout and in the published raw docs.
// Both website entry points use its first plain-text block as the copy source.
const prompt = gettingStarted.match(/```text\n([\s\S]*?)\n```/)?.[1]
if (!prompt) throw new Error("Getting started must include the agent prompt")

export const agentTutorialPrompt = prompt
