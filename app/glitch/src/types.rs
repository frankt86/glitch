//! Default note-type → emoji registry.
//!
//! In M2.75 this becomes user-customisable via `%APPDATA%\Glitch\types.toml`.
//! For M2.5 we bake in a sensible default set in the binary.

pub fn default_emoji(note_type: &str) -> Option<&'static str> {
    Some(match note_type.to_ascii_lowercase().as_str() {
        "meeting" => "🗓",
        "person" | "people" => "👤",
        "book" => "📚",
        "project" => "🚧",
        "article" => "📰",
        "idea" => "💡",
        "todo" | "task" => "✅",
        "log" | "journal" => "📋",
        "note" => "📄",
        "code" => "💻",
        "place" => "📍",
        "event" => "🎉",
        "question" => "❓",
        _ => return None,
    })
}
