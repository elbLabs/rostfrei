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
        className="mb-4 flex flex-wrap items-center gap-2 border-y border-[#3b3127] px-1 py-3"
      >
        <Button
          aria-label="Previous macro"
          className="border-[#44372b] bg-transparent text-stone-400 hover:bg-white/5 hover:text-stone-100"
          onClick={() => api?.scrollPrev()}
          size="icon-sm"
          variant="outline"
        >
          <ChevronLeft aria-hidden="true" />
        </Button>
        <Button
          aria-label="Next macro"
          className="border-[#44372b] bg-transparent text-stone-400 hover:bg-white/5 hover:text-stone-100"
          onClick={() => api?.scrollNext()}
          size="icon-sm"
          variant="outline"
        >
          <ChevronRight aria-hidden="true" />
        </Button>
        <div
          aria-label="Choose macro slide"
          className="ml-1 flex max-w-[210px] flex-wrap gap-1.5 sm:max-w-none"
        >
          {MACROS.map((item, dotIndex) => (
            <button
              aria-current={dotIndex === current ? "true" : undefined}
              aria-label={`Show ${item.name}`}
              className={`size-1.5 rounded-full transition-colors ${
                dotIndex === current
                  ? "bg-primary"
                  : "bg-stone-700 hover:bg-stone-500"
              }`}
              key={item.name}
              onClick={() => api?.scrollTo(dotIndex)}
              type="button"
            />
          ))}
        </div>
        <span className="ml-auto font-mono text-xs text-stone-600">
          <strong className="text-stone-200">
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
