//! The control port (design §7): HTTP/1.1 + JSON. The server runs on a dedicated
//! `control` thread with a `current_thread` tokio runtime and hyper; the client
//! (`run`) on the main thread. The measuring and accept threads never
//! touch tokio. The socket is bound synchronously, so a bind failure is reported
//! before the thread starts (exit 3).

pub mod client;
mod routes;
pub mod state;

use std::convert::Infallible;
use std::io;
use std::net::SocketAddrV4;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, header};
use hyper_util::rt::{TokioIo, TokioTimer};

use self::state::State;
use crate::sys::time;

/// How often the accept loop checks the stop flag.
const TICK: Duration = Duration::from_millis(200);
/// A client must send its request headers within this time.
const HEADER_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Server {
    pub local: SocketAddrV4,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start(addr: SocketAddrV4, state: Arc<State>) -> io::Result<Server> {
        let listener = std::net::TcpListener::bind(addr)?;
        listener.set_nonblocking(true)?;
        let std::net::SocketAddr::V4(local) = listener.local_addr()? else {
            return Err(io::Error::other("not IPv4"));
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let thread = std::thread::Builder::new()
            .name("control".into())
            .spawn(move || runtime.block_on(serve(listener, state, flag)))?;
        Ok(Server {
            local,
            stop,
            thread: Some(thread),
        })
    }

    /// Stops accepting and drops open connections.
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

async fn serve(listener: std::net::TcpListener, state: Arc<State>, stop: Arc<AtomicBool>) {
    let listener = match tokio::net::TcpListener::from_std(listener) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, "control port: cannot register the socket");
            return;
        }
    };
    while !stop.load(Ordering::SeqCst) {
        let Ok(Ok((stream, _))) = tokio::time::timeout(TICK, listener.accept()).await else {
            continue;
        };
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            let service = service_fn(move |req| {
                let response = handle(&state, &req);
                async move { Ok::<_, Infallible>(response) }
            });
            let _ = http1::Builder::new()
                .timer(TokioTimer::new())
                .header_read_timeout(HEADER_TIMEOUT)
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
}

fn handle(state: &State, req: &Request<Incoming>) -> Response<Full<Bytes>> {
    let uri = req.uri();
    let (status, body) = routes::respond(
        state,
        req.method(),
        uri.path(),
        uri.query(),
        time::monotonic_ns(),
    );
    let mut response = Response::new(Full::new(Bytes::from(body.to_string())));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    response
}
