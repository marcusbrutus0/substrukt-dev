# Schema Field Constraints in Form UI Design

## Goal

Make the form UI reflect JSON Schema constraints that are already enforced server-side by the `jsonschema` crate. Add HTML validation attributes and hint text so users see constraints before submitting.

## Current State

- `render_field` in `content/form.rs` generates HTML from JSON Schema properties
- Currently handles: `required`, `enum`, `format` (markdown, textarea, upload, reference), `type` (string, number, integer, boolean, object, array)
- No constraint properties (`minLength`, `maxLength`, `pattern`, `minimum`, `maximum`, etc.) are read or rendered
- Server-side validation via `jsonschema` already enforces all constraints — this is purely a UI enhancement
- `description` field in schema properties is not rendered anywhere

## Scope

In scope: `minLength`, `maxLength`, `pattern`, `minimum`, `maximum`, `exclusiveMinimum`, `exclusiveMaximum`, `multipleOf`, `minItems`, `maxItems`, `description`.

Out of scope: `const`, additional string formats (`date`, `email`, `uri`), `uniqueItems`. These are separate features.

## Design

### Approach

Inline constraint handling in each match arm of `render_field`. Read constraint properties from the schema JSON value, add HTML attributes to inputs, and build a hint line below the field. No new structs or abstractions.

### String fields (`<input type="text">` and `<textarea>`)

Constraints read from schema:
- `minLength` (integer) → `minlength="N"` HTML attribute. Skip if value is 0.
- `maxLength` (integer) → `maxlength="N"` HTML attribute
- `pattern` (string) → `pattern="..."` HTML attribute on `<input>` only (`pattern` is not valid on `<textarea>`). Skip if empty. For textarea/markdown fields, pattern appears as hint text only.

Hint text:
- Both min and max: "3–100 characters"
- Only min: "Min 3 characters"
- Only max: "Max 100 characters"
- Pattern: "Pattern: `^[a-z]+$`"
- Multiple hints joined with " · "

Applies to: plain string input, textarea, markdown textarea. Does NOT apply to enum select, upload, or reference fields (constraints are not meaningful for those).

### Number/integer fields (`<input type="number">`)

Constraints read from schema:
- `minimum` (number) → `min="N"` HTML attribute
- `maximum` (number) → `max="N"` HTML attribute
- `exclusiveMinimum` (number) → for integers: `min="N+1"` (exact); for floats: no HTML attribute (hint only, to avoid mismatch between inclusive HTML min and exclusive constraint)
- `exclusiveMaximum` (number) → for integers: `max="N-1"` (exact); for floats: no HTML attribute (hint only)
- `multipleOf` (number) → `step="N"` HTML attribute (overrides default `step="1"` for integers, `step="any"` for numbers)
- If both `minimum` and `exclusiveMinimum` are present, use the tighter (larger) bound. Same logic for maximum bounds.

Hint text:
- Both min and max: "5–100"
- Only min: "Min 5" or "> 0" for exclusive
- Only max: "Max 100" or "< 1000" for exclusive
- multipleOf: "Step: 0.5"
- Multiple hints joined with " · "

### Array fields

Constraints read from schema:
- `minItems` (integer) → hint text only
- `maxItems` (integer) → hint text only

No HTML attributes (arrays use custom add/remove UI, not native form controls).

Hint text:
- Both: "1–5 items"
- Only min: "Min 1 item" (singular for 1, plural otherwise)
- Only max: "Max 5 items"

### Object fields

No special handling. Object fields recursively call `render_form_fields` for nested properties, so constraints on nested fields are handled automatically.

### Description field

All field types: if `description` (string) is present in the schema property, render it as hint text. Appended after constraint hints with " · " separator. The description value must be HTML-escaped before rendering.

### Fields without constraint support

Boolean (checkbox), upload, reference, and enum select fields only get `description` hint text. No constraint attributes are added — these field types don't have meaningful JSON Schema numeric/string constraints.

### Hint text rendering

A `<p class="text-xs text-muted mt-1">` element below the input. Only rendered if there are hints to show. Multiple hints joined with " · ". Placed inside the field's `<div class="mb-4">` wrapper, after the input element.

### Escaping

- `pattern` values must be HTML-attribute-escaped before insertion into `pattern="..."` (escape `"`, `&`, `<`, `>`)
- `description` values must be HTML-escaped before rendering into the `<p>` hint element
- Use a simple escape helper function for both cases

### Files changed

- Modify: `src/content/form.rs` — add constraint attributes and hint text generation in `render_field` match arms, add HTML escape helper, add unit tests

### Testing

Unit tests in `form.rs`:
- String field with `minLength`/`maxLength` renders attributes and combined hint
- String field with `pattern` renders attribute and hint (input only, not textarea)
- Number field with `minimum`/`maximum` renders `min`/`max` attributes and hint
- Integer field with `exclusiveMinimum`/`exclusiveMaximum` adjusts bound values by 1
- Number field with `multipleOf` renders `step` attribute
- Array field with `minItems`/`maxItems` renders hint text
- Field with `description` renders hint text (HTML-escaped)
- No constraints = no hint line rendered

No integration tests needed. Server-side validation is unchanged.

### Error handling

Missing or non-numeric constraint values are silently skipped. Empty `pattern` strings are skipped. `minLength: 0` is skipped. The `jsonschema` crate remains the authoritative validator — form attributes are advisory.
