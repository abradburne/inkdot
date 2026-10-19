//! The game options: what they are (inkdot's description file), what they're set to and where
//! that comes from.
//!
//! A value comes from, first to last: a change made in inkdot's own menu during this run, the
//! command line (DSlot passes its settings for the game there), inkdot's settings file (where
//! changes made in its menu are kept, for the game or for its console), the option's default.

use crate::args::description;
use nibkit::dslot::EmuOption;
use nibkit::settings::{Kind, Scope, Settings, Value};
use std::collections::BTreeMap;

/// Where a value came from, as the Options page says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Menu,
    DSlot,
    Game,
    Console,
    Default,
}

impl Source {
    pub fn label(self, console: &str) -> String {
        match self {
            Source::Menu | Source::Game => "this game".into(),
            Source::DSlot => "from DSlot".into(),
            Source::Console => format!("all {console}"),
            Source::Default => "default".into(),
        }
    }
}

/// The options for one game.
pub struct Options {
    defs: Vec<EmuOption>,
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
            defs: description().options,
            console: console.to_owned(),
            rom_name: rom_name.to_owned(),
            given,
            changed: BTreeMap::new(),
            settings,
        }
    }

    /// The options that apply to this game's console, in the description's order.
    pub fn defs(&self) -> impl Iterator<Item = &EmuOption> {
        self.defs.iter().filter(|o| o.applies(&self.console))
    }

    fn def(&self, key: &str) -> Option<&EmuOption> {
        self.defs
            .iter()
            .find(|o| o.def.key == format!("inkdot.{key}"))
    }

    fn full(key: &str) -> String {
        format!("inkdot.{key}")
    }

    /// An option's value and where it came from.
    pub fn get(&self, key: &str) -> (Value, Source) {
        if let Some(v) = self.changed.get(key) {
            return (v.clone(), Source::Menu);
        }
        if let Some(v) = self.given.get(key) {
            return (v.clone(), Source::DSlot);
        }
        let full = Self::full(key);
        let game = Scope::Game(Scope::game_key(&self.console, &self.rom_name));
        if let Some(v) = self.settings.get(&game, &full) {
            return (v.clone(), Source::Game);
        }
        if let Some(v) = self
            .settings
            .get(&Scope::Console(self.console.clone()), &full)
        {
            return (v.clone(), Source::Console);
        }
        let default = self
            .def(key)
            .and_then(|o| o.def.default.clone())
            .unwrap_or(Value::Boolean(false));
        (default, Source::Default)
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
        let Some(def) = self.def(key) else { return };
        if !def.def.accepts(&value) {
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
        match self.def(key).map(|o| &o.def.kind) {
            Some(Kind::Choice(c)) => c.clone(),
            Some(Kind::Toggle) => vec![
                (Value::Boolean(false), "Off".into()),
                (Value::Boolean(true), "On".into()),
            ],
            Some(Kind::Number {
                min,
                max,
                step,
                unit,
            }) => (*min..=*max)
                .step_by(*step as usize)
                .map(|n| (Value::Integer(n), format!("{n}{unit}")))
                .collect(),
            None => Vec::new(),
        }
    }

    /// How a value is shown.
    pub fn show(&self, key: &str, value: &Value) -> String {
        self.def(key)
            .map_or_else(String::new, |o| o.def.show(value))
    }

    pub fn label(&self, key: &str) -> String {
        self.def(key)
            .map_or_else(String::new, |o| o.def.label.clone())
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
    fn defaults_dslot_and_the_menu_in_order() {
        let mut o = options("order", &[("lcd", Value::Boolean(false))]);
        assert_eq!(o.get("look"), (Value::from("color"), Source::Default));
        assert_eq!(o.get("lcd"), (Value::Boolean(false), Source::DSlot));
        o.set("lcd", Value::Boolean(true), true);
        assert_eq!(o.get("lcd"), (Value::Boolean(true), Source::Menu));
        // A value the option doesn't take is ignored.
        o.set("look", "sepia".into(), true);
        assert_eq!(o.str("look"), "color");
    }

    #[test]
    fn only_this_consoles_options() {
        let o = options("console", &[]);
        let keys: Vec<String> = o.defs().map(|d| d.def.key.clone()).collect();
        assert!(keys.contains(&"inkdot.look".to_owned()));
        assert!(
            !keys.contains(&"inkdot.colors".to_owned()),
            "colors is for GBC games"
        );
        assert_eq!(o.values("fast").len(), 7);
    }
}
