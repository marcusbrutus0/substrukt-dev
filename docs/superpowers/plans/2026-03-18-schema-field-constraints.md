# Schema Field Constraints Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make form UI fields reflect JSON Schema constraints with HTML attributes and hint text.

**Architecture:** Add an `escape_html_attr` helper and a `build_hint_line` helper to `form.rs`. Modify each match arm in `render_field` to read constraint properties from the schema, add HTML attributes, and append a hint `<p>` element. All changes in a single file.

**Tech Stack:** Rust, serde_json, HTML5 form attributes

---

## File Map

- **Modify:** `src/content/form.rs` — add helpers, modify match arms, add unit tests

---

### Task 1: Add helper functions and string field constraints

**Files:**
- Modify: `src/content/form.rs:1-6` (imports area), `src/content/form.rs:66-194` (render_field string arms)

- [ ] **Step 1: Write failing tests for string constraints**

Add to `#[cfg(test)] mod tests` in `src/content/form.rs`:

```rust
#[test]
fn string_field_minlength_maxlength_renders_attrs_and_hint() {
    let schema = json!({
        "properties": {
            "name": {
                "type": "string",
                "title": "Name",
                "minLength": 3,
                "maxLength": 100
            }
        }
    });
    let html = render_form_fields(&schema, None, "", &ReferenceOptions::new());
    assert!(html.contains(r#"minlength="3""#), "should have minlength attr");
    assert!(html.contains(r#"maxlength="100""#), "should have maxlength attr");
    assert!(html.contains("3–100 characters"), "should show combined hint");
}

#[test]
fn string_field_pattern_renders_attr_and_hint() {
    let schema = json!({
        "properties": {
            "slug": {
                "type": "string",
                "title": "Slug",
                "pattern": "^[a-z0-9-]+$"
            }
        }
    });
    let html = render_form_fields(&schema, None, "", &ReferenceOptions::new());
    assert!(html.contains(r#"pattern="^[a-z0-9-]+$""#), "should have pattern attr");
    assert!(html.contains("Pattern:"), "should show pattern hint");
}

#[test]
fn textarea_field_no_pattern_attr_but_shows_hint() {
    let schema = json!({
        "properties": {
            "bio": {
                "type": "string",
                "format": "textarea",
                "title": "Bio",
                "pattern": "^[A-Z]",
                "maxLength": 500
            }
        }
    });
    let html = render_form_fields(&schema, None, "", &ReferenceOptions::new());
    // textarea must NOT have pattern attr (invalid HTML)
    assert!(!html.contains(r#"pattern="#), "textarea should not have pattern attr");
    assert!(html.contains("Pattern:"), "should still show pattern as hint");
    assert!(html.contains(r#"maxlength="500""#), "should have maxlength attr");
}

#[test]
fn field_description_renders_as_hint() {
    let schema = json!({
        "properties": {
            "email": {
                "type": "string",
                "title": "Email",
                "description": "Your primary email address"
            }
        }
    });
    let html = render_form_fields(&schema, None, "", &ReferenceOptions::new());
    assert!(html.contains("Your primary email address"), "should show description");
    assert!(html.contains("text-xs text-muted"), "should use hint styling");
}

#[test]
fn no_constraints_no_hint_line() {
    let schema = json!({
        "properties": {
            "title": {
                "type": "string",
                "title": "Title"
            }
        }
    });
    let html = render_form_fields(&schema, None, "", &ReferenceOptions::new());
    assert!(!html.contains("text-xs text-muted"), "should not have hint line");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test --lib form 2>&1`
Expected: 5 new tests FAIL (missing attrs/hints).

- [ ] **Step 3: Add `escape_html_attr` and `build_hint_line` helpers**

Add after line 6 (after the `ReferenceOptions` type alias), before `render_form_fields`:

```rust
fn escape_html_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn build_hint_line(hints: &[String]) -> String {
    if hints.is_empty() {
        String::new()
    } else {
        format!(
            r#"  <p class="text-xs text-muted mt-1">{}</p>
"#,
            hints.join(" · ")
        )
    }
}

/// Extract `description` from a schema property, HTML-escaped.
fn get_description(schema: &Value) -> Option<String> {
    schema
        .get("description")
        .and_then(|d| d.as_str())
        .filter(|d| !d.is_empty())
        .map(|d| escape_html_attr(d))
}

/// Build string constraint HTML attrs and hint parts.
/// `is_textarea` controls whether `pattern` becomes an HTML attr or hint-only.
fn string_constraints(schema: &Value, is_textarea: bool) -> (String, Vec<String>) {
    let mut attrs = String::new();
    let mut hints = Vec::new();

    let min_len = schema.get("minLength").and_then(|v| v.as_u64()).filter(|&v| v > 0);
    let max_len = schema.get("maxLength").and_then(|v| v.as_u64());

    if let Some(min) = min_len {
        attrs.push_str(&format!(r#" minlength="{min}""#));
    }
    if let Some(max) = max_len {
        attrs.push_str(&format!(r#" maxlength="{max}""#));
    }

    match (min_len, max_len) {
        (Some(min), Some(max)) => hints.push(format!("{min}–{max} characters")),
        (Some(min), None) => hints.push(format!("Min {min} characters")),
        (None, Some(max)) => hints.push(format!("Max {max} characters")),
        _ => {}
    }

    let pattern = schema.get("pattern").and_then(|v| v.as_str()).filter(|p| !p.is_empty());
    if let Some(pat) = pattern {
        if !is_textarea {
            attrs.push_str(&format!(r#" pattern="{}""#, escape_html_attr(pat)));
        }
        hints.push(format!("Pattern: <code>{}</code>", escape_html_attr(pat)));
    }

    if let Some(desc) = get_description(schema) {
        hints.push(desc);
    }

    (attrs, hints)
}
```

- [ ] **Step 4: Modify the `("string", Some("markdown"))` arm (lines 80-88)**

Replace with:

```rust
        ("string", Some("markdown")) => {
            let val = value.and_then(|v| v.as_str()).unwrap_or("");
            let (constraint_attrs, hints) = string_constraints(schema, true);
            let hint_html = build_hint_line(&hints);
            format!(
                r#"<div class="mb-4">
  <label for="{name}" class="block text-sm font-medium text-secondary mb-1">{label}{req_star}</label>
  <textarea id="{name}" name="{name}" rows="12" data-markdown class="w-full px-3 py-2 border border-border rounded-md bg-input-bg focus:outline-none focus:ring-2 focus:ring-accent focus:border-accent"{constraint_attrs}{req_attr}>{val}</textarea>
{hint_html}</div>
"#
            )
        }
```

- [ ] **Step 5: Modify the `("string", Some("textarea"))` arm (lines 90-98)**

Replace with:

```rust
        ("string", Some("textarea")) => {
            let val = value.and_then(|v| v.as_str()).unwrap_or("");
            let (constraint_attrs, hints) = string_constraints(schema, true);
            let hint_html = build_hint_line(&hints);
            format!(
                r#"<div class="mb-4">
  <label for="{name}" class="block text-sm font-medium text-secondary mb-1">{label}{req_star}</label>
  <textarea id="{name}" name="{name}" rows="6" class="w-full px-3 py-2 border border-border rounded-md bg-input-bg focus:outline-none focus:ring-2 focus:ring-accent focus:border-accent"{constraint_attrs}{req_attr}>{val}</textarea>
{hint_html}</div>
"#
            )
        }
```

- [ ] **Step 6: Modify the plain string `<input>` branch (lines 185-193, inside `("string", _)` after the enum check)**

Replace the `else` branch:

```rust
            } else {
                let val = value.and_then(|v| v.as_str()).unwrap_or("");
                let (constraint_attrs, hints) = string_constraints(schema, false);
                let hint_html = build_hint_line(&hints);
                format!(
                    r#"<div class="mb-4">
  <label for="{name}" class="block text-sm font-medium text-secondary mb-1">{label}{req_star}</label>
  <input type="text" id="{name}" name="{name}" value="{val}" class="w-full px-3 py-2 border border-border rounded-md bg-input-bg focus:outline-none focus:ring-2 focus:ring-accent focus:border-accent"{constraint_attrs}{req_attr}>
{hint_html}</div>
"#
                )
            }
```

- [ ] **Step 7: Add description hints to upload, reference, enum, and boolean arms**

For the **enum select** arm (inside `("string", _)`, the `if let Some(enum_values)` branch), add description hint after the `</select>`:

```rust
            if let Some(enum_values) = schema.get("enum").and_then(|e| e.as_array()) {
                let val = value.and_then(|v| v.as_str()).unwrap_or("");
                let mut options = r#"<option value="">-- Select --</option>"#.to_string();
                for ev in enum_values {
                    let ev_str = ev.as_str().unwrap_or("");
                    let selected = if ev_str == val { " selected" } else { "" };
                    options.push_str(&format!(
                        r#"<option value="{ev_str}"{selected}>{ev_str}</option>"#
                    ));
                }
                let hint_html = build_hint_line(&get_description(schema).into_iter().collect::<Vec<_>>());
                format!(
                    r#"<div class="mb-4">
  <label for="{name}" class="block text-sm font-medium text-secondary mb-1">{label}{req_star}</label>
  <select id="{name}" name="{name}" class="w-full px-3 py-2 border border-border rounded-md bg-input-bg focus:outline-none focus:ring-2 focus:ring-accent focus:border-accent"{req_attr}>
    {options}
  </select>
{hint_html}</div>
"#
                )
```

For the **reference** arm, add description hint similarly after `</select>`.

For the **boolean** arm, add description hint after the `</label>`:

```rust
        ("boolean", _) => {
            let checked = value.and_then(|v| v.as_bool()).unwrap_or(false);
            let checked_attr = if checked { " checked" } else { "" };
            let hint_html = build_hint_line(&get_description(schema).into_iter().collect::<Vec<_>>());
            format!(
                r#"<div class="mb-4">
  <label class="flex items-center gap-2">
    <input type="hidden" name="{name}" value="false">
    <input type="checkbox" name="{name}" value="true" class="rounded border-border text-accent focus:ring-accent"{checked_attr}>
    <span class="text-sm font-medium text-secondary">{label}</span>
  </label>
{hint_html}</div>
"#
            )
        }
```

For the **upload** arm, add description hint before closing `</div>`:

```rust
        ("string", Some("upload")) => {
            // ... existing current_html logic unchanged ...

            let hint_html = build_hint_line(&get_description(schema).into_iter().collect::<Vec<_>>());
            format!(
                r#"<div class="mb-4">
  <label class="block text-sm font-medium text-secondary mb-1">{label}{req_star}</label>
  {current_html}
  <div class="upload-zone border-2 border-dashed border-border rounded-lg p-6 text-center cursor-pointer hover:border-accent transition-colors relative" data-upload-zone>
    <div class="upload-zone-prompt text-muted text-sm">Drag a file here or click to browse</div>
    <div class="upload-zone-info hidden text-sm text-secondary mt-2"></div>
    <div class="upload-zone-preview hidden mt-2 flex justify-center"></div>
    <input type="file" id="{name}" name="{name}" class="absolute inset-0 w-full h-full opacity-0 cursor-pointer" data-upload-input{req_attr}>
  </div>
{hint_html}</div>
"#
            )
        }
```

For the **reference** arm, add description hint before closing `</div>`:

```rust
        ("string", Some("reference")) => {
            let val = value.and_then(|v| v.as_str()).unwrap_or("");
            let empty_opts = Vec::new();
            let options = ref_options.get(name).unwrap_or(&empty_opts);
            let mut opts_html = r#"<option value="">-- Select --</option>"#.to_string();
            for (id, label_text) in options {
                let selected = if id == val { " selected" } else { "" };
                opts_html.push_str(&format!(
                    r#"<option value="{id}"{selected}>{label_text}</option>"#
                ));
            }
            let hint_html = build_hint_line(&get_description(schema).into_iter().collect::<Vec<_>>());
            format!(
                r#"<div class="mb-4">
  <label for="{name}" class="block text-sm font-medium text-secondary mb-1">{label}{req_star}</label>
  <select id="{name}" name="{name}" class="w-full px-3 py-2 border border-border rounded-md bg-input-bg focus:outline-none focus:ring-2 focus:ring-accent focus:border-accent"{req_attr}>
    {opts_html}
  </select>
{hint_html}</div>
"#
            )
        }
```

- [ ] **Step 8: Run tests**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test --lib form 2>&1`
Expected: All 8 tests pass (3 existing + 5 new).

- [ ] **Step 9: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add src/content/form.rs && git commit -m "feat: add string field constraints and description hints to form UI

Renders minLength, maxLength, pattern as HTML attributes and hint text.
Pattern attr omitted on textarea (invalid HTML), shown as hint instead.
Description field rendered as hint text on all field types.
Adds escape_html_attr, build_hint_line, string_constraints helpers.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 2: Number/integer field constraints

**Files:**
- Modify: `src/content/form.rs:196-211` (number/integer arm)

- [ ] **Step 1: Write failing tests for number constraints**

Add to `#[cfg(test)] mod tests`:

```rust
#[test]
fn number_field_min_max_renders_attrs_and_hint() {
    let schema = json!({
        "properties": {
            "price": {
                "type": "number",
                "title": "Price",
                "minimum": 0.01,
                "maximum": 9999.99
            }
        }
    });
    let html = render_form_fields(&schema, None, "", &ReferenceOptions::new());
    assert!(html.contains(r#"min="0.01""#), "should have min attr");
    assert!(html.contains(r#"max="9999.99""#), "should have max attr");
    assert!(html.contains("0.01–9999.99"), "should show range hint");
}

#[test]
fn integer_field_exclusive_bounds_adjusted() {
    let schema = json!({
        "properties": {
            "age": {
                "type": "integer",
                "title": "Age",
                "exclusiveMinimum": 0,
                "exclusiveMaximum": 150
            }
        }
    });
    let html = render_form_fields(&schema, None, "", &ReferenceOptions::new());
    assert!(html.contains(r#"min="1""#), "exclusive min 0 -> min 1 for integer");
    assert!(html.contains(r#"max="149""#), "exclusive max 150 -> max 149 for integer");
}

#[test]
fn number_field_exclusive_bounds_hint_only() {
    let schema = json!({
        "properties": {
            "rate": {
                "type": "number",
                "title": "Rate",
                "exclusiveMinimum": 0
            }
        }
    });
    let html = render_form_fields(&schema, None, "", &ReferenceOptions::new());
    // Float exclusive: no HTML min attr, hint only
    assert!(!html.contains(r#"min="#), "no min attr for exclusive float bound");
    assert!(html.contains("&gt; 0"), "should show > 0 hint");
}

#[test]
fn number_field_multiple_of_renders_step() {
    let schema = json!({
        "properties": {
            "quantity": {
                "type": "integer",
                "title": "Quantity",
                "multipleOf": 5
            }
        }
    });
    let html = render_form_fields(&schema, None, "", &ReferenceOptions::new());
    assert!(html.contains(r#"step="5""#), "should override step with multipleOf");
    assert!(html.contains("Step: 5"), "should show step hint");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test --lib form 2>&1`
Expected: 4 new tests FAIL.

- [ ] **Step 3: Add `number_constraints` helper**

Add after the `string_constraints` function:

```rust
/// Build number/integer constraint HTML attrs and hint parts.
fn number_constraints(schema: &Value, is_integer: bool) -> (String, Vec<String>) {
    let mut attrs = String::new();
    let mut hints = Vec::new();

    let minimum = schema.get("minimum").and_then(|v| v.as_f64());
    let maximum = schema.get("maximum").and_then(|v| v.as_f64());
    let exc_min = schema.get("exclusiveMinimum").and_then(|v| v.as_f64());
    let exc_max = schema.get("exclusiveMaximum").and_then(|v| v.as_f64());

    // Resolve effective min: tighter of minimum and exclusiveMinimum
    let (effective_min, min_exclusive) = match (minimum, exc_min) {
        (Some(m), Some(e)) => {
            let adj = if is_integer { e + 1.0 } else { e };
            if adj > m { (Some(adj), !is_integer) } else { (Some(m), false) }
        }
        (Some(m), None) => (Some(m), false),
        (None, Some(e)) => {
            if is_integer { (Some(e + 1.0), false) } else { (Some(e), true) }
        }
        (None, None) => (None, false),
    };

    // Resolve effective max: tighter of maximum and exclusiveMaximum
    let (effective_max, max_exclusive) = match (maximum, exc_max) {
        (Some(m), Some(e)) => {
            let adj = if is_integer { e - 1.0 } else { e };
            if adj < m { (Some(adj), !is_integer) } else { (Some(m), false) }
        }
        (Some(m), None) => (Some(m), false),
        (None, Some(e)) => {
            if is_integer { (Some(e - 1.0), false) } else { (Some(e), true) }
        }
        (None, None) => (None, false),
    };

    // Format a number: show as integer if it has no fractional part
    fn fmt_num(n: f64) -> String {
        if n.fract() == 0.0 { format!("{}", n as i64) } else { format!("{n}") }
    }

    if let Some(min) = effective_min {
        if !min_exclusive {
            attrs.push_str(&format!(r#" min="{}""#, fmt_num(min)));
        }
    }
    if let Some(max) = effective_max {
        if !max_exclusive {
            attrs.push_str(&format!(r#" max="{}""#, fmt_num(max)));
        }
    }

    // Hint text
    let min_hint = effective_min.map(|v| {
        if min_exclusive { format!("&gt; {}", fmt_num(v)) } else { fmt_num(v) }
    });
    let max_hint = effective_max.map(|v| {
        if max_exclusive { format!("&lt; {}", fmt_num(v)) } else { fmt_num(v) }
    });

    match (min_hint, max_hint) {
        (Some(min), Some(max)) => {
            if !min_exclusive && !max_exclusive {
                hints.push(format!("{min}–{max}"));
            } else {
                hints.push(format!("{min} to {max}"));
            }
        }
        (Some(min), None) => {
            if min_exclusive { hints.push(min); } else { hints.push(format!("Min {min}")); }
        }
        (None, Some(max)) => {
            if max_exclusive { hints.push(max); } else { hints.push(format!("Max {max}")); }
        }
        _ => {}
    }

    // multipleOf -> step
    if let Some(step) = schema.get("multipleOf").and_then(|v| v.as_f64()) {
        attrs.push_str(&format!(r#" step="{}""#, fmt_num(step)));
        hints.push(format!("Step: {}", fmt_num(step)));
    }

    if let Some(desc) = get_description(schema) {
        hints.push(desc);
    }

    (attrs, hints)
}
```

- [ ] **Step 4: Modify the `("number" | "integer", _)` arm (lines 196-211)**

Replace with:

```rust
        ("number" | "integer", _) => {
            let val = value.map(|v| v.to_string()).unwrap_or_default();
            let val = val.trim_matches('"');
            let is_integer = field_type == "integer";

            let (constraint_attrs, hints) = number_constraints(schema, is_integer);
            let hint_html = build_hint_line(&hints);

            // Default step if multipleOf not specified
            let has_step = constraint_attrs.contains("step=");
            let step = if has_step {
                String::new()
            } else if is_integer {
                r#" step="1""#.to_string()
            } else {
                r#" step="any""#.to_string()
            };

            format!(
                r#"<div class="mb-4">
  <label for="{name}" class="block text-sm font-medium text-secondary mb-1">{label}{req_star}</label>
  <input type="number" id="{name}" name="{name}" value="{val}"{step}{constraint_attrs} class="w-full px-3 py-2 border border-border rounded-md bg-input-bg focus:outline-none focus:ring-2 focus:ring-accent focus:border-accent"{req_attr}>
{hint_html}</div>
"#
            )
        }
```

- [ ] **Step 5: Run tests**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test --lib form 2>&1`
Expected: All 12 tests pass (3 existing + 5 from Task 1 + 4 new).

- [ ] **Step 6: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add src/content/form.rs && git commit -m "feat: add number/integer field constraints to form UI

Renders minimum, maximum, exclusiveMinimum, exclusiveMaximum, multipleOf
as HTML attributes and hint text. Integer exclusive bounds adjusted by 1.
Float exclusive bounds shown as hint only (no HTML attr).

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```

---

### Task 3: Array field constraints

**Files:**
- Modify: `src/content/form.rs:236-275` (array arm)

- [ ] **Step 1: Write failing test for array constraints**

Add to `#[cfg(test)] mod tests`:

```rust
#[test]
fn array_field_min_max_items_renders_hint() {
    let schema = json!({
        "properties": {
            "tags": {
                "type": "array",
                "title": "Tags",
                "minItems": 1,
                "maxItems": 5,
                "items": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string", "title": "Name" }
                    }
                }
            }
        }
    });
    let html = render_form_fields(&schema, None, "", &ReferenceOptions::new());
    assert!(html.contains("1–5 items"), "should show item count range hint");
    assert!(html.contains("text-xs text-muted"), "should use hint styling");
}

#[test]
fn array_field_min_items_singular() {
    let schema = json!({
        "properties": {
            "items": {
                "type": "array",
                "title": "Items",
                "minItems": 1,
                "items": { "type": "object", "properties": { "v": { "type": "string" } } }
            }
        }
    });
    let html = render_form_fields(&schema, None, "", &ReferenceOptions::new());
    assert!(html.contains("Min 1 item"), "should use singular 'item'");
    assert!(!html.contains("Min 1 items"), "should not use plural for 1");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test --lib form 2>&1`
Expected: 2 new tests FAIL.

- [ ] **Step 3: Modify the `("array", _)` arm**

Add constraint hint and description after the items container, before the "+ Add Item" button. Replace the array arm:

```rust
        ("array", _) => {
            let items_schema = schema
                .get("items")
                .cloned()
                .unwrap_or(Value::Object(Default::default()));
            let existing_items = value.and_then(|v| v.as_array());
            let mut items_html = String::new();

            if let Some(items) = existing_items {
                for (i, item) in items.iter().enumerate() {
                    let item_name = format!("{name}[{i}]");
                    items_html.push_str(&format!(
                        r#"<div class="array-item border border-border-light p-3 rounded mb-2" data-index="{i}">
  <div class="flex justify-end mb-1">
    <button type="button" onclick="this.closest('.array-item').remove()" class="text-danger text-sm hover:text-danger">Remove</button>
  </div>
  {}
</div>"#,
                        render_form_fields(&items_schema, Some(item), &item_name, ref_options)
                    ));
                }
            }

            // Template for new items (hidden, used by JS)
            let template_name = format!("{name}[__INDEX__]");
            let template_html =
                render_form_fields(&items_schema, None, &template_name, ref_options);

            // Array constraints (hint only)
            let mut hints = Vec::new();
            let min_items = schema.get("minItems").and_then(|v| v.as_u64());
            let max_items = schema.get("maxItems").and_then(|v| v.as_u64());
            match (min_items, max_items) {
                (Some(min), Some(max)) => hints.push(format!("{min}–{max} items")),
                (Some(1), None) => hints.push("Min 1 item".to_string()),
                (Some(min), None) => hints.push(format!("Min {min} items")),
                (None, Some(max)) => hints.push(format!("Max {max} items")),
                _ => {}
            }
            if let Some(desc) = get_description(schema) {
                hints.push(desc);
            }
            let hint_html = build_hint_line(&hints);

            format!(
                r#"<div class="mb-4">
  <label class="block text-sm font-medium text-secondary mb-1">{label}</label>
  <div id="array-{name}" class="array-container">
    {items_html}
  </div>
  <template id="template-{name}">{template_html}</template>
  <button type="button" onclick="addArrayItem('{name}')" class="mt-2 px-3 py-1 text-sm bg-card-alt border border-border rounded hover:bg-card-alt">+ Add Item</button>
{hint_html}</div>
"#
            )
        }
```

- [ ] **Step 4: Run tests**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test --lib form 2>&1`
Expected: All 14 tests pass (3 existing + 5 + 4 + 2 new).

- [ ] **Step 5: Run full test suite**

Run: `eval "$(direnv export bash 2>/dev/null)" && cargo test 2>&1`
Expected: All tests pass (unit + integration).

- [ ] **Step 6: Commit**

```bash
eval "$(direnv export bash 2>/dev/null)" && git add src/content/form.rs && git commit -m "feat: add array field constraints and complete hint support

Renders minItems/maxItems as hint text for array fields.
Singular 'item' for minItems=1. Description hints on all field types.

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>"
```
