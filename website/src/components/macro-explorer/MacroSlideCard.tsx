import { Fragment } from "react"
import { CheckCircle2 } from "lucide-react"

import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"

import { CodePane } from "./CodePane"
import type { MacroSlide } from "./macro-data"

export function MacroSlideCard({ macro }: { macro: MacroSlide }) {
  return (
    <div className="grid min-w-0 overflow-hidden border border-border bg-card lg:grid-cols-[minmax(0,0.78fr)_minmax(0,1.22fr)]">
      <article className="flex min-w-0 flex-col border-b border-border p-4 lg:border-r lg:border-b-0">
        <div>
          <p className="text-[11px] text-primary">{macro.family}</p>
          <h3 className="mt-2 text-xl leading-tight font-semibold wrap-anywhere">
            {macro.name.split(/(?<=_)/).map((part, index) => (
              <Fragment key={index}>
                {part}
                <wbr />
              </Fragment>
            ))}
          </h3>
          <p className="mt-3 text-sm leading-6 font-medium">{macro.headline}</p>
          <p className="mt-3 text-xs leading-6 text-muted-foreground">
            {macro.description}
          </p>
          <ul className="mt-4 space-y-3">
            {macro.points.map((point) => (
              <li
                className="flex items-start gap-2 text-xs leading-6 text-muted-foreground"
                key={point}
              >
                <CheckCircle2
                  aria-hidden="true"
                  className="mt-1 size-3.5 shrink-0 text-success"
                />
                <span>{point}</span>
              </li>
            ))}
          </ul>
        </div>

        <code className="mt-auto pt-5 font-mono text-[10px] wrap-anywhere text-muted-foreground">
          {macro.file}
        </code>
      </article>

      <div className="min-w-0 bg-muted">
        <Tabs className="h-full gap-0" defaultValue="authored">
          <div className="flex min-h-10 items-center gap-3 border-b border-border px-3">
            <TabsList className="h-10 text-xs" variant="line">
              <TabsTrigger className="px-3 text-xs" value="authored">
                Example
              </TabsTrigger>
              <TabsTrigger className="px-3 text-xs" value="generated">
                Generated
              </TabsTrigger>
            </TabsList>
          </div>
          <TabsContent className="mt-0" value="authored">
            <CodePane code={macro.authored} label={macro.file} />
          </TabsContent>
          <TabsContent className="mt-0" value="generated">
            <CodePane
              code={macro.generated}
              label="Simplified conceptual output"
            />
          </TabsContent>
        </Tabs>
      </div>
    </div>
  )
}
