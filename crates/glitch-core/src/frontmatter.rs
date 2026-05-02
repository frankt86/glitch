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
}
