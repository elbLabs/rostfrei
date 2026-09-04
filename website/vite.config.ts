import mdx from "@mdx-js/rollup"
import { defineConfig } from "vite"
import { devtools } from "@tanstack/devtools-vite"

import { tanstackRouter } from "@tanstack/router-plugin/vite"

import viteReact from "@vitejs/plugin-react"
import rehypeAutolinkHeadings from "rehype-autolink-headings"
import rehypeSlug from "rehype-slug"
import remarkGfm from "remark-gfm"
import tailwindcss from "@tailwindcss/vite"

const config = defineConfig({
  base: process.env.VITE_BASE_PATH ?? "/",
  resolve: { tsconfigPaths: true },
  plugins: [
    devtools(),
    tanstackRouter({ target: "react", autoCodeSplitting: true }),
    {
      enforce: "pre",
      ...mdx({
        providerImportSource: "@mdx-js/react",
        remarkPlugins: [remarkGfm],
        rehypePlugins: [rehypeSlug, rehypeAutolinkHeadings],
      }),
    },
    viteReact({ include: /\.(js|jsx|md|mdx|ts|tsx)$/ }),
    tailwindcss(),
  ],
})

export default config
