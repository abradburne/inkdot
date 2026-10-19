//! Renders each page of the bottom screen to PNG files, to look at the interface without the
//! handheld: `cargo run --example ui_pages -- OUT_DIR`.

// The example draws pages; the interface's input handling is the program's.
#![allow(dead_code)]

#[path = "../src/ui/mod.rs"]
mod ui;

use ui::{LookTile, Model, OptionRow, Page, SlotInfo, Ui};

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
    slots[1] = SlotInfo {
        picture: Some(picture.clone()),
        made: "Oct 19 12:40".into(),
        played: "1 h 23 min played".into(),
    };
    slots[3] = SlotInfo {
        picture: Some(picture),
        made: "Oct 18 21:02".into(),
        played: "58 min played".into(),
    };
    let looks = [
        (
            "",
            "Use GB",
            [
                [0x6f, 0xd8, 0xb8],
                [0x3c, 0xa3, 0x89],
                [0x1f, 0x67, 0x58],
                [0x0b, 0x2e, 0x2a],
            ],
        ),
        (
            "color",
            "Game Boy Color",
            [
                [0xff, 0xff, 0xff],
                [0x7b, 0xff, 0x31],
                [0x00, 0x63, 0xc5],
                [0, 0, 0],
            ],
        ),
        (
            "gameboy",
            "Game Boy",
            [
                [0xc4, 0xcc, 0x92],
                [0x87, 0x97, 0x62],
                [0x4b, 0x5c, 0x41],
                [0x22, 0x30, 0x29],
            ],
        ),
        (
            "pocket",
            "Game Boy Pocket",
            [
                [0xdc, 0xdb, 0xcf],
                [0xa2, 0xa1, 0x96],
                [0x5f, 0x5f, 0x58],
                [0x24, 0x25, 0x23],
            ],
        ),
        (
            "light",
            "Game Boy Light",
            [
                [0x6f, 0xd8, 0xb8],
                [0x3c, 0xa3, 0x89],
                [0x1f, 0x67, 0x58],
                [0x0b, 0x2e, 0x2a],
            ],
        ),
        (
            "grey",
            "Plain grey",
            [[0xff; 3], [0xaa; 3], [0x55; 3], [0; 3]],
        ),
    ]
    .into_iter()
    .map(|(v, l, s)| LookTile {
        value: v.into(),
        label: l.into(),
        swatch: s,
    })
    .collect();
    // A row set at this level (`here`), or showing where its value comes from.
    let row = |key: &str, label: &str, shown: &str, here: bool| OptionRow {
        key: key.into(),
        label: label.into(),
        shown: shown.into(),
        inherited: !here,
        values: vec![(None, shown.into())],
        at: 0,
        wraps: true,
        use_hint: "use GB".into(),
    };
    let m = Model {
        title: "Super Mario Land 2 - 6 Golden Coins (USA, Europe)".into(),
        clock: "12:41".into(),
        battery: Some(76),
        slots,
        quick_slot: 1,
        looks,
        look: "gameboy".into(),
        look_use: "use GB".into(),
        options: vec![
            row("lcd", "LCD texture", "Use default (On)", false),
            row("speed", "Timing", "Use default (Smooth)", false),
            row(
                "bottom",
                "Bottom screen in play",
                "Use GB (Dim when idle)",
                false,
            ),
            row(
                "resume",
                "Resume where I left off",
                "Use default (On)",
                false,
            ),
            row("fast", "Fast-forward speed", "6x", true),
        ],
        fast: false,
        top_brightness: Some(60),
        bottom_brightness: Some(40),
        volume: Some(70),
        for_game: true,
        console: "GB".into(),
        toast: None,
    };
    let mut ui = Ui::new();
    for (name, page, menu) in [
        ("play", Page::Play, false),
        ("play-menu", Page::Play, true),
        ("states", Page::States, false),
        ("look", Page::Look, false),
        ("options", Page::Options, false),
    ] {
        if menu {
            ui.open_menu()
        } else {
            ui.close_menu();
        }
        ui.go(page);
        let mut m = m.clone();
        if name == "play" {
            m.toast = Some("Saved to slot 1".into());
        }
        let px = ui.render(&m).unwrap().to_vec();
        let file = std::fs::File::create(out.join(format!("{name}.png"))).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), ui::W as u32, ui::H as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.write_header().unwrap().write_image_data(&px).unwrap();
    }
}
