import { useRef, useState, type ComponentProps } from "react"
import {
  ArrowRight,
  Check,
  ChevronRight,
  Info,
  Play,
  RotateCcw,
} from "lucide-react"
import { Brand, DuckMascot } from "@rostfrei/ui/brand"
import { ThemeToggle, useTheme } from "@rostfrei/ui/theme"
import { Badge } from "@rostfrei/ui/components/badge"
import { Button } from "@rostfrei/ui/components/button"
import { Input } from "@rostfrei/ui/components/input"
import { Textarea } from "@rostfrei/ui/components/textarea"
import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@rostfrei/ui/components/tabs"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@rostfrei/ui/components/tooltip"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@rostfrei/ui/components/popover"
import { Separator } from "@rostfrei/ui/components/separator"
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@rostfrei/ui/components/sheet"
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@rostfrei/ui/components/collapsible"
import { Skeleton } from "@rostfrei/ui/components/skeleton"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@rostfrei/ui/components/dialog"
import {
  Table,
  TableBody,
  TableCaption,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@rostfrei/ui/components/table"
import {
  Panel,
  PanelContent,
  PanelHeader,
  PanelTitle,
} from "@rostfrei/ui/components/panel"

function CodeExample({ children }: ComponentProps<"pre">) {
  return (
    <pre className="my-4 overflow-auto border border-border bg-muted p-4 text-xs leading-6">
      {children}
    </pre>
  )
}

const variants = [
  "default",
  "secondary",
  "outline",
  "ghost",
  "link",
  "destructive",
] as const
const sizes = ["xs", "sm", "default", "lg"] as const
const statuses = [
  "neutral",
  "success",
  "warning",
  "info",
  "destructive",
  "command",
  "domain-event",
  "integration-event",
] as const

export function Gallery() {
  const { theme, setTheme } = useTheme()
  const focusTarget = useRef<HTMLButtonElement>(null)
  const [message, setMessage] = useState("Ready. Try a control below.")
  const [fleet, setFleet] = useState("harbour-fleet")
  const [notes, setNotes] = useState("")
  const [runs, setRuns] = useState(0)
  const [invalid, setInvalid] = useState(false)
  const [details, setDetails] = useState(true)
  const [dialogOpen, setDialogOpen] = useState(false)

  function runExample() {
    if (!fleet.trim()) {
      setInvalid(true)
      setMessage("Enter a fleet ID before running the local example.")
      return
    }
    setInvalid(false)
    setRuns((count) => count + 1)
    setMessage(
      `Example ${runs + 1} accepted for ${fleet}. This gallery runs locally.`
    )
  }

  return (
    <main className="gallery-shell">
      <TooltipProvider>
        <p className="section-kicker">@rostfrei/ui / Local component gallery</p>
        <h1 className="page-title">A shared working language.</h1>
        <p className="mt-4 max-w-2xl text-sm leading-7 text-muted-foreground">
          The same components, steel surfaces, rust actions, and blue links used
          by the Rostfrei website and Tracer Studio. Every control below is live.
        </p>
        <div className="my-5 flex flex-wrap gap-3 text-xs">
          <a className="text-link" href="#themes">
            Themes
          </a>
          <a className="text-link" href="#buttons">
            Buttons
          </a>
          <a className="text-link" href="#forms">
            Forms
          </a>
          <a className="text-link" href="#statuses">
            Statuses
          </a>
          <a className="text-link" href="#panels">
            Panels & tabs
          </a>
          <a className="text-link" href="#overlays">
            Overlays
          </a>
          <a className="text-link" href="#brand">
            Brand
          </a>
        </div>
        <div
          role="status"
          aria-live="polite"
          className="mb-5 border border-border bg-muted px-3 py-2 text-xs wrap-anywhere"
        >
          {message}
        </div>

        <section
          id="themes"
          className="gallery-section"
          aria-labelledby="themes-title"
        >
          <h2 id="themes-title">Themes & surfaces</h2>
          <p>
            Light, dark, or your system preference. Selection is shared through
            the <code>rostfrei-theme</code> setting.
          </p>
          <div
            className="gallery-example mb-5"
            role="group"
            aria-label="Choose a theme"
          >
            {(["light", "dark", "system"] as const).map((value) => (
              <Button
                key={value}
                variant={theme === value ? "default" : "outline"}
                aria-pressed={theme === value}
                onClick={() => setTheme(value)}
              >
                {value.charAt(0).toUpperCase() + value.slice(1)}
              </Button>
            ))}
            <ThemeToggle />
          </div>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            {(["background", "card", "muted", "accent"] as const).map(
              (token) => (
                <div key={token} className="min-w-0 border border-border">
                  <div
                    className="h-14 border-b border-border"
                    style={{ backgroundColor: `var(--${token})` }}
                  />
                  <p className="px-2 py-1.5 font-mono text-[11px]">{token}</p>
                </div>
              )
            )}
          </div>
        </section>

        <section
          id="buttons"
          className="gallery-section"
          aria-labelledby="buttons-title"
        >
          <h2 id="buttons-title">Buttons & keyboard focus</h2>
          <p>
            Flat actions in six variants. Use Tab to inspect the shared focus
            treatment.
          </p>
          <div className="gallery-example mb-5">
            {variants.map((variant) => (
              <Button
                key={variant}
                variant={variant}
                onClick={() => setMessage(`${variant} button activated.`)}
              >
                {variant.charAt(0).toUpperCase() + variant.slice(1)}
              </Button>
            ))}
          </div>
          <div className="gallery-example mb-5">
            {sizes.map((size) => (
              <Button
                key={size}
                size={size}
                variant="outline"
                onClick={() => setMessage(`${size} button activated.`)}
              >
                {size === "default" ? "Default" : size.toUpperCase()}
              </Button>
            ))}
            <Button
              size="icon"
              aria-label="Run local example"
              onClick={runExample}
            >
              <Play aria-hidden="true" />
            </Button>
            <Button disabled>Disabled</Button>
            <Button variant="outline" disabled>
              Disabled outline
            </Button>
          </div>
          <div className="gallery-example">
            <Button variant="link" onClick={() => focusTarget.current?.focus()}>
              Focus the next control <ArrowRight aria-hidden="true" />
            </Button>
            <Button
              ref={focusTarget}
              variant="outline"
              className="focus:ring-2 focus:ring-ring focus:ring-offset-2 focus:ring-offset-card"
              onClick={() =>
                setMessage(
                  "Focused control activated with keyboard or pointer."
                )
              }
            >
              Focus target
            </Button>
          </div>
        </section>

        <section
          id="forms"
          className="gallery-section"
          aria-labelledby="forms-title"
        >
          <h2 id="forms-title">Inputs & validation</h2>
          <p>
            Editable fields, an explicit invalid state, and disabled controls.
            Run the local example to update the result panel.
          </p>
          <form
            onSubmit={(event) => {
              event.preventDefault()
              runExample()
            }}
            noValidate
          >
            <div className="gallery-grid">
              <div className="space-y-2">
                <label
                  className="text-xs font-semibold"
                  htmlFor="gallery-fleet"
                >
                  Fleet ID
                </label>
                <Input
                  id="gallery-fleet"
                  value={fleet}
                  onChange={(event) => {
                    setFleet(event.target.value)
                    setInvalid(false)
                  }}
                  aria-invalid={invalid}
                  aria-describedby="fleet-hint"
                />
                <p
                  id="fleet-hint"
                  className={`text-xs ${invalid ? "text-destructive" : "text-muted-foreground"}`}
                >
                  {invalid
                    ? "A fleet ID is required."
                    : "Clear the field to try validation."}
                </p>
              </div>
              <div className="space-y-2">
                <label
                  className="text-xs font-semibold"
                  htmlFor="gallery-invalid"
                >
                  Invalid example
                </label>
                <Input
                  id="gallery-invalid"
                  aria-invalid="true"
                  aria-describedby="invalid-hint"
                  placeholder="Bicycle ID required"
                />
                <p id="invalid-hint" className="text-xs text-destructive">
                  A bicycle ID is required.
                </p>
              </div>
              <div className="space-y-2">
                <label
                  className="text-xs font-semibold"
                  htmlFor="gallery-notes"
                >
                  Notes
                </label>
                <Textarea
                  id="gallery-notes"
                  value={notes}
                  onChange={(event) => setNotes(event.target.value)}
                  placeholder="Add context for this example…"
                  rows={3}
                />
              </div>
              <div className="space-y-2">
                <label
                  className="text-xs font-semibold"
                  htmlFor="gallery-disabled"
                >
                  Application (disabled)
                </label>
                <Input id="gallery-disabled" value="bike-rental" disabled />
                <label
                  className="block text-xs font-semibold"
                  htmlFor="gallery-readonly"
                >
                  Read-only namespace
                </label>
                <Input
                  id="gallery-readonly"
                  value="bike-rental.test"
                  readOnly
                />
              </div>
            </div>
            <Button type="submit" className="mt-4">
              <Play aria-hidden="true" />
              Run local example
            </Button>
          </form>
        </section>

        <section
          id="statuses"
          className="gallery-section"
          aria-labelledby="statuses-title"
        >
          <h2 id="statuses-title">Badges & message kinds</h2>
          <p>
            Color reinforces a written status. Commands, private domain events,
            and public integration events retain distinct identities.
          </p>
          <div className="gallery-example">
            {statuses.map((variant) => (
              <Badge key={variant} variant={variant}>
                {variant.replaceAll("-", " ")}
              </Badge>
            ))}
            <Badge variant="outline">Outline</Badge>
            <Badge variant="default">Selected</Badge>
            <Badge variant="success">
              <Check aria-hidden="true" />
              Accepted
            </Badge>
          </div>
        </section>

        <section
          id="panels"
          className="gallery-section"
          aria-labelledby="panels-title"
        >
          <h2 id="panels-title">Panels, tables & tabs</h2>
          <p>
            Compact working surfaces with meaningful headings. The example
            result reflects the form above.
          </p>
          <Panel>
            <PanelHeader>
              <PanelTitle>Rental command</PanelTitle>
              <Badge variant={runs ? "success" : "neutral"}>
                {runs ? "Accepted" : "Ready"}
              </Badge>
            </PanelHeader>
            <PanelContent>
              <Tabs defaultValue="result">
                <TabsList variant="line" aria-label="Example details">
                  <TabsTrigger value="result">Result</TabsTrigger>
                  <TabsTrigger value="payload">Payload</TabsTrigger>
                  <TabsTrigger value="loading">Loading</TabsTrigger>
                </TabsList>
                <TabsContent value="result">
                  <Table>
                    <TableCaption>
                      Illustrative local flow · {runs}{" "}
                      {runs === 1 ? "example" : "examples"} run
                    </TableCaption>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Message</TableHead>
                        <TableHead>Kind</TableHead>
                        <TableHead>State</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      <TableRow>
                        <TableCell className="font-mono text-xs">
                          RentBicycle
                        </TableCell>
                        <TableCell>
                          <Badge variant="command">command</Badge>
                        </TableCell>
                        <TableCell>{runs ? "Accepted" : "Ready"}</TableCell>
                      </TableRow>
                      <TableRow>
                        <TableCell className="font-mono text-xs">
                          BicycleRented
                        </TableCell>
                        <TableCell>
                          <Badge variant="domain-event">domain event</Badge>
                        </TableCell>
                        <TableCell>Illustrative</TableCell>
                      </TableRow>
                      <TableRow>
                        <TableCell className="font-mono text-xs">
                          BicycleRentalStarted
                        </TableCell>
                        <TableCell>
                          <Badge variant="integration-event">
                            integration event
                          </Badge>
                        </TableCell>
                        <TableCell>Illustrative</TableCell>
                      </TableRow>
                    </TableBody>
                  </Table>
                </TabsContent>
                <TabsContent value="payload">
                  <CodeExample>
                    <code className="language-json">
                      {JSON.stringify(
                        { fleet_id: fleet, notes, runs },
                        null,
                        2
                      )}
                    </code>
                  </CodeExample>
                </TabsContent>
                <TabsContent value="loading">
                  <div
                    className="space-y-3 py-4"
                    role="status"
                    aria-label="Loading state example"
                  >
                    <Skeleton className="h-4 w-2/3" />
                    <Skeleton className="h-4 w-full" />
                    <Skeleton className="h-4 w-4/5" />
                    <span className="sr-only">Loading example</span>
                  </div>
                </TabsContent>
              </Tabs>
            </PanelContent>
          </Panel>
          <Collapsible className="mt-3 border border-border">
            <CollapsibleTrigger className="group flex w-full items-center gap-2 bg-muted px-3 py-2 text-left text-xs font-semibold">
              <ChevronRight
                className="size-3.5 group-data-[state=open]:rotate-90"
                aria-hidden="true"
              />
              About this example
            </CollapsibleTrigger>
            <CollapsibleContent className="border-t border-border p-3 text-xs leading-6 text-muted-foreground">
              These rows illustrate message kinds; this gallery makes no runtime
              requests. In Tracer, Isolated Test publishes through the Test bus.
              Simulate handler runs the real handler and returns uncommitted
              proposals; injected dependencies are not automatically sandboxed.{" "}
              <a
                href="https://elblabs.github.io/rostfrei/docs/getting-started"
                className="text-link underline"
              >
                Run the bike-rental example
              </a>{" "}
              to explore a connected Tracer.
            </CollapsibleContent>
          </Collapsible>
        </section>

        <section
          id="overlays"
          className="gallery-section"
          aria-labelledby="overlays-title"
        >
          <h2 id="overlays-title">Popovers, dialogs & sheets</h2>
          <p>
            Open a surface, interact with it, then dismiss with Escape. Focus
            returns to its trigger.
          </p>
          <div className="gallery-example">
            <Popover>
              <PopoverTrigger asChild>
                <Button variant="outline">Display options</Button>
              </PopoverTrigger>
              <PopoverContent align="start">
                <h3 className="font-semibold">Example display</h3>
                <p className="mt-1 text-xs text-muted-foreground">
                  Toggle the explanation below the controls.
                </p>
                <Button
                  className="mt-3"
                  size="sm"
                  variant="outline"
                  aria-pressed={details}
                  onClick={() => setDetails((value) => !value)}
                >
                  {details ? "Hide" : "Show"} explanation
                </Button>
              </PopoverContent>
            </Popover>
            <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
              <DialogTrigger asChild>
                <Button variant="outline">
                  <RotateCcw aria-hidden="true" />
                  Reset example
                </Button>
              </DialogTrigger>
              <DialogContent>
                <DialogHeader>
                  <DialogTitle>Reset this local example?</DialogTitle>
                  <DialogDescription>
                    Restore the initial fleet, clear notes, and reset the
                    example count.
                  </DialogDescription>
                </DialogHeader>
                <DialogFooter>
                  <DialogClose asChild>
                    <Button variant="outline">Cancel</Button>
                  </DialogClose>
                  <Button
                    onClick={() => {
                      setFleet("harbour-fleet")
                      setNotes("")
                      setRuns(0)
                      setInvalid(false)
                      setMessage("Example reset. Ready for another run.")
                      setDialogOpen(false)
                    }}
                  >
                    Reset example
                  </Button>
                </DialogFooter>
              </DialogContent>
            </Dialog>
            <Sheet>
              <SheetTrigger asChild>
                <Button variant="outline">Open inspector</Button>
              </SheetTrigger>
              <SheetContent className="overflow-y-auto">
                <SheetHeader>
                  <SheetTitle>Local example inspector</SheetTitle>
                  <SheetDescription>
                    Current gallery form values.
                  </SheetDescription>
                </SheetHeader>
                <div className="px-4">
                  <Badge variant="command">RentBicycle</Badge>
                  <CodeExample>
                    <code className="language-json">
                      {JSON.stringify(
                        { fleet_id: fleet, notes, runs },
                        null,
                        2
                      )}
                    </code>
                  </CodeExample>
                </div>
              </SheetContent>
            </Sheet>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label="About gallery overlays"
                >
                  <Info aria-hidden="true" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                Shared Radix primitives with keyboard support.
              </TooltipContent>
            </Tooltip>
          </div>
          {details && (
            <p className="mt-4 text-xs text-muted-foreground">
              The dialog resets only this page’s example state. The inspector
              and payload tab read the same live values.
            </p>
          )}
        </section>

        <section
          id="brand"
          className="gallery-section"
          aria-labelledby="brand-title"
        >
          <h2 id="brand-title">The duck & the wordmark</h2>
          <p>
            Shared artwork adapts to the active theme. The wordmark is
            noninteractive; its parent owns navigation.
          </p>
          <Panel>
            <PanelContent className="flex flex-wrap items-center gap-6">
              <a
                href="https://elblabs.github.io/rostfrei/"
                aria-label="Rostfrei home"
              >
                <Brand />
              </a>
              <Separator
                orientation="vertical"
                className="hidden h-12 sm:block"
              />
              <Brand compact />
              <DuckMascot className="w-28" alt="The Rostfrei duck mascot" />
            </PanelContent>
          </Panel>
          <CodeExample>
            <code className="language-tsx">{`import { Button } from "@rostfrei/ui/components/button"\nimport { Brand, DuckMascot } from "@rostfrei/ui/brand"\nimport { ThemeProvider } from "@rostfrei/ui/theme"\n\n<ThemeProvider defaultTheme="system" storageKey="rostfrei-theme">\n  <Button>Run local example</Button>\n  <DuckMascot />\n</ThemeProvider>`}</code>
          </CodeExample>
        </section>
      </TooltipProvider>
    </main>
  )
}
