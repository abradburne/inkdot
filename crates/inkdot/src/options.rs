//! The game options: what they are, what they're set to and where that comes from.
//!
//! inkdot defines its options here, by itself: it runs without DSlot (`inkdot game.gbc`, or
//! from the stock OS's Apps). For DSlot it also ships a description file declaring the same
//! options (`packaging/nibos/inkdot.toml`), so DSlot can show them per console and per game and
//! pass them on the command line; a test (feature `dslot`) checks the two agree.
//!
//! A value comes from, first to last: a change made in inkdot's own menu during this run, the
//! command line, the settings file (for the game, then for its console), the option's default.
//! The settings file is DSlot's when inkdot is built for DSlot and DSlot has one, so its menu and
//! inkdot's show the same values; otherwise inkdot's own (`~/.config/inkdot/settings.toml` on
//! nibOS, beside the program on the stock OS). Either way the keys are `inkdot.<option>` at the
//! console level (`[console.GB.inkdot]`) and the game level, as DSlot keeps them.

use nibkit::settings::{Def, Kind, Levels, Scope, Settings, Value};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// What values an option takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Takes {
    /// One of these (value, label).
    Choice(&'static [(&'static str, &'static str)]),
    /// On or off: `--key` and `--no-key` on the command line.
    Toggle,
    /// A whole number in a range, shown with a unit.
    Number {
        min: i64,
        max: i64,
        unit: &'static str,
    },
}

/// An option's default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Default {
    Str(&'static str),
    Bool(bool),
    Int(i64),
}

/// One option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spec {
    pub key: &'static str,
    pub label: &'static str,
    /// The consoles it is for; empty for both.
    pub consoles: &'static [&'static str],
    pub takes: Takes,
    pub default: Default,
}

const PALETTES: &[(&str, &str)] = &[
    ("auto", "By title"),
    ("up", "Up"),
    ("a-up", "A + Up"),
    ("b-up", "B + Up"),
    ("left", "Left"),
    ("a-left", "A + Left"),
    ("b-left", "B + Left"),
    ("down", "Down"),
    ("a-down", "A + Down"),
    ("b-down", "B + Down"),
    ("right", "Right"),
    ("a-right", "A + Right"),
    ("b-right", "B + Right"),
];

/// Every option, in the order the menus show them.
pub const SPECS: &[Spec] = &[
    Spec {
        key: "look",
        label: "Look",
        consoles: &["GB"],
        takes: Takes::Choice(&[
            ("color", "Game Boy Color"),
            ("gameboy", "Game Boy (green)"),
            ("pocket", "Game Boy Pocket"),
            ("light", "Game Boy Light"),
            ("grey", "Plain grey"),
        ]),
        default: Default::Str("color"),
    },
    Spec {
        key: "palette",
        label: "Colours (Color look)",
        consoles: &["GB"],
        takes: Takes::Choice(PALETTES),
        default: Default::Str("auto"),
    },
    Spec {
        key: "colors",
        label: "Colours",
        consoles: &["GBC"],
        takes: Takes::Choice(&[("screen", "As on its screen"), ("raw", "As stored")]),
        default: Default::Str("screen"),
    },
    Spec {
        key: "lcd",
        label: "LCD texture",
        consoles: &[],
        takes: Takes::Toggle,
        default: Default::Bool(true),
    },
    Spec {
        key: "speed",
        label: "Timing",
        consoles: &[],
        takes: Takes::Choice(&[("panel", "Smooth (panel rate)"), ("exact", "Exact speed")]),
        default: Default::Str("panel"),
    },
    Spec {
        key: "bottom",
        label: "Bottom screen in play",
        consoles: &[],
        takes: Takes::Choice(&[
            ("dim", "Dim when idle"),
            ("off", "Off when idle"),
            ("on", "Always lit"),
        ]),
        default: Default::Str("dim"),
    },
    Spec {
        key: "resume",
        label: "Resume where I left off",
        consoles: &[],
        takes: Takes::Toggle,
        default: Default::Bool(true),
    },
    Spec {
        key: "fast",
        label: "Fast-forward speed",
        consoles: &[],
        takes: Takes::Number {
            min: 2,
            max: 8,
            unit: "x",
        },
        default: Default::Int(4),
    },
];

impl Spec {
    pub fn applies(&self, console: &str) -> bool {
        self.consoles.is_empty() || self.consoles.contains(&console)
    }

    pub fn default_value(&self) -> Value {
        match self.default {
            Default::Str(s) => Value::from(s),
            Default::Bool(b) => Value::Boolean(b),
            Default::Int(n) => Value::Integer(n),
        }
    }

    /// The option as a setting, keyed as DSlot keys an emulator's options.
    pub fn def(&self) -> Def {
        let kind = match self.takes {
            Takes::Choice(c) => Kind::choice(c),
            Takes::Toggle => Kind::Toggle,
            Takes::Number { min, max, unit } => Kind::Number {
                min,
                max,
                step: 1,
                unit: unit.into(),
            },
        };
        Def {
            key: format!("inkdot.{}", self.key),
            label: self.label.into(),
            kind,
            levels: Levels::CONSOLE_AND_GAME,
            default: Some(self.default_value()),
        }
    }

    /// Reads the option from the command line at `words`: how many words it took and its value.
    pub fn recognise(&self, words: &[String]) -> Option<Result<(usize, Value), String>> {
        let flag = format!("--{}", self.key);
        let first = words.first()?;
        if self.takes == Takes::Toggle {
            if *first == flag {
                return Some(Ok((1, Value::Boolean(true))));
            }
            return (*first == format!("--no-{}", self.key))
                .then_some(Ok((1, Value::Boolean(false))));
        }
        if *first != flag {
            return None;
        }
        let Some(word) = words.get(1) else {
            return Some(Err(format!("{flag} needs a value")));
        };
        let value = word
            .parse::<i64>()
            .map(Value::Integer)
            .unwrap_or_else(|_| Value::from(word.as_str()));
        if self.def().accepts(&value) {
            Some(Ok((2, value)))
        } else {
            Some(Err(format!("{flag} {word}: not one of its values")))
        }
    }
}

/// Where a value came from, as the Options page says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Menu,
    Launch,
    Game,
    Console,
    Default,
}

impl Source {
    pub fn label(self, console: &str) -> String {
        match self {
            Source::Menu | Source::Game => "this game".into(),
            Source::Launch => "set at launch".into(),
            Source::Console => format!("all {console}"),
            Source::Default => "default".into(),
        }
    }
}

/// The settings file: DSlot's when built for DSlot and DSlot has one, else inkdot's own.
pub fn settings_file() -> PathBuf {
    #[cfg(feature = "dslot")]
    if let Some(file) = nibkit::dslot::settings_file().filter(|f| f.exists()) {
        return file;
    }
    nibkit::paths::AppDirs::new("inkdot")
        .config
        .join("settings.toml")
}

/// The options for one game.
pub struct Options {
    console: String,
    rom_name: String,
    given: BTreeMap<String, Value>,
    changed: BTreeMap<String, Value>,
    pub settings: Settings,
}

impl Options {
    pub fn new(
        console: &str,
        rom_name: &str,
        given: BTreeMap<String, Value>,
        settings: Settings,
    ) -> Options {
        Options {
            console: console.to_owned(),
            rom_name: rom_name.to_owned(),
            given,
            changed: BTreeMap::new(),
            settings,
        }
    }

    /// The keys of the options for this game's console, in order.
    pub fn keys(&self) -> Vec<&'static str> {
        SPECS
            .iter()
            .filter(|s| s.applies(&self.console))
            .map(|s| s.key)
            .collect()
    }

    fn spec(key: &str) -> Option<&'static Spec> {
        SPECS.iter().find(|s| s.key == key)
    }

    fn full(key: &str) -> String {
        format!("inkdot.{key}")
    }

    /// An option's value and where it came from.
    pub fn get(&self, key: &str) -> (Value, Source) {
        let Some(spec) = Self::spec(key) else {
            return (Value::Boolean(false), Source::Default);
        };
        let def = spec.def();
        let ok = |v: &&Value| def.accepts(v);
        if let Some(v) = self.changed.get(key) {
            return (v.clone(), Source::Menu);
        }
        if let Some(v) = self.given.get(key).filter(ok) {
            return (v.clone(), Source::Launch);
        }
        let full = Self::full(key);
        let game = Scope::Game(Scope::game_key(&self.console, &self.rom_name));
        if let Some(v) = self.settings.get(&game, &full).filter(ok) {
            return (v.clone(), Source::Game);
        }
        if let Some(v) = self
            .settings
            .get(&Scope::Console(self.console.clone()), &full)
            .filter(ok)
        {
            return (v.clone(), Source::Console);
        }
        (spec.default_value(), Source::Default)
    }

    pub fn str(&self, key: &str) -> String {
        self.get(key).0.as_str().unwrap_or("").to_owned()
    }

    pub fn bool(&self, key: &str) -> bool {
        self.get(key).0.as_bool().unwrap_or(false)
    }

    pub fn int(&self, key: &str) -> i64 {
        self.get(key).0.as_integer().unwrap_or(0)
    }

    /// Changes an option from the menu, for the game or for its console, and keeps it.
    pub fn set(&mut self, key: &str, value: Value, for_game: bool) {
        let Some(spec) = Self::spec(key) else { return };
        if !spec.def().accepts(&value) {
            return;
        }
        let scope = if for_game {
            Scope::Game(Scope::game_key(&self.console, &self.rom_name))
        } else {
            Scope::Console(self.console.clone())
        };
        self.settings.set(scope, &Self::full(key), value.clone());
        self.changed.insert(key.to_owned(), value);
        if let Err(e) = self.settings.save() {
            eprintln!(
                "inkdot: cannot save settings to {}: {e}",
                self.settings.file().display()
            );
        }
    }

    /// An option's values with their labels, for stepping through them.
    pub fn values(&self, key: &str) -> Vec<(Value, String)> {
        match Self::spec(key).map(|s| s.takes) {
            Some(Takes::Choice(c)) => c
                .iter()
                .map(|(v, l)| (Value::from(*v), (*l).to_owned()))
                .collect(),
            Some(Takes::Toggle) => vec![
                (Value::Boolean(false), "Off".into()),
                (Value::Boolean(true), "On".into()),
            ],
            Some(Takes::Number { min, max, unit }) => (min..=max)
                .map(|n| (Value::Integer(n), format!("{n}{unit}")))
                .collect(),
            None => Vec::new(),
        }
    }

    /// How a value is shown.
    pub fn show(&self, key: &str, value: &Value) -> String {
        Self::spec(key).map_or_else(String::new, |s| s.def().show(value))
    }

    pub fn label(&self, key: &str) -> String {
        Self::spec(key).map_or_else(String::new, |s| s.label.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(name: &str, given: &[(&str, Value)]) -> Options {
        let file =
            std::env::temp_dir().join(format!("inkdot-options-{name}-{}.toml", std::process::id()));
        let _ = std::fs::remove_file(&file);
        let given = given
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect();
        Options::new("GB", "Tetris (World)", given, Settings::load(file))
    }

    #[test]
    fn defaults_the_launch_and_the_menu_in_order() {
        let mut o = options("order", &[("lcd", Value::Boolean(false))]);
        assert_eq!(o.get("look"), (Value::from("color"), Source::Default));
        assert_eq!(o.get("lcd"), (Value::Boolean(false), Source::Launch));
        o.set("lcd", Value::Boolean(true), true);
        assert_eq!(o.get("lcd"), (Value::Boolean(true), Source::Menu));
        // A value the option doesn't take is ignored.
        o.set("look", "sepia".into(), true);
        assert_eq!(o.str("look"), "color");
    }

    #[test]
    fn kept_settings_are_read_back_by_level() {
        let mut o = options("levels", &[]);
        o.set("fast", Value::Integer(6), false);
        let again = Options::new(
            "GB",
            "Other game",
            BTreeMap::new(),
            Settings::load(o.settings.file()),
        );
        assert_eq!(again.get("fast"), (Value::Integer(6), Source::Console));
    }

    #[test]
    fn only_this_consoles_options() {
        let o = options("console", &[]);
        assert!(o.keys().contains(&"look"));
        assert!(!o.keys().contains(&"colors"), "colors is for GBC games");
        assert_eq!(o.values("fast").len(), 7);
    }

    #[test]
    fn command_line_forms() {
        let w = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        let look = &SPECS[0];
        assert_eq!(
            look.recognise(&w("--look pocket x")),
            Some(Ok((2, Value::from("pocket"))))
        );
        assert!(matches!(look.recognise(&w("--look sepia")), Some(Err(_))));
        let lcd = SPECS.iter().find(|s| s.key == "lcd").unwrap();
        assert_eq!(
            lcd.recognise(&w("--no-lcd")),
            Some(Ok((1, Value::Boolean(false))))
        );
        assert_eq!(lcd.recognise(&w("--other")), None);
    }
}
