import { CheckCircle2 } from "lucide-react"

import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"

import { CodePane } from "./CodePane"
import type { MacroSlide } from "./macro-data"

export function MacroSlideCard({ macro }: { macro: MacroSlide }) {
  return (
    <div className="grid min-h-[650px] overflow-hidden rounded-[22px] border border-[#3b3127] bg-[#1b1712] lg:grid-cols-[0.78fr_1.22fr]">
      <article className="flex min-w-0 flex-col border-b border-[#3b3127] p-6 sm:p-9 lg:border-r lg:border-b-0 lg:p-11">
        <div>
          <p className="font-mono text-[10px] font-semibold tracking-[0.18em] text-primary uppercase">
            {macro.family}
          </p>
          <h3 className="mt-3 max-w-md text-3xl leading-tight font-semibold tracking-[-0.035em] text-stone-50 sm:text-4xl">
            {macro.name}
          </h3>
          <p className="mt-5 max-w-md text-lg leading-snug font-medium text-stone-200 sm:text-xl">
            {macro.headline}
          </p>
          <p className="mt-4 max-w-md text-sm leading-7 text-stone-400 sm:text-base">
            {macro.description}
          </p>
          <ul className="mt-8 space-y-4">
            {macro.points.map((point) => (
              <li
                className="flex items-start gap-3 text-sm leading-6 text-stone-300"
                key={point}
              >
                <CheckCircle2
                  aria-hidden="true"
                  className="mt-1 size-4 shrink-0 text-primary"
                />
                <span>{point}</span>
              </li>
            ))}
          </ul>
        </div>

        <code className="mt-auto truncate pt-12 font-mono text-[10px] text-stone-600">
          {macro.file}
        </code>
      </article>

      <div className="min-w-0 bg-[#110e0b]">
        <Tabs className="h-full gap-0" defaultValue="authored">
          <div className="flex min-h-12 items-center justify-between gap-3 border-b border-[#342a22] px-4 sm:px-6">
            <div className="flex items-center gap-2" aria-hidden="true">
              <span className="size-2.5 rounded-full bg-[#ff6759]" />
              <span className="size-2.5 rounded-full bg-[#f3b64f]" />
              <span className="size-2.5 rounded-full bg-[#56c96b]" />
            </div>
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
