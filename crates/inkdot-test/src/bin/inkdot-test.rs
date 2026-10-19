//! `inkdot-test ROM... [--cgb] [--mooneye]`: runs test ROMs and prints each one's outcome.

use inkdot_core::Model;
use inkdot_test::{run, Outcome, Protocol};
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let model = if args.iter().any(|a| a == "--cgb") {
        Model::Cgb
    } else {
        Model::Dmg
    };
    let protocol = if args.iter().any(|a| a == "--mooneye") {
        Protocol::Mooneye
    } else {
        Protocol::Blargg
    };
    let mut failed = 0;
    for rom in args.iter().filter(|a| !a.starts_with("--")) {
        let outcome = run(Path::new(rom), model, protocol, 60);
        if outcome != Outcome::Pass {
            failed += 1;
        }
        println!("{outcome:?}\t{rom}");
    }
    std::process::exit(if failed == 0 { 0 } else { 1 });
}
