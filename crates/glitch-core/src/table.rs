use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use rhai::{Dynamic, Engine, Scope};
use serde::{Deserialize, Serialize};

pub const TABLE_INFO_STRING: &str = "glitch-table";

// ---------------------------------------------------------------------------
// Legacy block extractor (kept for M1 compatibility)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// M4 Schema types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ColType {
    #[default]
    Text,
    Number,
    Date,
    Checkbox,
    Select,
    Formula,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NumberFormat {
    #[default]
    Plain,
    Money,
    Percent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColumnDef {
    pub name: String,
    #[serde(rename = "type", default)]
    pub col_type: ColType,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub expr: Option<String>,
    #[serde(default)]
    pub format: Option<NumberFormat>,
    /// Currency symbol for Money format (default "$")
    #[serde(default)]
    pub symbol: Option<String>,
    /// Decimal places for Money format (default 2)
    #[serde(default)]
    pub decimals: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Schema {
    pub columns: Vec<ColumnDef>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GlitchTable {
    pub schema: Schema,
    pub rows: Vec<Vec<serde_json::Value>>,
    /// Not serialised — tracks which ```glitch-table block this came from.
    #[serde(skip)]
    pub block_index: usize,
}

// ---------------------------------------------------------------------------
// Sanitize column names → valid rhai identifiers
// ---------------------------------------------------------------------------

fn sanitize_name(name: &str) -> String {
    let mut out = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
        if i == 0 && ch.is_ascii_digit() {
            // prefix digit start: prepend underscore (handled below)
        }
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if out.is_empty() {
        out.push('_');
    }
    out
}

// ---------------------------------------------------------------------------
// Rhai engine (shared, configured once)
// ---------------------------------------------------------------------------

fn build_engine() -> Engine {
    let mut engine = Engine::new();
    engine.set_max_operations(50_000);
    engine.set_max_expr_depths(32, 16);

    // Cross-type arithmetic: f64 op i64 and i64 op f64
    engine.register_fn("*", |a: f64, b: i64| a * b as f64);
    engine.register_fn("*", |a: i64, b: f64| a as f64 * b);
    engine.register_fn("+", |a: f64, b: i64| a + b as f64);
    engine.register_fn("+", |a: i64, b: f64| a as f64 + b);
    engine.register_fn("-", |a: f64, b: i64| a - b as f64);
    engine.register_fn("-", |a: i64, b: f64| a as f64 - b);
    engine.register_fn("/", |a: f64, b: i64| a / b as f64);
    engine.register_fn("/", |a: i64, b: f64| a as f64 / b);

    // Aggregate functions (operate on rhai::Array)
    engine.register_fn("SUM", |arr: rhai::Array| -> f64 {
        arr.iter()
            .filter_map(|v| dynamic_to_f64(v))
            .sum()
    });
    engine.register_fn("AVG", |arr: rhai::Array| -> f64 {
        let vals: Vec<f64> = arr.iter().filter_map(|v| dynamic_to_f64(v)).collect();
        if vals.is_empty() {
            0.0
        } else {
            vals.iter().sum::<f64>() / vals.len() as f64
        }
    });
    engine.register_fn("COUNT", |arr: rhai::Array| -> i64 { arr.len() as i64 });
    engine.register_fn("MIN", |arr: rhai::Array| -> f64 {
        arr.iter()
            .filter_map(|v| dynamic_to_f64(v))
            .fold(f64::INFINITY, f64::min)
            .max(f64::NEG_INFINITY) // return NEG_INFINITY if empty, then clamp below
            .max(0.0) // return 0 for empty
    });
    engine.register_fn("MAX", |arr: rhai::Array| -> f64 {
        arr.iter()
            .filter_map(|v| dynamic_to_f64(v))
            .fold(f64::NEG_INFINITY, f64::max)
            .min(f64::INFINITY)
            .max(0.0)
    });

    // IF function
    engine.register_fn("IF", |cond: bool, a: f64, b: f64| -> f64 {
        if cond { a } else { b }
    });

    engine
}

fn dynamic_to_f64(v: &Dynamic) -> Option<f64> {
    if let Some(n) = v.as_float().ok() {
        Some(n)
    } else if let Some(n) = v.as_int().ok() {
        Some(n as f64)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// GlitchTable impl
// ---------------------------------------------------------------------------

impl GlitchTable {
    /// Parse a JSON body from a ```glitch-table block.
    pub fn parse(raw_json: &str, block_index: usize) -> Result<Self, serde_json::Error> {
        let mut table: GlitchTable = serde_json::from_str(raw_json)?;
        table.block_index = block_index;
        let ncols = table.schema.columns.len();
        // Pad each row so it has ncols cells.
        for row in &mut table.rows {
            while row.len() < ncols {
                row.push(serde_json::Value::Null);
            }
        }
        Ok(table)
    }

    /// Serialize to pretty JSON, omitting block_index.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into())
    }

    /// Return the computed value for a cell (runs formula if needed).
    pub fn computed_value(&self, row_idx: usize, col_idx: usize) -> serde_json::Value {
        let col = match self.schema.columns.get(col_idx) {
            Some(c) => c,
            None => return serde_json::Value::Null,
        };
        let row = match self.rows.get(row_idx) {
            Some(r) => r,
            None => return serde_json::Value::Null,
        };

        if col.col_type != ColType::Formula {
            return row.get(col_idx).cloned().unwrap_or(serde_json::Value::Null);
        }

        let expr = match &col.expr {
            Some(e) => e.clone(),
            None => return serde_json::Value::Null,
        };

        // Build rhai scope with this row's column values.
        let engine = build_engine();
        let mut scope = Scope::new();

        // Preprocess: replace SUM(colname) → SUM(_col_<sanitized>)
        let mut processed_expr = expr.clone();
        for agg in &["SUM", "AVG", "COUNT", "MIN", "MAX"] {
            for (ci, c) in self.schema.columns.iter().enumerate() {
                let sname = sanitize_name(&c.name);
                let from = format!("{}({})", agg, c.name);
                let to = format!("{}(_col_{})", agg, sname);
                processed_expr = processed_expr.replace(&from, &to);
                // Also handle already-sanitized names
                let from2 = format!("{}({})", agg, sname);
                processed_expr = processed_expr.replace(&from2, &to);
                // Push column arrays for aggregation
                let col_arr: rhai::Array = self
                    .rows
                    .iter()
                    .filter_map(|r| {
                        r.get(ci).and_then(|v| json_to_dynamic(v))
                    })
                    .collect();
                let arr_name = format!("_col_{}", sname);
                scope.push(arr_name, col_arr);
            }
        }

        // Push individual row cell values.
        for (ci, c) in self.schema.columns.iter().enumerate() {
            if c.col_type == ColType::Formula {
                continue; // skip formula cols to avoid circular
            }
            let sname = sanitize_name(&c.name);
            let cell = row.get(ci).unwrap_or(&serde_json::Value::Null);
            match cell {
                serde_json::Value::Number(n) => {
                    scope.push(sname, n.as_f64().unwrap_or(0.0));
                }
                serde_json::Value::Bool(b) => {
                    scope.push(sname, *b);
                }
                serde_json::Value::String(s) => {
                    scope.push(sname, s.clone());
                }
                _ => {
                    scope.push(sname, 0.0_f64);
                }
            }
        }

        match engine.eval_with_scope::<Dynamic>(&mut scope, &processed_expr) {
            Ok(result) => {
                if let Some(f) = result.as_float().ok() {
                    serde_json::Value::Number(
                        serde_json::Number::from_f64(f)
                            .unwrap_or(serde_json::Number::from(0)),
                    )
                } else if let Some(i) = result.as_int().ok() {
                    serde_json::Value::Number(serde_json::Number::from(i))
                } else if let Some(b) = result.as_bool().ok() {
                    serde_json::Value::Bool(b)
                } else if let Some(s) = result.into_string().ok() {
                    serde_json::Value::String(s)
                } else {
                    serde_json::Value::String("#ERR".into())
                }
            }
            Err(_) => serde_json::Value::String("#ERR".into()),
        }
    }

    /// Return a formatted display string for a cell.
    pub fn cell_display(&self, row_idx: usize, col_idx: usize) -> String {
        let col = match self.schema.columns.get(col_idx) {
            Some(c) => c,
            None => return String::new(),
        };
        let value = self.computed_value(row_idx, col_idx);
        format_value(&value, col)
    }
}

/// Helper that converts a json Value to a rhai Dynamic (for array pushing).
fn json_to_dynamic(v: &serde_json::Value) -> Option<Dynamic> {
    match v {
        serde_json::Value::Number(n) => Some(Dynamic::from(n.as_f64().unwrap_or(0.0))),
        serde_json::Value::Bool(b) => Some(Dynamic::from(*b)),
        serde_json::Value::String(s) => Some(Dynamic::from(s.clone())),
        _ => None,
    }
}

/// Format a value according to the column definition.
pub fn format_value(v: &serde_json::Value, col: &ColumnDef) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::Bool(b) => {
            if *b { "✓".into() } else { String::new() }
        }
        serde_json::Value::String(s) => {
            if s == "#ERR" {
                "#ERR".into()
            } else {
                s.clone()
            }
        }
        serde_json::Value::Number(n) => {
            let f = n.as_f64().unwrap_or(0.0);
            let fmt = col.format.as_ref().unwrap_or(&NumberFormat::Plain);
            match fmt {
                NumberFormat::Plain => {
                    // Show as integer if no fractional part, else as float
                    if f.fract() == 0.0 && f.abs() < 1e15 {
                        format!("{}", f as i64)
                    } else {
                        format!("{}", f)
                    }
                }
                NumberFormat::Money => {
                    let symbol = col.symbol.as_deref().unwrap_or("$");
                    let decimals = col.decimals.unwrap_or(2) as usize;
                    format_money(f, symbol, decimals)
                }
                NumberFormat::Percent => {
                    format!("{}%", f * 100.0)
                }
            }
        }
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => String::new(),
    }
}

/// Format a number as money with thousands separator.
fn format_money(value: f64, symbol: &str, decimals: usize) -> String {
    let negative = value < 0.0;
    let abs_val = value.abs();
    let factor = 10f64.powi(decimals as i32);
    let rounded = (abs_val * factor).round() / factor;

    let int_part = rounded.floor() as u64;
    let frac_part = ((rounded - rounded.floor()) * factor).round() as u64;

    // Thousands-separated integer part
    let int_str = int_part.to_string();
    let mut with_commas = String::new();
    for (i, ch) in int_str.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            with_commas.push(',');
        }
        with_commas.push(ch);
    }
    let int_formatted: String = with_commas.chars().rev().collect();

    let sign = if negative { "-" } else { "" };
    if decimals == 0 {
        format!("{}{}{}", sign, symbol, int_formatted)
    } else {
        format!(
            "{}{}{}.{:0>width$}",
            sign,
            symbol,
            int_formatted,
            frac_part,
            width = decimals
        )
    }
}

// ---------------------------------------------------------------------------
// Parse all tables from a markdown document
// ---------------------------------------------------------------------------

pub fn parse_all_tables(markdown: &str) -> Vec<GlitchTable> {
    extract_table_blocks(markdown)
        .into_iter()
        .filter_map(|b| GlitchTable::parse(&b.raw_json, b.block_index).ok())
        .collect()
}

// ---------------------------------------------------------------------------
// replace_table_block — splice new JSON into the nth glitch-table block
// ---------------------------------------------------------------------------

pub fn replace_table_block(
    markdown: &str,
    block_index: usize,
    new_json: &str,
) -> Option<String> {
    // Ensure new_json ends with a newline before we splice it in.
    let body = if new_json.ends_with('\n') {
        new_json.to_string()
    } else {
        format!("{}\n", new_json)
    };

    // Walk the markdown line by line to find the nth ```glitch-table block.
    let lines: Vec<&str> = markdown.split('\n').collect();
    let mut in_block = false;
    let mut found_count = 0usize;
    let mut block_start: Option<usize> = None; // line index of the ``` opening line
    let mut block_end: Option<usize> = None;   // line index of the ``` closing line

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if !in_block {
            let trimmed = line.trim_start();
            if trimmed.starts_with("```glitch-table") {
                // Opening fence
                if found_count == block_index {
                    block_start = Some(i);
                    in_block = true;
                } else {
                    found_count += 1;
                    // skip past this block
                    let mut j = i + 1;
                    while j < lines.len() {
                        if lines[j].trim_start().starts_with("```") && lines[j].trim() == "```" {
                            i = j;
                            break;
                        }
                        j += 1;
                    }
                }
            }
        } else if block_start.is_some() {
            // We're inside the target block; look for closing fence
            if lines[i].trim_start() == "```" || lines[i].trim() == "```" {
                block_end = Some(i);
                break;
            }
        }
        i += 1;
    }

    let start = block_start?;
    let end = block_end?;

    // Reconstruct: everything up to and including the opening fence line,
    // then new body, then the closing fence, then the rest.
    let mut result = String::new();
    // Lines 0..=start (the opening ``` line)
    for line in &lines[..=start] {
        result.push_str(line);
        result.push('\n');
    }
    // New JSON body
    result.push_str(&body);
    // Closing fence line
    result.push_str(lines[end]);
    result.push('\n');
    // Everything after the closing fence
    // Rejoin: the original split('\\n') splits on \n; if there's a trailing \n
    // the last element will be "". We need to handle end+1..lines.len().
    for line in &lines[end + 1..] {
        result.push_str(line);
        result.push('\n');
    }
    // Remove the extra trailing newline if the original didn't have one.
    // The original was split by '\n'; if it ended with '\n' last element is "".
    // We've added \n after every line, so there's always an extra one at the end.
    // Remove it only if the original did NOT end with '\n'.
    if !markdown.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    Some(result)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_glitch_table_blocks_only() {
        let md = "# Notes\n\n```glitch-table\n{\"schema\":{\"columns\":[]},\"rows\":[]}\n```\n\n```rust\nfn main(){}\n```\n";
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

    #[test]
    fn parse_table_basic() {
        let json = r#"{
            "schema": {
                "columns": [
                    {"name":"task","type":"text"},
                    {"name":"hours","type":"number"}
                ]
            },
            "rows": [["wire claude", 6]]
        }"#;
        let table = GlitchTable::parse(json, 0).unwrap();
        assert_eq!(table.schema.columns.len(), 2);
        assert_eq!(table.rows.len(), 1);
        assert_eq!(table.block_index, 0);
    }

    #[test]
    fn parse_pads_short_rows() {
        let json = r#"{
            "schema": {
                "columns": [
                    {"name":"a","type":"text"},
                    {"name":"b","type":"text"},
                    {"name":"c","type":"text"}
                ]
            },
            "rows": [["only_one"]]
        }"#;
        let table = GlitchTable::parse(json, 0).unwrap();
        assert_eq!(table.rows[0].len(), 3);
        assert_eq!(table.rows[0][1], serde_json::Value::Null);
    }

    #[test]
    fn format_money() {
        let col = ColumnDef {
            name: "price".into(),
            col_type: ColType::Number,
            options: vec![],
            expr: None,
            format: Some(NumberFormat::Money),
            symbol: Some("$".into()),
            decimals: Some(2),
        };
        let v = serde_json::Value::Number(
            serde_json::Number::from_f64(1234.56).unwrap(),
        );
        assert_eq!(format_value(&v, &col), "$1,234.56");
    }

    #[test]
    fn format_percent() {
        let col = ColumnDef {
            name: "pct".into(),
            col_type: ColType::Number,
            options: vec![],
            expr: None,
            format: Some(NumberFormat::Percent),
            symbol: None,
            decimals: None,
        };
        let v = serde_json::Value::Number(
            serde_json::Number::from_f64(0.425).unwrap(),
        );
        let s = format_value(&v, &col);
        assert!(s.contains('%'), "expected percent sign in: {}", s);
    }

    #[test]
    fn checkbox_display() {
        let col = ColumnDef {
            name: "done".into(),
            col_type: ColType::Checkbox,
            options: vec![],
            expr: None,
            format: None,
            symbol: None,
            decimals: None,
        };
        assert_eq!(format_value(&serde_json::Value::Bool(true), &col), "✓");
        assert_eq!(format_value(&serde_json::Value::Bool(false), &col), "");
    }

    #[test]
    fn formula_eval_basic() {
        let json = r#"{
            "schema": {
                "columns": [
                    {"name":"hours","type":"number"},
                    {"name":"cost","type":"formula","expr":"hours * 75.0"}
                ]
            },
            "rows": [[6, null]]
        }"#;
        let table = GlitchTable::parse(json, 0).unwrap();
        let v = table.computed_value(0, 1);
        assert_eq!(v, serde_json::Value::Number(
            serde_json::Number::from_f64(450.0).unwrap()
        ));
    }

    #[test]
    fn formula_sum_aggregate() {
        let json = r#"{
            "schema": {
                "columns": [
                    {"name":"hours","type":"number"},
                    {"name":"total","type":"formula","expr":"SUM(hours)"}
                ]
            },
            "rows": [[2, null], [3, null], [5, null]]
        }"#;
        let table = GlitchTable::parse(json, 0).unwrap();
        let v = table.computed_value(0, 1);
        if let serde_json::Value::Number(n) = v {
            assert_eq!(n.as_f64().unwrap(), 10.0);
        } else {
            panic!("expected number");
        }
    }

    #[test]
    fn formula_error_returns_err_string() {
        let json = r#"{
            "schema": {
                "columns": [
                    {"name":"x","type":"number"},
                    {"name":"bad","type":"formula","expr":"x / 0.0 + undefined_var"}
                ]
            },
            "rows": [[1, null]]
        }"#;
        let table = GlitchTable::parse(json, 0).unwrap();
        let v = table.computed_value(0, 1);
        // Should be either a number or #ERR, not a panic
        match v {
            serde_json::Value::String(s) => assert_eq!(s, "#ERR"),
            serde_json::Value::Number(_) => {} // division by zero in rhai returns inf, ok
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn replace_table_block_basic() {
        let md = "# Title\n\n```glitch-table\n{\"old\":true}\n```\n\nMore text\n";
        let new_json = "{\"new\":true}";
        let result = replace_table_block(md, 0, new_json).unwrap();
        assert!(result.contains("\"new\":true"), "result: {}", result);
        assert!(!result.contains("\"old\":true"), "result: {}", result);
        assert!(result.contains("More text"));
    }

    #[test]
    fn replace_table_block_second_block() {
        let md = "```glitch-table\n{\"a\":1}\n```\n\n```glitch-table\n{\"b\":2}\n```\n";
        let result = replace_table_block(md, 1, "{\"b\":99}").unwrap();
        assert!(result.contains("\"a\":1"), "first block preserved: {}", result);
        assert!(result.contains("\"b\":99"), "second block updated: {}", result);
        assert!(!result.contains("\"b\":2"), "old second block gone: {}", result);
    }

    #[test]
    fn parse_all_tables_extracts_multiple() {
        let md = "```glitch-table\n{\"schema\":{\"columns\":[]},\"rows\":[]}\n```\n\n```glitch-table\n{\"schema\":{\"columns\":[]},\"rows\":[]}\n```\n";
        let tables = parse_all_tables(md);
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[0].block_index, 0);
        assert_eq!(tables[1].block_index, 1);
    }

    #[test]
    fn sanitize_name_basic() {
        assert_eq!(sanitize_name("hello"), "hello");
        assert_eq!(sanitize_name("my col"), "my_col");
        assert_eq!(sanitize_name("123abc"), "_123abc");
        assert_eq!(sanitize_name("a-b"), "a_b");
    }
}
