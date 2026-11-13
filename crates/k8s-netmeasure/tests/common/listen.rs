//! A running `listen` process for end-to-end tests, with a minimal HTTP client for its
//! control port (no client crate needed for plain GETs).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Child;
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

pub struct Running {
    pub child: Child,
    /// Measured port, `127.0.0.1:<port>`.
    pub addr: String,
    /// Control port, `127.0.0.1:<port>`.
    pub control: String,
}

/// Starts `listen` on free loopback ports and waits until the control port reports
/// ready.
pub fn start() -> Running {
    start_with(&[])
}

/// Like [`start`], with extra arguments.
pub fn start_with(extra: &[&str]) -> Running {
    let addr = super::closed_port();
    let control = super::closed_port();
    let port = |a: &str| a.rsplit(':').next().unwrap().to_owned();
    let (p, c) = (port(&addr), port(&control));
    let mut args = vec![
        "listen",
        "--bind",
        "127.0.0.1",
        "--port",
        &p,
        "--control-bind",
        "127.0.0.1",
        "--control-port",
        &c,
    ];
    args.extend_from_slice(extra);
    let child = super::spawn(&args);
    let deadline = Instant::now() + Duration::from_secs(10);
    while get(&control, "/ready").map(|(code, _)| code) != Some(200) {
        assert!(Instant::now() < deadline, "listen did not become ready");
        std::thread::sleep(Duration::from_millis(20));
    }
    Running {
        child,
        addr,
        control,
    }
}

/// Sends `signal` and returns the exit code and the combined output.
pub fn stop_with(mut child: Child, signal: Signal) -> (i32, String) {
    kill(Pid::from_raw(child.id() as i32), signal).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let out = child.wait_with_output().unwrap();
            let log =
                String::from_utf8(out.stdout).unwrap() + &String::from_utf8(out.stderr).unwrap();
            return (status.code().expect("exit code, not a signal"), log);
        }
        assert!(Instant::now() < deadline, "listen did not stop");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `GET path`; `None` if the connection fails.
pub fn get(addr: &str, path: &str) -> Option<(u16, serde_json::Value)> {
    request(addr, "GET", path)
}

/// One HTTP/1.1 request with `Connection: close`: status code and JSON body.
pub fn request(addr: &str, method: &str, path: &str) -> Option<(u16, serde_json::Value)> {
    let mut s = TcpStream::connect(addr).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    write!(
        s,
        "{method} {path} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n"
    )
    .ok()?;
    let mut text = String::new();
    s.read_to_string(&mut text).ok()?;
    let code = text.split_whitespace().nth(1)?.parse().ok()?;
    let body = text.split_once("\r\n\r\n")?.1;
    Some((code, serde_json::from_str(body).ok()?))
}
