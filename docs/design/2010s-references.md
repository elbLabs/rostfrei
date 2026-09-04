# Instrument, through the 2010s developer web

## Direction from feedback

The user preferred **Instrument** and found the warmer paper/serif explorations
too close to current website conventions. The second research round examines
developer sites and working interfaces from 2011–2015.

After reviewing these studies, the user selected **Reference ’15** on 15 October
and supplied their own duck illustration as the mascot/logo. Reference is now
the default preview, with the full illustration beside the introduction, a head
crop in both wordmarks, and a matching favicon. Dark-surface variants preserve
the artwork's colors and add a fine silhouette keyline.

Two research agents investigated the early and middle parts of the decade using
dated posts, actual period screenshots, and versioned HTML/CSS. These studies
are contemporary interpretations of those sources. Their paired light/dark
themes are new designs rather than claims about the originals.

## Open the two new examples

- [Workshop ’12 — website](http://localhost:4178/design-study/?direction=workshop&theme=light&view=website)
- [Workshop ’12 — dark Studio](http://localhost:4178/design-study/?direction=workshop&theme=dark&view=studio)
- [Reference ’15 — website](http://localhost:4178/design-study/?direction=reference&theme=light&view=website)
- [Reference ’15 — dark Studio](http://localhost:4178/design-study/?direction=reference&theme=dark&view=studio)

Use the existing SSH tunnel to port 4178. Reference ’15 is the default. The toolbar
also includes Instrument and an “Earlier studies” menu.

## What the period actually looked like

### GitHub, September 2012

[Dated engineering post](https://github.blog/news-insights/how-we-keep-github-fast/)
· [repository screenshot](https://github.blog/wp-content/uploads/2012/09/mission-control.png)
· [trace screenshot](https://github.blog/wp-content/uploads/2012/09/render-trace.png)

The repository screenshot shows a white workspace, blue links, framed toolbars,
gray-gradient controls, tightly grouped actions, and explicitly selected tabs.
The diagnostic view uses dense, indented rows with timings. Its black metrics bar
was a staff tool, and the post notes that some pictured features were unreleased.

**Borrow:** connected controls, an always-visible execution summary, strong
grouping, readable trace rows, and deliberate selected states.

### Redis.io, May 2012

[Dated source snapshot](https://github.com/redis/redis-io/tree/3f230d2b974ced753a3c4a5057e006a01d7058b7)
· [stylesheet](https://raw.githubusercontent.com/redis/redis-io/3f230d2b974ced753a3c4a5057e006a01d7058b7/views/styles.sass)
· [homepage](https://raw.githubusercontent.com/redis/redis-io/3f230d2b974ced753a3c4a5057e006a01d7058b7/views/home.haml)

The source specifies a 900px maximum-width page, a 45px dark header, gray outer
canvas, near-white content, Helvetica-style 13px body type, and 26px headings.
The homepage combines a technical introduction, “Try it,” a versioned download,
and a community sidebar.

**Borrow:** the compact project homepage. Put a specific explanation, working
example, and reference destinations together.

### Bootstrap 2.0.4, June 2012

[Preserved site](https://getbootstrap.com/2.0.4/)
· [components](https://getbootstrap.com/2.0.4/components.html)
· [dated tag](https://api.github.com/repos/twbs/bootstrap/git/tags/41d322043ba069913aec7b982c79401aadcf5293)

The component system uses Helvetica Neue/Helvetica/Arial, 1px borders, 4px button
corners, shallow gradients, inset highlights, joined button groups, tabs, and
breadcrumbs. Its homepage also has an 81px headline: the decade contained large
marketing typography as well as compact working interfaces.

**Borrow:** visible control affordances and tab geometry. Contemporary readability
and responsive layout should guide the actual sizes.

### Django, May 2012

[Dated source snapshot](https://github.com/django/djangoproject.com/tree/7e8c265cc2554f11805259d71e8a59721da25acc)
· [stylesheet](https://raw.githubusercontent.com/django/djangoproject.com/7e8c265cc2554f11805259d71e8a59721da25acc/media/css/base.css)

The source uses a deep-green masthead, Verdana body type, Trebuchet headings,
white main content, supporting columns, and underlined brown-orange links.
Explanation, documentation/downloads, and project activity sit together.

**Borrow:** the practical hierarchy and a brand accent with a real navigation role.

### Heroku Postgres, November 2011

[Launch post](https://www.heroku.com/blog/heroku_postgres_launches/)
· [attached interface screenshot](https://www.heroku.com/wp-content/uploads/2025/03/1473343697-heroku-postgres-screenshot.jpg)

The screenshot has a blue-black canvas, a shallow raised toolbar, small utility
navigation, resource rows, and inline synchronization state. Its current image
hosting path dates from 2025; period attribution comes from the 2011 launch post.

**Borrow:** dark steel as a working environment, with modest toolbar depth and
state shown alongside the resource it describes.

### Rust, May 2015

[Dated source snapshot](https://github.com/rust-lang/prev.rust-lang.org/tree/77e65109c281aa56216a8967136f495170825227)
· [homepage](https://github.com/rust-lang/prev.rust-lang.org/blob/77e65109c281aa56216a8967136f495170825227/index.html)
· [stylesheet](https://github.com/rust-lang/prev.rust-lang.org/blob/77e65109c281aa56216a8967136f495170825227/css/style.css)

Fira Sans, an 820px content width, a 25px product statement, grouped documentation
links, explicit version/install controls, and an editable example with Run and
output regions. The homepage makes the language tangible through use.

**Borrow:** show the actual model and an inspection command immediately.

### GitLab 8.0, September 2015

[Design variables](https://github.com/gitlabhq/gitlabhq/blob/v8.0.0/app/assets/stylesheets/base/variables.scss)
· [sidebar](https://github.com/gitlabhq/gitlabhq/blob/v8.0.0/app/assets/stylesheets/generic/sidebar.scss)
· [project navigation](https://github.com/gitlabhq/gitlabhq/blob/v8.0.0/app/views/layouts/nav/_project.html.haml)

15px Source Sans Pro, 13px monospace, a 58px header, a 230px project sidebar,
steel-tinted canvas, white panels, and labeled destinations with useful counts.

**Borrow:** stable project context, task-oriented navigation, bordered work areas,
and readable data. This is especially relevant to Studio.

### Read the Docs 0.1.9, September 2015

[Versioned theme](https://github.com/readthedocs/sphinx_rtd_theme/tree/0.1.9)
· [compiled CSS](https://github.com/readthedocs/sphinx_rtd_theme/blob/0.1.9/sphinx_rtd_theme/static/css/theme.css)

Lato body and Roboto Slab headings, a 300px dark navigation rail, blue project
header, nested contents, breadcrumbs, and a near-white reading surface capped at
800px.

**Borrow:** a persistent reference structure. The rostfrei interpretation uses
Instrument-compatible sans headings and restrained nesting.

### Go / godoc, August 2015

[Dated source snapshot](https://github.com/golang/tools/tree/93604a3dc2a5ae0168456c672ec35cc90ea881e6/godoc/static)
· [Go 1.5 announcement](https://go.dev/blog/go1.5)

Arial body text, Menlo code, blue links, pale-blue section bars, explicit reference
destinations, search, and playground controls for Run, Format, and Share.

**Borrow:** code, results, and reference navigation as first-class page elements.

## What changes in the prototypes

| | Workshop ’12 | Reference ’15 |
| --- | --- | --- |
| Website | Centered project canvas, tabbed header, right resource rail | Left navigation spine, compact document-like page |
| Typography | 34px bold system-sans heading | 32px restrained system-sans heading |
| Surface treatment | Shallow toolbars and button depth, 3px corners | Flat layers, square panels |
| Studio | Framed flow nodes and connected tabs | Compact message rows and selection rail |
| Shared | Real source, a model-inspection command, explicit links, docked inspector | Real source, a model-inspection command, explicit links, docked inspector |

Both retain the Rust accent. Ordinary links use steel blue; selection and the
primary action use oxide/copper. Status remains independently labeled. The name
and product behavior supply the identity rather than decorative nostalgia.

The inspection command is taken from the runnable bike-rental example:

```sh
cargo run --locked -p bike-rental --bin bike-rental-model
```

This preserves #110's emphasis on implemented examples and precise descriptions.
The source example and expected flow remain fixture-backed, and the UI labels
sample results explicitly.

## Selected direction

Build on **Reference ’15**: persistent navigation, visible technical evidence,
compact working surfaces, and shared website/Studio styling. The user's duck
artwork is integrated with variants suited to the website masthead and the
compact Studio header; the original image is preserved in `website/public/brand/`.

## Verification

The expanded playground was checked in Chromium across five directions, two
themes, two surfaces, and widths of 320, 390, 768, 1024, and 1440px: 100
combinations. Checks include overflow, JavaScript errors, disclosure/focus
behavior, clipboard copying, navigation, sample command/mode/message selection,
and deep-link reloads.

The new palettes' checked text-role combinations have a minimum contrast of
5.04:1; form-control boundaries reach at least 3.23:1. Subtle divider colors are
used for grouping rather than as the sole indication of an interactive control.
