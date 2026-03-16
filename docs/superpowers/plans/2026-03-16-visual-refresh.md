# Visual Refresh Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Modernize the Substrukt CMS with dark mode, amber accents, and a sharp technical aesthetic.

**Architecture:** CSS custom properties define light/dark themes. twind is configured with semantic color names mapped to those variables. A `.dark` class on `<html>` switches themes. No build step added.

**Tech Stack:** twind CDN (existing), CSS custom properties, vanilla JS for theme toggle.

**Spec:** `docs/superpowers/specs/2026-03-16-visual-refresh-design.md`

---

## Chunk 1: Infrastructure & Core Templates

### Task 1: Theme infrastructure in base.html

**Files:**
- Modify: `templates/base.html`

This is the foundation everything else depends on. Add CSS variables `<style>` block, twind config with semantic colors, dark mode init script in `<head>`, and global theme transition CSS. Update body/flash message classes to semantic equivalents. Update addArrayItem JS classes. Add toggleTheme function.

See spec Section 1 for CSS variable values (light + dark), Section 2 for dark mode init/toggle code, and Section 5 for class migration table.

- [ ] **Step 1: Rewrite base.html with full theme infrastructure**
- [ ] **Step 2: Commit** — `git commit -m "feat: add theme infrastructure to base.html"`

---

### Task 2: Sidebar navigation

**Files:**
- Modify: `templates/_nav.html`

Inline the logo SVG from `website/roundedicon.svg` (simplified, using `fill="currentColor"`). Apply semantic color classes throughout. Add theme toggle button (sun/moon entity) in the bottom bar next to logout. Use `.theme-icon` span populated by a small inline script (no document.write).

See spec Section 4 (Sidebar) for component classes and Section 3 for typography (font-mono tracking-tight on logo text).

- [ ] **Step 1: Rewrite _nav.html with logo, semantic colors, theme toggle**
- [ ] **Step 2: Commit** — `git commit -m "feat: update sidebar with logo, semantic colors, theme toggle"`

---

### Task 3: Partial template

**Files:**
- Modify: `templates/_partial.html`

Only change: update hardcoded color classes in the addArrayItem JS function.
- `border-gray-100` -> `border-border-light`
- `text-red-500` -> `text-danger`
- `hover:text-red-700` -> `hover:text-danger`

- [ ] **Step 1: Update _partial.html JS classes**
- [ ] **Step 2: Commit** — `git commit -m "feat: update partial template with semantic color classes"`

---

### Task 4: Standalone pages (login.html, setup.html)

**Files:**
- Modify: `templates/login.html`
- Modify: `templates/setup.html`

These don't extend base.html, so each needs its own:
1. CSS variables `<style>` block (same as base.html, can omit sidebar/success vars)
2. Dark mode init `<script>` in `<head>` (same IIFE + matchMedia listener)
3. twind.install config `<script>` (same color mappings)
4. Theme toggle button (top-right corner of card, `absolute top-4 right-4`)
5. All form classes migrated per spec Section 5

See spec Section 2 for toggle code and Section 5 for full class migration table.

- [ ] **Step 1: Rewrite login.html with full standalone theme support**
- [ ] **Step 2: Rewrite setup.html with full standalone theme support**
- [ ] **Step 3: Commit** — `git commit -m "feat: add dark mode and semantic colors to login/setup pages"`

---

### Task 5: Form renderer (form.rs)

**Files:**
- Modify: `src/content/form.rs`

Apply all class substitutions from spec Section 5 migration table to Rust string literals. Key changes:
- All `text-gray-700` -> `text-secondary`
- All `border-gray-300` -> `border-border`
- All `border-gray-200`/`border-gray-100` -> `border-border-light`
- All `bg-gray-100` -> `bg-card-alt`, `hover:bg-gray-200` -> `hover:bg-card-alt`
- Remove all `shadow-sm`
- All `focus:ring-blue-500` -> `focus:ring-accent`, `focus:border-blue-500` -> `focus:border-accent`
- All `text-blue-600` -> `text-accent`
- All `text-red-500` -> `text-danger`, `hover:text-red-700` -> `hover:text-danger`
- Add `bg-input-bg` to text inputs, textareas, selects, number inputs (NOT checkboxes/file inputs)
- Checkbox: `rounded border-border text-accent focus:ring-accent`

- [ ] **Step 1: Apply all class migrations in form.rs**
- [ ] **Step 2: Run `cargo check`** — Expected: no errors
- [ ] **Step 3: Commit** — `git commit -m "feat: update form renderer with semantic color classes"`

---

## Chunk 2: Content Templates

### Task 6: Dashboard and error page

**Files:**
- Modify: `templates/dashboard.html`
- Modify: `templates/error.html`

Simple class migrations:
- Dashboard: cards get `bg-card border border-border-light` (remove shadow), text colors to semantic, `tracking-tight` on heading
- Error: `text-gray-300` -> `text-muted`, `text-gray-500` -> `text-secondary`, `text-blue-600` -> `text-accent`

- [ ] **Step 1: Update dashboard.html**
- [ ] **Step 2: Update error.html**
- [ ] **Step 3: Commit** — `git commit -m "feat: update dashboard and error page with semantic colors"`

---

### Task 7: Schema templates

**Files:**
- Modify: `templates/schemas/list.html`
- Modify: `templates/schemas/edit.html`

List: table container `bg-card border border-border-light` (remove shadow), thead `bg-card-alt`, row hover `hover:bg-card-alt`, dividers `divide-border-light`, cells `py-2.5`, links `text-accent`, edit link `text-muted hover:text-accent`, primary button amber, `tracking-tight` on heading.

Edit: card `bg-card border border-border-light` (remove shadow), JSON editor border `var(--border)`, delete button `text-danger hover:bg-danger-soft`, error banner `bg-danger-soft text-danger`, back link `text-muted hover:text-primary`, `tracking-tight` on heading.

- [ ] **Step 1: Update schemas/list.html**
- [ ] **Step 2: Update schemas/edit.html**
- [ ] **Step 3: Commit** — `git commit -m "feat: update schema templates with semantic colors and dark mode"`

---

### Task 8: Content templates

**Files:**
- Modify: `templates/content/list.html`
- Modify: `templates/content/edit.html`

Same table/card patterns as schema templates. Content edit has:
- Validation errors: `bg-danger-soft text-danger`
- Cancel button: secondary style `border border-border text-secondary hover:bg-card-alt`
- Form border-t: `border-border-light`

- [ ] **Step 1: Update content/list.html**
- [ ] **Step 2: Update content/edit.html**
- [ ] **Step 3: Commit** — `git commit -m "feat: update content templates with semantic colors and dark mode"`

---

### Task 9: Uploads and tokens templates

**Files:**
- Modify: `templates/uploads/list.html`
- Modify: `templates/settings/tokens.html`

Uploads: filter inputs get `bg-input-bg border-border focus:ring-accent`, table borders `border-border-light`, links `text-accent`, keep `text-amber-600` for orphaned label.

Tokens: token-created banner `bg-success-soft border-success text-success`, token code `bg-card-alt text-primary font-mono`, create input `bg-input-bg`, all table patterns same as other list pages.

- [ ] **Step 1: Update uploads/list.html**
- [ ] **Step 2: Update settings/tokens.html**
- [ ] **Step 3: Commit** — `git commit -m "feat: update uploads and tokens templates with semantic colors"`

---

### Task 10: Build verification

- [ ] **Step 1: Run `cargo check`** — Expected: compiles without errors
- [ ] **Step 2: Run `cargo test`** — Expected: all existing tests pass
- [ ] **Step 3: Visual verification** — Run `cargo run -- serve`, check in browser:
  - Light mode: slate backgrounds, amber buttons, clean borders
  - Dark mode: zinc backgrounds, amber accents pop
  - Toggle works in sidebar and on login page
  - OS preference detection works
  - Forms render with correct colors
  - Tables have tighter rows, card-alt headers
  - Flash messages show correct semantic colors
  - JSON editor border respects theme
- [ ] **Step 4: Final commit if any tweaks needed**
