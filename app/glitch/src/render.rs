use pulldown_cmark::{Options, Parser, html};

pub fn markdown_to_html(markdown: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_FOOTNOTES);
    opts.insert(Options::ENABLE_SMART_PUNCTUATION);

    let parser = Parser::new_ext(markdown, opts);
    let mut output = String::with_capacity(markdown.len() * 2);
    html::push_html(&mut output, parser);
    output
}

/// Build a standalone HTML document from a note title + full markdown content
/// (frontmatter is stripped before rendering). The result is a self-contained
/// file suitable for archiving or printing to PDF.
pub fn export_note_html(display_title: &str, full_content: &str) -> String {
    use glitch_core::frontmatter as fm;
    let (_, body) = fm::split_raw(full_content);
    let body_html = markdown_to_html(&body);
    let safe_title = html_escape(display_title);

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{safe_title}</title>
<style>
*,*::before,*::after{{box-sizing:border-box}}
body{{font-family:Georgia,'Times New Roman',serif;font-size:18px;line-height:1.7;color:#1a1a1a;background:#fff;margin:0;padding:0}}
article{{max-width:740px;margin:0 auto;padding:48px 32px}}
h1,h2,h3,h4,h5,h6{{font-family:'Segoe UI',system-ui,sans-serif;line-height:1.3;margin-top:1.5em;margin-bottom:0.5em}}
h1{{font-size:2em;margin-top:0;border-bottom:2px solid #e0e0e0;padding-bottom:0.3em}}
h2{{font-size:1.5em;border-bottom:1px solid #e0e0e0;padding-bottom:0.2em}}
h3{{font-size:1.25em}}
p{{margin:0 0 1em}}
a{{color:#2563eb}}
code{{font-family:'Cascadia Mono','Consolas',monospace;font-size:0.875em;background:#f4f4f4;border-radius:3px;padding:0.1em 0.4em}}
pre{{background:#f4f4f4;border-radius:6px;padding:1em;overflow-x:auto}}
pre code{{background:none;padding:0}}
blockquote{{border-left:4px solid #e0e0e0;margin:1em 0;padding:0.5em 1em;color:#555}}
table{{border-collapse:collapse;width:100%;margin:1em 0}}
th,td{{border:1px solid #ddd;padding:8px 12px;text-align:left}}
th{{background:#f4f4f4;font-weight:600}}
tr:nth-child(even){{background:#f9f9f9}}
ul,ol{{margin:0 0 1em;padding-left:2em}}
li{{margin-bottom:0.25em}}
hr{{border:none;border-top:1px solid #e0e0e0;margin:2em 0}}
img{{max-width:100%;height:auto;border-radius:4px}}
input[type=checkbox]{{margin-right:0.5em}}
@media print{{
  body{{font-size:12pt}}
  article{{padding:0}}
  a{{color:inherit;text-decoration:none}}
}}
</style>
</head>
<body>
<article>
<h1>{safe_title}</h1>
{body_html}
</article>
</body>
</html>
"#
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
