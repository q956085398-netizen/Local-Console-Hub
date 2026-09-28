//! Health layer — process-alive and TCP port readiness (spec §12).
//!
//! MVP needs a small abstraction only: a reading of "is the run's process still
//! there" and "does the configured port answer". The third check §12 names — an
//! optional HTTP GET — is not implemented; [`ServiceHealth`] is where its
//! result lands, and [`POLL_INTERVAL`] is the cadence it would share. Nothing
//! else about the layer would have to change.
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
//! One connect per interval, for a running service that names a port, on a
//! thread that is awake anyway: the reading rides the run's own watcher
//! (`session::core::watch_run`), so it starts and stops with the run and no
//! second monitor thread exists to leak (spec §14). It is published only when
//! the reading *changes*, so a steady service costs one connect per interval
//! and no events at all (D-009).
//!
//! ## Local only
//!
//! The probe dials `127.0.0.1`. The Hub manages local services, and `port` is
//! the port such a service listens on; a probe that resolved a hostname or
//! dialled outward would be a health check with an off-machine side effect. A
//! config whose `url` names another host therefore gets a reading about the
//! port on *this* machine, which is the honest answer for a control surface
//! that only manages what is here.

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
/// is up. The HTTP check §12 leaves optional would add its result here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceHealth {
    /// The run's own process was still alive when the reading was taken.
    pub process_alive: bool,
    /// The configured port accepted a connection when the reading was taken.
    pub port_open: bool,
}

impl ServiceHealth {
    /// Read a service's health now: `process_alive` as the caller observed it,
    /// plus a fresh probe of `port`.
    ///
    /// The process half is passed in rather than probed here because the layer
    /// that owns the process handle is the one that can answer without guessing
    /// at a PID (`crate::process` — D-008's "never kill by name" has the same
    /// root: a number is not an identity).
    pub fn read(process_alive: bool, port: u16) -> Self {
        ServiceHealth {
            process_alive,
            port_open: port_open(port),
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

        let up = ServiceHealth::read(true, port);
        let booting = ServiceHealth::read(true, closed_port());
        let gone = ServiceHealth::read(false, closed_port());

        assert_eq!(
            up,
            ServiceHealth {
                process_alive: true,
                port_open: true
            }
        );
        assert_eq!(
            booting,
            ServiceHealth {
                process_alive: true,
                port_open: false
            }
        );
        assert_eq!(
            gone,
            ServiceHealth {
                process_alive: false,
                port_open: false
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
        })
        .expect("a reading serializes");

        assert_eq!(value["processAlive"], serde_json::json!(true));
        assert_eq!(value["portOpen"], serde_json::json!(false));
    }
}
