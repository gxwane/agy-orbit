use crate::domain::orbit::OrbitIndex;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, Row, Table};

pub fn render_orbits_table(index: &OrbitIndex) {
    if index.orbits.is_empty() {
        println!("No orbits saved yet. Run `agyo save <orbit-name>` to create your first orbit.");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec![
            Cell::new("Active").fg(Color::Cyan),
            Cell::new("Orbit").fg(Color::Cyan),
            Cell::new("Email").fg(Color::Cyan),
            Cell::new("Label").fg(Color::Cyan),
            Cell::new("Last Used").fg(Color::Cyan),
        ]);

    for (name, record) in &index.orbits {
        let is_active = index.active_orbit.as_deref() == Some(name);
        let active_cell = if is_active {
            Cell::new("  * ").fg(Color::Green)
        } else {
            Cell::new("    ")
        };

        let name_cell = if is_active {
            Cell::new(name).fg(Color::Green)
        } else {
            Cell::new(name)
        };

        let last_used = record
            .last_used_at
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "Never".into());

        table.add_row(Row::from(vec![
            active_cell,
            name_cell,
            Cell::new(&record.email),
            Cell::new(record.label.as_deref().unwrap_or("-")),
            Cell::new(last_used),
        ]));
    }

    println!("{table}");
}
