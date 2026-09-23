# AgentWay UI system

This is the implementation contract for every UI change, including small fixes. Read it before implementation along with AGENTS.md. Screenshots in chat are feedback, not a substitute for applying these rules. Do not require the user to specify spacing, alignment or standard interaction states.

## Before implementation

Inspect the actual page, its related pages and source. Identify the existing component to reuse. Decide its data source, heading hierarchy, alignment anchors, spacing tokens, responsive layout and loading/empty/error/saving/disabled/success states before writing JSX or CSS. Use the current dark slate/pale-green palette and corresponding light tokens. Do not create another visual variant of an existing control. If a shared component is inadequate, improve it deliberately rather than adding an unrelated one-off style.

## Foundations

Use the CSS variables in web/src/style.css. Spacing scale: 4, 8, 12, 16, 24, 32 pixels (`--space-*`). Use 4 for icon/detail separation, 8 for tightly related controls, 12 for control contents, 16 for table cells, 24 for section padding and label-to-control gaps, 32 for major sections. No arbitrary margins to make a single screenshot appear aligned. Borders use --border; page/surface/control colors use semantic tokens. Controls use a 6px radius; containers use 9px. Retain existing page-heading/navigation typography and palette when fixing content.

One h1 per page. Setting labels use 14px/600 with 20px line height; table labels and dates use 12px. Titles use 600, ordinary values 400. Supporting text must serve a real decision, state or recovery action. No slogans, rationale, preview notices, planned features or redundant subtitles. Whitespace groups related controls; it must not create empty panels or oversized heading bands.

## Settings

Use `.settings-list` and `.setting-row`: a bounded 880px content area, 180px label column, 24px column gap/vertical padding and subtle dividers. On narrow screens stack label above controls with 12px gap. Controls share a 44px minimum height; align labels with the first control's text, not a container's arbitrary outer edge. Avoid a card/header for each checkbox.

Theme is one native radio group styled as a segmented selector, with Light/sun and Dark/moon choices. Keep native keyboard interaction and a visible selected and focus state. No duplicate visible radio circles inside button outlines. Saveable text fields have an adjacent Save action; disable it when unchanged or saving. Keep field and action aligned, clear stale success feedback on edit, retain draft on failure and report feedback in the affected row. Automatic settings show pending/error state and must not reload the page. Labels must identify the affected platform and scope.

## Operational lists

Tasks and Activity log use the same WorkTable and WorkMetadata components in web/src/workspace.tsx. Same column widths, header treatment, padding, alignment, date formatting and badges. Columns: Task/Activity, Agent, Platform, Created, Status. The title is primary. Tasks navigate to work details/recovery; Activity expands one correlated operation into its steps. Use an explicit chevron button with aria-expanded/aria-controls. Never replace a whole list on a row update or fetch step history before expansion.

Below 1000px, reflow each record into labelled fields; do not hide Agent, Platform, date, status or actions. Long titles wrap without pushing columns offscreen. Keep semantic table markup and column headers. Agent names must come from recorded attribution, never a guessed product or today's renamed connection. Historical missing attribution displays Not recorded; new uploads snapshot the configured connection name. Do not imply support for independently identified credentials until the backend supports it.

## Verification before presenting any UI change

Check all affected pages in dark and light modes, desktop and 390px mobile, and one intermediate width. Inspect screenshots, not just build output. Check column alignment, consistent spacing, wrapped titles, keyboard focus and control hit areas; no horizontal page overflow or missing fields. Exercise changed interactions, pending/failure recovery and scoped refresh. Mock writes when testing permissions/configuration; never mutate the user's live settings as test data. Compare related screens using the shared component and review the diff for unrelated changes. Document what was actually tested. Existing violations are not precedents to copy.

Feature branch deployments are for user testing. Only after explicit merge approval: merge, return to main, rebuild/restart and verify for normal use.
