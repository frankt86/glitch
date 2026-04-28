use pulldown_cmark::{Options, Parser, html};

#[allow(dead_code)]
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
