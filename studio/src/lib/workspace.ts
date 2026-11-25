import {
  ClipboardCheck,
  FlaskConical,
  History,
  Radio,
  Send,
  Waypoints,
} from "lucide-react"

import { evaluationConfigured } from "@/lib/evaluation-api"

const tracerModes = [
  {
    id: "commands",
    label: "Commands",
    icon: Send,
    scope: "Isolated Test bus",
    description: "Send a command and inspect its effects.",
    emptyTitle: "Send a command",
    emptyMessage: "Choose a command in the form to start a new flow.",
  },
  {
    id: "tests",
    label: "Tests",
    icon: FlaskConical,
    scope: "Behavioral tests",
    description: "Run a scenario against its fixture.",
    emptyTitle: "Choose a test",
    emptyMessage:
      "Open the test library to preview a scenario and its expected messages.",
  },
  {
    id: "runs",
    label: "Runs",
    icon: History,
    scope: "Saved in this browser",
    description: "Revisit a test result and its recorded flow.",
    emptyTitle: "No saved runs yet",
    emptyMessage: "Completed tests appear here for later inspection.",
  },
  {
    id: "observation",
    label: "Observe",
    icon: Radio,
    scope: "Live traffic",
    description: "Follow events as they arrive.",
    emptyTitle: "Choose a live flow",
    emptyMessage: "Open Flows to select Test or Production traffic.",
  },
  {
    id: "correlations",
    label: "Correlations",
    icon: Waypoints,
    scope: "Retained history",
    description: "Explore recorded event chains.",
    emptyTitle: "Find a correlation",
    emptyMessage: "Choose an aggregate and ID, or open a correlation directly.",
  },
] as const

const evaluationMode = {
  id: "evaluation",
  label: "Evaluation",
  icon: ClipboardCheck,
  scope: "Eval runs",
  description: "Compare eval runs, verdicts and regressions.",
  emptyTitle: "No eval runs yet",
  emptyMessage: "Runs started in the evaluation application appear here.",
} as const

export type Workspace =
  (typeof tracerModes)[number]["id"] | (typeof evaluationMode)["id"]

// Evaluation is an optional mode: it exists only when an evaluation API is
// configured, so a plain Tracer Studio keeps its five tabs and shortcuts.
export const workspaceModes: readonly (
  (typeof tracerModes)[number] | typeof evaluationMode
)[] = evaluationConfigured() ? [...tracerModes, evaluationMode] : tracerModes
