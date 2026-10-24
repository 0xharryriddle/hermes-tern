use tern_sdk::{PrintOptions, ui};

pub fn render() -> Result<(), tern_sdk::Error> {
    tern_sdk::print(
        ui::col()
            .child(ui::text("Hermes / Tern"))
            .child(ui::text("Preview only — No agent is connected"))
            .child(
                ui::kv()
                    .item("Surface", "tern-sdk / Rust")
                    .item("Hermes runtime", "not connected")
                    .item("Peer actors", "not connected")
                    .item("Next gate", "review docs/architecture.md"),
            ),
        PrintOptions::new(),
    )
}
