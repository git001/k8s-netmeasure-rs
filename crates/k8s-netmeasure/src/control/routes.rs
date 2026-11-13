//! Routing of the control port (design §7), without any I/O: method, path and query
//! in, status and JSON body out. Unknown path → 404, a method other than `GET` → 405,
//! any query string → 400 (unknown parameters are hard failures, §9.1).

use hyper::{Method, StatusCode};
use serde_json::{Value, json};

use super::state::State;

const PATHS: [&str; 4] = ["/startup", "/live", "/ready", "/v1/info"];

pub fn respond(
    state: &State,
    method: &Method,
    path: &str,
    query: Option<&str>,
    now_ns: u64,
) -> (StatusCode, Value) {
    if !PATHS.contains(&path) {
        return error(StatusCode::NOT_FOUND, format!("unknown path {path}"));
    }
    if method != Method::GET {
        return error(
            StatusCode::METHOD_NOT_ALLOWED,
            format!("{method} is not allowed"),
        );
    }
    if query.is_some() {
        return error(
            StatusCode::BAD_REQUEST,
            "query parameters are not accepted".into(),
        );
    }
    match path {
        "/startup" if state.started() => status(StatusCode::OK, "started"),
        "/startup" => status(StatusCode::SERVICE_UNAVAILABLE, "starting"),
        "/live" => match state.stalled(now_ns) {
            stalled if stalled.is_empty() => status(StatusCode::OK, "live"),
            stalled => (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({ "status": "stalled", "threads": stalled }),
            ),
        },
        "/ready" if state.stopping() => status(StatusCode::SERVICE_UNAVAILABLE, "stopping"),
        "/ready" if state.started() => status(StatusCode::OK, "ready"),
        "/ready" => status(StatusCode::SERVICE_UNAVAILABLE, "starting"),
        _ => (StatusCode::OK, state.info().clone()),
    }
}

fn status(code: StatusCode, text: &str) -> (StatusCode, Value) {
    (code, json!({ "status": text }))
}

fn error(code: StatusCode, text: String) -> (StatusCode, Value) {
    (code, json!({ "error": text }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::state::Heartbeat;

    fn state(beat: u64) -> State {
        State::new(
            json!({ "listener_instance": "x" }),
            vec![Heartbeat {
                name: "main",
                read: Box::new(move || beat),
                bound_ns: 10,
            }],
        )
    }

    fn get(s: &State, path: &str) -> (StatusCode, Value) {
        respond(s, &Method::GET, path, None, 100)
    }

    #[test]
    fn strictness() {
        let s = state(100);
        assert_eq!(get(&s, "/nope").0, StatusCode::NOT_FOUND);
        assert_eq!(
            respond(&s, &Method::POST, "/ready", None, 100).0,
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            respond(&s, &Method::GET, "/ready", Some("a=1"), 100).0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            respond(&s, &Method::GET, "/ready", Some(""), 100).0,
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn probes_follow_startup_and_shutdown() {
        let s = state(100);
        assert_eq!(get(&s, "/startup").0, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(get(&s, "/ready").1["status"], "starting");
        s.mark_started();
        assert_eq!(get(&s, "/startup").0, StatusCode::OK);
        assert_eq!(get(&s, "/ready").0, StatusCode::OK);
        assert_eq!(get(&s, "/v1/info").1["listener_instance"], "x");
        s.mark_stopping();
        assert_eq!(get(&s, "/ready").1["status"], "stopping");
        assert_eq!(get(&s, "/startup").0, StatusCode::OK, "startup stays 200");
        assert_eq!(
            get(&s, "/live").0,
            StatusCode::OK,
            "live stays 200 during shutdown"
        );
    }

    #[test]
    fn live_reports_stalled_threads() {
        let (code, body) = get(&state(50), "/live");
        assert_eq!(code, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["threads"], json!(["main"]));
    }
}
