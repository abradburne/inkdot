//! The command line: `inkdot [OPTIONS] [--saves DIR] [--boot-rom FILE] [--model dmg|cgb] ROM`.
//!
//! The game options (`--look gameboy`, `--no-lcd`, ...) are the ones inkdot declares to DSlot
//! in its description file, `packaging/nibos/inkdot.toml`, which is built in and read here the
//! same way DSlot reads it (nibkit's `dslot` module): what DSlot offers in its menu and passes
//! as `{options}` is exactly what inkdot understands.

use nibkit::dslot::{check, Emulator};
use nibkit::settings::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// inkdot's description for DSlot, as packaged.
pub const DESCRIPTION: &str = include_str!("../../../packaging/nibos/inkdot.toml");

/// The description, parsed.
pub fn description() -> Emulator {
    check("inkdot", DESCRIPTION).expect("packaging/nibos/inkdot.toml is a valid description")
}

/// Which console to be, when the game would run on either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelChoice {
    /// As the options say (the Look of a DMG game).
    Auto,
    /// A DMG, whatever the look.
    Dmg,
    /// A CGB.
    Cgb,
}

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    pub rom: PathBuf,
    /// The game's saves folder (DSlot makes it); else next to the ROM.
    pub saves: Option<PathBuf>,
    pub boot_rom: Option<PathBuf>,
    pub model: ModelChoice,
    /// Game options given, by their key in the description (`look`, `lcd`, ...).
    pub options: BTreeMap<String, Value>,
}

/// Usage, for `--help` and mistakes.
pub fn usage() -> String {
    let mut s = String::from(
        "usage: inkdot [OPTIONS] [--saves DIR] [--boot-rom FILE] [--model dmg|cgb] ROM\n\n\
         ROM is a .gb or .gbc file, or a .zip with one in it.\n\
         --saves DIR      where battery saves and save states go (default: next to the ROM)\n\
         --boot-rom FILE  run this boot ROM first (your own dump; none is included)\n\
         --model dmg|cgb  the console to be, overriding the look\n\nOptions:\n",
    );
    for o in &description().options {
        let key = o.def.key.trim_start_matches("inkdot.");
        let values = match &o.def.kind {
            nibkit::settings::Kind::Choice(c) => c
                .iter()
                .map(|(v, _)| v.as_str().unwrap_or("?").to_owned())
                .collect::<Vec<_>>()
                .join("|"),
            nibkit::settings::Kind::Toggle => format!("(--{key} / --no-{key})"),
            nibkit::settings::Kind::Number { min, max, .. } => format!("{min}..{max}"),
        };
        s += &format!("  {key:<8} {:<26} {values}\n", o.def.label);
    }
    s
}

/// Reads the arguments (without the program's name).
pub fn parse(words: &[String]) -> Result<Args, String> {
    let emulator = description();
    let mut args = Args {
        rom: PathBuf::new(),
        saves: None,
        boot_rom: None,
        model: ModelChoice::Auto,
        options: BTreeMap::new(),
    };
    let mut rom = None;
    let mut i = 0;
    while i < words.len() {
        let rest = &words[i..];
        if let Some((n, key, value)) = emulator.options.iter().find_map(|o| {
            o.recognise(rest)
                .map(|(n, v)| (n, o.def.key.trim_start_matches("inkdot.").to_owned(), v))
        }) {
            args.options.insert(key, value);
            i += n;
            continue;
        }
        let value = || {
            words
                .get(i + 1)
                .cloned()
                .ok_or(format!("{} needs a value", words[i]))
        };
        match words[i].as_str() {
            "--saves" => args.saves = Some(PathBuf::from(value()?)),
            "--boot-rom" => args.boot_rom = Some(PathBuf::from(value()?)),
            "--model" => {
                args.model = match value()?.as_str() {
                    "dmg" => ModelChoice::Dmg,
                    "cgb" => ModelChoice::Cgb,
                    other => return Err(format!("--model {other}: dmg or cgb")),
                }
            }
            w if w.starts_with("--") => return Err(format!("unknown option {w}")),
            w => {
                if rom.replace(PathBuf::from(w)).is_some() {
                    return Err("more than one ROM".into());
                }
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    args.rom = rom.ok_or("no ROM given")?;
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn dslots_command_line() {
        let a = parse(&words(
            "--look pocket --no-lcd --fast 6 --saves /s/GB/Tetris /r/Tetris.gb",
        ))
        .unwrap();
        assert_eq!(a.rom, PathBuf::from("/r/Tetris.gb"));
        assert_eq!(a.saves, Some(PathBuf::from("/s/GB/Tetris")));
        assert_eq!(a.options["look"], Value::from("pocket"));
        assert_eq!(a.options["lcd"], Value::Boolean(false));
        assert_eq!(a.options["fast"], Value::Integer(6));
    }

    #[test]
    fn mistakes_are_refused() {
        assert!(parse(&words("--look sepia a.gb")).is_err());
        assert!(parse(&words("--fast 20 a.gb")).is_err());
        assert!(parse(&words("a.gb b.gb")).is_err());
        assert!(parse(&words("--saves")).is_err());
        assert!(parse(&words("--lcd")).is_err());
    }

    #[test]
    fn every_option_is_in_the_usage() {
        let u = usage();
        for o in description().options {
            assert!(
                u.contains(o.def.key.trim_start_matches("inkdot.")),
                "{}",
                o.def.key
            );
        }
    }
}
