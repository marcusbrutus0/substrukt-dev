# Visual Refresh Design

Modernize the Substrukt CMS visual layer: dark mode support, technical/sharp aesthetic, amber accent color. Keep the existing sidebar + content layout.

## Constraints

- No build step — continue using twind CDN
- No new JS dependencies beyond what exists (htmx, vanilla-jsoneditor)
- Keep the existing sidebar + main content layout structure
- All 13 templates + the Rust form renderer (`src/content/form.rs`) need updating

## 1. Color System & Theme Architecture

### CSS Custom Properties

A `<style>` block in `base.html` (and duplicated in `login.html`/`setup.html` standalone pages) defines semantic color tokens as CSS custom properties. The `.dark` class on `<html>` switches between light and dark palettes.

**Light theme** (slate-based for a slightly cool, technical tone):

| Token | Value | Usage |
|-------|-------|-------|
| `--surface` | `#f8fafc` (slate-50) | Page background |
| `--card` | `#ffffff` | Card/panel background |
| `--card-alt` | `#f1f5f9` (slate-100) | Table headers, alternating rows |
| `--sidebar` | `#0f172a` (slate-900) | Sidebar background |
| `--sidebar-hover` | `#1e293b` (slate-800) | Sidebar hover state |
| `--text-primary` | `#0f172a` (slate-900) | Headings, main text |
| `--text-secondary` | `#475569` (slate-600) | Labels, descriptions |
| `--text-muted` | `#94a3b8` (slate-400) | Placeholders, disabled |
| `--border` | `#cbd5e1` (slate-300) | Standard borders |
| `--border-light` | `#e2e8f0` (slate-200) | Subtle dividers |
| `--accent` | `#f59e0b` (amber-500) | Primary accent |
| `--accent-hover` | `#d97706` (amber-600) | Accent hover |
| `--accent-soft` | `#fef3c7` (amber-100) | Light accent background |
| `--danger` | `#ef4444` (red-500) | Destructive actions |
| `--danger-soft` | `#fef2f2` (red-50) | Danger background |
| `--success` | `#22c55e` (green-500) | Success states |
| `--success-soft` | `#f0fdf4` (green-50) | Success background |
| `--input-bg` | `#ffffff` | Form input background |

**Dark theme** (zinc-based for a sharper, more technical dark mode):

| Token | Value | Usage |
|-------|-------|-------|
| `--surface` | `#09090b` (zinc-950) | Page background |
| `--card` | `#18181b` (zinc-900) | Card/panel background |
| `--card-alt` | `#27272a` (zinc-800) | Table headers, alternating rows |
| `--sidebar` | `#09090b` (zinc-950) | Sidebar blends with surface |
| `--sidebar-hover` | `#27272a` (zinc-800) | Sidebar hover state |
| `--text-primary` | `#fafafa` (zinc-50) | Headings, main text |
| `--text-secondary` | `#a1a1aa` (zinc-400) | Labels, descriptions |
| `--text-muted` | `#52525b` (zinc-600) | Placeholders, disabled |
| `--border` | `#3f3f46` (zinc-700) | Standard borders |
| `--border-light` | `#27272a` (zinc-800) | Subtle dividers |
| `--accent` | `#f59e0b` (amber-500) | Primary accent |
| `--accent-hover` | `#fbbf24` (amber-400) | Brighter hover on dark |
| `--accent-soft` | `#451a03` (amber-950) | Dark accent background |
| `--danger` | `#f87171` (red-400) | Brighter on dark |
| `--danger-soft` | `#451a1a` | Danger background |
| `--success` | `#4ade80` (green-400) | Brighter on dark |
| `--success-soft` | `#052e16` (green-950) | Success background |
| `--input-bg` | `#27272a` (zinc-800) | Form input background |

### Twind Configuration

Configure twind to map semantic color names to CSS variables:

```js
twind.install({
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        surface: 'var(--surface)',
        card: 'var(--card)',
        'card-alt': 'var(--card-alt)',
        sidebar: 'var(--sidebar)',
        'sidebar-hover': 'var(--sidebar-hover)',
        primary: 'var(--text-primary)',
        secondary: 'var(--text-secondary)',
        muted: 'var(--text-muted)',
        accent: {
          DEFAULT: 'var(--accent)',
          hover: 'var(--accent-hover)',
          soft: 'var(--accent-soft)',
        },
        danger: { DEFAULT: 'var(--danger)', soft: 'var(--danger-soft)' },
        success: { DEFAULT: 'var(--success)', soft: 'var(--success-soft)' },
        border: 'var(--border)',
        'border-light': 'var(--border-light)',
        'input-bg': 'var(--input-bg)',
      }
    }
  }
})
```

This allows templates to use `bg-surface`, `text-primary`, `bg-accent`, etc. Dark mode is handled entirely by the CSS variable switch — no `dark:` prefixes needed on individual elements.

## 2. Dark Mode Toggle

### Initialization (inline script in `<head>`, runs before body)

```js
(function() {
  var stored = localStorage.getItem('theme');
  if (stored === 'dark' || (!stored && window.matchMedia('(prefers-color-scheme: dark)').matches)) {
    document.documentElement.classList.add('dark');
  }
})();
```

### Toggle Function

A button in the sidebar (above logout) and top-right corner of login/setup pages. Appearance: a small `text-muted hover:text-primary` button with sun entity (`&#9728;`) in dark mode, moon entity (`&#9790;`) in light mode. No icon library needed.

```js
function toggleTheme() {
  var isDark = document.documentElement.classList.toggle('dark');
  localStorage.setItem('theme', isDark ? 'dark' : 'light');
  updateToggleIcon();
}
```

### OS Preference Tracking

Register in the same inline `<head>` script. If `localStorage` has no `theme` key, listen for OS changes and toggle `.dark` accordingly. Once the user manually toggles, the listener is ignored (localStorage takes precedence).

```js
window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', function(e) {
  if (!localStorage.getItem('theme')) {
    document.documentElement.classList.toggle('dark', e.matches);
  }
});
```

### Browser Hint

`<meta name="color-scheme" content="light dark">` in `<head>` to prevent white flash.

## 3. Typography & Visual Polish

- **Body font**: System font stack (twind default) — no change.
- **Monospace accents**: `font-mono` on slugs, IDs, API tokens, filenames. "Substrukt" logo text in sidebar becomes `font-mono tracking-tight font-bold`.
- **Headings**: Add `tracking-tight` to page titles.
- **Theme transition**: Global `transition: background-color 0.15s, color 0.15s, border-color 0.15s` on `*` for smooth theme switching.
- **Borders over shadows**: Cards use `border border-border-light` instead of `shadow`. Login/setup card keeps a subtle shadow for the floating effect.
- **Tighter tables**: Cell padding `px-4 py-2.5` (from `px-4 py-3`).
- **Active nav indicator**: Current page sidebar link gets `border-l-2 border-accent bg-sidebar-hover` — amber left edge.

## 4. Component Updates

### Sidebar (`_nav.html`)
- Logo: inline SVG from `website/roundedicon.svg` at ~24px + "Substrukt" in `font-mono tracking-tight`
- Background: `bg-sidebar`
- Links: `hover:bg-sidebar-hover`, active state with `border-l-2 border-accent`
- Section headers: `text-muted uppercase tracking-wider`
- Right edge: `border-r border-border-light`
- Theme toggle button above logout

### Cards
- `bg-card border border-border-light rounded-lg p-6`

### Tables
- Container: `bg-card border border-border-light rounded-lg overflow-hidden`
- Header: `bg-card-alt`
- Row hover: `hover:bg-card-alt`
- Dividers: `divide-y divide-border-light`

### Buttons
- Primary: `bg-accent text-black font-medium px-4 py-2 rounded-md hover:bg-accent-hover`
- Secondary: `bg-card border border-border text-secondary px-4 py-2 rounded-md hover:bg-card-alt`
- Danger (text-style): `text-danger hover:bg-danger/10`

### Form Inputs
- Background: `bg-input-bg` (white in light, zinc-800 in dark)
- Border: `border-border`
- Focus: `focus:ring-2 focus:ring-accent focus:border-accent`
- Labels: `text-secondary`
- Remove `shadow-sm` from inputs (borders over shadows)

### Flash Messages
- Success: `bg-success-soft text-success border border-success/20`
- Error: `bg-danger-soft text-danger border border-danger/20`
- Info: `bg-accent-soft text-accent border border-accent/20`

Note: The token-created banner in `settings/tokens.html` uses the same success flash pattern above. The token display code block uses `bg-card-alt font-mono`.

### Empty States
- `text-muted` centered, links in `text-accent hover:underline`

## 5. Comprehensive Class Migration

### Global mappings (apply everywhere — templates, form.rs, and JS strings)

| Current | New |
|---------|-----|
| `bg-gray-50` | `bg-surface` |
| `bg-white` | `bg-card` |
| `bg-gray-100` / `bg-gray-50` (thead) | `bg-card-alt` |
| `bg-gray-900` (sidebar) | `bg-sidebar` |
| `hover:bg-gray-700` (sidebar) | `hover:bg-sidebar-hover` |
| `hover:bg-gray-50` (rows) | `hover:bg-card-alt` |
| `hover:bg-gray-200` | `hover:bg-card-alt` |
| `text-gray-900` / `text-gray-800` | `text-primary` |
| `text-gray-700` / `text-gray-600` | `text-secondary` |
| `text-gray-500` / `text-gray-400` | `text-muted` |
| `text-gray-100` (sidebar text) | `text-primary` (sidebar inherits light text via CSS vars) |
| `border-gray-300` | `border-border` |
| `border-gray-200` / `border-gray-100` | `border-border-light` |
| `border-gray-700` (sidebar divider) | `border-border-light` |
| `divide-gray-100` / `divide-gray-200` | `divide-border-light` |
| `bg-blue-600` (primary buttons) | `bg-accent` |
| `hover:bg-blue-700` | `hover:bg-accent-hover` |
| `text-white` (on blue buttons) | `text-black` (amber is light, needs dark text) |
| `focus:ring-blue-500` | `focus:ring-accent` |
| `focus:border-blue-500` | `focus:border-accent` |
| `text-blue-600` (links/accents) | `text-accent` |
| `hover:underline` (on links) | `hover:underline` (keep) |
| `text-red-500` / `text-red-600` | `text-danger` |
| `text-red-700` / `hover:text-red-700` | `text-danger` / `hover:text-danger` |
| `bg-red-50 text-red-700/800` (flash) | `bg-danger-soft text-danger` |
| `border-red-200` | `border-danger` |
| `bg-red-50 text-red-600 hover:bg-red-100` (delete btn) | `text-danger hover:bg-danger-soft` |
| `bg-green-50 text-green-800` (flash) | `bg-success-soft text-success` |
| `border-green-200` | `border-success` |
| `bg-green-100` (token code bg) | `bg-card-alt` |
| `bg-blue-50 text-blue-800` (info flash) | `bg-accent-soft text-accent` |
| `border-blue-200` | `border-accent` |
| `text-blue-600 underline` (file link) | `text-accent underline` |
| `text-gray-500 hover:text-gray-700` (back links) | `text-muted hover:text-primary` |
| `text-gray-400 hover:text-blue-600` (edit link) | `text-muted hover:text-accent` |
| `shadow` / `shadow-lg` (cards) | Remove (use border instead). Keep `shadow-lg` only on login/setup card. |
| `shadow-sm` (inputs) | Remove |
| `text-amber-600` (orphaned label) | `text-amber-600` (keep — intentionally distinct from accent for warning semantics) |
| `bg-amber-400` (dirty dot) | `bg-accent` |
| `bg-green-400` (clean dot) | `bg-success` |

### JavaScript hardcoded classes

Both `base.html` and `_partial.html` contain an `addArrayItem()` function with hardcoded classes in JS strings. These must also be updated:

- `'border border-gray-100 p-3 rounded mb-2'` → `'border border-border-light p-3 rounded mb-2'`
- `'text-red-500 text-sm hover:text-red-700'` → `'text-danger text-sm hover:text-danger'`

### JSON editor inline style

In `schemas/edit.html`, the JSON editor container has hardcoded `border: 1px solid #d1d5db`. Replace with `border: 1px solid var(--border)`.

## 6. Files to Modify

**Templates (13 files):**
- `templates/base.html` — CSS variables `<style>` block, twind config, theme init script, theme transition CSS, twind install, semantic class migration, JS array item class fix
- `templates/_nav.html` — logo SVG, active state indicator, theme toggle button, semantic colors
- `templates/_partial.html` — JS array item class fix (same as base.html)
- `templates/login.html` — needs its own: CSS variables `<style>`, twind config `<script>`, theme init `<script>`, toggle button (top-right of card), semantic class migration
- `templates/setup.html` — same as login.html
- `templates/error.html` — semantic colors
- `templates/dashboard.html` — semantic colors
- `templates/schemas/list.html` — semantic colors, table updates
- `templates/schemas/edit.html` — semantic colors, JSON editor border fix
- `templates/content/list.html` — semantic colors, table updates
- `templates/content/edit.html` — semantic colors
- `templates/uploads/list.html` — semantic colors, table updates
- `templates/settings/tokens.html` — semantic colors, table updates, token banner

**Rust source:**
- `src/content/form.rs` — update all hardcoded color classes per migration table, add `bg-input-bg` to inputs

**Static assets:**
- Inline `website/roundedicon.svg` in `_nav.html` for sidebar logo

## 7. What This Does NOT Change

- Layout structure (sidebar + content)
- htmx behavior
- Template inheritance / partial rendering
- Any Rust routing or business logic
- JavaScript functionality (array items, delete handlers, JSON editor)
