use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

pub const TABLE_INFO_STRING: &str = "glitch-table";

/// A glitch-table fenced code block extracted from a markdown document.
///
/// In M1 we only locate the blocks and capture their raw JSON body; full schema
/// parsing (datatypes, formulas, view state) lands in M4.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlitchTableBlock {
    pub block_index: usize,
    pub raw_json: String,
}

pub fn extract_table_blocks(markdown: &str) -> Vec<GlitchTableBlock> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    let mut block_index = 0usize;

    for event in Parser::new(markdown) {
        match event {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(info))) => {
                if info.as_ref() == TABLE_INFO_STRING {
                    current = Some(String::new());
                }
            }
            Event::Text(text) => {
                if let Some(buf) = current.as_mut() {
                    buf.push_str(&text);
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(raw_json) = current.take() {
                    blocks.push(GlitchTableBlock {
                        block_index,
                        raw_json,
                    });
                    block_index += 1;
                }
            }
            _ => {}
        }
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_glitch_table_blocks_only() {
        let md = "# Notes\n\n```glitch-table\n{\"schema\":{},\"rows\":[]}\n```\n\n```rust\nfn main(){}\n```\n";
        let blocks = extract_table_blocks(md);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].block_index, 0);
        assert!(blocks[0].raw_json.contains("\"schema\""));
    }

    #[test]
    fn no_blocks_in_plain_markdown() {
        let md = "# hello\nno tables here";
        assert!(extract_table_blocks(md).is_empty());
    }
}
