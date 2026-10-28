//! RetroAchievements in play: the sign-in DSlot passes in the environment, the game's set
//! loaded and evaluated once a frame (`inkdot-cheevos`), and what happens said on stderr, where
//! DSlot's bug report picks it up. Softcore only; nothing here changes how the game runs.
//!
//! DSlot starts a game with `RA_TOKEN_FILE` (the token file: username, then token, one a
//! line), `RA_USERNAME` (for a one-line file) and `RA_HARDCORE` (`1` when the player chose
//! hardcore). Without `RA_TOKEN_FILE` there is no session and nothing is logged. The password
//! never reaches inkdot, and the token is never written or logged.

use inkdot_cheevos::{Client, Kind, Message};
use std::path::PathBuf;

/// The session, when the launcher passed a sign-in: signed in and the game loading, both on
/// their way; the outcome arrives through [`show`]. `rom` is the file as it is (not padded),
/// since the service's identity for a Game Boy game is the file's MD5.
pub fn start(rom: &[u8], color: bool, verbose: bool) -> Option<Client> {
    let token_file = PathBuf::from(std::env::var_os("RA_TOKEN_FILE").filter(|v| !v.is_empty())?);
    let mut credentials = match inkdot_cheevos::read_token_file(&token_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cheevos: {e}");
            return None;
        }
    };
    if credentials.username.is_empty() {
        credentials.username = std::env::var("RA_USERNAME").unwrap_or_default();
    }
    if credentials.username.is_empty() {
        eprintln!(
            "cheevos: {} has a token but no username, and RA_USERNAME is not set",
            token_file.display()
        );
        return None;
    }
    if std::env::var("RA_HARDCORE").is_ok_and(|v| v == "1") {
        eprintln!("cheevos: hardcore was asked for (RA_HARDCORE=1); inkdot plays softcore, hardcore is not supported yet");
    }
    let mut client = match Client::start(verbose) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cheevos: achievements unavailable: {e}");
            return None;
        }
    };
    eprintln!(
        "cheevos: using the token from {} (the launcher's sign-in)",
        token_file.display()
    );
    client.sign_in_with_token(&credentials);
    match inkdot_cheevos::hash(rom, color) {
        Some(hash) => client.load_game(&hash),
        None => eprintln!("cheevos: cannot identify this ROM: rcheevos could not hash it"),
    }
    Some(client)
}

/// Says what happened since the last frame, on stderr: `cheevos: <what> -- <detail>`, an unlock
/// with its points. The same shape as DSperate's lines, so DSlot's bug report reads the same
/// for both.
pub fn show(client: &mut Client) {
    for m in client.take_messages() {
        eprintln!("cheevos: {}", line(&m));
        // TODO(toast): an unlock should show on the top screen while the game plays. nibkit's
        // shared overlay (the pause menu's crate) has no toast yet; when it gets one, draw
        // `line(&m)` with it here, as DSperate shows its own. The bottom screen's toast
        // (`App::toast`) is dark in play, so it is no place for it.
    }
}

/// One line for a message: "Achievement unlocked: Title (5 points) -- description", or
/// "text -- detail".
pub fn line(m: &Message) -> String {
    let head = match m.kind {
        Kind::Unlock if m.points == 1 => format!("Achievement unlocked: {} (1 point)", m.text),
        Kind::Unlock => format!("Achievement unlocked: {} ({} points)", m.text, m.points),
        Kind::Info | Kind::Problem => m.text.clone(),
    };
    if m.detail.is_empty() {
        head
    } else {
        format!("{head} -- {}", m.detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_read_as_dsperate_logs_them() {
        let m = |kind, text: &str, detail: &str, points| Message {
            kind,
            text: text.into(),
            detail: detail.into(),
            points,
        };
        assert_eq!(
            line(&m(Kind::Info, "Tetris", "achievements active", 0)),
            "Tetris -- achievements active"
        );
        assert_eq!(
            line(&m(Kind::Unlock, "First Line", "Clear a line", 5)),
            "Achievement unlocked: First Line (5 points) -- Clear a line"
        );
        assert_eq!(
            line(&m(Kind::Unlock, "Warm-up", "", 1)),
            "Achievement unlocked: Warm-up (1 point)"
        );
        assert_eq!(
            line(&m(Kind::Problem, "RetroAchievements offline", "", 0)),
            "RetroAchievements offline"
        );
    }
}
