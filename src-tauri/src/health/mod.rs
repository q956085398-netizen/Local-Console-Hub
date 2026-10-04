//! Health layer — process-alive, TCP port readiness, and an optional HTTP GET
//! (spec §12).
//!
//! MVP needs a small abstraction only: a reading of "is the run's process still
//! there", "does the configured port answer", and, when the session names an
//! http(s) URL, "what did one GET of that URL return". The HTTP result is its
//! own field on [`ServiceHealth`]. It does not replace [`port_open`], and it
//! does not say that whatever accepted the connection is this session's process.
//!
//! ## Health is not lifecycle
//!
//! `DECISIONS.md` D-008: the architecture must not freeze "PID exists ==
//! service healthy". A reading therefore never moves a
//! [`SessionStatus`](crate::session::state::SessionStatus) — a `Running`
//! service whose port is not answering is still running — and it is carried as
//! its own field on the snapshot rather than as a lifecycle state. That is what
//! lets the UI say both things at once, which `docs/PRODUCT_SPEC.md` §3
//! requires ("the UI must distinguish the process being alive from the service
//! being available").
//!
//! ## Cost, and being cancellable
//!
//! One TCP connect per interval, for a running service that names a port, on a
//! thread that is awake anyway — and, when that session also names an http(s)
//! URL, one GET on the same pass. The reading rides the run's own watcher
//! (`session::core::watch_run`), so it starts and stops with the run and no
//! second monitor thread exists to leak (spec §14). It is published only when
//! the reading *changes*, so a steady service costs those probes per interval
//! and no events at all (D-009).
//!
//! ## The TCP probe stays on this machine
//!
//! The port probe dials `127.0.0.1`. The Hub manages local services, and `port`
//! is the port such a service listens on; retargeting that dial at the URL's
//! host would be answering a different question than "is the configured port
//! accepting connections". A config whose `url` names another host therefore
//! still gets a TCP reading about the port on *this* machine.
//!
//! The HTTP GET is the other question, aimed at the URL the session named. Its
//! answer is stored beside the TCP reading and is not copied into it. A
//! timeout, a refused connection, or a non-2xx status is a failed GET — not
//! "nothing is listening", and not "this process belongs to the session".
//! Sessions with no http(s) URL do not issue that GET. The probe does not
//! restart a session or end a process, and it does not need an administrator.

mod http;

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::time::Duration;

use serde::Serialize;

/// How long one TCP probe may take before the port is called closed.
///
/// A probe of a loopback port answers in microseconds — the connection either
/// completes or is refused — so this bound is not what makes the check cheap.
/// It is what keeps a port that *accepts* a connection but never completes the
/// handshake from stalling the watcher thread that also notices the run ending.
pub const PROBE_TIMEOUT: Duration = Duration::from_millis(300);

/// How often a running service's health is re-read.
///
/// The number is a deliberate trade the spec asks for rather than a default:
/// §12 wants checks low-frequency, D-009 wants them event-driven or on demand.
/// A service that takes a minute to become ready shows as not listening for at
/// most one interval longer than it really is, and the steady-state cost is one
/// loopback connect every five seconds.
pub const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// One reading of a running service's health.
///
/// Present on the wire (`SessionRuntime::health`) only while there is something
/// to read: a service that names a port and has a run in flight. Absent
/// everywhere else, because "we did not check" is not the same claim as "the
/// port is closed" and only one of the two is true.
///
/// Both fields are facts about the moment of the probe, not conclusions about
/// the session: they are what lets a user tell a service that is still booting
/// (nothing listening yet, process alive) from one that has died, from one that
/// is up. An http(s) URL adds a third fact, [`HttpProbe`], which is absent when
/// that GET was not issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceHealth {
    /// The run's own process was still alive when the reading was taken.
    pub process_alive: bool,
    /// The configured port accepted a connection when the reading was taken.
    ///
    /// This is the loopback TCP probe only. An HTTP result never writes it.
    pub port_open: bool,
    /// The GET of the session's http(s) URL, when it has one.
    ///
    /// `None` means the probe was not issued. `Some` with `ok: false` means it
    /// was issued and failed — a timeout, a transport error, or a non-2xx
    /// status — which is neither "the port is closed" nor "the listener is
    /// this session".
    pub http: Option<HttpProbe>,
}

/// What one HTTP GET returned.
///
/// Present only after a request was sent. A failed probe is still present:
/// "we asked and it failed" is a different claim from "we did not ask".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpProbe {
    /// `true` only when a response arrived with a 2xx status.
    pub ok: bool,
    /// The status line's code when one was read. `None` on timeout or when
    /// the exchange ended before a status.
    pub status: Option<u16>,
}

impl HttpProbe {
    pub(super) fn failed() -> Self {
        Self {
            ok: false,
            status: None,
        }
    }

    /// A status that came back. Codes outside the HTTP range are not a
    /// response this layer will report as one; they are a failed probe.
    pub(super) fn from_status(status: u16) -> Self {
        if !(100..600).contains(&status) {
            return Self::failed();
        }
        Self {
            ok: (200..300).contains(&status),
            status: Some(status),
        }
    }
}

impl ServiceHealth {
    /// Read a service's health now: `process_alive` as the caller observed it,
    /// a fresh TCP probe of `port`, and — when `url` is an http(s) address —
    /// one GET of that address.
    ///
    /// The process half is passed in rather than probed here because the layer
    /// that owns the process handle is the one that can answer without guessing
    /// at a PID (`crate::process` — D-008's "never kill by name" has the same
    /// root: a number is not an identity).
    ///
    /// `None`, or a string that is not an http(s) URL, sends no HTTP request.
    /// The GET's result is stored on [`ServiceHealth::http`] and does not
    /// change [`ServiceHealth::port_open`].
    pub fn read(process_alive: bool, port: u16, url: Option<&str>) -> Self {
        ServiceHealth {
            process_alive,
            port_open: port_open(port),
            http: url.and_then(http::probe),
        }
    }
}

/// Whether `port` on the loopback interface accepts a connection right now.
///
/// A refused connection (nothing listening) and a timed-out one are both
/// "closed" as far as a user is concerned: the service is not answering on the
/// port its config names.
///
/// The timeout is [`PROBE_TIMEOUT`] rather than a parameter: it bounds one call
/// the product always wants bounded the same way, and a knob with one setting
/// is a knob that only misleads.
pub fn port_open(port: u16) -> bool {
    let address = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port));
    TcpStream::connect_timeout(&address, PROBE_TIMEOUT).is_ok()
}

/// A port on this machine that nothing is listening on.
///
/// For tests: the OS hands out an ephemeral port and the listener is released
/// so a probe finds it closed. Here rather than in each test module because
/// "a port nobody answers on" is this layer's idea, and two copies could drift
/// into two different notions of it.
#[cfg(test)]
pub(crate) fn closed_port() -> u16 {
    let listener =
        std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind an ephemeral port");
    let port = listener.local_addr().expect("the bound address").port();
    drop(listener);
    port
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// The check that matters: a listening port reads open, and the same port
    /// reads closed once nothing is listening on it.
    ///
    /// One socket, so the "closed" half is not a guess about some other
    /// process's behaviour on this machine.
    #[test]
    fn a_port_reads_open_while_something_listens_and_closed_after_it_stops() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind an ephemeral port");
        let port = listener.local_addr().expect("the bound address").port();

        assert!(port_open(port), "a bound port accepts a connection");

        drop(listener);

        assert!(
            !port_open(port),
            "the port is closed once the listener is gone"
        );
    }

    /// A reading keeps the two facts apart, which is the whole point of it: a
    /// service that is starting answers "the process is alive" and "nothing is
    /// listening yet" at the same time.
    #[test]
    fn a_reading_carries_the_process_and_the_port_as_separate_facts() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind an ephemeral port");
        let port = listener.local_addr().expect("the bound address").port();

        let up = ServiceHealth::read(true, port, None);
        let booting = ServiceHealth::read(true, closed_port(), None);
        let gone = ServiceHealth::read(false, closed_port(), None);

        assert_eq!(
            up,
            ServiceHealth {
                process_alive: true,
                port_open: true,
                http: None,
            }
        );
        assert_eq!(
            booting,
            ServiceHealth {
                process_alive: true,
                port_open: false,
                http: None,
            }
        );
        assert_eq!(
            gone,
            ServiceHealth {
                process_alive: false,
                port_open: false,
                http: None,
            }
        );
    }

    /// The wire shape the frontend mirror is written against
    /// (`src/types/runtime.ts`).
    #[test]
    fn a_reading_serializes_in_camel_case() {
        let value = serde_json::to_value(ServiceHealth {
            process_alive: true,
            port_open: false,
            http: None,
        })
        .expect("a reading serializes");

        assert_eq!(value["processAlive"], serde_json::json!(true));
        assert_eq!(value["portOpen"], serde_json::json!(false));
        assert!(value["http"].is_null(), "no probe is null, not a failure");
    }

    /// A 200 from another port must not turn a closed configured port into an
    /// open one, and a non-success must not turn an open port into a closed one.
    #[test]
    fn an_http_result_does_not_overwrite_port_open() {
        let server = ScriptedServer::spawn(Reply::Status(200));
        let url = format!("http://127.0.0.1:{}/", server.port);
        let http_up = ServiceHealth::read(false, closed_port(), Some(&url));

        assert!(
            !http_up.port_open,
            "a successful GET is not the configured port being open"
        );
        assert!(
            !http_up.process_alive,
            "a successful GET does not claim this session's process"
        );
        assert_eq!(
            http_up.http,
            Some(HttpProbe {
                ok: true,
                status: Some(200),
            })
        );
        assert_eq!(server.gets(), 1, "the URL was actually requested");

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind an ephemeral port");
        let open = listener.local_addr().expect("the bound address").port();
        let failing = ScriptedServer::spawn(Reply::Status(503));
        let bad_url = format!("http://127.0.0.1:{}/", failing.port);
        let http_down = ServiceHealth::read(true, open, Some(&bad_url));

        assert!(http_down.port_open, "a failed GET must not clear port_open");
        assert!(http_down.process_alive);
        assert_eq!(
            http_down.http,
            Some(HttpProbe {
                ok: false,
                status: Some(503),
            })
        );
        drop(listener);
    }

    /// Timeout and a non-success status are both a failed probe. Neither one
    /// is "nothing is listening" and neither one rewrites who the process is.
    #[test]
    fn a_timeout_or_a_non_success_is_a_failed_http_probe() {
        let silent = ScriptedServer::spawn(Reply::Silence);
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind an ephemeral port");
        let open = listener.local_addr().expect("the bound address").port();
        let url = format!("http://127.0.0.1:{}/", silent.port);
        let timed_out = ServiceHealth::read(true, open, Some(&url));

        assert!(
            timed_out.port_open,
            "a timed-out GET is not a closed configured port"
        );
        assert!(timed_out.process_alive);
        assert_eq!(
            silent.gets(),
            1,
            "the request was sent and then not answered"
        );
        assert_eq!(
            timed_out.http,
            Some(HttpProbe {
                ok: false,
                status: None,
            })
        );

        let rejected = ScriptedServer::spawn(Reply::Status(404));
        let rejected_url = format!("http://127.0.0.1:{}/", rejected.port);
        let non_success = ServiceHealth::read(false, closed_port(), Some(&rejected_url));
        assert!(!non_success.port_open);
        assert!(!non_success.process_alive);
        assert_eq!(
            non_success.http,
            Some(HttpProbe {
                ok: false,
                status: Some(404),
            })
        );
        drop(listener);
    }

    /// No URL, and a URL that is not http(s), must not send a GET. The TCP
    /// reading is the one it was before.
    #[test]
    fn a_session_with_no_http_url_does_not_issue_the_probe() {
        let server = ScriptedServer::spawn(Reply::Status(200));
        let reading = ServiceHealth::read(true, server.port, None);
        assert!(reading.port_open, "TCP reachability is unchanged");
        assert!(reading.process_alive);
        assert!(reading.http.is_none());

        let ftp = format!("ftp://127.0.0.1:{}/", server.port);
        let not_http = ServiceHealth::read(false, closed_port(), Some(&ftp));
        assert!(not_http.http.is_none(), "a non-http URL is not probed");
        assert!(!not_http.port_open);

        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(server.gets(), 0, "no GET was sent");
    }

    /// An https URL is probed. A listener that is not TLS fails that probe
    /// without changing whether the configured port accepted a TCP connection.
    #[cfg(windows)]
    #[test]
    fn a_failed_https_probe_stays_beside_an_open_port() {
        let server = ScriptedServer::spawn(Reply::Status(200));
        let url = format!("https://127.0.0.1:{}/", server.port);
        let reading = ServiceHealth::read(true, server.port, Some(&url));

        assert!(reading.port_open);
        assert!(reading.process_alive);
        let http = reading.http.expect("an https URL issues a probe");
        assert!(!http.ok, "a failed handshake is a failed probe");
        assert!(http.status.is_none());
    }

    /// How a test server answers a GET. Anything that is not a GET is dropped, so
    /// the TCP reachability probe — which connects and writes nothing — is not
    /// counted as an HTTP request.
    #[derive(Clone, Copy)]
    enum Reply {
        Status(u16),
        /// Accept the GET and never write a response, until the client gives up.
        Silence,
    }

    struct ScriptedServer {
        port: u16,
        gets: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl ScriptedServer {
        fn spawn(reply: Reply) -> Self {
            use std::io::{Read, Write};
            use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
            use std::sync::{mpsc, Arc};
            use std::time::Instant;

            let listener =
                TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind an ephemeral port");
            listener.set_nonblocking(true).expect("nonblocking accept");
            let port = listener.local_addr().expect("the bound address").port();
            let gets = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let (ready_tx, ready_rx) = mpsc::channel();
            let gets_thread = Arc::clone(&gets);
            let stop_thread = Arc::clone(&stop);
            let thread = std::thread::spawn(move || {
                let _ = ready_tx.send(());
                let deadline = Instant::now() + Duration::from_secs(15);
                while Instant::now() < deadline && !stop_thread.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            let _ = stream.set_nodelay(true);
                            let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                            let mut buf = [0u8; 2048];
                            let n = stream.read(&mut buf).unwrap_or(0);
                            let is_get = n >= 4 && &buf[..4] == b"GET ";
                            if !is_get {
                                continue;
                            }
                            gets_thread.fetch_add(1, Ordering::Relaxed);
                            match reply {
                                Reply::Status(status) => {
                                    let message = format!(
                                    "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                                );
                                    let _ = stream.write_all(message.as_bytes());
                                }
                                Reply::Silence => {
                                    let _ =
                                        stream.set_read_timeout(Some(Duration::from_millis(1_000)));
                                    let _ = stream.read(&mut [0u8; 1]);
                                }
                            }
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            });
            ready_rx.recv().expect("the server thread starts");
            Self {
                port,
                gets,
                stop,
                thread: Some(thread),
            }
        }

        fn gets(&self) -> usize {
            self.gets.load(std::sync::atomic::Ordering::Relaxed)
        }
    }

    impl Drop for ScriptedServer {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            let _ = TcpStream::connect_timeout(
                &SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.port)),
                Duration::from_millis(200),
            );
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }
}
