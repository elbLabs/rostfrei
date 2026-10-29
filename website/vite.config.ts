import mdx from "@mdx-js/rollup"
import { defineConfig } from "vite"
import { devtools } from "@tanstack/devtools-vite"

import { tanstackRouter } from "@tanstack/router-plugin/vite"

import viteReact from "@vitejs/plugin-react"
import rehypeAutolinkHeadings from "rehype-autolink-headings"
import rehypeSlug from "rehype-slug"
import remarkGfm from "remark-gfm"
import tailwindcss from "@tailwindcss/vite"
import { metadataPlugin } from "./metadata-plugin.ts"
import { referenceTheme } from "@rostfrei/ui/vite"
import { documentationPlugin, remarkDocumentationLinks } from "./docs-plugin.ts"

const documentationMdx = mdx({
  providerImportSource: "@mdx-js/react",
  remarkPlugins: [remarkGfm, remarkDocumentationLinks],
  rehypePlugins: [rehypeSlug, rehypeAutolinkHeadings],
})

const config = defineConfig({
  base: process.env.VITE_BASE_PATH ?? "/",
  resolve: {
    tsconfigPaths: true,
    dedupe: ["react", "react-dom", "@mdx-js/react"],
  },
  plugins: [
    referenceTheme(),
    devtools(),
    tanstackRouter({ target: "react", autoCodeSplitting: true }),
    {
      enforce: "pre",
      ...documentationMdx,
      transform(source, id) {
        // The macro explorer reads source text rather than compiled components.
        if (/[?&]raw(?:&|$)/.test(id)) return
        return documentationMdx.transform(source, id)
      },
    },
    viteReact({ include: /\.(js|jsx|md|mdx|ts|tsx)$/ }),
    tailwindcss(),
    metadataPlugin(),
    documentationPlugin(),
  ],
})

export default config
