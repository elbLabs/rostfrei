import { useEffect, useState } from "react"
import { ChevronLeft, ChevronRight } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Carousel,
  type CarouselApi,
  CarouselContent,
  CarouselItem,
} from "@/components/ui/carousel"

import { MACROS } from "./macro-data"
import { MacroSlideCard } from "./MacroSlideCard"

export function MacroCarousel() {
  const [api, setApi] = useState<CarouselApi>()
  const [current, setCurrent] = useState(0)

  useEffect(() => {
    if (!api) return
    const update = () => setCurrent(api.selectedScrollSnap())
    update()
    api.on("select", update)
    api.on("reInit", update)
    return () => {
      api.off("select", update)
      api.off("reInit", update)
    }
  }, [api])

  return (
    <>
      <div
        aria-label="Macro carousel controls"
        className="mb-3 flex flex-wrap items-center gap-2 border-y border-border py-2"
      >
        <Button
          aria-label="Previous macro"
          className="text-muted-foreground"
          onClick={() => api?.scrollPrev()}
          size="icon-sm"
          variant="outline"
        >
          <ChevronLeft aria-hidden="true" />
        </Button>
        <Button
          aria-label="Next macro"
          className="text-muted-foreground"
          onClick={() => api?.scrollNext()}
          size="icon-sm"
          variant="outline"
        >
          <ChevronRight aria-hidden="true" />
        </Button>
        <div
          aria-label="Choose macro slide"
          className="flex max-w-full flex-wrap gap-0.5"
        >
          {MACROS.map((item, dotIndex) => (
            <button
              aria-current={dotIndex === current ? "true" : undefined}
              aria-label={`Show ${item.name}`}
              className={`grid size-5 place-items-center transition-colors after:size-1.5 after:bg-current ${
                dotIndex === current
                  ? "text-primary"
                  : "text-muted-foreground hover:text-primary"
              }`}
              key={item.name}
              onClick={() => api?.scrollTo(dotIndex)}
              type="button"
            />
          ))}
        </div>
        <span className="ml-auto font-mono text-xs text-muted-foreground">
          <strong className="text-foreground">
            {String(current + 1).padStart(2, "0")}
          </strong>
          {" / "}
          {String(MACROS.length).padStart(2, "0")}
        </span>
      </div>

      <Carousel
        aria-label="Rostfrei macros"
        className="min-w-0"
        opts={{ loop: true }}
        setApi={setApi}
      >
        <CarouselContent className="-ml-0">
          {MACROS.map((macro, index) => (
            <CarouselItem
              aria-label={`${index + 1} of ${MACROS.length}`}
              className="pl-0"
              key={macro.name}
            >
              <MacroSlideCard macro={macro} />
            </CarouselItem>
          ))}
        </CarouselContent>
      </Carousel>
    </>
  )
}
