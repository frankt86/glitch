use serde::{Deserialize, Serialize};

/// YAML frontmatter recognised by Glitch. All fields are optional and
/// unrecognised keys are ignored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Frontmatter {
    #[serde(default)]
    pub title: Option<String>,
    /// Note type — drives icon + template lookup. e.g. "meeting", "book".
    #[serde(default, rename = "type")]
    pub note_type: Option<String>,
    /// Explicit emoji override. Falls back to type emoji, then default.
    #[serde(default)]
    pub icon: Option<String>,
    /// Keywords/tags. `tags` and `keywords` are aliases — merged on load.
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Explicit cross-note relations — relative paths within the vault.
    #[serde(default)]
    pub related: Vec<String>,
    #[serde(default)]
    pub created: Option<String>,
    /// Parent note — vault-relative path, bare filename, or title.
    #[serde(default)]
    pub parent: Option<String>,
}

impl Frontmatter {
    /// Tags and keywords merged + lowercased + deduplicated.
    pub fn all_keywords(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .tags
            .iter()
            .chain(self.keywords.iter())
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

/// Split a markdown document into its frontmatter (parsed) and body.
/// If the document does not start with `---\n`, returns an empty frontmatter
/// and the original content.
pub fn split(content: &str) -> (Frontmatter, &str) {
    let Some(rest) = content.strip_prefix("---\n").or_else(|| content.strip_prefix("---\r\n")) else {
        return (Frontmatter::default(), content);
    };
    // Find the closing `---` on its own line.
    let mut yaml_end = None;
    let mut idx = 0;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if trimmed == "---" {
            yaml_end = Some((idx, idx + line.len()));
            break;
        }
        idx += line.len();
    }
    let Some((yaml_end_excl, body_start)) = yaml_end else {
        return (Frontmatter::default(), content);
    };
    let yaml = &rest[..yaml_end_excl];
    let body = &rest[body_start..];
    let fm: Frontmatter = serde_yaml::from_str(yaml).unwrap_or_default();
    (fm, body)
}

// ---------------------------------------------------------------------------
// String-level YAML helpers (no serde, no schema, works on raw text)
// ---------------------------------------------------------------------------

/// Split a note into its raw YAML string and body. Returns `("", full_content)` if
/// there is no `---` fence. CRLF is normalised to LF before splitting.
pub fn split_raw(content: &str) -> (String, String) {
    let c = content.replace("\r\n", "\n");
    if let Some(after) = c.strip_prefix("---\n") {
        if let Some(pos) = after.find("\n---\n") {
            return (after[..pos].to_string(), after[pos + 5..].to_string());
        }
        if after.ends_with("\n---") {
            let pos = after.len() - 4;
            return (after[..pos].to_string(), String::new());
        }
    }
    (String::new(), c)
}

/// Recombine a raw YAML block and body into a full note. An empty `yaml`
/// returns `body` unchanged.
pub fn join_raw(yaml: &str, body: &str) -> String {
    if yaml.is_empty() {
        body.to_string()
    } else {
        format!("---\n{yaml}\n---\n{body}")
    }
}

/// Read one scalar field from a raw YAML block.
/// Uses `split_once(':')` so URLs with colons are handled correctly.
pub fn get_field(yaml: &str, key: &str) -> String {
    for line in yaml.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim() == key {
                let v = v.trim();
                if let Some(inner) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
                    return inner.to_string();
                }
                if let Some(inner) = v.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
                    return inner.to_string();
                }
                return v.to_string();
            }
        }
    }
    String::new()
}

/// Write (or append) one field in a raw YAML block.
pub fn set_field(yaml: &str, key: &str, value: &str) -> String {
    let formatted = format_value(key, value);
    let mut found = false;
    let mut lines: Vec<String> = yaml
        .lines()
        .map(|line| {
            if let Some((k, _)) = line.split_once(':') {
                if k.trim() == key {
                    found = true;
                    return format!("{key}: {formatted}");
                }
            }
            line.to_string()
        })
        .collect();
    if !found {
        lines.push(format!("{key}: {formatted}"));
    }
    lines.join("\n")
}

/// Format a value for inline YAML. Tags/keywords get list syntax; strings that
/// need quoting are double-quoted; everything else is passed through verbatim.
pub fn format_value(key: &str, value: &str) -> String {
    if key == "tags" || key == "keywords" {
        return str_to_tags(value);
    }
    if value.is_empty() {
        return "\"\"".to_string();
    }
    if value.contains(':')
        || value.contains('#')
        || value.starts_with('{')
        || value.starts_with('[')
        || value.starts_with('\'')
    {
        return scalar(value);
    }
    value.to_string()
}

/// Always-quoted YAML scalar: `hello "world"` → `"hello \"world\""`.
pub fn scalar(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// `"[tag1, tag2]"` → `"tag1, tag2"`;  `"[]"` → `""`
pub fn tags_to_str(raw: &str) -> String {
    let t = raw.trim();
    if t == "[]" || t.is_empty() {
        return String::new();
    }
    t.trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

/// `"tag1, tag2"` → `"[tag1, tag2]"`;  `""` → `"[]"`
pub fn str_to_tags(s: &str) -> String {
    let parts: Vec<&str> = s.split(',').map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
    if parts.is_empty() { "[]".into() } else { format!("[{}]", parts.join(", ")) }
}

/// Update one YAML field inside a full note (frontmatter + body). Creates a
/// minimal frontmatter block if none exists.
pub fn update_field(content: &str, key: &str, value: &str) -> String {
    let (yaml, body) = split_raw(content);
    let new_yaml = if yaml.is_empty() {
        format!("{key}: {}", format_value(key, value))
    } else {
        set_field(&yaml, key, value)
    };
    join_raw(&new_yaml, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_frontmatter_returns_empty() {
        let (fm, body) = split("# hello\nplain markdown");
        assert!(fm.title.is_none());
        assert_eq!(body, "# hello\nplain markdown");
    }

    #[test]
    fn parses_frontmatter() {
        let doc = "---\ntitle: Glitch\ntype: project\nicon: 🪲\ntags:\n  - rust\n  - ai\nkeywords: [vault]\nrelated:\n  - alice.md\n---\n\n# body\n";
        let (fm, body) = split(doc);
        assert_eq!(fm.title.as_deref(), Some("Glitch"));
        assert_eq!(fm.note_type.as_deref(), Some("project"));
        assert_eq!(fm.icon.as_deref(), Some("🪲"));
        assert_eq!(fm.tags, vec!["rust", "ai"]);
        assert_eq!(fm.keywords, vec!["vault"]);
        assert_eq!(fm.related, vec!["alice.md"]);
        assert_eq!(body, "\n# body\n");
        let kw = fm.all_keywords();
        assert_eq!(kw, vec!["ai", "rust", "vault"]);
    }

    #[test]
    fn unterminated_frontmatter_falls_back() {
        let doc = "---\ntitle: never closed\nplain body\n";
        let (fm, body) = split(doc);
        assert!(fm.title.is_none());
        assert_eq!(body, doc);
    }

    #[test]
    fn handles_crlf() {
        let doc = "---\r\ntitle: hi\r\n---\r\nbody\r\n";
        let (fm, _body) = split(doc);
        assert_eq!(fm.title.as_deref(), Some("hi"));
    }

    #[test]
    fn split_raw_basic() {
        let note = "---\ntitle: \"Hello\"\ntype: article\n---\n# Hello\n\nBody here.";
        let (yaml, body) = split_raw(note);
        assert_eq!(yaml, "title: \"Hello\"\ntype: article");
        assert_eq!(body, "# Hello\n\nBody here.");
    }

    #[test]
    fn get_field_handles_urls() {
        let yaml = "title: \"Test\"\nsource: https://example.com/page?q=1\ntype: article";
        assert_eq!(get_field(yaml, "source"), "https://example.com/page?q=1");
        assert_eq!(get_field(yaml, "title"), "Test");
    }

    #[test]
    fn tags_roundtrip() {
        assert_eq!(tags_to_str("[rust, dioxus]"), "rust, dioxus");
        assert_eq!(tags_to_str("[]"), "");
        assert_eq!(str_to_tags("rust, dioxus"), "[rust, dioxus]");
        assert_eq!(str_to_tags(""), "[]");
    }

    #[test]
    fn update_field_creates_frontmatter() {
        let plain = "# Hello\n\nno frontmatter";
        let result = update_field(plain, "title", "Hello");
        assert!(result.starts_with("---\n"));
        assert!(result.contains("title: Hello"));
    }
}
