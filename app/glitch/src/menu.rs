use dioxus::desktop::muda::{
    accelerator::Accelerator, CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu,
};
use std::cell::RefCell;

pub const OPEN_VAULT: &str = "file.open_vault";
pub const NEW_VAULT: &str = "file.new_vault";
pub const SAVE: &str = "file.save";
pub const EXTRACT_URL: &str = "file.extract_url";
pub const DAILY_NOTE: &str = "file.daily_note";
pub const SETTINGS: &str = "file.settings";
pub const NOTES_PANEL: &str = "view.notes_panel";
pub const CLAUDE_PANEL: &str = "view.claude_panel";
pub const GRAPH: &str = "view.graph";
pub const SYNC_NOW: &str = "sync.now";
pub const BULK_OPS: &str = "file.bulk_ops";

thread_local! {
    static NOTES_CHECK: RefCell<Option<CheckMenuItem>> = const { RefCell::new(None) };
    static CLAUDE_CHECK: RefCell<Option<CheckMenuItem>> = const { RefCell::new(None) };
}

pub fn set_notes_panel_checked(checked: bool) {
    NOTES_CHECK.with(|c| {
        if let Some(item) = c.borrow().as_ref() {
            item.set_checked(checked);
        }
    });
}

pub fn set_claude_panel_checked(checked: bool) {
    CLAUDE_CHECK.with(|c| {
        if let Some(item) = c.borrow().as_ref() {
            item.set_checked(checked);
        }
    });
}

pub fn build_app_menu() -> Menu {
    let menu = Menu::new();

    // ── File ─────────────────────────────────────────────
    let file = Submenu::new("File", true);
    let open_vault = MenuItem::with_id(
        MenuId::from(OPEN_VAULT),
        "Open Vault…",
        true,
        Some("Ctrl+O".parse::<Accelerator>().expect("accel")),
    );
    let new_vault = MenuItem::with_id(MenuId::from(NEW_VAULT), "New Vault…", true, None);
    let save = MenuItem::with_id(
        MenuId::from(SAVE),
        "Save",
        true,
        Some("Ctrl+S".parse::<Accelerator>().expect("accel")),
    );
    let extract_url = MenuItem::with_id(MenuId::from(EXTRACT_URL), "Extract URL…", true, None);
    let bulk_ops = MenuItem::with_id(MenuId::from(BULK_OPS), "Bulk AI Ops…", true, None);
    let daily_note = MenuItem::with_id(
        MenuId::from(DAILY_NOTE),
        "Daily Note",
        true,
        Some("Ctrl+D".parse::<Accelerator>().expect("accel")),
    );
    let settings = MenuItem::with_id(MenuId::from(SETTINGS), "Settings…", true, None);
    file.append_items(&[
        &open_vault,
        &new_vault,
        &PredefinedMenuItem::separator(),
        &save,
        &PredefinedMenuItem::separator(),
        &daily_note,
        &extract_url,
        &bulk_ops,
        &PredefinedMenuItem::separator(),
        &settings,
    ])
    .ok();

    // ── View ─────────────────────────────────────────────
    let view = Submenu::new("View", true);
    let notes_check =
        CheckMenuItem::with_id(MenuId::from(NOTES_PANEL), "Notes Panel", true, true, None);
    let claude_check =
        CheckMenuItem::with_id(MenuId::from(CLAUDE_PANEL), "Claude Panel", true, true, None);
    let graph = MenuItem::with_id(
        MenuId::from(GRAPH),
        "Graph…",
        true,
        Some("Ctrl+G".parse::<Accelerator>().expect("accel")),
    );
    view.append_items(&[
        &notes_check,
        &claude_check,
        &PredefinedMenuItem::separator(),
        &graph,
    ])
    .ok();

    // ── Sync ─────────────────────────────────────────────
    let sync_sub = Submenu::new("Sync", true);
    let sync_now = MenuItem::with_id(MenuId::from(SYNC_NOW), "Sync Now", true, None);
    sync_sub.append(&sync_now).ok();

    menu.append_items(&[&file, &view, &sync_sub]).ok();

    NOTES_CHECK.with(|c| *c.borrow_mut() = Some(notes_check));
    CLAUDE_CHECK.with(|c| *c.borrow_mut() = Some(claude_check));

    menu
}
