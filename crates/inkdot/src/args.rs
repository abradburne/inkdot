//! The command line: `inkdot [OPTIONS] [--saves DIR] [--boot-rom FILE] [--model dmg|cgb] [ROM]`.
//!
//! Without a ROM, inkdot shows the games it finds and lets the player pick one (as when it is
//! started from the stock OS's Apps). The game options (`--look gameboy`, `--no-lcd`, ...) are
//! those in [`crate::options::SPECS`]; DSlot passes the same ones, from its description of inkdot.

use crate::options::{Takes, SPECS};
use nibkit::settings::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Which console to be, when the game would run on either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelChoice {
    /// As the options say (the Look of a DMG game).
    Auto,
    Dmg,
    Cgb,
}

/// What the command line asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct Args {
    /// The game; `None` to pick one.
    pub rom: Option<PathBuf>,
    /// The game's saves folder (DSlot makes one); else the system's saves folder for it.
    pub saves: Option<PathBuf>,
    pub boot_rom: Option<PathBuf>,
    pub model: ModelChoice,
    /// Game options given, by key (`look`, `lcd`, ...).
    pub options: BTreeMap<String, Value>,
}

/// Usage, for `--help` and mistakes.
pub fn usage() -> String {
    let mut s = String::from(
        "usage: inkdot [OPTIONS] [--saves DIR] [--boot-rom FILE] [--model dmg|cgb] [ROM]\n\n\
         ROM is a .gb or .gbc file, or a .zip with one in it; without one, inkdot lists the\n\
         games it finds and you pick one.\n\
         --saves DIR      where battery saves and save states go (default: the system's saves\n\
         \x20                folder, <saves>/GB or GBC/<game>/)\n\
         --boot-rom FILE  run this boot ROM first (your own dump; none is included)\n\
         --model dmg|cgb  the console to be, overriding the look\n\nOptions:\n",
    );
    for o in SPECS {
        let values = match o.takes {
            Takes::Choice(c) => c.iter().map(|(v, _)| *v).collect::<Vec<_>>().join("|"),
            Takes::Toggle => format!("--{0} / --no-{0}", o.key),
            Takes::Number { min, max, .. } => format!("{min}..{max}"),
        };
        s += &format!("  --{:<8} {:<26} {values}\n", o.key, o.label);
    }
    s
}

/// Reads the arguments (without the program's name).
pub fn parse(words: &[String]) -> Result<Args, String> {
    let mut args = Args {
        rom: None,
        saves: None,
        boot_rom: None,
        model: ModelChoice::Auto,
        options: BTreeMap::new(),
    };
    let mut i = 0;
    while i < words.len() {
        let rest = &words[i..];
        if let Some(found) = SPECS
            .iter()
            .find_map(|o| o.recognise(rest).map(|r| (o.key, r)))
        {
            let (key, result) = found;
            let (n, value) = result?;
            args.options.insert(key.to_owned(), value);
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
                if args.rom.replace(PathBuf::from(w)).is_some() {
                    return Err("more than one ROM".into());
                }
                i += 1;
                continue;
            }
        }
        i += 2;
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn a_full_command_line() {
        let a = parse(&words(
            "--look pocket --no-lcd --fast 6 --saves /s/GB/Tetris /r/Tetris.gb",
        ))
        .unwrap();
        assert_eq!(a.rom, Some(PathBuf::from("/r/Tetris.gb")));
        assert_eq!(a.saves, Some(PathBuf::from("/s/GB/Tetris")));
        assert_eq!(a.options["look"], Value::from("pocket"));
        assert_eq!(a.options["lcd"], Value::Boolean(false));
        assert_eq!(a.options["fast"], Value::Integer(6));
    }

    #[test]
    fn no_rom_is_allowed() {
        assert_eq!(parse(&[]).unwrap().rom, None);
    }

    #[test]
    fn mistakes_are_refused() {
        assert!(parse(&words("--look sepia a.gb")).is_err());
        assert!(parse(&words("--fast 20 a.gb")).is_err());
        assert!(parse(&words("a.gb b.gb")).is_err());
        assert!(parse(&words("--saves")).is_err());
        assert!(parse(&words("--sepia a.gb")).is_err());
    }

    #[test]
    fn every_option_is_in_the_usage() {
        let u = usage();
        for o in SPECS {
            assert!(u.contains(&format!("--{}", o.key)), "{}", o.key);
        }
    }

    /// The description DSlot reads declares exactly inkdot's options, with the same values,
    /// defaults and command-line forms.
    #[cfg(feature = "dslot")]
    #[test]
    fn the_dslot_description_matches() {
        let text = include_str!("../../../packaging/nibos/inkdot.toml");
        let emulator = nibkit::dslot::check("inkdot", text).expect("a valid description");
        assert_eq!(emulator.options.len(), SPECS.len());
        for (o, spec) in emulator.options.iter().zip(SPECS) {
            assert_eq!(o.def, spec.def(), "{}", spec.key);
            for console in ["GB", "GBC"] {
                assert_eq!(
                    o.applies(console),
                    spec.applies(console),
                    "{} on {console}",
                    spec.key
                );
            }
            // DSlot's words for each value are what inkdot reads back.
            for (value, _) in crate::options::Options::new(
                "GB",
                "x",
                BTreeMap::new(),
                nibkit::settings::Settings::load(std::env::temp_dir().join("inkdot-desc.toml")),
            )
            .values(spec.key)
            {
                let w = o.words(&value);
                assert_eq!(
                    spec.recognise(&w),
                    Some(Ok((w.len(), value.clone()))),
                    "{} {value}",
                    spec.key
                );
            }
        }
    }
}
