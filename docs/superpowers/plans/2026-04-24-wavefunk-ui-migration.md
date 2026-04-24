# Wavefunk UI Migration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace substrukt's twind-based UI with the wavefunk company design system — pure CSS, zero-radius, hairline-driven, dark-by-default.

**Architecture:** Copy wavefunk CSS + fonts into `static/css/`, override accent to amber. Port 29 minijinja templates from twind utility classes to `.wf-*` component classes. Remove twind CDN entirely. Add modeline statusbar. Single release.

**Tech Stack:** Pure CSS (wavefunk.css), minijinja templates, htmx, EasyMDE (as-is)

**Verification model:** No unit tests — this is a visual reskin. Each task verifies via `cargo check` (compile), `cargo fmt` (format), `cargo run -- serve` + manual browser check. Run the PORTING.md 14-step checklist mentally on each converted template.

**Advisor cadence per task:** advisor(plan) before starting → implement → `cargo check` + browser verify → advisor(review) after completing → commit.

---

## File Structure

**New files:**
- `static/css/substrukt.css` — imports wavefunk.css, overrides accent to amber
- `static/css/wavefunk.css` — copied from `../design/css/wavefunk.css`
- `static/css/01-tokens.css` — copied, Martian Mono self-hosted edit
- `static/css/02-base.css` — copied from `../design/css/`
- `static/css/03-layout.css` — copied from `../design/css/`
- `static/css/04-components.css` — copied from `../design/css/`
- `static/css/05-utilities.css` — copied from `../design/css/`
- `static/css/fonts/MartianGrotesk-VF.woff2` — copied from `../design/css/fonts/`
- `static/css/fonts/MartianMono-VF.woff2` — downloaded and self-hosted

**Modified files (all 29 templates):**
- `templates/base.html` — shell restructure, wavefunk.css, data-mode toggle, modeline
- `templates/_partial.html` — match new shell, wavefunk flash messages, OOB sidebar
- `templates/_nav.html` — wavefunk sidebar components
- `templates/login.html`, `signup.html`, `reset_password.html`, `forgot_password.html`, `verify_pending.html`, `verify_result.html`, `setup.html` — auth pages
- `templates/apps/list.html`, `apps/new.html`, `apps/data.html`, `apps/settings.html`
- `templates/schemas/list.html`, `schemas/edit.html`
- `templates/content/list.html`, `content/edit.html`, `content/history.html`, `content/diff.html`, `content/_status_control.html`
- `templates/uploads/list.html`
- `templates/deployments/list.html`, `deployments/form.html`
- `templates/settings/profile.html`, `settings/users.html`, `settings/audit_log.html`, `settings/backups.html`
- `templates/error.html`

---

## Reference Material

Before starting any task, read these files from the design system:
- `../design/docs/PORTING.md` — 14-step mechanical porting checklist
- `../design/docs/COMPOSITION.md` — cross-component rules (container nesting, action primitives, color semantics, geometry)
- `../design/docs/state-classes.md` — the 14 global state hooks
- `../design/docs/form-layouts.md` — three canonical form patterns
- `../design/partials/` — live HTML examples for every component
- `../design/templates/app-shell-with-modeline.html` — reference shell layout

---

### Task 0: Create feature branch

- [ ] **Step 1: Create and switch to feature branch**

```bash
git checkout -b ui/wavefunk-design-system
```

- [ ] **Step 2: Commit**

```bash
git commit --allow-empty -m "chore: start wavefunk UI migration branch"
```

---

### Task 1: Copy CSS + fonts, create substrukt.css

**Files:**
- Create: `static/css/substrukt.css`
- Create: `static/css/wavefunk.css` (copy)
- Create: `static/css/01-tokens.css` (copy + edit)
- Create: `static/css/02-base.css` (copy)
- Create: `static/css/03-layout.css` (copy)
- Create: `static/css/04-components.css` (copy)
- Create: `static/css/05-utilities.css` (copy)
- Create: `static/css/fonts/MartianGrotesk-VF.woff2` (copy)
- Create: `static/css/fonts/MartianMono-VF.woff2` (download)

- [ ] **Step 1: advisor(plan)** — call advisor before starting

- [ ] **Step 2: Copy CSS files from design system**

```bash
mkdir -p static/css/fonts
cp ../design/css/wavefunk.css static/css/
cp ../design/css/02-base.css static/css/
cp ../design/css/03-layout.css static/css/
cp ../design/css/04-components.css static/css/
cp ../design/css/05-utilities.css static/css/
cp ../design/css/fonts/MartianGrotesk-VF.woff2 static/css/fonts/
```

- [ ] **Step 3: Copy and edit 01-tokens.css — self-host Martian Mono**

Copy `../design/css/01-tokens.css` to `static/css/01-tokens.css`. Replace the Google Fonts `@import url("https://fonts.googleapis.com/css2?family=Martian+Mono:...")` with a local `@font-face` declaration:

```css
@font-face {
  font-family: "Martian Mono";
  font-display: swap;
  src: url("./fonts/MartianMono-VF.woff2") format("woff2-variations"),
       url("./fonts/MartianMono-VF.woff2") format("woff2");
  font-weight: 100 800;
  font-stretch: 75% 112.5%;
  font-style: normal;
}
```

Download Martian Mono variable WOFF2 from Google Fonts and save to `static/css/fonts/MartianMono-VF.woff2`. Use the Google Fonts API URL to get the woff2 file URL, then `curl` it down.

- [ ] **Step 4: Create substrukt.css with amber accent overrides**

```css
@import url("./wavefunk.css");

:root {
  --accent: #f59e0b;
  --accent-ink: #000000;
  --accent-dim: color-mix(in srgb, #f59e0b 55%, black);
  --accent-wash: color-mix(in srgb, #f59e0b 14%, black);
  --accent-hover: color-mix(in srgb, #f59e0b 82%, white);
  --accent-press: color-mix(in srgb, #f59e0b 70%, black);
}

[data-mode="light"] {
  --accent: #d97706;
  --accent-ink: #ffffff;
  --accent-dim: color-mix(in srgb, #d97706 55%, white);
  --accent-wash: color-mix(in srgb, #d97706 10%, white);
  --accent-hover: color-mix(in srgb, #d97706 82%, black);
  --accent-press: color-mix(in srgb, #d97706 70%, white);
}
```

- [ ] **Step 5: advisor(review)** — call advisor to review the CSS setup

- [ ] **Step 6: Commit**

```bash
git add static/css/
git commit -m "feat(ui): add wavefunk design system CSS + self-hosted fonts"
```

---

### Task 2: Port base.html — shell, modeline, theme toggle

**Files:**
- Modify: `templates/base.html`

- [ ] **Step 1: advisor(plan)** — call advisor before starting

- [ ] **Step 2: Rewrite base.html**

Structure (read `../design/templates/app-shell-with-modeline.html` for reference):

```html
<html lang="en" data-mode="dark">
<head>
  <!-- meta, title, favicon — keep existing -->
  <link rel="stylesheet" href="/static/css/substrukt.css">
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/easymde/dist/easymde.min.css">
  <script src="https://cdn.jsdelivr.net/npm/easymde/dist/easymde.min.js"></script>
  <script src="https://cdn.jsdelivr.net/npm/htmx.org@2/dist/htmx.min.js"></script>
  <!-- EasyMDE dark mode overrides kept as inline <style> -->
</head>
<body hx-boost="true" hx-target="#main-content" hx-swap="innerHTML">
  <div class="wf-shell"> <!-- CSS grid: sidebar | main | modeline -->
    <aside class="wf-sidebar">{% include "_nav.html" %}</aside>
    <main class="wf-main" id="main-content">
      {% if flash_kind %}...wf-alert...{% endif %}
      {% block content %}{% endblock %}
    </main>
    <div class="wf-modeline" role="status">
      <!-- segments: chevron, buffer (current app), mode (role), fill, user -->
    </div>
  </div>
  <script>/* keep ALL existing JS functions unchanged: addArrayItem, initMarkdownEditors,
     initUploadZones, initFormValidation, initSubmitSpinner, resetSubmitSpinners,
     highlightActiveNav, copyToClipboard, toggleTheme */</script>
</body>
</html>
```

**Key changes:**
- Remove: all `<style>` inline CSS variables (`:root`, `.dark` custom properties) EXCEPT EasyMDE dark overrides
- Remove: twind CDN `<script>` and `twind.install({...})` block
- Remove: `bg-surface text-primary min-h-screen` and all twind classes from `<body>` and structural elements
- Add: `<link rel="stylesheet" href="/static/css/substrukt.css">`
- Add: `data-mode="dark"` on `<html>` (replaces `.dark` class)
- Add: `.wf-shell` grid wrapper, `.wf-sidebar`, `.wf-main`, `.wf-modeline`
- Add: app-shell grid CSS (inline `<style>` or in substrukt.css) — sidebar + main + modeline:
  ```css
  .wf-shell {
    display: grid;
    grid-template-columns: 240px 1fr;
    grid-template-rows: 1fr 22px;
    grid-template-areas: "side main" "ml ml";
    height: 100vh;
  }
  .wf-shell > .wf-sidebar { grid-area: side; }
  .wf-shell > .wf-main { grid-area: main; overflow: auto; }
  .wf-shell > .wf-modeline { grid-area: ml; }
  ```
- Change: flash messages from twind classes to `.wf-alert.ok` / `.wf-alert.err` / `.wf-alert.info`
- Change: `toggleTheme()` to toggle `data-mode` attribute instead of `.dark` class, store in localStorage
- Change: theme detection IIFE to set `data-mode="dark"` or `data-mode="light"` based on localStorage/system preference
- Change: `highlightActiveNav()` to add/remove `.is-active` instead of twind classes

**Modeline content:**

```html
<div class="wf-modeline" role="status">
  <span class="wf-ml-seg wf-ml-chevron">SK</span>
  <span class="wf-ml-seg wf-ml-buffer">
    {% if app %}{{ app.slug }}{% else %}substrukt{% endif %}
  </span>
  <span class="wf-ml-seg wf-ml-mode">
    <span class="kicker">Role</span><span>{{ user_role }}</span>
  </span>
  <span class="wf-ml-fill" aria-hidden="true"></span>
  <span class="wf-ml-seg">{{ current_username }}</span>
</div>
```

**JS functions to keep verbatim:** `addArrayItem`, `initMarkdownEditors`, `formatFileSize`, `initUploadZones`, `initFormValidation`, `initSubmitSpinner`, `resetSubmitSpinners`, `copyToClipboard`. Update class references inside them: `array-item` div classes should use wavefunk styling instead of twind (`border border-border-light p-3 rounded mb-2` → `wf-framed` or keep minimal inline since these are dynamic).

- [ ] **Step 3: Verify** — `cargo check` + `cargo run -- serve` + browser check

- [ ] **Step 4: advisor(review)** — call advisor to review base.html

- [ ] **Step 5: Commit**

```bash
git add templates/base.html
git commit -m "feat(ui): port base.html to wavefunk shell with modeline"
```

---

### Task 3: Port _nav.html — sidebar navigation

**Files:**
- Modify: `templates/_nav.html`

- [ ] **Step 1: advisor(plan)** — call advisor before starting

- [ ] **Step 2: Rewrite _nav.html**

Map current elements to wavefunk sidebar components (read `../design/partials/sidebar.html` for reference):

| Current | Wavefunk |
|---|---|
| `<img> + <span>` brand | `<div class="wf-brand"><span class="wf-brand-name">Substrukt</span></div>` |
| `<a href="/apps">Apps</a>` | `<a class="wf-nav-item" href="/apps">▸ Apps</a>` |
| App name header `<div class="...">{{ app.name }}</div>` | `<div class="wf-nav-section">{{ app.name }}</div>` |
| Schema link `<a href="...">Schemas</a>` | `<a class="wf-nav-item" href="...">▸ Schemas</a>` |
| Content section toggle button | `<div class="wf-nav-section">Content</div>` with collapsible children |
| Content type links | `<a class="wf-nav-item" href="...">▸ {{ s.title }}</a>` |
| Uploads/Deployments/Data/Settings links | `<a class="wf-nav-item" href="...">▸ Uploads</a>` etc. |
| Admin section | `<div class="wf-nav-section">Admin</div>` + nav items |
| User profile + logout | `<div class="wf-pop-anchor"><button class="wf-user">...</button>` with `.wf-popover` for logout/settings/theme |
| Theme toggle button | Inside the `.wf-user` popover menu, or as `.wf-icon-btn` at bottom |
| "Powered by wavefunk" | Remove (separate task) |

Active state: use `.is-active` class on the current page's `.wf-nav-item`. The `highlightActiveNav()` JS in base.html adds/removes `.is-active`.

Section collapse: keep `toggleNavSection()` JS, but toggle display on the `.wf-nav-list` children or a wrapper div. Store in localStorage with same keys.

Theme icon: `☀` / `☽` glyph in the `.wf-user` popover menu or as standalone `.wf-icon-btn` at sidebar bottom.

- [ ] **Step 3: Verify** — `cargo check` + browser check (sidebar renders, nav works, collapse works, active highlighting works)

- [ ] **Step 4: advisor(review)** — call advisor to review

- [ ] **Step 5: Commit**

```bash
git add templates/_nav.html
git commit -m "feat(ui): port sidebar nav to wavefunk components"
```

---

### Task 4: Port _partial.html — htmx partial responses

**Files:**
- Modify: `templates/_partial.html`

- [ ] **Step 1: advisor(plan)** — call advisor before starting

- [ ] **Step 2: Rewrite _partial.html**

Changes:
- Flash messages: replace twind classes with `.wf-alert.ok` / `.wf-alert.err` / `.wf-alert.info` with `.wf-alert-bar` and optional `.wf-alert-kicker`
- `addArrayItem` function: update the div classes for array items from twind to wavefunk (use appropriate classes or minimal inline)
- OOB sidebar swap: update `<nav id="sidebar-nav" hx-swap-oob="true" class="...">` to match the new sidebar structure — use `<aside class="wf-sidebar" hx-swap-oob="true" id="sidebar-nav">` (must match the element in base.html that we want to swap)
- Keep the `page-title` hidden span for title updates

- [ ] **Step 3: Verify** — navigate between pages, confirm htmx partial swaps work, flash messages show correctly

- [ ] **Step 4: advisor(review)** — call advisor to review

- [ ] **Step 5: Commit**

```bash
git add templates/_partial.html
git commit -m "feat(ui): port partial template to wavefunk alerts + sidebar OOB"
```

---

### Task 5: Port auth pages (7 templates)

**Files:**
- Modify: `templates/login.html`
- Modify: `templates/signup.html`
- Modify: `templates/reset_password.html`
- Modify: `templates/forgot_password.html`
- Modify: `templates/verify_pending.html`
- Modify: `templates/verify_result.html`
- Modify: `templates/setup.html`

- [ ] **Step 1: advisor(plan)** — call advisor before starting

- [ ] **Step 2: Port all 7 auth templates**

All auth pages are standalone (no sidebar shell). They share identical boilerplate which changes as follows:

**Head section (same for all 7):**
- Remove: all `<style>` inline CSS variables
- Remove: twind CDN `<script>` and `twind.install({...})`
- Remove: `.dark` class theme detection — replace with `data-mode` attribute
- Add: `<link rel="stylesheet" href="/static/css/substrukt.css">`
- Add: theme detection IIFE that sets `data-mode="dark"` or `data-mode="light"` on `<html>`

**Body structure (same pattern for all 7):**
- `<body>` classes: remove all twind → no classes needed (wavefunk base handles body styling)
- Outer container: remove `bg-card rounded-lg shadow-lg p-8 w-full max-w-sm` → use wavefunk auth layout. Reference `../design/partials/layout-auth.html`. Use centered form approach with `.wf-auth-top` for top bar.
- Theme toggle button: keep, but style as `.wf-icon-btn ghost` with glyph, adapt to `data-mode` toggle

**Form elements mapping (all auth forms):**

| Current | Wavefunk |
|---|---|
| `<label class="block text-sm font-medium text-secondary mb-1">` | `<label class="wf-label">` |
| `<input class="w-full px-3 py-2 border border-border rounded-md bg-input-bg focus:...">` | `<input class="wf-input">` |
| `<button class="w-full bg-accent text-black py-2 px-4 rounded-md hover:bg-accent-hover font-medium">` | `<button class="wf-btn primary" style="width: 100%;">` |
| `<div class="bg-danger-soft text-danger p-3 rounded mb-4 text-sm">` | `<div class="wf-alert err"><div class="wf-alert-bar"></div><div>...</div></div>` |
| `<div class="bg-success-soft text-success p-3 rounded mb-4 text-sm">` | `<div class="wf-alert ok"><div class="wf-alert-bar"></div><div>...</div></div>` |
| `<a class="text-secondary hover:text-primary">` | Plain `<a>` (wavefunk base styles links) |
| Logo `<img src="/static/favicon.svg" class="w-10 h-10 mb-3">` + heading | Substrukt brand text, styled with `--font-mono`, `--fw-black`, uppercase via CSS |

**Per-page notes:**
- `login.html`: has `show_resend` conditional link in error — keep logic, use wavefunk classes
- `signup.html`: has readonly email field — add `disabled` attribute, wavefunk styles `:disabled` natively
- `forgot_password.html`: has `{% if sent %}` conditional — both branches get wavefunk treatment
- `verify_pending.html`: similar to forgot_password — two-state template
- `verify_result.html`: `{% if success %}` conditional — use `.wf-alert.ok` or `.wf-alert.err`
- `setup.html`: uses inline `data:image/svg+xml` favicon — replace with `/static/favicon.svg`

- [ ] **Step 3: Verify** — visit `/login`, `/signup` (with token), `/forgot-password`, `/setup` in browser. Check dark/light toggle.

- [ ] **Step 4: advisor(review)** — call advisor to review

- [ ] **Step 5: Commit**

```bash
git add templates/login.html templates/signup.html templates/reset_password.html templates/forgot_password.html templates/verify_pending.html templates/verify_result.html templates/setup.html
git commit -m "feat(ui): port auth pages to wavefunk design system"
```

---

### Task 6: Port app pages (4 templates)

**Files:**
- Modify: `templates/apps/list.html`
- Modify: `templates/apps/new.html`
- Modify: `templates/apps/data.html`
- Modify: `templates/apps/settings.html`

- [ ] **Step 1: advisor(plan)** — call advisor before starting

- [ ] **Step 2: Port all 4 app templates**

These extend `base_template`, so only the `{% block content %}` changes.

**Page headers (all pages):**
- `<div class="flex items-center justify-between mb-6">` → use flex utility or inline style. Wavefunk has `wf-flex` / `wf-justify-between` if available, otherwise bare CSS.
- `<h1 class="text-2xl font-bold tracking-tight">` → `<h1>` (wavefunk base styles h1 with --font-mono, --fw-black, uppercase)
- "New App" / "Back" buttons → `.wf-btn.primary` / `.wf-btn`

**apps/list.html:**

| Current | Wavefunk |
|---|---|
| `<div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">` | `<div class="wf-grid cols-3">` |
| `<a class="block bg-card border ... rounded-lg p-6 hover:border-accent">` | `<a class="wf-card">` with `.wf-card-title`, `.wf-card-body` |
| Empty state `<div class="bg-card border ... text-center text-muted">` | `<div class="wf-empty bordered">` with `.wf-empty-title`, `.wf-empty-body`, `.wf-empty-actions` |

**apps/new.html:**
- Form → wrap fields in stacked form layout: `.wf-field` + `.wf-label` + `.wf-input`
- Container `<div class="bg-card border ... rounded-lg p-6 max-w-lg">` → `.wf-panel` with `.wf-panel-body`
- Hint text `<p class="mt-1 text-xs text-muted">` → hint inside `.wf-field` (wavefunk handles field hints natively)
- Keep the name→slug auto-generation JS

**apps/data.html:**
- Export/Import sections → each becomes a `.wf-panel` with `.wf-panel-head` + `.wf-panel-body`
- Result alerts → `.wf-alert.ok` / `.wf-alert.warn` / `.wf-alert.err`
- Import modal → `.wf-modal` + `.wf-overlay` with `.wf-modal-head`, `.wf-modal-body`, `.wf-modal-foot`
- Confirm input + buttons → `.wf-input`, `.wf-btn`, `.wf-btn.danger`

**apps/settings.html:**
- Each section (App Name, User Access, API Tokens, Danger Zone) → `.wf-panel` with `.wf-panel-head` + `.wf-panel-body`
- Token table → `.wf-table` inside `.wf-panel` with `padding: 0`
- Role badges → `.wf-tag` (`.wf-tag.err` for admin, `.wf-tag.warn` for editor, plain `.wf-tag` for viewer)
- Danger zone → `.wf-panel.is-danger` with `.wf-btn.danger`
- Checkboxes → `.wf-check` inside `.wf-check-row`

- [ ] **Step 3: Verify** — visit `/apps`, `/apps/new`, `/apps/<slug>/data`, `/apps/<slug>/settings`

- [ ] **Step 4: advisor(review)** — call advisor to review

- [ ] **Step 5: Commit**

```bash
git add templates/apps/
git commit -m "feat(ui): port app pages to wavefunk design system"
```

---

### Task 7: Port schema pages (2 templates)

**Files:**
- Modify: `templates/schemas/list.html`
- Modify: `templates/schemas/edit.html`

- [ ] **Step 1: advisor(plan)** — call advisor before starting

- [ ] **Step 2: Port both schema templates**

**schemas/list.html:**
- Table → `.wf-table` inside `.wf-panel` (padding: 0)
- Header cells → `<th>` (wavefunk auto-uppercases via CSS)
- Title column → `.strong` class
- Slug column → wavefunk mono font applied automatically
- Entry count link → accent color via `.wf-panel-link` or plain anchor
- Edit link → `.wf-btn.ghost.sm` or plain accent link
- Empty state → `.wf-empty.bordered`

**schemas/edit.html:**
- Schema reference `<details>` → `.wf-accordion` / `.wf-accordion-item` with `.wf-accordion-trigger` + `.wf-accordion-body`
- JSON editor container → `.wf-panel` wrapping the `#jsoneditor` div
- Error alert → `.wf-alert.err`
- Buttons → `.wf-btn.primary` for save, `.wf-btn.danger` for delete
- Keep all JS (vanilla-jsoneditor module import, deleteSchema)

- [ ] **Step 3: Verify** — visit `/apps/<slug>/schemas`, `/apps/<slug>/schemas/new`, `/apps/<slug>/schemas/<schema>/edit`

- [ ] **Step 4: advisor(review)** — call advisor to review

- [ ] **Step 5: Commit**

```bash
git add templates/schemas/
git commit -m "feat(ui): port schema pages to wavefunk design system"
```

---

### Task 8: Port content pages (5 templates)

**Files:**
- Modify: `templates/content/list.html`
- Modify: `templates/content/edit.html`
- Modify: `templates/content/history.html`
- Modify: `templates/content/diff.html`
- Modify: `templates/content/_status_control.html`

- [ ] **Step 1: advisor(plan)** — call advisor before starting

- [ ] **Step 2: Port all 5 content templates**

**content/list.html:**
- Search/filter bar → `.wf-filterbar` inside `.wf-panel` (padding: 0) with `.wf-input` + `.wf-select`
- Bulk bar → `.wf-bulkbar` (replaces filterbar when items selected — toggle visibility via JS). Use `.wf-sel-count`, `.wf-bar-sep`, `.wf-btn.sm.ghost`
- Table → `.wf-table.is-interactive` with `.wf-check` for row selection
- Status badges: `Published` → `<span class="wf-tag ok"><span class="dot"></span>Published</span>`, `Draft` → `<span class="wf-tag warn"><span class="dot"></span>Draft</span>`
- Boolean values: `true` → `<span class="wf-tag ok">Yes</span>`, `false` → `<span class="wf-tag">No</span>`
- ID column → mono font applied by wavefunk base
- Pagination → `.wf-pagination` with `←` / `→` buttons, `.is-active` on current page
- Empty state → `.wf-empty.bordered`
- "New Entry" → `.wf-btn.primary`
- Sort indicators → keep `▲` / `▼` glyphs
- Keep all JS (toggleAll, updateBulkBar)

**content/edit.html:**
- Form container → `.wf-panel` with `.wf-panel-body`
- `form_fields|safe` renders server-side — the rendered HTML from `src/form_builder.rs` will still use twind classes. This is addressed separately in the form builder Rust code (see Task 12).
- Validation errors → `.wf-alert.err` with list inside
- Status control → included via `{% include "content/_status_control.html" %}`
- Buttons: Save → `.wf-btn.primary`, Cancel → `.wf-btn`, History → `.wf-btn`
- Viewer-only message → plain text with muted styling
- Keep all JS (dirty-form tracking)

**content/history.html:**
- Version table → `.wf-timeline` instead of table. Each version → `.wf-timeline-item` with `.wf-timeline-time` (timestamp), `.wf-timeline-title` (author + source), `.wf-timeline-body` (size). OR keep as `.wf-table` — either works. Table is more natural for tabular data with Compare/Revert actions.
- Compare/Revert links → accent links or `.wf-btn.ghost.sm`
- Empty state → `.wf-empty`

**content/diff.html:**
- Diff table → `.wf-table` with Field/Previous/Current columns
- Row coloring for diff type: `changed` → `background: var(--accent-wash)`, `added` → `background: color-mix(in srgb, var(--ok) 10%, transparent)`, `removed` → `background: color-mix(in srgb, var(--err) 10%, transparent)`. Use inline styles with wavefunk token vars.
- Empty state → `.wf-empty`

**content/_status_control.html:**
- Draft badge → `<span class="wf-tag warn"><span class="dot"></span>Draft</span>`
- Published badge → `<span class="wf-tag ok"><span class="dot"></span>Published</span>`
- Publish button → `<button class="wf-btn sm primary">Publish</button>`
- Unpublish button → `<button class="wf-btn sm">Unpublish</button>`
- Keep htmx attributes (hx-post, hx-target, hx-swap)

- [ ] **Step 3: Verify** — visit content list, edit, history, diff pages. Test publish/unpublish htmx interaction. Test bulk selection.

- [ ] **Step 4: advisor(review)** — call advisor to review

- [ ] **Step 5: Commit**

```bash
git add templates/content/
git commit -m "feat(ui): port content pages to wavefunk design system"
```

---

### Task 9: Port uploads page (1 template)

**Files:**
- Modify: `templates/uploads/list.html`

- [ ] **Step 1: advisor(plan)** — call advisor before starting

- [ ] **Step 2: Port uploads/list.html**

- Filter form → `.wf-filterbar` with `.wf-input`, `.wf-select`, `.wf-btn.primary.sm` (Filter button)
- Table → `.wf-table` inside `.wf-panel` (padding: 0)
- Filename column → `.strong` (primary identifier)
- Size column → `.num` (right-aligned)
- "Orphaned" label → `<span class="wf-tag warn"><span class="dot"></span>Orphaned</span>`
- View/Download links → accent links
- Empty state → `.wf-empty`

- [ ] **Step 3: Verify** — visit `/apps/<slug>/uploads`

- [ ] **Step 4: advisor(review)** — call advisor to review

- [ ] **Step 5: Commit**

```bash
git add templates/uploads/
git commit -m "feat(ui): port uploads page to wavefunk design system"
```

---

### Task 10: Port deployment pages (2 templates)

**Files:**
- Modify: `templates/deployments/list.html`
- Modify: `templates/deployments/form.html`

- [ ] **Step 1: advisor(plan)** — call advisor before starting

- [ ] **Step 2: Port both deployment templates**

**deployments/list.html:**
- Deployments table → `.wf-table` inside `.wf-panel` (padding: 0)
- Status dot → `.wf-dot` with appropriate color class
- Mode badges: Auto → `<span class="wf-tag accent">Auto</span>`, Manual → `<span class="wf-tag">Manual</span>`
- Drafts: Yes → `<span class="wf-tag accent">Yes</span>`, No → plain text
- Action links: Fire → `.wf-btn.sm.ghost` or accent link, Edit → accent link, Delete → danger link
- Webhook history table → separate `.wf-panel` with `.wf-panel-head` + `.wf-table`
- Success/Failed status → `.wf-tag.ok` / `.wf-tag.err` with dot
- Empty state → `.wf-empty.bordered`
- "Create Deployment" → `.wf-btn.primary`

**deployments/form.html:**
- Form → stacked layout: `.wf-field` + `.wf-label` + `.wf-input` / `.wf-select`
- Checkboxes → `.wf-check` inside `.wf-check-row` (for include_drafts, auto_deploy)
- Alternatively, auto_deploy → `.wf-switch` (persistent on/off setting — wavefunk's switch is designed for this)
- Error alert → `.wf-alert.err`
- Buttons: Create/Save → `.wf-btn.primary`, Cancel → `.wf-btn`
- Keep all JS (name→slug, toggleDebounce, toggleTokenInput)

- [ ] **Step 3: Verify** — visit `/apps/<slug>/deployments`, `/apps/<slug>/deployments/new`, edit form

- [ ] **Step 4: advisor(review)** — call advisor to review

- [ ] **Step 5: Commit**

```bash
git add templates/deployments/
git commit -m "feat(ui): port deployment pages to wavefunk design system"
```

---

### Task 11: Port settings pages (4 templates)

**Files:**
- Modify: `templates/settings/profile.html`
- Modify: `templates/settings/users.html`
- Modify: `templates/settings/audit_log.html`
- Modify: `templates/settings/backups.html`

- [ ] **Step 1: advisor(plan)** — call advisor before starting

- [ ] **Step 2: Port all 4 settings templates**

**settings/profile.html:**
- Account section → `.wf-panel` with `.wf-panel-head` (title: "Account") + `.wf-panel-body` containing `.wf-dl` for username display
- Change Password section → `.wf-panel` with `.wf-panel-head` + `.wf-panel-body` containing stacked form (`.wf-field` + `.wf-label` + `.wf-input`)
- Submit → `.wf-btn.primary`

**settings/users.html:**
- Registered Users → `.wf-panel` with `.wf-panel-head` + `.wf-table` (padding: 0 on panel body)
- Role badges → `.wf-tag.err` (admin), `.wf-tag.warn` (editor), `.wf-tag` (viewer)
- Role select (inline change) → `.wf-select.sm` or keep as styled native select
- Invite User form → `.wf-panel` with `.wf-panel-head` + `.wf-panel-body` containing inline form
- Invite URL display → `.wf-alert.ok` with code block
- Pending invitations table → `.wf-panel` + `.wf-table`
- Empty states → `.wf-empty`

**settings/audit_log.html:**
- Filter bar → `.wf-filterbar` with `.wf-select`, date `.wf-input`, `.wf-btn.sm` for presets
- Table → `.wf-table` inside `.wf-panel` (padding: 0)
- Action badges → `.wf-tag.accent` for content/schema, `.wf-tag.ok` for auth, plain `.wf-tag` for other
- Pagination → `.wf-pagination`
- Keep all JS (applyFilters, setDatePreset)

**settings/backups.html:**
- Status banner → `.wf-alert` variants (.ok, .err, .warn, .info)
- Backup spinner → `.wf-spinner` (wavefunk has a hairline spinner component)
- Next Scheduled section → `.wf-panel` with `.wf-panel-body` containing flex layout
- Configuration form → `.wf-panel` with `.wf-panel-head` + `.wf-panel-body`, fields as `.wf-field` + `.wf-label` + `.wf-select` / `.wf-input`
- Enabled checkbox → `.wf-switch` (persistent setting) inside `.wf-check-row`
- S3 Credentials table → `.wf-table` inside `.wf-panel`
- Status indicators: "Set" → `<span style="color: var(--ok);">Set</span>`, "Missing" → `<span style="color: var(--err);">Missing</span>`
- Recent Backups table → `.wf-panel` + `.wf-table`

- [ ] **Step 3: Verify** — visit `/settings/profile`, `/settings/users`, `/settings/audit-log`, `/settings/backups`

- [ ] **Step 4: advisor(review)** — call advisor to review

- [ ] **Step 5: Commit**

```bash
git add templates/settings/
git commit -m "feat(ui): port settings pages to wavefunk design system"
```

---

### Task 12: Port error page + update form builder

**Files:**
- Modify: `templates/error.html`
- Modify: `src/form_builder.rs` (if it exists — the server-side form field renderer that outputs `form_fields|safe` in content/edit.html)

- [ ] **Step 1: advisor(plan)** — call advisor before starting

- [ ] **Step 2: Port error.html**

```html
{% extends base_template %}
{% block title %}{{ status }} — Substrukt{% endblock %}
{% block content %}
<div class="wf-empty bordered">
  <div class="wf-empty-glyph">{{ status }}</div>
  <div class="wf-empty-title">{{ message }}</div>
  <div class="wf-empty-actions">
    <a href="/" class="wf-btn primary">Back to Dashboard</a>
  </div>
</div>
{% endblock %}
```

- [ ] **Step 3: Find and update the form builder**

Search for the Rust code that generates HTML for content editing forms (`form_fields|safe`). It likely lives in `src/form_builder.rs` or similar. Update all twind utility classes in the generated HTML to use wavefunk equivalents:

| Generated HTML element | Current twind classes | Wavefunk |
|---|---|---|
| Label | `block text-sm font-medium mb-1` | `wf-label` |
| Text input | `w-full px-3 py-2 border border-border rounded-md bg-input-bg focus:...` | `wf-input` |
| Textarea | same pattern | `wf-textarea` |
| Select | same pattern | `wf-select` |
| Checkbox | `rounded border-border` | `wf-check` inside `wf-check-row` |
| Field wrapper | `mb-4` or similar | `wf-field` |
| Upload zone | custom drag-drop zone | `wf-dropzone` |
| Array item container | `array-item border border-border-light p-3 rounded mb-2` | `array-item wf-framed` |
| Remove button | `text-danger text-sm hover:text-danger` | `wf-btn ghost sm danger` or accent link |
| Add button | `text-accent hover:underline text-sm` | `wf-btn ghost sm` |
| Nested object fieldset | indented container | Use hairline separator (`border-top: 1px solid var(--hairline-dim)`) + padding |

- [ ] **Step 4: Verify** — `cargo check` + visit a content edit page with various field types

- [ ] **Step 5: advisor(review)** — call advisor to review

- [ ] **Step 6: Commit**

```bash
git add templates/error.html src/
git commit -m "feat(ui): port error page + form builder to wavefunk classes"
```

---

### Task 13: Remove twind + cleanup

**Files:**
- Modify: `templates/base.html` (remove any remaining twind references)
- Possibly: any template that still has twind classes

- [ ] **Step 1: advisor(plan)** — call advisor before starting

- [ ] **Step 2: Search for remaining twind references**

```bash
grep -rn "cdn.twind.style\|twind.install\|twind\." templates/
grep -rn "bg-card\|bg-surface\|text-primary\|rounded-md\|rounded-lg\|border-border\|hover:bg-" templates/
```

Fix any remaining twind utility classes found.

- [ ] **Step 3: Search for remaining inline color literals**

```bash
grep -rn "style=\"color:\|#[0-9a-fA-F]\{3,6\}\|rgb(" templates/ | grep -v "csrf\|favicon\|easymde\|htmx"
```

Replace any raw hex/rgb with wavefunk token vars.

- [ ] **Step 4: Verify** — `cargo check` + full browser walkthrough of all pages

- [ ] **Step 5: advisor(review)** — call advisor to review

- [ ] **Step 6: Commit**

```bash
git add templates/
git commit -m "chore(ui): remove twind remnants and inline color literals"
```

---

### Task 14: Final PORTING.md audit

- [ ] **Step 1: advisor(plan)** — call advisor. Ask it to run the full PORTING.md 14-step checklist across all templates:

1. No `border-radius` anywhere
2. No `box-shadow` anywhere
3. No gradients
4. No color literals (hex/rgb) except in substrukt.css token overrides and EasyMDE dark overrides
5. No nested containers (panel in panel, card in card)
6. No SVG icons — only character glyphs
7. All labels uppercase via CSS, not in source HTML
8. Status pills use `.wf-tag` with `.ok` / `.warn` / `.err` + `<span class="dot">`
9. Number columns use `.num`
10. Primary columns use `.strong`
11. Status from semantic classes, not raw color
12. Correct container choices (panel vs card vs framed)
13. All hit targets ≥ 32px
14. No rogue geometry violations

- [ ] **Step 2: Fix any issues found**

- [ ] **Step 3: Full browser walkthrough**

Run `cargo run -- serve` and visit every page:
- `/setup` (first-run), `/login`, `/forgot-password`
- `/apps`, `/apps/new`
- `/apps/<slug>/schemas`, `/apps/<slug>/schemas/new`
- `/apps/<slug>/content/<schema>`, `/apps/<slug>/content/<schema>/new`, edit, history, diff
- `/apps/<slug>/uploads`
- `/apps/<slug>/deployments`, `/apps/<slug>/deployments/new`
- `/apps/<slug>/settings`, `/apps/<slug>/data`
- `/settings/profile`, `/settings/users`, `/settings/audit-log`, `/settings/backups`
- Test dark/light toggle on multiple pages
- Test htmx navigation (sidebar stays, content swaps)
- Test flash messages (success/error/info)
- Test publish/unpublish, bulk actions
- Force a 404 and 500 to see error page

- [ ] **Step 4: advisor(review)** — call advisor for final sign-off

- [ ] **Step 5: Commit any final fixes**

```bash
git add .
git commit -m "fix(ui): final audit fixes from PORTING.md checklist"
```

---

### Task 15: Merge to main

- [ ] **Step 1: Run cargo check + cargo clippy + cargo test**

```bash
cargo check && cargo clippy && cargo test
```

- [ ] **Step 2: Merge branch to main (keeping history)**

```bash
git checkout main
git merge --no-ff ui/wavefunk-design-system -m "feat: replace twind UI with wavefunk design system

Replaces the entire frontend with the wavefunk company design system.
Dark-by-default, amber accent, hairline-driven, zero-radius aesthetic.
Adds modeline statusbar. Self-hosts Martian Grotesk + Martian Mono fonts.
Removes twind CDN dependency entirely."
```

- [ ] **Step 3: Verify on main** — `cargo run -- serve` + quick browser check
