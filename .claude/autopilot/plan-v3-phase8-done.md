# Galileo V3 — Phase 8 (UX)

Rules: tick only when it compiles/tests pass (cargo build, `npx tsc --noEmit`, docker images rebuilt). Never git commit/push. Validate in the browser on the melea project. Last phase of V3: finish with a recap.

## ⌘K and keyboard
- [x] ⌘K / Ctrl-K command palette (`components/command-palette.tsx`): pages of the current project, "go to trace <id>" (32-hex detection), issues (title search via `/issues?q=`), boards, gateway routes, users (identity search → sessions/query by user_id), recent queries (history), actions (new trigger, new board, ask Galileo); backed by `GET /projects/{id}/search?q=` (issues, boards, routes, users, prompts, triggers in one call)
- [x] keyboard navigation: `g` then `o/q/t/l/i/b/a/s` jumps to Overview/Query/Traces/Logs/Issues/Boards/AI/Settings; `/` focuses the page filter; `j/k` + Enter move through issue and trace lists; `?` shows a shortcuts sheet; `Esc` closes drawers; hints in the palette

## Themes, language, small screens
- [x] light theme: tokens in globals.css under `[data-theme=light]`, ECharts palette aware, toggle in the sidebar footer (system / dark / light) persisted in localStorage + `prefers-color-scheme`
- [x] pt-BR: `lib/i18n.ts` dictionary (navigation, page titles, buttons, empty states, settings tab names, common labels) with `useT()`; locale switch (en / pt-BR) in Settings → Personal, persisted; date/number formatting follows locale
- [x] tablet/phone: sidebar becomes a drawer under 900px, tables scroll horizontally, Overview / Issues / Triggers / Trace pages usable at 768 and 390 widths (checked with the browser resize)

## Trace diff, onboarding, changelog
- [x] trace diff: `/traces/diff?a=<id>&b=<id>` (button "Compare with…" on a trace, picks a second trace of the same route from the issue/trace list); spans aligned by (service, name) in order, columns A/B with duration delta, missing spans highlighted, summary line (total delta, extra DB calls, new errors)
- [x] onboarding wizard for a new project (`/p/{id}/welcome`, shown while the project has no spans): step 1 pick stack (Django/Python/Node/PHP/Rust/Android/browser) → step 2 key + snippet → step 3 "waiting for the first trace" (poll `/usage`) → step 4 create a template board and go to Overview; link from the empty Overview
- [x] in-app changelog: `docs/CHANGELOG.md` (V1 → V3 entries) rendered at `/changelog`, "What's new" item in the sidebar with a dot until the latest version is seen (localStorage)

## Validation + wrap-up
- [x] melea: ⌘K finds an issue, a board, a route and a trace id; `g i` navigates; light theme readable on Overview/Query/trace; pt-BR switches the nav; phone width shows the drawer; trace diff between a fast and a slow /api/consultas trace shows the delta; onboarding on a fresh project reaches the first trace with the Python SDK example; changelog renders
- [x] docs/ux.md, memory updated, plan archived, V3 recap written to docs/v3-recap.md
