// One-shot Node.js Yjs operations.
//
// This spawns `node -e <script>` for each operation, passing data via
// temp files. Avoids all the pipe/fifo complexity of a persistent worker.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use serde_json::Value;

/// Renders a one-shot JS script that loads yjs, applies an update from a base64
/// file, runs an operation, and writes the result as JSON to stdout.
pub fn run_tag_op(
    scripts_dir: &PathBuf,
    b64: &str,
    op_js: &str,
) -> Result<Value> {
    // Write the base64 data to a temp file (avoids pipe buffer issues)
    let tmp_dir = std::env::temp_dir();
    let b64_path = tmp_dir.join(format!("yjs_b64_{}.txt", std::process::id()));
    let result_path = tmp_dir.join(format!("yjs_result_{}.json", std::process::id()));

    let mut f = std::fs::File::create(&b64_path)
        .with_context(|| format!("failed to create {}", b64_path.display()))?;
    f.write_all(b64.as_bytes())?;
    f.flush()?;
    drop(f);

    let script = format!(
        r#"
const fs = require('fs');
const b64 = fs.readFileSync('{b64_path}', 'utf8').trim();
const raw = Buffer.from(b64, 'base64');
const arr = new Uint8Array(raw);

// Load yjs bundle (IIFE, needs eval in global scope)
const bundleCode = fs.readFileSync('{bundle_path}', 'utf8');
eval(bundleCode);
const doc = new Y.Doc();
Y.applyUpdate(doc, arr);

// Run the operation
const result = (function() {{ {op_js} }})();

fs.writeFileSync('{result_path}', JSON.stringify(result));
"#,
        b64_path = b64_path.display().to_string().replace('\\', "\\\\"),
        bundle_path = scripts_dir.join("yjs_bundle.js").display().to_string().replace('\\', "\\\\"),
        result_path = result_path.display().to_string().replace('\\', "\\\\"),
        op_js = op_js,
    );

    let output = std::process::Command::new("node")
        .arg("-e")
        .arg(&script)
        .output()
        .context("failed to spawn node for yjs operation")?;

    // Clean up the b64 temp file
    let _ = std::fs::remove_file(&b64_path);

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("yjs operation failed: {stderr}"));
    }

    // Read the result from the temp file
    let result_str = std::fs::read_to_string(&result_path)
        .with_context(|| format!("failed to read result from {}", result_path.display()))?;
    let _ = std::fs::remove_file(&result_path);

    let result: Value = serde_json::from_str(&result_str)
        .context("failed to parse yjs operation result")?;

    Ok(result)
}

/// List tags.
pub fn list_tags_op(scripts_dir: &PathBuf, b64: &str) -> Result<Vec<crate::tags::TagOption>> {
    let op = r#"
const meta = doc.getMap('meta');
const properties = meta.get('properties');
if (!(properties instanceof Y.Map)) return [];
const tags = properties.get('tags');
if (!(tags instanceof Y.Map)) return [];
const options = tags.get('options');
if (!(options instanceof Y.Array)) return [];
const result = [];
options.forEach(opt => {
  if (opt instanceof Y.Map) {
    result.push({
      id: opt.get('id') || '',
      value: opt.get('value') || '',
      color: opt.get('color') || '',
      create_date: opt.get('createDate') || null,
      update_date: opt.get('updateDate') || null,
    });
  }
});
return result;
"#;
    let val = run_tag_op(scripts_dir, b64, op)?;
    let tags: Vec<crate::tags::TagOption> =
        serde_json::from_value(val).context("failed to parse tag list")?;
    Ok(tags)
}

/// Create a tag. Returns the new tag and the updated doc state as base64.
pub fn create_tag_op(scripts_dir: &PathBuf, b64: &str, name: &str, color: &str) -> Result<(crate::tags::TagOption, String)> {
    let op = format!(r#"
const meta = doc.getMap('meta');
let properties = meta.get('properties');
if (!(properties instanceof Y.Map)) {{ properties = new Y.Map(); meta.set('properties', properties); }}
let tags = properties.get('tags');
if (!(tags instanceof Y.Map)) {{ tags = new Y.Map(); properties.set('tags', tags); }}
let options = tags.get('options');
if (!(options instanceof Y.Array)) {{ options = new Y.Array(); tags.set('options', options); }}

const tagId = Math.random().toString(36).substring(2, 10);
const now = Date.now();
const opt = new Y.Map();
opt.set('id', tagId);
opt.set('value', '{name}');
opt.set('color', '{color}');
opt.set('createDate', now);
opt.set('updateDate', now);
options.push([opt]);

// Encode the new state
const update = Y.encodeStateAsUpdate(doc);
const newB64 = Buffer.from(update.buffer, update.byteOffset, update.byteLength).toString('base64');
return {{ tag: {{ id: tagId, value: '{name}', color: '{color}', create_date: now, update_date: now }}, state: newB64 }};
"#);
    let val = run_tag_op(scripts_dir, b64, &op)?;
    let tag: crate::tags::TagOption = serde_json::from_value(
        val.get("tag").cloned().unwrap_or_default()
    ).context("failed to parse created tag")?;
    let state = val.get("state")
        .and_then(|v| v.as_str())
        .map(String::from)
        .ok_or_else(|| anyhow!("missing state in create_tag response"))?;
    Ok((tag, state))
}

/// Assign a tag to a doc. Returns the new doc state as base64.
pub fn assign_tag_op(scripts_dir: &PathBuf, b64: &str, doc_id: &str, tag_id: &str) -> Result<String> {
    let op = format!(r#"
const meta = doc.getMap('meta');
let pages = meta.get('pages');
if (!(pages instanceof Y.Map)) {{ pages = new Y.Map(); meta.set('pages', pages); }}
let docEntry = pages.get('{doc_id}');
if (!(docEntry instanceof Y.Map)) {{ docEntry = new Y.Map(); pages.set('{doc_id}', docEntry); }}
let tags = docEntry.get('tags');
if (!(tags instanceof Y.Array)) {{ tags = new Y.Array(); docEntry.set('tags', tags); }}
let found = false;
tags.forEach(t => {{ if (t === '{tag_id}') found = true; }});
if (!found) tags.push(['{tag_id}']);

const update = Y.encodeStateAsUpdate(doc);
return Buffer.from(update.buffer, update.byteOffset, update.byteLength).toString('base64');
"#);
    let val = run_tag_op(scripts_dir, b64, &op)?;
    val.as_str().map(String::from).ok_or_else(|| anyhow!("assign_tag_op: expected string result"))
}

/// Remove a tag from a doc. Returns the new doc state as base64.
pub fn unassign_tag_op(scripts_dir: &PathBuf, b64: &str, doc_id: &str, tag_id: &str) -> Result<String> {
    let op = format!(r#"
const meta = doc.getMap('meta');
const pages = meta.get('pages');
if (!(pages instanceof Y.Map)) return '';
const docEntry = pages.get('{doc_id}');
if (!(docEntry instanceof Y.Map)) return '';
const tags = docEntry.get('tags');
if (!(tags instanceof Y.Array)) return '';
for (let i = 0; i < tags.length; i++) {{
  if (tags.get(i) === '{tag_id}') {{
    tags.delete(i, 1);
    break;
  }}
}}
const update = Y.encodeStateAsUpdate(doc);
return Buffer.from(update.buffer, update.byteOffset, update.byteLength).toString('base64');
"#);
    let val = run_tag_op(scripts_dir, b64, &op)?;
    val.as_str().map(String::from).ok_or_else(|| anyhow!("unassign_tag_op: expected string result"))
}

/// Delete a tag from the workspace. Returns the new doc state as base64.
pub fn delete_tag_op(scripts_dir: &PathBuf, b64: &str, tag_id: &str) -> Result<String> {
    let op = format!(r#"
const meta = doc.getMap('meta');
// Remove from options
const properties = meta.get('properties');
if (properties instanceof Y.Map) {{
  const tags = properties.get('tags');
  if (tags instanceof Y.Map) {{
    const options = tags.get('options');
    if (options instanceof Y.Array) {{
      for (let i = 0; i < options.length; i++) {{
        const opt = options.get(i);
        if (opt instanceof Y.Map && opt.get('id') === '{tag_id}') {{
          options.delete(i, 1);
          break;
        }}
      }}
    }}
  }}
}}
// Remove from all pages
const pages = meta.get('pages');
if (pages instanceof Y.Map) {{
  pages.forEach(page => {{
    if (page instanceof Y.Map) {{
      const pageTags = page.get('tags');
      if (pageTags instanceof Y.Array) {{
        for (let i = 0; i < pageTags.length; i++) {{
          if (pageTags.get(i) === '{tag_id}') {{
            pageTags.delete(i, 1);
            break;
          }}
        }}
      }}
    }}
  }});
}}
const update = Y.encodeStateAsUpdate(doc);
return Buffer.from(update.buffer, update.byteOffset, update.byteLength).toString('base64');
"#);
    let val = run_tag_op(scripts_dir, b64, &op)?;
    val.as_str().map(String::from).ok_or_else(|| anyhow!("delete_tag_op: expected string result"))
}
