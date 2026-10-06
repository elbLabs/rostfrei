import { useEffect } from "react"
import { useLocation } from "@tanstack/react-router"

import { metadataForPath, pageHeadTags } from "@/site"

export function PageMetadata() {
  const pathname = useLocation({ select: (location) => location.pathname })

  useEffect(() => {
    document.head
      .querySelectorAll("[data-rostfrei-meta]")
      .forEach((tag) => tag.remove())

    for (const { tag, attrs, children } of pageHeadTags(
      metadataForPath(pathname)
    )) {
      const element = document.createElement(tag)
      for (const [name, value] of Object.entries(attrs)) {
        element.setAttribute(name, value)
      }
      if (children) element.textContent = children
      document.head.append(element)
    }
  }, [pathname])

  return null
}
