# rostfrei identity studies

Five interactive design directions for the website and Tracer Studio, researched
on 14 October 2026. Each includes a homepage, a sample Studio workbench, light and
dark themes, palette swatches, and reference links. Reference ’15 is the selected
direction. Workshop ’12 and Reference ’15 extend Instrument using researched 2010s
developer-web references; Foundry and Fieldnotes remain in “Earlier studies.”

## Open the studies

From the repository root, serve the static preview:

```sh
python3 -m http.server 4178 --directory website/public
```

Open <http://localhost:4178/design-study/>. The same `/design-study/` directory is
served by the website's Vite development server and copied into its build. With
a deployment base path, append `design-study/` to that base.

The URL preserves the direction, theme, and surface, for example:

```text
/design-study/?direction=workshop&theme=light&view=website
/design-study/?direction=instrument&theme=dark&view=studio
/design-study/?direction=reference&theme=dark&view=studio
```

Theme initially follows the operating system. The preview toolbar lets you
compare explicit light and dark variants. The initial three studies use Google
Fonts with system fallbacks; the two 2010s studies use system sans/monospace.
Self-host any selected webfonts when implementing the identity.

Studio is an explicitly labeled design sample. Command selection, mode selection,
message inspection, and sample-result controls work locally. Its examples come
from the rental and return fixtures; simulation shows a proposed domain event,
and dispatch is unavailable without a runtime connection.

## Recommendation

**Build from Reference ’15, the user's selected direction.**

The user found the cream/editorial directions too familiar. The follow-up studies
emphasize visible navigation, compact technical content, borders, tabs, and
working examples. Workshop ’12 explores subtle control depth and a classic
project homepage; Reference ’15 explores a documentation-led shell with flat
surfaces. See [the dated research and comparison](2010s-references.md).

On 15 October, the user chose Reference ’15 and supplied their duck illustration
as the mascot/logo. The preview now uses the full rust-orange duck beside the
introduction, a head crop in the website and Studio wordmarks, and a matching
favicon. Dark variants add a fine neutral keyline around the charcoal details.
The original upload and derived assets are in `website/public/brand/`, with
processing notes in its README. This is a new brand choice following #110's
removal of the previous raccoon mascot.

The name suggests both Rust and stainless material. A clean, instrument-like
foundation with small oxide details makes that wordplay visible. Let rust mark
actions, selections, and annotations. Keep status colors independent.

### 01 / Foundry

- **Character:** warm paper, iron-charcoal, oxide; a contemporary technical manual.
- **Type:** IBM Plex Sans and IBM Plex Mono.
- **Composition:** asymmetric headline and source specimen, numbered flow rows,
  small corners, long rules, generous website spacing.
- **Headline:** “Domain logic. In plain sight.”
- **Studio:** warm neutral work surfaces, oxide selection rail, readable message
  cards, persistent detail panel.
- **Watch for:** turning everything brown. Most of the interface should remain
  neutral; the accent earns its place through interaction and hierarchy.

### 02 / Instrument

- **Character:** cool steel, blue-black graphite, burnt copper.
- **Type:** IBM Plex Sans and Mono, with more mono in product identifiers.
- **Composition:** wide headline and explanation, then a full-width source/flow
  specimen; square edges and compact, aligned panels.
- **Headline:** “Model. Execute. Inspect.”
- **Studio:** a denser workbench with clearly layered surfaces and panel headers.
- **Watch for:** generic enterprise-console styling or decorative industrial
  labels. Real domain names and meaningful connections supply the identity.

### 03 / Fieldnotes

- **Character:** chalk, aubergine ink, terracotta; an engineering publication.
- **Type:** Source Serif 4 for editorial headings; IBM Plex Sans and Mono for UI.
- **Composition:** marginal rust rule, open source specimen, horizontal numbered
  sections instead of feature cards.
- **Headline:** “Good systems start with a clear model.”
- **Studio:** serif only for orientation; controls, payloads, and messages retain
  practical sans/mono typography.
- **Watch for:** cream plus serif becoming another AI-product visual cliché.
  Actual source, diagram labels, and specific technical explanations matter.

### 04 / Workshop ’12

- **Character:** early-2010s open-source project site; cool steel and burnt rust.
- **Type:** Arial/Helvetica-style system sans and system monospace.
- **Composition:** centered content canvas, tabbed navigation, resource sidebar,
  34px headline, source example, a copyable inspection command, and reference rows.
- **Studio:** shallow toolbar gradients, visibly connected tabs, command fields
  above the flow, and a docked detail inspector.
- **Evidence:** GitHub's September 2012 engineering screenshots, Redis.io's May
  2012 source, and preserved Bootstrap 2.0.4 components.

### 05 / Reference ’15

- **Character:** a documentation-led product with Instrument's steel/rust palette.
- **Type:** system sans and monospace; 32px headline, 14px introduction.
- **Composition:** left navigation, compact section headers, plain links, source
  and flow evidence, runnable model inspection, structured reference tables.
- **Studio:** project tree, message list, and right-hand inspector; rust selection
  rails with ordinary links in blue.
- **Evidence:** Rust's May 2015 homepage, GitLab 8.0, and Read the Docs 0.1.9.

## Paired palettes

These are the implemented study colors. The complete role tokens are in
`website/public/design-study/study.css` and `eras.css`; each study displays its
current palette.

| Direction / theme | Canvas | Surface | Text | Secondary | Rust |
| --- | --- | --- | --- | --- | --- |
| Foundry / light | `#F3F0E9` | `#FAF8F3` | `#272821` | `#626257` | `#9C432B` |
| Foundry / dark | `#1D1E1A` | `#252620` | `#EEEAE0` | `#B2B0A3` | `#E09B78` |
| Instrument / light | `#EEF1F2` | `#F9FAFB` | `#212A30` | `#57636B` | `#A14125` |
| Instrument / dark | `#181E22` | `#20282D` | `#E8EEF0` | `#A4B1B9` | `#EC9B75` |
| Fieldnotes / light | `#F5F0EA` | `#FCF8F3` | `#352B30` | `#6D5D64` | `#9A4439` |
| Fieldnotes / dark | `#241F23` | `#2D272C` | `#F0E7DF` | `#BEADB2` | `#E7A48D` |
| Workshop / light | `#E4E8EB` | `#FFFFFF` | `#29313A` | `#555F69` | `#A54122` |
| Workshop / dark | `#141A20` | `#222A32` | `#EDF0F3` | `#B2BEC9` | `#EFA079` |
| Reference / light | `#EDF0F2` | `#FFFFFF` | `#242B33` | `#53606B` | `#9F421F` |
| Reference / dark | `#141B22` | `#1C252E` | `#EAF0F5` | `#ADBDCC` | `#F0A479` |

Dark mode uses a brighter copper accent with dark button text. Light mode uses
deeper oxide with pale button text. A single orange cannot do both jobs equally
well. Surface elevation gets lighter in dark mode. Code and diagrams participate
in the theme rather than remaining permanently dark.

Use subtle borders for grouping, stronger borders for form controls, and an
explicit focus outline. Labels and shapes accompany status and message kind.

## Research and what it contributes

Two parallel research agents investigated brand references and the local/PR-head
interfaces. The following observations informed the proposals; the palettes and
compositions above are original design interpretations.

| Reference | Observed design principle | Application to rostfrei |
| --- | --- | --- |
| [Oxide identity / Pentagram](https://www.pentagram.com/work/oxide) | Technical diagram and terminal-derived language spans hardware, website, and product UI. | Build recognition from actual domain artifacts and shared message-card anatomy. |
| [Teenage Engineering TX–6](https://teenage.engineering/products/tx-6) | Precise equipment labels, material references, and detailed specifications. | Calm surfaces, aligned information, small purposeful controls. |
| [IBM Carbon color](https://carbondesignsystem.com/guidelines/color/overview/) | Role-based tokens and theme-aware neutral layers. | Shared surface, focus, selection, and semantic status roles. |
| [Ink & Switch: Local-first](https://www.inkandswitch.com/essay/local-first/) | Technical explanations integrate figures, captions, and editorial typography. | Explain one implemented example beside its source and outcome. |
| [Tufte CSS](https://edwardtufte.github.io/tufte-css/) | Restrained color, readable prose, and integrated annotations. | Fieldnotes' rules, figure hierarchy, and source references. |
| [Grafana trace exploration](https://grafana.com/docs/grafana/latest/explore/trace-integration/) | Execution context, overview, detail rows, and selected-span attributes form a hierarchy. | A stable Studio flow-to-inspector relationship. |
| [Zed themes](https://zed.dev/docs/themes) | Explicit light/dark/system preference and themed working surfaces. | Consistent theme behavior across website, docs, and Studio. |

Typeface sources: [IBM Plex](https://github.com/IBM/plex) and
[Source Serif](https://github.com/adobe-fonts/source-serif), both open source.

## What the audit found

The current checkout's website and Studio have separate visual vocabularies:

- `website/src/index.css`: brown-black, cream, coral; the theme provider exists,
  but the stylesheet supplies a dark-only foundation.
- `studio/src/index.css`: cool blue/cyan, hard-coded light text, luminous graph
  treatments, and several very small labels.
- `website/src/components/project-structure/ProjectStructureSection.tsx` and
  `website/src/components/macro-explorer/MacroSlideCard.tsx`: independent panel
  colors and large corner radii create additional visual systems.
- `website/src/components/docs/DomainFlow.tsx` and code-rendering components:
  dark-specific rendering needs to join the theme contract.

The perceived inconsistency is structural as well as chromatic. Large reference
carousels, atmospheric graph decoration, and competing card treatments obscure
the simpler story: find a rule, read its owner, inspect its outcome.

### Relationship to #110

[PR #110](https://github.com/elbLabs/rostfrei/pull/110) was open during research.
It removes the mascot, tightens documentation, corrects model inventory and flow
labels, and replaces fictional FleetPlanning material with the implemented
BicycleTransfer service. Its final scope is copy/documentation and website
examples; newer Studio features are inherited from its main-branch baseline.

This checkout predates that baseline. The eventual production redesign should
start from the reconciled code and preserve its newer command execution,
observation, message-card, and persistent-inspector capabilities.

The studies apply #110's editorial direction through specific product language,
source-linked examples, and correct distinctions between acceptance, emitted
events, simulation, isolated tests, and production dispatch. The compact rental
example is verified against:

- `examples/bike-rental/src/domain/bike_rental/rental_fleet/rent_bicycle/command.rs`
- `examples/bike-rental/tests/tracer/rent-available-bicycle.json`
- `examples/bike-rental/tests/tracer/return-rented-bicycle.json`

## Proposed production rework

The selected direction is now being implemented through the
[Reference ’15 rollout](reference-15-rollout.md): one internal `@rostfrei/ui`
package shared by the production website, docs, and Tracer Studio, with a live
local component gallery in `packages/ui/gallery` (`pnpm dev:ui`).

### Website

1. **Short hero:** one useful headline, explicit Rust-platform descriptor, primary
   getting-started action, secondary runnable-example link.
2. **One real example:** show rental command → domain event → integration event,
   with a clearly marked private/public boundary and source references. Explain
   the unavailable-bicycle rejection alongside the accepted path.
3. **Find the owner → read the Rust → inspect the outcome:** three concise sections
   connecting structure to behavior. Use BicycleTransfer when explaining a real
   cross-aggregate domain service.
4. **Studio as evidence:** reuse the product's message cards and diagram language.
   Clearly distinguish expected, proposed, and observed messages.
5. **Reference depth through docs:** compact structure and macro excerpts link to
   browsable reference pages. Give the full inventory proper navigation.
6. **Run the example:** start with the verified model-inspection command, then
   provide NATS-backed execution and Studio setup from the reconciled baseline.

Voice: concrete nouns, active verbs, short claims. Describe what a feature does
and show its evidence. Keep implementation details beside the relevant example.

### Tracer Studio

Use a stable navigation → working area → inspector structure. Keep execution
mode, connection state, command identity, and outcome visible. Let canvas focus
collapse the surrounding docks while preserving selection.

Use 13–14px working text and roughly 12px metadata in the production UI. Density
comes from aligned rows and predictable panels. On narrow screens, provide a
message list and a detail view with persistent selection.

Keep three independent concepts visually distinct:

- **Kind:** command, domain event, integration event.
- **Status:** accepted/rejected, pending/completed, passing/failing test.
- **Selection:** the item or path currently under inspection.

A rejected command can be the expected result of a passing test. Exact causality,
association, fixture context, and stream order need distinct labeled treatments.

### Shared foundation

Extract semantic CSS tokens and a small set of common presentation components:
brand lockup, theme control, buttons, tabs, panel headers, message cards, status
labels, code/identifier treatment, and connection/empty states. Both frontends
already use compatible React/Tailwind/Radix building blocks.

Then apply the chosen identity to website, docs, and the newer Studio shell,
including graph edges, syntax highlighting, overlays, and browser-native controls.

## Preview verification

The initial three studies were checked in Chromium at 320, 390, 768, and 1440px: three directions × two themes
× two surfaces × four widths, with no document-level horizontal overflow or
JavaScript errors. Also checked command/message selection, fixture names and
schema versions against repository data, sample execution modes, URL-preserved
appearance, keyboard skip navigation, system-theme initialization, and reduced
motion.

Calculated minimum contrast across the checked role/background combinations:
body text 10.61:1, secondary text 4.63:1, rust text 4.84:1, filled-button labels
6.16:1, success labels 5.96:1, and form-control borders 3.19:1. These are targeted
preview checks, not a complete accessibility audit.
