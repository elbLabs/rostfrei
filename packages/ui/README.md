# @rostfrei/ui

The shared Reference ’15 interface for the website, documentation, and Tracer
Studio. React 19, Tailwind 4, and shadcn-style Radix primitives; source is consumed
directly by each application's bundler. The visual contract is cool steel,
compact Arial/Helvetica and Consolas typography, flat square panels, rust actions
and selection, and blue links.

## Application setup

Declare `"@rostfrei/ui": "workspace:*"` in the application's dependencies. Import
Tailwind first, then the shared stylesheet, and scan the shared sources (adjust
the source path relative to your application's stylesheet):

```css
@import "tailwindcss";
@import "@rostfrei/ui/styles.css";
@source "../../packages/ui/src";
```

```tsx
import { ThemeProvider, ThemeToggle } from "@rostfrei/ui/theme"
import { TooltipProvider } from "@rostfrei/ui/components/tooltip"

<ThemeProvider>
  <TooltipProvider>
    <App />
    <ThemeToggle />
  </TooltipProvider>
</ThemeProvider>
```

The stylesheet provides tokens, typography, link/selection/focus defaults, and
theme-aware artwork. Applications own document sizing, viewport behavior,
navigation, and content layout. No animation extension is required.

## Components

All component modules are exported as `@rostfrei/ui/components/<name>`. Native
props, React 19 refs, and `className` are forwarded. `cn` is exported at
`@rostfrei/ui/lib/utils`; it merges class names with Tailwind conflict resolution.
Package-internal imports are relative.

| Module | Exports / API |
| --- | --- |
| `button` | `Button`, `buttonVariants`, `ButtonProps`. `asChild`; variants `default` (rust), `outline`, `secondary`, `ghost`, `destructive`, `link`; sizes `default`, `xs`, `sm`, `lg`, `icon`, `icon-xs`, `icon-sm`, `icon-lg`. Native button submission behavior is preserved: set `type="button"` for non-submit actions in forms. |
| `badge` | `Badge`, `badgeVariants`, `BadgeProps`. `asChild`; variants `neutral` (default), `default`, `secondary`, `outline`, `success`, `warning`, `info`, `destructive`, `danger`, `live`, `command`, `domain-event`, `integration-event`. `danger`/`live` retain Studio compatibility. |
| `input`, `textarea` | `Input`, `Textarea`. Native inputs with disabled, focus, and `aria-invalid` styling. Supply a visible label with `htmlFor`/`id`. |
| `native-select` | `NativeSelect`, `NativeSelectOption`, `NativeSelectOptGroup`, `NativeSelectProps`. Native `<select>` API: `value`, `onChange(event)`, `multiple`, numeric `size`, `<option>` and `<optgroup>` children. `className` styles the select; `wrapperClassName` styles its container. |
| `select` | The same native API as above, also aliased to `Select`, `SelectOption`, `SelectOptGroup`. **Not** the compound Radix `SelectTrigger`/`SelectContent` API. |
| `tabs` | `Tabs`, `TabsList`, `TabsTrigger`, `TabsContent`, `tabsListVariants`. Radix controlled/uncontrolled values and keyboard navigation. `TabsList variant="default" \| "line"`; `Tabs orientation="horizontal" \| "vertical"`. |
| `tooltip` | `TooltipProvider`, `Tooltip`, `TooltipTrigger`, `TooltipContent`. Provider delay defaults to 120 ms; portaled content defaults to a 6 px side offset. |
| `popover` | `Popover`, `PopoverTrigger`, `PopoverAnchor`, `PopoverClose`, `PopoverContent`. Radix APIs; portaled content defaults to a 6 px side offset. |
| `separator` | `Separator`, horizontal by default; `orientation="vertical"` supported. Decorative by default. |
| `sheet` | `Sheet`, `SheetTrigger`, `SheetClose`, `SheetPortal`, `SheetOverlay`, `SheetContent`, `SheetHeader`, `SheetFooter`, `SheetTitle`, `SheetDescription`. Content supports `side="top" \| "right" \| "bottom" \| "left"` (default right) and `showCloseButton` (default true). |
| `dialog` | `Dialog`, `DialogTrigger`, `DialogClose`, `DialogPortal`, `DialogOverlay`, `DialogContent`, `DialogHeader`, `DialogFooter`, `DialogTitle`, `DialogDescription`. Content supports `showCloseButton` (default true); footer supports an optional close button (default false). |
| `collapsible` | `Collapsible`, `CollapsibleTrigger`, `CollapsibleContent`; standard Radix API. |
| `skeleton` | `Skeleton`; decorative, with reduced-motion-aware pulse. Give its loading region `aria-busy` and an accessible status when needed. |
| `table` | `Table`, `TableHeader`, `TableBody`, `TableFooter`, `TableRow`, `TableHead`, `TableCell`, `TableCaption`. Table includes a horizontal overflow wrapper; selected rows use `data-state="selected"`. |
| `panel` | `Panel`, `PanelHeader`, `PanelTitle` (`h2`), `PanelContent`. Flat border, compact muted header, white/dark-steel content. |

Radix primitives retain focus management, keyboard interactions, controlled and
uncontrolled state, and `asChild` where supported. Give dialogs and sheets a
title, and use their description components for supporting text. Name icon-only
buttons using `aria-label` or visually hidden text. Tooltips supplement control
names rather than replacing them.

```tsx
import { NativeSelect } from "@rostfrei/ui/components/native-select"

<label htmlFor="run-mode">Run mode</label>
<NativeSelect id="run-mode" value={mode} onChange={(e) => setMode(e.target.value)}>
  <option value="simulate">Simulate</option>
  <option value="dispatch">Dispatch</option>
</NativeSelect>
```

## Theme

`ThemeProvider` accepts `defaultTheme` (default `"system"`), `storageKey` (default
`"rostfrei-theme"`), and `disableTransitionOnChange` (default `true`). Mount one
provider per application. `useTheme()` returns `{ theme, resolvedTheme,
setTheme }`; `theme` is `"light" | "dark" | "system"`, and `resolvedTheme` is
`"light" | "dark"`. The root receives exactly one of `.light` and `.dark`.

System preference changes and cross-tab local-storage changes are subscribed to
and cleaned up. Invalid/cleared stored values fall back to `defaultTheme`.
Rendering is SSR-safe with a deterministic server snapshot; blocked/full storage
still permits in-memory theme changes. No global keyboard shortcuts are bound.

`ThemeToggle` uses a keyboard-accessible radio menu with all three settings. It
accepts button props (`variant`, `size`, `className`, `disabled`, etc.) plus
`compact` for an icon-only trigger. `asChild` and custom `children` are omitted.

For flash-free startup in Vite, add the shared plugin to the application's
existing configuration:

```ts
import { defineConfig } from "vite"
import { referenceTheme } from "@rostfrei/ui/vite"

export default defineConfig({
  plugins: [referenceTheme()],
})
```

`referenceTheme({ defaultTheme?, storageKey? })` injects a synchronous inline
script at `head-prepend` with `transformIndexHtml.order: "pre"` in development
and production builds. It applies the stored/system theme before CSS and app
modules execute. The entry point and boot generator are plain JavaScript with
TypeScript declarations, so Node can import the plugin when Vite externalizes
config dependencies; it loads neither React nor Vite at runtime.

The plugin and `ThemeProvider` must use the same storage key and default theme.
Both default to `"rostfrei-theme"` and `"system"`. Configure a CSP nonce/hash
when your app uses CSP.

For other HTML integrations, `getThemeScript({ storageKey?, defaultTheme? })`
returns the same small inline boot script. `THEME_INIT_SCRIPT` is the default
script and `THEME_STORAGE_KEY` exports the default storage key. Insert the
script in the document head before styles. For a rendered HTML template:

```tsx
import { THEME_INIT_SCRIPT } from "@rostfrei/ui/theme"

<script dangerouslySetInnerHTML={{ __html: THEME_INIT_SCRIPT }} />
```

## Tokens

Standard shadcn tokens include `--background`, `--foreground`, `--card`,
`--popover`, `--primary`, `--secondary`, `--muted`, `--accent`, their foreground
partners, `--border`, `--input`, `--ring`, sidebar tokens, and chart tokens.
`--accent` is the soft rust selection surface; `--primary` is the rust action.
`--control-border` is stronger than the panel divider color.

All shared portals (theme menu, tooltip, popover, dialog, and sheet) use
`z-(--layer-overlay)` with `--layer-overlay: 100`. Applications with dynamically
raised panels should isolate their workspace stacking context (for example,
`.studio-shell { isolation: isolate; }`) so those panels stay below the shared
body-portaled overlays.

Additional semantic colors each have matching Tailwind aliases (`text-link`,
`bg-success-soft`, `border-command`, etc.):

- `--link`
- `--success`, `--success-soft`
- `--warning`, `--warning-soft`
- `--info`, `--info-soft`
- `--destructive`, `--destructive-soft`, `--destructive-foreground`
- `--command`, `--command-soft`
- `--domain-event`, `--domain-event-soft`
- `--integration-event`, `--integration-event-soft`
- `--control-border`

Semantic foreground colors are designed for their corresponding soft surface.
Keep state/message labels or icons alongside color distinctions. Both palettes
are defined in `src/styles.css`; core colors match the selected design study.

## Brand and assets

```tsx
import { Brand, DuckMascot } from "@rostfrei/ui/brand"

<a href="/" aria-label="Rostfrei home"><Brand /></a>
<Brand compact className="shrink-0" />
<DuckMascot alt="Rostfrei duck mascot" className="w-32" />
```

`Brand({ compact?, className? })` is a noninteractive duck-and-wordmark span.
`DuckMascot({ className?, alt? })` displays the full illustration (`alt=""` makes
it decorative). The containing link owns navigation and accessible naming.
Light/dark art follows explicit theme classes through shared CSS, including a
user override that differs from the operating system. Bundled asset URLs honor
Vite's deployment base and asset hashing.

Vite HTML can resolve the favicon directly:

```html
<link rel="icon" type="image/png" href="@rostfrei/ui/brand/duck-icon.png" />
```

All PNGs are exported via `@rostfrei/ui/brand/*.png`. See
[`src/brand/README.md`](src/brand/README.md) for provenance and derivation details.

## Maintenance

Run from the repository root after the workspace install:

```sh
pnpm --filter @rostfrei/ui lint
pnpm --filter @rostfrei/ui typecheck
```

`components.json` declares the shadcn workspace paths. After generating a
primitive, retain relative imports inside the package and adapt its tokens and
spacing to Reference ’15. Preserve existing public APIs while extending the
shared package. Consumers exercise visual/interaction behavior through the live
local component gallery and Studio's browser fixtures.

## Local component gallery

Run `pnpm dev:ui` from the repository root (or `pnpm --filter @rostfrei/ui dev`).
The development gallery lives in `gallery/` and opens at
<http://localhost:4177/>. It uses the shared library directly and has no website
router or page-shell dependency.
