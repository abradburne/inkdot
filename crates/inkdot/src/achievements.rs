//! RetroAchievements in play: the sign-in DSlot passes in the environment, the game's set
//! loaded and evaluated once a frame (nibkit's `nib-cheevos`), what happens said on stderr, where
//! DSlot's bug report picks it up, and worded for the notices over the game. Casual (softcore)
//! only; nothing here changes how the game runs.
//!
//! DSlot starts a game with `RA_TOKEN_FILE` (the token file: username, then token, one a
//! line), `RA_USERNAME` (for a one-line file) and `RA_HARDCORE` (`1` when the player chose
//! hardcore). Without `RA_TOKEN_FILE` there is no session and nothing is logged. The password
//! never reaches inkdot, and the token is never written or logged.

use crate::ui::{self, Picture, Ui};
use nib_cheevos::badges::{self, Badges};
use nib_cheevos::launch::Launch;
use nib_cheevos::{Achievement, Client, Event, Settings, Summary};
use std::collections::HashMap;
use std::path::Path;

/// The session, and what it shows over the game: the badges, fetched into the cache DSlot
/// shares.
pub struct Session {
    pub client: Client,
    badges: Option<Badges>,
    /// Badges decoded, by name.
    pictures: HashMap<String, Picture>,
}

impl Session {
    /// The events since the last frame, said: logged, and shown over the game.
    pub fn show(&mut self, ui: &mut Ui) {
        for (name, file) in self
            .badges
            .as_mut()
            .map(Badges::fetched)
            .unwrap_or_default()
        {
            let Some(pic) = decode(&file) else { continue };
            self.pictures.insert(name.clone(), pic.clone());
            // To the notices and indicators awaiting it.
            ui.badge(&name, pic);
        }
        for said in said(&mut self.client) {
            match said {
                Said::Nothing => {}
                Said::Notice(n) => {
                    let mut toast = n.toast();
                    if let Some(name) = &n.badge {
                        toast = self.with_badge(toast, name, &n.badge_url);
                    }
                    ui.notice(toast);
                }
                Said::Challenge(a) => {
                    let n = ui::toast::Notice::challenge(a.title.as_str());
                    ui.challenge(a.id, self.with_badge(n, &a.badge_name, &a.badge_url));
                }
                Said::ChallengeOver(id) => ui.challenge_over(id),
                Said::Progress(a) => {
                    let n =
                        ui::toast::Notice::progress(a.title.as_str(), a.measured_progress.as_str());
                    ui.progress(a.id, self.with_badge(n, &a.badge_name, &a.badge_url));
                }
                Said::ProgressOver => ui.hide_progress(),
            }
        }
    }

    /// `n` with the badge `name` when it is decoded or in the cache; else awaiting it while it
    /// is fetched (nibkit holds a notice or an indicator a moment for it).
    fn with_badge(&mut self, n: ui::toast::Notice, name: &str, url: &str) -> ui::toast::Notice {
        if name.is_empty() {
            return n;
        }
        match self.picture(name, url) {
            Some((rgba, w, h)) => n.badge(rgba, w, h),
            None if self.badges.is_some() => n.awaits_badge(name),
            None => n,
        }
    }

    /// The badge `name`, decoded, when it is in the cache; else it is fetched (from `url`, or
    /// the service's usual place), for next time.
    fn picture(&mut self, name: &str, url: &str) -> Option<Picture> {
        if let Some(p) = self.pictures.get(name) {
            return Some(p.clone());
        }
        let file = self.badges.as_mut()?.want(name, url)?;
        let pic = decode(&file)?;
        self.pictures.insert(name.to_owned(), pic.clone());
        Some(pic)
    }
}

/// A badge's PNG as RGBA.
fn decode(file: &Path) -> Option<Picture> {
    let data = std::fs::read(file).ok()?;
    let mut decoder = png::Decoder::new(&data[..]);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let px = &buf[..info.buffer_size()];
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => px
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return None,
    };
    (rgba.len() == w * h * 4).then_some((rgba, w, h))
}

/// The session, when the launcher passed a sign-in: signed in and the game loading, both on
/// their way; the outcome arrives as events ([`said`]). `rom` is the file as it is (not
/// padded), since the service's identity for a Game Boy game is the file's MD5.
pub fn start(rom: &[u8], color: bool, verbose: bool) -> Option<Session> {
    let launch = match Launch::from_env() {
        Ok(Some(launch)) => launch,
        Ok(None) => return None,
        Err(e) => {
            eprintln!("cheevos: {e}");
            return None;
        }
    };
    if launch.hardcore {
        eprintln!("cheevos: hardcore was asked for (RA_HARDCORE=1); inkdot plays Casual, hardcore is not supported yet");
    }
    let user_agent = nib_cheevos::ua::user_agent("inkdot", env!("CARGO_PKG_VERSION"), None);
    let mut client = match Client::start(&user_agent, Settings::default(), verbose) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("cheevos: achievements unavailable: {e}");
            return None;
        }
    };
    eprintln!(
        "cheevos: using the token from {} (the launcher's sign-in)",
        launch.token_file.display()
    );
    client.sign_in_with_token(&launch.credentials);
    let system = if color { "GBC" } else { "GB" };
    match nib_cheevos::hash::buffer(system, "", rom) {
        Ok(hash) => client.load_game(&hash),
        Err(e) => eprintln!("cheevos: cannot identify this ROM: {e}"),
    }
    let badges = badges::dir().and_then(|dir| Badges::start(dir, &user_agent));
    if badges.is_none() {
        eprintln!("cheevos: no badges: no cache folder or no curl");
    }
    Some(Session {
        client,
        badges,
        pictures: HashMap::new(),
    })
}

/// How a notice looks: its heading's colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Something to know.
    Info,
    /// An achievement earned, or the game completed: gold.
    Unlock,
    /// Something went wrong (play goes on): red.
    Problem,
}

/// A notice over the game, worded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub tone: Tone,
    pub header: String,
    pub title: String,
    pub detail: String,
    pub points: u32,
    /// The badge to show at the left, by the service's name for it, and where it is.
    pub badge: Option<String>,
    pub badge_url: String,
}

impl Notice {
    fn new(
        tone: Tone,
        header: &str,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Notice {
        Notice {
            tone,
            header: header.to_owned(),
            title: title.into(),
            detail: detail.into(),
            points: 0,
            badge: None,
            badge_url: String::new(),
        }
    }

    /// The notice as nibkit's toast queue takes it, for its kind's time (DSperate's four
    /// seconds for an unlock), without its badge.
    pub fn toast(&self) -> ui::toast::Notice {
        use ui::toast::Kind;
        let kind = match self.tone {
            Tone::Info => Kind::Info,
            Tone::Unlock => Kind::Unlock,
            Tone::Problem => Kind::Problem,
        };
        ui::toast::Notice::new(kind, self.title.as_str())
            .header(self.header.as_str())
            .detail(self.detail.as_str())
            .points(self.points)
    }
}

/// What an event changes over the game.
#[derive(Clone, Debug, PartialEq)]
pub enum Said {
    /// Nothing to show (it is logged).
    Nothing,
    /// A notice.
    Notice(Notice),
    /// An achievement can be earned right now: its badge stays up while the challenge lasts.
    Challenge(Achievement),
    /// The challenge for this achievement is over.
    ChallengeOver(u32),
    /// A measured achievement's progress, shown until rcheevos says it can go.
    Progress(Achievement),
    /// The progress can go.
    ProgressOver,
}

/// What the events since the last frame change over the game; each is said on stderr too:
/// `cheevos: <what> -- <detail>`, the same shape as DSperate's lines, so DSlot's bug report
/// reads the same for both.
fn said(client: &mut Client) -> Vec<Said> {
    client
        .take_events()
        .into_iter()
        .map(|e| {
            if let Some(line) = line(&e) {
                eprintln!("cheevos: {line}");
            }
            word(e)
        })
        .collect()
}

/// "Casual: 12 of 40 unlocked, 3 unsupported", for the placard.
fn standing(summary: &Summary, hardcore: bool) -> String {
    let mode = if hardcore { "Hardcore" } else { "Casual" };
    let mut s = format!("{mode}: {} of {} unlocked", summary.unlocked, summary.total);
    if summary.unsupported > 0 {
        s += &format!(", {} unsupported", summary.unsupported);
    }
    s
}

fn points(n: u32) -> String {
    if n == 1 {
        "1 point".to_owned()
    } else {
        format!("{n} points")
    }
}

/// An event in words, for the notice over the game.
pub fn word(e: Event) -> Said {
    use Tone::*;
    const RA: &str = "RetroAchievements";
    let notice = |tone, header, title: &str, detail: &str| {
        Said::Notice(Notice::new(tone, header, title, detail))
    };
    match e {
        // DSlot signed in already; the placard that follows says the rest.
        Event::SignedIn { .. } | Event::NoAchievements { .. } | Event::ResetAsked => Said::Nothing,
        Event::SignInFailed {
            token_refused: true,
            ..
        } => notice(
            Problem,
            RA,
            "Sign-in refused",
            "sign in again in DSlot's settings",
        ),
        Event::SignInFailed { message, .. } => notice(Problem, RA, "Sign-in failed", &message),
        Event::GameLoaded {
            title,
            summary,
            hardcore,
        } => notice(Info, RA, &title, &standing(&summary, hardcore)),
        Event::UnknownGame { .. } => notice(
            Problem,
            RA,
            "No achievements for this ROM",
            "RetroAchievements does not recognise this dump",
        ),
        Event::LoadFailed { message } => {
            notice(Problem, RA, "Could not load achievements", &message)
        }
        Event::Unlocked(a) => {
            let mut n = Notice::new(Unlock, "Achievement unlocked", &a.title, &a.description);
            n.points = a.points;
            n.badge = (!a.badge_name.is_empty()).then_some(a.badge_name);
            n.badge_url = a.badge_url;
            Said::Notice(n)
        }
        Event::ChallengeShown(a) => Said::Challenge(a),
        Event::ChallengeHidden { id } => Said::ChallengeOver(id),
        Event::Progress(a) => Said::Progress(a),
        Event::ProgressHidden => Said::ProgressOver,
        // Leaderboards run in hardcore only, which inkdot doesn't play: logged.
        Event::LeaderboardStarted(_)
        | Event::LeaderboardFailed(_)
        | Event::LeaderboardSubmitted(_)
        | Event::Tracker { .. }
        | Event::TrackerHidden { .. } => Said::Nothing,
        Event::GameCompleted {
            title,
            summary,
            hardcore,
        } => notice(
            Unlock,
            if hardcore { "Mastered" } else { "Completed" },
            &title,
            &format!(
                "all {} achievements, {}",
                summary.total,
                points(summary.points)
            ),
        ),
        Event::SubsetCompleted { title } => notice(Unlock, "Subset completed", &title, ""),
        Event::ServerError { message, .. } => notice(Problem, RA, "Server error", &message),
        Event::Disconnected => notice(
            Problem,
            RA,
            "Offline",
            "unlocks will be sent when the connection returns",
        ),
        Event::Reconnected => notice(Info, RA, "Reconnected", "pending unlocks sent"),
    }
}

/// An event as a line for the log; `None` for the ones that come every frame or so.
pub fn line(e: &Event) -> Option<String> {
    let with = |head: String, detail: &str| {
        Some(if detail.is_empty() {
            head
        } else {
            format!("{head} -- {detail}")
        })
    };
    match e {
        Event::SignedIn { display_name } => with("Signed in".into(), display_name),
        Event::SignInFailed { message, .. } => with("Sign-in failed".into(), message),
        Event::GameLoaded {
            title,
            summary,
            hardcore,
        } => with(title.clone(), &standing(summary, *hardcore)),
        Event::NoAchievements { title } => with(title.clone(), "no published achievements"),
        Event::UnknownGame { hash } => with(
            "No achievements for this ROM".into(),
            &format!("RetroAchievements does not recognise this dump (hash {hash})"),
        ),
        Event::LoadFailed { message } => with("Could not load achievements".into(), message),
        Event::Unlocked(a) => with(
            format!("Achievement unlocked: {} ({})", a.title, points(a.points)),
            &a.description,
        ),
        Event::ChallengeShown(a) => with("Challenge".into(), &a.title),
        Event::ChallengeHidden { .. }
        | Event::Progress(_)
        | Event::ProgressHidden
        | Event::Tracker { .. }
        | Event::TrackerHidden { .. } => None,
        Event::LeaderboardStarted(l) => with("Leaderboard started".into(), &l.title),
        Event::LeaderboardFailed(l) => with("Leaderboard failed".into(), &l.title),
        Event::LeaderboardSubmitted(l) => with(
            "Leaderboard submitted".into(),
            &format!("{}: {}", l.title, l.value),
        ),
        Event::GameCompleted { title, .. } => with("Game completed".into(), title),
        Event::SubsetCompleted { title } => with("Subset completed".into(), title),
        Event::ServerError { api, message } => with(format!("Server error ({api})"), message),
        Event::Disconnected => with(
            "RetroAchievements offline".into(),
            "unlocks will be sent when the connection returns",
        ),
        Event::Reconnected => with(
            "RetroAchievements reconnected".into(),
            "pending unlocks sent",
        ),
        Event::ResetAsked => with("Reset asked for".into(), "ignored: hardcore is not played"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(total: u32, unlocked: u32, unsupported: u32) -> Summary {
        Summary {
            total,
            unlocked,
            unsupported,
            points: 400,
            ..Summary::default()
        }
    }

    #[test]
    fn lines_read_as_dsperate_logs_them() {
        let a = |title: &str, description: &str, points| {
            Event::Unlocked(Achievement {
                title: title.into(),
                description: description.into(),
                points,
                ..Achievement::default()
            })
        };
        assert_eq!(
            line(&a("First Line", "Clear a line", 5)).unwrap(),
            "Achievement unlocked: First Line (5 points) -- Clear a line"
        );
        assert_eq!(
            line(&a("Warm-up", "", 1)).unwrap(),
            "Achievement unlocked: Warm-up (1 point)"
        );
        assert_eq!(
            line(&Event::Disconnected).unwrap(),
            "RetroAchievements offline -- unlocks will be sent when the connection returns"
        );
        assert_eq!(line(&Event::ProgressHidden), None);
    }

    #[test]
    fn the_placard_says_casual_and_the_standing() {
        let loaded = |unsupported| Event::GameLoaded {
            title: "Tetris".into(),
            summary: summary(40, 12, unsupported),
            hardcore: false,
        };
        let Said::Notice(n) = word(loaded(0)) else {
            panic!("a placard");
        };
        assert_eq!(
            (n.title.as_str(), n.detail.as_str()),
            ("Tetris", "Casual: 12 of 40 unlocked")
        );
        let Said::Notice(n) = word(loaded(3)) else {
            panic!("a placard");
        };
        assert_eq!(n.detail, "Casual: 12 of 40 unlocked, 3 unsupported");
        assert_eq!(
            line(&loaded(3)).unwrap(),
            "Tetris -- Casual: 12 of 40 unlocked, 3 unsupported"
        );
    }

    #[test]
    fn completion_and_unlocks_in_gold() {
        let Said::Notice(n) = word(Event::GameCompleted {
            title: "Tetris".into(),
            summary: summary(40, 40, 0),
            hardcore: false,
        }) else {
            panic!("a notice");
        };
        assert_eq!(n.tone, Tone::Unlock);
        assert_eq!(n.header, "Completed");
        assert_eq!(n.detail, "all 40 achievements, 400 points");
        let Said::Notice(n) = word(Event::Unlocked(Achievement {
            title: "First Line".into(),
            points: 5,
            badge_name: "12345".into(),
            ..Achievement::default()
        })) else {
            panic!("a notice");
        };
        assert_eq!((n.tone, n.points), (Tone::Unlock, 5));
        assert_eq!(n.badge.as_deref(), Some("12345"));
    }

    #[test]
    fn a_refused_token_says_where_to_sign_in() {
        let Said::Notice(n) = word(Event::SignInFailed {
            token_refused: true,
            message: "Invalid token".into(),
        }) else {
            panic!("a notice");
        };
        assert_eq!(n.tone, Tone::Problem);
        assert!(n.detail.contains("DSlot"));
    }
}
