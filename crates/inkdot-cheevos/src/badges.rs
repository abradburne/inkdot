//! The achievements' badge pictures, in the cache every app on the device shares with DSlot:
//! `$XDG_CACHE_HOME/retroachievements/badges/<name>.png` (an absolute `XDG_CACHE_HOME`; else
//! `~/.cache`). A badge not there yet is fetched by `curl` on a thread of its own, written
//! whole or not at all, and handed back by name on a later frame; decoding it is the app's.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};

/// Where the service keeps badges, when an event gives no URL.
const MEDIA: &str = "https://media.retroachievements.org/Badge";

/// The shared badge folder; `None` without a home.
pub fn dir() -> Option<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|h| h.join(".cache"))
        })?;
    Some(cache.join("retroachievements/badges"))
}

/// A badge's file in `dir`; `None` for a name that could reach outside it.
fn file(dir: &Path, name: &str) -> Option<PathBuf> {
    let plain = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    plain.then(|| dir.join(format!("{name}.png")))
}

/// The badges asked for, and the thread that fetches them.
pub struct Badges {
    dir: PathBuf,
    asks: Sender<(PathBuf, String)>,
    fetched: Receiver<(String, PathBuf)>,
    asked: HashSet<String>,
}

impl Badges {
    /// Badges in `dir` (normally [`dir`]), fetched with `curl` when they aren't there. `None`
    /// without a `curl`.
    pub fn start(dir: PathBuf) -> Option<Badges> {
        let curl = crate::curl_path()?;
        let (asks, incoming) = channel::<(PathBuf, String)>();
        let (outgoing, fetched) = channel();
        std::thread::Builder::new()
            .name("cheevos-badges".into())
            .spawn(move || {
                crate::transport::ordinary_priority();
                for (to, url) in incoming {
                    let name = to
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    match fetch(&curl, &url, &to) {
                        Ok(()) => {
                            if outgoing.send((name, to)).is_err() {
                                break;
                            }
                        }
                        Err(e) => eprintln!("cheevos: badge {name}: {e}"),
                    }
                }
            })
            .ok()?;
        Some(Badges {
            dir,
            asks,
            fetched,
            asked: HashSet::new(),
        })
    }

    /// The badge `name`'s file when it is there already; else `None`, and it is fetched (from
    /// `url`, or the service's usual place when that is empty), once, and comes out of
    /// [`Badges::fetched`] when it has been.
    pub fn want(&mut self, name: &str, url: &str) -> Option<PathBuf> {
        let file = file(&self.dir, name)?;
        if file.is_file() {
            return Some(file);
        }
        if self.asked.insert(name.to_owned()) {
            let url = if url.starts_with("https://") {
                url.to_owned()
            } else {
                format!("{MEDIA}/{name}.png")
            };
            let _ = self.asks.send((file, url));
        }
        None
    }

    /// The badges fetched since the last call: their names and files.
    pub fn fetched(&mut self) -> Vec<(String, PathBuf)> {
        self.fetched.try_iter().collect()
    }
}

/// Fetches `url` into `to`: into a file beside it, then renamed over it.
fn fetch(curl: &Path, url: &str, to: &Path) -> Result<(), String> {
    let dir = to.parent().ok_or("no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let part = to.with_extension("png.part");
    let out = Command::new(curl)
        .args(["-sSf", "--max-time", "20", "--connect-timeout", "10", "-o"])
        .arg(&part)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_file(&part);
        let why = String::from_utf8_lossy(&out.stderr);
        return Err(why
            .lines()
            .next()
            .unwrap_or("curl failed")
            .trim()
            .to_owned());
    }
    std::fs::rename(&part, to).map_err(|e| format!("{}: {e}", to.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_in_the_folder() {
        let d = Path::new("/c");
        assert_eq!(file(d, "12345"), Some(PathBuf::from("/c/12345.png")));
        assert_eq!(
            file(d, "12345_lock"),
            Some(PathBuf::from("/c/12345_lock.png"))
        );
        assert_eq!(file(d, ""), None);
        assert_eq!(file(d, "../x"), None);
        assert_eq!(file(d, "a/b"), None);
    }

    #[test]
    fn a_badge_already_there_is_not_fetched() {
        if crate::curl_path().is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("inkdot-badges-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("00001.png"), b"png").unwrap();
        let mut b = Badges::start(dir.clone()).unwrap();
        assert_eq!(b.want("00001", ""), Some(dir.join("00001.png")));
        assert_eq!(b.want("../x", ""), None);
        assert!(b.fetched().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
