use dioxus::prelude::*;
use glitch_core::{ColType, ColumnDef, GlitchTable};

/// Interactive grid view for a single glitch-table block.
#[component]
pub fn GlitchTableView(table: GlitchTable, on_change: EventHandler<GlitchTable>) -> Element {
    let mut sort_col: Signal<Option<usize>> = use_signal(|| None);
    let mut sort_asc: Signal<bool> = use_signal(|| true);
    let mut filter: Signal<String> = use_signal(String::new);
    let mut editing: Signal<Option<(usize, usize)>> = use_signal(|| None);
    let edit_value: Signal<String> = use_signal(String::new);

    // Add-column form state
    let mut add_col_open: Signal<bool> = use_signal(|| false);
    let mut add_col_name: Signal<String> = use_signal(String::new);
    let mut add_col_type: Signal<String> = use_signal(|| "text".to_string());

    let ncols = table.schema.columns.len();

    // Build the visible_rows list: sorted + filtered indices into table.rows
    let filter_str = filter.read().to_lowercase();
    let mut visible_rows: Vec<usize> = (0..table.rows.len())
        .filter(|&ri| {
            if filter_str.is_empty() {
                return true;
            }
            (0..ncols).any(|ci| {
                table
                    .cell_display(ri, ci)
                    .to_lowercase()
                    .contains(&filter_str)
            })
        })
        .collect();

    if let Some(sc) = *sort_col.read() {
        let col = &table.schema.columns[sc];
        let is_numeric = matches!(col.col_type, ColType::Number | ColType::Formula);
        let asc = *sort_asc.read();
        visible_rows.sort_by(|&a, &b| {
            if is_numeric {
                let fa = table.computed_value(a, sc);
                let fb = table.computed_value(b, sc);
                let na = fa.as_f64().unwrap_or(0.0);
                let nb = fb.as_f64().unwrap_or(0.0);
                let cmp = na.partial_cmp(&nb).unwrap_or(std::cmp::Ordering::Equal);
                if asc { cmp } else { cmp.reverse() }
            } else {
                let sa = table.cell_display(a, sc);
                let sb = table.cell_display(b, sc);
                let cmp = sa.cmp(&sb);
                if asc { cmp } else { cmp.reverse() }
            }
        });
    }

    let add_row = {
        let mut table_clone = table.clone();
        let on_change_clone = on_change.clone();
        move |_| {
            let new_row = vec![serde_json::Value::Null; table_clone.schema.columns.len()];
            table_clone.rows.push(new_row);
            on_change_clone.call(table_clone.clone());
        }
    };

    rsx! {
        div { class: "gtable-wrap",
            div { class: "gtable-toolbar",
                input {
                    class: "gtable-filter",
                    r#type: "text",
                    placeholder: "Filter…",
                    value: "{filter}",
                    oninput: move |evt| filter.set(evt.value()),
                }
                button {
                    class: "gtable-add-btn",
                    onclick: add_row,
                    "+ Row"
                }
                button {
                    class: "gtable-add-btn",
                    onclick: move |_| {
                        add_col_name.set(String::new());
                        add_col_type.set("text".to_string());
                        let cur = *add_col_open.read();
                        add_col_open.set(!cur);
                    },
                    "+ Column"
                }
            }

            if *add_col_open.read() {
                div { class: "gtable-add-col-form",
                    input {
                        class: "gtable-col-name-input",
                        r#type: "text",
                        placeholder: "Column name",
                        value: "{add_col_name}",
                        oninput: move |evt| add_col_name.set(evt.value()),
                        onkeydown: {
                            let table_kd = table.clone();
                            let on_change_kd = on_change.clone();
                            move |evt: KeyboardEvent| {
                                if evt.key() == Key::Enter {
                                    let name = add_col_name.read().clone();
                                    let typ = add_col_type.read().clone();
                                    commit_add_col(
                                        &name,
                                        &typ,
                                        &table_kd,
                                        &on_change_kd,
                                        &mut add_col_open,
                                        &mut add_col_name,
                                    );
                                } else if evt.key() == Key::Escape {
                                    add_col_open.set(false);
                                }
                            }
                        },
                    }
                    select {
                        class: "gtable-col-type-select",
                        value: "{add_col_type}",
                        onchange: move |evt| add_col_type.set(evt.value()),
                        option { value: "text", "Text" }
                        option { value: "number", "Number" }
                        option { value: "date", "Date" }
                        option { value: "checkbox", "Checkbox" }
                        option { value: "select", "Select" }
                        option { value: "formula", "Formula (=expr)" }
                    }
                    button {
                        class: "btn",
                        onclick: {
                            let table_add = table.clone();
                            let on_change_add = on_change.clone();
                            move |_| {
                                let name = add_col_name.read().clone();
                                let typ = add_col_type.read().clone();
                                commit_add_col(
                                    &name,
                                    &typ,
                                    &table_add,
                                    &on_change_add,
                                    &mut add_col_open,
                                    &mut add_col_name,
                                );
                            }
                        },
                        "Add"
                    }
                    button {
                        class: "gtable-cancel-btn",
                        onclick: move |_| add_col_open.set(false),
                        "Cancel"
                    }
                }
            }

            div { class: "gtable-scroll",
                table { class: "gtable",
                    thead {
                        tr {
                            for (ci, col) in table.schema.columns.iter().enumerate() {
                                {
                                    let col_name = col.name.clone();
                                    let indicator = if *sort_col.read() == Some(ci) {
                                        if *sort_asc.read() { " ↑" } else { " ↓" }
                                    } else { "" };
                                    let table_del_col = table.clone();
                                    let on_change_del_col = on_change.clone();
                                    rsx! {
                                        th {
                                            class: "gtable-th",
                                            key: "{ci}",
                                            span {
                                                class: "gtable-th-label",
                                                onclick: move |_| {
                                                    if *sort_col.read() == Some(ci) {
                                                        let cur = *sort_asc.read();
                                                        sort_asc.set(!cur);
                                                    } else {
                                                        sort_col.set(Some(ci));
                                                        sort_asc.set(true);
                                                    }
                                                },
                                                "{col_name}{indicator}"
                                            }
                                            button {
                                                class: "gtable-th-del-btn",
                                                title: "Delete column",
                                                onclick: move |evt| {
                                                    evt.stop_propagation();
                                                    let mut updated = table_del_col.clone();
                                                    updated.schema.columns.remove(ci);
                                                    for row in &mut updated.rows {
                                                        if ci < row.len() {
                                                            row.remove(ci);
                                                        }
                                                    }
                                                    editing.set(None);
                                                    if *sort_col.read() == Some(ci) {
                                                        sort_col.set(None);
                                                    }
                                                    on_change_del_col.call(updated);
                                                },
                                                "×"
                                            }
                                        }
                                    }
                                }
                            }
                            th { class: "gtable-th-del" }
                        }
                    }
                    tbody {
                        for (display_idx, &row_idx) in visible_rows.iter().enumerate() {
                            TableRow {
                                key: "{row_idx}",
                                table: table.clone(),
                                row_idx,
                                display_idx,
                                editing,
                                edit_value,
                                on_change: on_change.clone(),
                            }
                        }
                    }
                }
            }
        }
    }
}

fn commit_add_col(
    name: &str,
    col_type_str: &str,
    table: &GlitchTable,
    on_change: &EventHandler<GlitchTable>,
    open: &mut Signal<bool>,
    name_sig: &mut Signal<String>,
) {
    let name = name.trim().to_string();
    if name.is_empty() {
        return;
    }
    let col_type = match col_type_str {
        "number" => ColType::Number,
        "date" => ColType::Date,
        "checkbox" => ColType::Checkbox,
        "select" => ColType::Select,
        "formula" => ColType::Formula,
        _ => ColType::Text,
    };
    let expr = if col_type == ColType::Formula {
        Some(String::new())
    } else {
        None
    };
    let mut updated = table.clone();
    updated.schema.columns.push(ColumnDef {
        name,
        col_type,
        options: vec![],
        expr,
        format: None,
        symbol: None,
        decimals: None,
    });
    for row in &mut updated.rows {
        row.push(serde_json::Value::Null);
    }
    open.set(false);
    name_sig.set(String::new());
    on_change.call(updated);
}

/// A single table row component, extracted to keep the lifetime/borrow graph clean.
#[component]
fn TableRow(
    table: GlitchTable,
    row_idx: usize,
    display_idx: usize,
    editing: Signal<Option<(usize, usize)>>,
    edit_value: Signal<String>,
    on_change: EventHandler<GlitchTable>,
) -> Element {
    let ncols = table.schema.columns.len();

    rsx! {
        tr { class: "gtable-row",
            for ci in 0..ncols {
                TableCell {
                    key: "{ci}",
                    table: table.clone(),
                    row_idx,
                    col_idx: ci,
                    display_idx,
                    editing,
                    edit_value,
                    on_change: on_change.clone(),
                }
            }
            // Delete row button
            td { class: "gtable-td-del",
                {
                    let table_del = table.clone();
                    let on_change_del = on_change.clone();
                    rsx! {
                        button {
                            class: "gtable-del-btn",
                            onclick: move |_| {
                                let mut updated = table_del.clone();
                                if row_idx < updated.rows.len() {
                                    updated.rows.remove(row_idx);
                                }
                                editing.set(None);
                                on_change_del.call(updated);
                            },
                            "✕"
                        }
                    }
                }
            }
        }
    }
}

/// A single table cell component.
#[component]
fn TableCell(
    table: GlitchTable,
    row_idx: usize,
    col_idx: usize,
    display_idx: usize,
    editing: Signal<Option<(usize, usize)>>,
    edit_value: Signal<String>,
    on_change: EventHandler<GlitchTable>,
) -> Element {
    let col = &table.schema.columns[col_idx];
    let col_type = col.col_type.clone();
    let col_options = col.options.clone();
    let is_formula = col_type == ColType::Formula;
    let is_editing = *editing.read() == Some((display_idx, col_idx));
    let display = table.cell_display(row_idx, col_idx);
    let raw_val = table
        .rows
        .get(row_idx)
        .and_then(|r| r.get(col_idx))
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let td_class = if is_formula {
        "gtable-td gtable-td-formula"
    } else {
        "gtable-td"
    };

    // onclick handler — enter edit mode or toggle checkbox
    let onclick = {
        let table_click = table.clone();
        let raw_val_click = raw_val.clone();
        let col_type_click = col_type.clone();
        let on_change_click = on_change.clone();
        move |_| {
            if col_type_click == ColType::Formula {
                return;
            }
            if col_type_click == ColType::Checkbox {
                let new_bool = !raw_val_click.as_bool().unwrap_or(false);
                let mut updated = table_click.clone();
                if let Some(row) = updated.rows.get_mut(row_idx) {
                    if let Some(cell) = row.get_mut(col_idx) {
                        *cell = serde_json::Value::Bool(new_bool);
                    }
                }
                on_change_click.call(updated);
                return;
            }
            let init = match &raw_val_click {
                serde_json::Value::Null => String::new(),
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                _ => String::new(),
            };
            edit_value.set(init);
            editing.set(Some((display_idx, col_idx)));
        }
    };

    rsx! {
        td {
            class: "{td_class}",
            onclick,

            if is_editing && !is_formula {
                CellEditor {
                    table: table.clone(),
                    row_idx,
                    col_idx,
                    col_type: col_type.clone(),
                    col_options: col_options.clone(),
                    editing,
                    edit_value,
                    on_change: on_change.clone(),
                }
            } else {
                span { "{display}" }
            }
        }
    }
}

/// The edit-mode widget for a cell.
#[component]
fn CellEditor(
    table: GlitchTable,
    row_idx: usize,
    col_idx: usize,
    col_type: ColType,
    col_options: Vec<String>,
    editing: Signal<Option<(usize, usize)>>,
    edit_value: Signal<String>,
    on_change: EventHandler<GlitchTable>,
) -> Element {
    // Commit helper: parse value, update table, fire on_change
    let col_type_for_commit = col_type.clone();
    let do_commit = move |val: String| {
        let new_cell = parse_cell_value(&val, &col_type_for_commit);
        let mut updated = table.clone();
        if let Some(row) = updated.rows.get_mut(row_idx) {
            if let Some(cell) = row.get_mut(col_idx) {
                *cell = new_cell;
            }
        }
        editing.set(None);
        on_change.call(updated);
    };

    match col_type {
        ColType::Select => rsx! {
            select {
                class: "gtable-cell-select",
                value: "{edit_value}",
                onchange: {
                    let mut commit = do_commit.clone();
                    move |evt: FormEvent| commit(evt.value())
                },
                onblur: {
                    let mut commit = do_commit.clone();
                    let ev = edit_value.read().clone();
                    move |_| commit(ev.clone())
                },
                for opt in col_options {
                    option { value: "{opt}", "{opt}" }
                }
            }
        },
        ColType::Number => rsx! {
            input {
                class: "gtable-cell-input",
                r#type: "number",
                value: "{edit_value}",
                oninput: move |evt| edit_value.set(evt.value()),
                onblur: {
                    let mut commit = do_commit.clone();
                    let ev = edit_value.read().clone();
                    move |_| commit(ev.clone())
                },
                onkeydown: {
                    let mut commit = do_commit.clone();
                    move |evt: KeyboardEvent| {
                        if evt.key() == Key::Enter {
                            commit(edit_value.read().clone());
                        } else if evt.key() == Key::Escape {
                            editing.set(None);
                        }
                    }
                },
            }
        },
        ColType::Date => rsx! {
            input {
                class: "gtable-cell-input",
                r#type: "date",
                value: "{edit_value}",
                oninput: move |evt| edit_value.set(evt.value()),
                onblur: {
                    let mut commit = do_commit.clone();
                    let ev = edit_value.read().clone();
                    move |_| commit(ev.clone())
                },
                onkeydown: {
                    let mut commit = do_commit.clone();
                    move |evt: KeyboardEvent| {
                        if evt.key() == Key::Enter {
                            commit(edit_value.read().clone());
                        } else if evt.key() == Key::Escape {
                            editing.set(None);
                        }
                    }
                },
            }
        },
        _ => rsx! {
            input {
                class: "gtable-cell-input",
                r#type: "text",
                value: "{edit_value}",
                oninput: move |evt| edit_value.set(evt.value()),
                onblur: {
                    let mut commit = do_commit.clone();
                    let ev = edit_value.read().clone();
                    move |_| commit(ev.clone())
                },
                onkeydown: {
                    let mut commit = do_commit.clone();
                    move |evt: KeyboardEvent| {
                        if evt.key() == Key::Enter {
                            commit(edit_value.read().clone());
                        } else if evt.key() == Key::Escape {
                            editing.set(None);
                        }
                    }
                },
            }
        },
    }
}

/// Parse a string input back to a serde_json::Value for the given column type.
fn parse_cell_value(s: &str, col_type: &ColType) -> serde_json::Value {
    if s.is_empty() {
        return serde_json::Value::Null;
    }
    match col_type {
        ColType::Number => {
            if let Ok(f) = s.parse::<f64>() {
                serde_json::Number::from_f64(f)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null)
            } else {
                serde_json::Value::Null
            }
        }
        ColType::Checkbox => serde_json::Value::Bool(
            matches!(s.to_lowercase().as_str(), "true" | "1" | "yes" | "✓"),
        ),
        _ => serde_json::Value::String(s.to_string()),
    }
}
