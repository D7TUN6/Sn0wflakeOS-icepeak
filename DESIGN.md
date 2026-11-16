# Design

This file is the visual direction for Sn0wflakeOS: the console, the README and
screenshots, and anything built later such as an installer or a website. It is
direction, not rules.

Read this as: a systems project for people who read kernel code, in a cold and
precise visual language, dial ENERGY 2 / RHYTHM 2 / MOTION 1.

## Idea

Sn0wflakeOS looks like a machine that runs in a cold room. The name is playful,
the machine is not. One accent, hard edges, dense text, no decoration that does
not carry information. The snowflake in the name shows up in the console banner
and in the release art only, never as a background pattern or a floating 3D
object.

## Dials

- **ENERGY 2.** The project has a typographic voice (lowercase `icepeak`,
  uppercase `Sn0wflakeOS`, PID and exit-code lines) but it does not shout. A
  boot log is already dramatic; the design does not need to add more.
- **RHYTHM 2.** Documentation uses one grid with deliberate breaks: a wide
  screenshot, a compact feature list, a short table. No two adjacent sections
  share the same layout.
- **MOTION 1.** Boot output is the only motion. Hover states and links are
  enough. A kernel page that animates is a kernel page that lies about what it
  measures.

## Palette

| Token | Hex | Use |
| --- | --- | --- |
| deep ice | `#0B1B24` | background, console body, badge text on light |
| snow | `#F4F8FB` | primary text, light surface |
| glacier | `#58C4DC` | one accent: prompt, links, active state |
| frost | `#8CA3AD` | secondary text, borders, comments |
| warning amber | `#E0A458` | status, errors, prerelease labels |

Reasons: deep ice and snow are almost black and almost white, so the console
stays readable on cheap panels and in screenshots. Glacier is the only accent
and it is reserved for the prompt and interactive state, which is where the eye
should go. Frost keeps secondary text at about 6.6:1 against deep ice, above
WCAG AA for body text. Amber appears only when something is a warning or a prerelease,
never as a second accent.

Glacier is not used for small text on snow: it fails contrast on light
surfaces. On light backgrounds use deep ice for text and glacier for fills or
borders only.

## Type

- Prose: IBM Plex Sans. It was drawn for a technical company, has a calm
  lowercase and open apertures, and it is not the default sans the tools pick
  on their own.
- Code, console, shell output: IBM Plex Mono. Tabular by design, so `free`,
  `ls -l` and `top` columns line up. The console itself uses the in-tree 8x16
  bitmap font because a framebuffer without a font driver has no other option.
- Headings: Plex Sans, weight 600, sentence case. No uppercase tracking, no
  monospace headlines.

## Console

The console is the product. Rules for anything added to it:

- One prompt format: `path> `, path in glacier, no ASCII art, no emoji.
- Informational lines are prefixed with the subsystem name and a colon:
  `smp:`, `persist:`, `ramfs:`, `[sched]`. The prefix is the only color; the
  message is snow.
- Every line states a fact. No "loading...", no progress bars that do not
  track real work, no success banners for things that did not happen.
- Errors are amber, name the failing subsystem, and do not address the user in
  the first person. `persist: cannot write superblock`, not `Oops!`.
- A tool that shows columns aligns them with spaces, never with tabs.

## Documentation

- One screenshot at the top of the README, full frame, no fake window chrome,
  no added glow, no cursor drawn on top.
- Badges are static facts: version, state, kernel language, architecture,
  firmware, license. No build badge for a pipeline that has never run.
- No statistics without a source. Memory numbers come from a real `free -h`
  in the screenshot, not from a benchmark table.
- ASCII punctuation only: `,` `.` `:` `()` instead of em dashes, straight
  quotes instead of curly ones.
- Diagrams only when text cannot show the shape: boot chain, disk layout.

## Shape and space

- Corners: 0 radius on the console and on screenshots. 4 px on documentation
  cards or callouts, 8 px on the installer window. Nothing is a pill.
- Spacing scale: 4, 8, 16, 24, 40, 64. Sections breathe at 64.
- Borders are 1 px frost. Shadows are not used; this is a terminal aesthetic
  and elevation is expressed by background steps, not blur.
- Icons: none in documentation. The console has no icons.

## Voice

Dry, factual, first person in the history section only. No superlatives, no
"seamless", no "next generation". A feature is described by what it does and
what it costs. "The journal survives a power cut" is a claim; "rock solid
storage" is not.

## Anti-patterns for this project

- Snowflake clip art, 3D crystals, particle snow.
- A dark page with a purple or blue gradient behind the title.
- A fake terminal window around a real screenshot.
- A "trusted by" row, testimonials, or a pricing table for a hobby OS.
- Emoji in commit messages, docs or the console.
- More than one accent color on a screen.

## Contribution to this file

Changing the direction is a design decision, not a cleanup. Open an issue with
the reason and the surface it affects.
