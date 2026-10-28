//! The service's requests, carried by `curl` on a thread of their own: inkdot has no TLS of
//! its own, and nibOS's tools (and DSlot's sign-in) use `curl` the same way. The emulation
//! thread hands a request over and goes on; the answer comes back on a channel and is given to
//! rcheevos on the next frame. The form data (which carries the token) goes to `curl` on its
//! standard input, never in an argument.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};

/// How long one request may take, in all (the service answers in well under a second; a
/// stalled network should not hold a thread for long, and rcheevos retries).
const TIMEOUT_SECS: u32 = 30;
const CONNECT_TIMEOUT_SECS: u32 = 10;

/// A request rcheevos made, with the ticket that answers it.
pub struct Job {
    pub url: String,
    /// The form to POST; `None` for a GET.
    pub post: Option<(String, String)>,
    pub ticket: usize,
}

/// An answer: the HTTP status (0 when the service was never reached) and the body.
pub struct Done {
    pub ticket: usize,
    pub status: i32,
    pub body: Vec<u8>,
    /// Why the service wasn't reached, for the log.
    pub error: Option<String>,
}

/// Starts the thread; jobs go in one end, answers come out the other. The thread ends when
/// the sender is dropped.
pub fn start(user_agent: String) -> (Sender<Job>, Receiver<Done>) {
    let (jobs, incoming) = channel::<Job>();
    let (outgoing, done) = channel::<Done>();
    let spawned = std::thread::Builder::new()
        .name("inkdot-cheevos".into())
        .spawn(move || {
            ordinary_priority();
            for job in incoming {
                let (status, body, error) = perform(&job, &user_agent);
                if outgoing
                    .send(Done {
                        ticket: job.ticket,
                        status,
                        body,
                        error,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
    if let Err(e) = spawned {
        eprintln!("cheevos: cannot start the network thread: {e}");
    }
    (jobs, done)
}

/// A thread that waits on the network must not hold the emulation thread's scheduling
/// (a real-time class, or its utilization clamp, where the launcher gave it one).
fn ordinary_priority() {
    #[cfg(target_os = "linux")]
    {
        let param = libc::sched_param { sched_priority: 0 };
        // SAFETY: this thread, an ordinary policy, a fully initialised param.
        unsafe {
            libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_OTHER, &param);
            libc::nice(10);
        }
    }
}

/// Runs `curl` for one request: the status, the body, and the transport error if there was
/// one.
fn perform(job: &Job, user_agent: &str) -> (i32, Vec<u8>, Option<String>) {
    let mut cmd = Command::new("curl");
    cmd.args([
        "-sS",
        "--max-time",
        &TIMEOUT_SECS.to_string(),
        "--connect-timeout",
        &CONNECT_TIMEOUT_SECS.to_string(),
        "-A",
        user_agent,
        "--compressed",
        "-o",
        "-",
        "-w",
        "\n%{http_code}",
    ]);
    if let Some((content_type, _)) = &job.post {
        cmd.args(["-H", &format!("Content-Type: {content_type}")]);
        // No 100-continue wait.
        cmd.args(["-H", "Expect:", "--data-binary", "@-"]);
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.arg(&job.url);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return (0, Vec::new(), Some(format!("curl: {e}"))),
    };
    if let (Some((_, form)), Some(mut stdin)) = (&job.post, child.stdin.take()) {
        let _ = stdin.write_all(form.as_bytes());
    }
    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return (0, Vec::new(), Some(format!("curl: {e}"))),
    };
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        let why = why.lines().next().unwrap_or("").trim();
        let why = if why.is_empty() {
            format!("curl exited with {}", out.status)
        } else {
            why.to_owned()
        };
        return (0, Vec::new(), Some(why));
    }
    let (body, status) = status_and_body(out.stdout);
    (status, body, None)
}

/// The body and the status `curl` wrote after it on a line of its own.
fn status_and_body(mut stdout: Vec<u8>) -> (Vec<u8>, i32) {
    let Some(nl) = stdout.iter().rposition(|&b| b == b'\n') else {
        return (stdout, 0);
    };
    let status = std::str::from_utf8(&stdout[nl + 1..])
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0);
    stdout.truncate(nl);
    (stdout, status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_follows_the_body() {
        assert_eq!(
            status_and_body(b"{\"Success\":true}\n200".to_vec()),
            (b"{\"Success\":true}".to_vec(), 200)
        );
        assert_eq!(
            status_and_body(b"body\n\n404".to_vec()),
            (b"body\n".to_vec(), 404)
        );
        assert_eq!(status_and_body(b"".to_vec()), (Vec::new(), 0));
        assert_eq!(status_and_body(b"\n000".to_vec()), (Vec::new(), 0));
    }
}
