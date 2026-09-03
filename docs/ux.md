# UX: keyboard, themes, language, small screens

## ⌘K

`⌘K` / `Ctrl-K` (or the Search box at the top of the sidebar) opens the command palette. It
jumps to any page, runs actions (new trigger, new board, Ask Galileo), and searches issues,
boards, gateway routes, triggers, prompts, users (identity seen in the last 7 days) and your
recent queries in one call (`GET /api/projects/{id}/search?q=`). Paste a 32-hex trace id to
open it directly.

## Keyboard

| Keys | Action |
| --- | --- |
| `⌘K` | search / jump |
| `⌘J` | Ask Galileo |
| `g` then `o q t l i b a s m r` | Overview, Query, Traces, Logs, Issues, Boards, AI, Settings, Map, Triggers |
| `/` | focus the page filter (issues, traces) |
| `j` / `k`, `Enter` | move through issue and trace lists, open |
| `Esc` | close drawers and dialogs |
| `?` | shortcuts sheet |

Shortcuts are ignored while typing in a field.

## Theme and language

The sidebar footer switches the theme (system / dark / light, remembered per browser) and the
language (English / Português do Brasil). Charts follow the theme. The translation covers
navigation, page titles, common buttons and empty states; query fields, attribute names and
data stay as they are. Add strings in `web/src/lib/i18n.ts` and use `useT()`.

## Small screens

Under 900px the sidebar becomes a drawer (menu button in the header), tables scroll
horizontally inside their own container and pages use tighter padding. Overview, Issues,
Triggers and Trace pages are usable on a phone for triage; the query builder and boards
want a tablet or larger.

## Trace diff

"Compare with…" on a trace opens `/traces/diff?a=<id>`; pick a second trace of the same root
(slowest first, last 24 h) or paste an id. Spans are aligned by service and name in order;
each row shows A, B and the delta, spans present on one side only are highlighted, and the
summary gives the total delta, DB-call count change, error change and the three biggest moves.

## Onboarding

A project without data shows "Get started" on the Overview, which opens `/welcome`: pick a
stack, create a key, copy the snippet, wait for the first trace (polls usage every 3 s) and
create a RED board from a template.

## What's new

`docs/CHANGELOG.md` is bundled into the app by `scripts/sync-changelog.sh` and shown at
`/changelog`; the sidebar item shows a dot until the latest version has been opened.
