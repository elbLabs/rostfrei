import {
  Database,
  FlaskConical,
  History,
  Layers,
  Radio,
  Send,
  Server,
  Waypoints,
} from "lucide-react"

export const workspaceViews = [
  {
    id: "tests",
    requiresBehavioralTests: true,
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
    requiresBehavioralTests: true,
    label: "Runs",
    icon: History,
    scope: "Saved in this browser",
    description: "Revisit a test result and its recorded flow.",
    emptyTitle: "No saved runs yet",
    emptyMessage: "Completed tests appear here for later inspection.",
  },
  {
    id: "commands",
    label: "Commands",
    icon: Send,
    scope: "Isolated Test bus",
    description: "Send a command and inspect its effects.",
    emptyTitle: "Send a command",
    emptyMessage: "Choose a command in the form to start a new flow.",
  },
  { id: "infrastructure", label: "Infrastructure", icon: Server },
  { id: "domain", label: "Domain", icon: Layers },
  { id: "read-models", label: "Read models", icon: Database },
  {
    id: "observation",
    label: "Live flows",
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

export type WorkspaceView = (typeof workspaceViews)[number]["id"]

export interface StudioFeatures {
  behavioralTests: boolean
  initialView: WorkspaceView
}

export function configuredFeatures(
  legacyWorkspaces: string | undefined,
  inspectionConfigured: boolean
): StudioFeatures | undefined {
  if (legacyWorkspaces === "observe")
    return { behavioralTests: false, initialView: "infrastructure" }
  if (legacyWorkspaces === undefined || legacyWorkspaces === "develop")
    return {
      behavioralTests: true,
      initialView: inspectionConfigured ? "domain" : "tests",
    }
}
