//! Renders each page of the menu to PNG files, the bottom screen and what goes over the game on
//! the top one, to look at the interface without the handheld:
//! `cargo run --example ui_pages -- OUT_DIR`.

// The example draws pages; the interface's input handling is the program's.
#![allow(dead_code)]

#[path = "../src/ui/mod.rs"]
mod ui;

use ui::{Model, Nav, OptionRow, Page, SlotInfo, Ui};

fn row(key: &str, label: &str, here: Option<&str>, inherit: &str, shows: bool) -> OptionRow {
    OptionRow {
        key: key.into(),
        label: label.into(),
        here: here.map(str::to_owned),
        inherit: inherit.into(),
        values: vec![None],
        at: 0,
        wraps: true,
        use_hint: "use GB".into(),
        shows,
    }
}

fn save(path: std::path::PathBuf, px: &[u8]) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), ui::W as u32, ui::H as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.write_header().unwrap().write_image_data(px).unwrap();
}

fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| ".".into()));
    let picture: Vec<u8> = (0..160 * 144)
        .flat_map(|i| {
            let (x, y) = (i % 160, i / 160);
            let v = (((x / 8) + (y / 8)) % 4 * 60 + 40) as u8;
            [v, v / 2 + 60, 255 - v, 255]
        })
        .collect();
    let mut slots = vec![SlotInfo::default(); 7];
    for (n, made) in [(1, "Today 4:41 PM"), (2, "Yesterday"), (0, "Today 5:02 PM")] {
        slots[n] = SlotInfo {
            picture: Some(picture.clone()),
            made: made.into(),
            played: "1 h 23 min played".into(),
        };
    }
    let m = Model {
        title: "Super Mario Land 2".into(),
        console: "GB".into(),
        clock: "5:05 PM".into(),
        battery: Some(76),
        played: "24 min today".into(),
        slots,
        quick_slot: 2,
        fast: false,
        game_options: vec![
            row(
                "look",
                "Look",
                Some("Game Boy Pocket"),
                "Use GB (Game Boy (green))",
                true,
            ),
            row("lcd", "LCD texture", None, "Use default (On)", true),
            row("speed", "Timing", None, "Use default (Smooth)", false),
            row(
                "fast",
                "Fast-forward speed",
                Some("6x"),
                "Use default (4x)",
                false,
            ),
        ],
        console_options: vec![row(
            "look",
            "Look",
            Some("Game Boy (green)"),
            "Use default (Game Boy Color)",
            true,
        )],
        top_brightness: Some(60),
        bottom_brightness: Some(40),
        volume: Some(70),
        hour12: true,
        restart: Some("Restart on a Game Boy".into()),
        toast: None,
    };
    let mut ui = Ui::new();
    ui.open_menu();
    for (name, page, moves) in [
        ("play", Page::Play, 0),
        ("states", Page::States, 1),
        ("game", Page::Game, 0),
        ("game-timing", Page::Game, 2),
        ("console", Page::Console, 0),
        ("system", Page::System, 0),
    ] {
        ui.go(page);
        for _ in 0..moves {
            ui.nav(
                &m,
                if page == Page::States {
                    Nav::Right
                } else {
                    Nav::Down
                },
            );
        }
        let bottom = ui.render(&m).unwrap().to_vec();
        save(out.join(format!("{name}-bottom.png")), &bottom);
        if let Some(Some(top)) = ui.render_top(&m) {
            save(out.join(format!("{name}-top.png")), top);
        }
    }
}
