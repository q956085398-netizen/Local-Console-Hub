//! Listening ports on this machine, and the process that owns each one (#96).
//!
//! This is a read of the OS ownership tables. It is not the configured-port TCP
//! reachability probe in [`crate::health`]: a connect that succeeds does not
//! say which process is listening, and a row here is not a health reading.
//! Nothing in this module terminates a process, connects a socket, or sends HTTP.
//!
//! A field that cannot be read — permission, or the OS did not supply it — is
//! [`Readable::Unavailable`]. The row stays. Unavailable is not an empty list,
//! and an empty list is not a failed check. A failed check leaves the previous
//! successful snapshot, and the time it was taken, where they were.
//!
//! Which managed session owns a row is a second reading, [`attribute`]. It does
//! not change what [`collect`] returns, and it does not turn a health reading
//! into proof of ownership.

use std::net::IpAddr;
use std::path::PathBuf;
use std::time::SystemTime;

#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod win;

#[cfg(not(windows))]
use unsupported as backend;
#[cfg(windows)]
use win as backend;

mod attribute;

pub use attribute::{attribute, AttributedRecord, Attribution, SessionProcess, EXTERNAL_LABEL};

/// Words a later view shows for [`Readable::Unavailable`].
///
/// The enum is the reading. This label is only how that reading is spelled for
/// a person; it is not itself a process name or a path.
pub const UNAVAILABLE_LABEL: &str = "信息不可用";

/// Which IP stack a row belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IpFamily {
    V4,
    V6,
}

impl std::fmt::Display for IpFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            IpFamily::V4 => "IPv4",
            IpFamily::V6 => "IPv6",
        })
    }
}

/// Transport of one listening row. TCP and UDP stay distinct: a UDP bind is not
/// a TCP listener, and the two are not interchangeable occupancy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Protocol::Tcp => "TCP",
            Protocol::Udp => "UDP",
        })
    }
}

/// Local address a socket is bound to.
///
/// `scope_id` is the IPv6 zone index. It is zero for IPv4 and for an IPv6
/// address that is not scoped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ListenAddress {
    pub ip: IpAddr,
    pub scope_id: u32,
}

/// One field of a listening row.
///
/// [`Readable::Unavailable`] means this field could not be read. It does not
/// drop the row, and it does not mean the machine has nothing listening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readable<T> {
    Known(T),
    Unavailable,
}

impl<T> Readable<T> {
    /// The value, when it was read.
    pub fn as_ref(&self) -> Option<&T> {
        match self {
            Readable::Known(value) => Some(value),
            Readable::Unavailable => None,
        }
    }

    pub fn is_unavailable(&self) -> bool {
        matches!(self, Readable::Unavailable)
    }
}

/// One socket this machine has bound for incoming traffic, and its owner.
///
/// TCP rows are listeners. UDP rows are bound endpoints: UDP has no listen
/// state, and a bound port is what the row records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenRecord {
    pub protocol: Protocol,
    pub address: ListenAddress,
    pub port: u16,
    pub pid: Readable<u32>,
    /// Image file name (`svchost.exe`), not a full path.
    pub process_name: Readable<String>,
    /// Win32 path of the image, when it could be opened.
    pub program_path: Readable<PathBuf>,
}

/// Why a collection did not produce a list.
///
/// Any of these is a failed check. None of them is "nothing is listening".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectError {
    /// This platform has no collector.
    Unsupported,
    /// An owner table call failed. No partial list is returned in its place.
    Table {
        protocol: Protocol,
        family: IpFamily,
        code: u32,
    },
    /// The buffer was shorter than the row count written in its header.
    Truncated {
        protocol: Protocol,
        family: IpFamily,
    },
}

impl std::fmt::Display for CollectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CollectError::Unsupported => {
                f.write_str("listening-port collection is not supported on this platform")
            }
            CollectError::Table {
                protocol,
                family,
                code,
            } => write!(
                f,
                "reading the {protocol} {family} owner table failed (os error {code})"
            ),
            CollectError::Truncated { protocol, family } => write!(
                f,
                "the {protocol} {family} owner table was shorter than its row count"
            ),
        }
    }
}

impl std::error::Error for CollectError {}

/// One successful collection: the rows, and when they were read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub at: SystemTime,
    pub records: Vec<ListenRecord>,
}

/// One finished attempt.
///
/// Success carries the time the list was read. Failure does not, so folding a
/// failure cannot present an older list as a check that just completed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attempt {
    Succeeded(Snapshot),
    Failed(CollectError),
}

/// Last successful collection, whether a check is in progress, and the failure
/// of the latest completed attempt when that attempt failed.
///
/// Not synchronized. The caller decides when a refresh runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefreshState {
    last_success: Option<Snapshot>,
    in_progress: bool,
    failure: Option<CollectError>,
}

impl Default for RefreshState {
    fn default() -> Self {
        Self::new()
    }
}

impl RefreshState {
    pub fn new() -> Self {
        RefreshState {
            last_success: None,
            in_progress: false,
            failure: None,
        }
    }

    /// The last collection that succeeded, with the time it succeeded.
    /// Unchanged when a later attempt fails.
    pub fn last_success(&self) -> Option<&Snapshot> {
        self.last_success.as_ref()
    }

    pub fn in_progress(&self) -> bool {
        self.in_progress
    }

    /// Failure of the latest *completed* attempt, if that attempt failed.
    /// [`Self::begin`] does not clear it: a check in progress has not replaced
    /// the previous outcome yet.
    pub fn failure(&self) -> Option<&CollectError> {
        self.failure.as_ref()
    }

    /// Mark a check in progress without moving the last success or its time.
    pub fn begin(&mut self) {
        self.in_progress = true;
    }

    /// Fold a finished attempt into this state.
    ///
    /// [`Attempt::Succeeded`] replaces the last success, including when the list
    /// is empty, and clears any failure. [`Attempt::Failed`] records the failure
    /// and leaves the previous success and its time untouched.
    pub fn complete(&mut self, attempt: Attempt) {
        self.in_progress = false;
        match attempt {
            Attempt::Succeeded(snapshot) => {
                self.failure = None;
                self.last_success = Some(snapshot);
            }
            Attempt::Failed(error) => {
                self.failure = Some(error);
            }
        }
    }

    /// Collect now and fold the outcome in.
    ///
    /// The success time is the moment a real collection returned, not a time
    /// supplied by the caller. A failure records no new time.
    pub fn refresh(&mut self) {
        self.begin();
        let attempt = match collect() {
            Ok(records) => Attempt::Succeeded(Snapshot {
                at: SystemTime::now(),
                records,
            }),
            Err(error) => Attempt::Failed(error),
        };
        self.complete(attempt);
    }
}

/// Read the listening rows now.
///
/// On Windows this is the owner-PID tables. Elsewhere it is
/// [`CollectError::Unsupported`], not an empty success.
pub fn collect() -> Result<Vec<ListenRecord>, CollectError> {
    backend::collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, UdpSocket};
    use std::time::Duration;

    #[test]
    fn unavailable_is_its_own_reading() {
        assert_eq!(UNAVAILABLE_LABEL, "信息不可用");
        let missing: Readable<u32> = Readable::Unavailable;
        assert!(missing.is_unavailable());
        assert!(missing.as_ref().is_none());
        assert_eq!(Readable::Known(7u32).as_ref(), Some(&7));
        assert_ne!(Readable::Known(0u32), Readable::Unavailable);
    }

    #[test]
    fn an_empty_success_is_a_check_and_a_failure_is_not() {
        let mut state = RefreshState::new();
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        state.complete(Attempt::Succeeded(Snapshot {
            at,
            records: Vec::new(),
        }));

        assert!(state.failure().is_none());
        assert!(!state.in_progress());
        let success = state
            .last_success()
            .expect("an empty list is still a success");
        assert_eq!(success.at, at);
        assert!(success.records.is_empty());

        state.begin();
        assert!(state.in_progress());
        assert_eq!(state.last_success().unwrap().at, at);

        state.complete(Attempt::Failed(CollectError::Unsupported));
        assert!(!state.in_progress());
        assert_eq!(state.failure(), Some(&CollectError::Unsupported));
        assert_eq!(state.last_success().unwrap().at, at);
        assert!(state.last_success().unwrap().records.is_empty());
    }

    #[test]
    fn a_failed_first_check_has_no_list() {
        let mut state = RefreshState::new();
        state.begin();
        assert!(state.in_progress());
        assert!(state.last_success().is_none());

        state.complete(Attempt::Failed(CollectError::Table {
            protocol: Protocol::Tcp,
            family: IpFamily::V4,
            code: 5,
        }));

        assert!(!state.in_progress());
        assert!(state.last_success().is_none());
        assert!(matches!(
            state.failure(),
            Some(CollectError::Table {
                protocol: Protocol::Tcp,
                family: IpFamily::V4,
                code: 5,
            })
        ));
    }

    #[cfg(not(windows))]
    #[test]
    fn refresh_reports_unsupported_instead_of_an_empty_list() {
        let mut state = RefreshState::new();
        state.refresh();
        assert!(state.last_success().is_none());
        assert_eq!(state.failure(), Some(&CollectError::Unsupported));
        assert!(!state.in_progress());
    }

    #[cfg(windows)]
    #[test]
    fn table_addresses_and_ports_use_network_order() {
        // 127.0.0.1 stored as network-order octets in a little-endian u32.
        assert_eq!(
            win::ipv4(u32::from_le_bytes([127, 0, 0, 1])),
            Ipv4Addr::LOCALHOST
        );
        // Port 8080 (0x1F90) in network order in the low 16 bits.
        assert_eq!(win::tcp_port(u32::from_le_bytes([0x1F, 0x90, 0, 0])), 8080);
    }

    #[cfg(windows)]
    #[test]
    fn a_missing_owner_field_is_unavailable_and_the_pid_stays() {
        let (pid, name, path) = win::owner_reading(0);
        assert!(pid.is_unavailable());
        assert!(name.is_unavailable());
        assert!(path.is_unavailable());

        // Not a live process. The pid was still named; the name and path were not read.
        let (pid, name, path) = win::owner_reading(u32::MAX);
        assert_eq!(pid, Readable::Known(u32::MAX));
        assert!(name.is_unavailable());
        assert!(path.is_unavailable());
    }

    #[cfg(windows)]
    #[test]
    fn a_real_bind_is_collected_and_a_closed_socket_is_not() {
        let tcp4 = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind tcp/ipv4");
        let tcp6 = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).expect("bind tcp/ipv6");
        let udp4 = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind udp/ipv4");
        let udp6 = UdpSocket::bind((Ipv6Addr::LOCALHOST, 0)).expect("bind udp/ipv6");

        let tcp4_addr = tcp4.local_addr().expect("tcp/ipv4 address");
        let tcp6_addr = tcp6.local_addr().expect("tcp/ipv6 address");
        let udp4_addr = udp4.local_addr().expect("udp/ipv4 address");
        let udp6_addr = udp6.local_addr().expect("udp/ipv6 address");

        let records = collect().expect("reading the owner tables");
        let tcp4_row = expect_row(&records, Protocol::Tcp, &tcp4_addr);
        let tcp6_row = expect_row(&records, Protocol::Tcp, &tcp6_addr);
        let udp4_row = expect_row(&records, Protocol::Udp, &udp4_addr);
        let udp6_row = expect_row(&records, Protocol::Udp, &udp6_addr);

        assert_ne!(tcp4_row.protocol, udp4_row.protocol);
        assert!(tcp4_row.address.ip.is_ipv4());
        assert!(udp4_row.address.ip.is_ipv4());
        assert!(tcp6_row.address.ip.is_ipv6());
        assert!(udp6_row.address.ip.is_ipv6());
        assert_ours(tcp4_row);
        assert_ours(tcp6_row);
        assert_ours(udp4_row);
        assert_ours(udp6_row);

        drop(tcp4);
        drop(tcp6);
        drop(udp4);
        drop(udp6);

        assert_gone(Protocol::Tcp, &tcp4_addr);
        assert_gone(Protocol::Tcp, &tcp6_addr);
        assert_gone(Protocol::Udp, &udp4_addr);
        assert_gone(Protocol::Udp, &udp6_addr);
    }

    #[cfg(windows)]
    #[test]
    fn refresh_keeps_a_real_snapshot_when_a_later_attempt_fails() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind tcp/ipv4");
        let address = listener.local_addr().expect("bound address");

        let mut state = RefreshState::new();
        let before = SystemTime::now();
        state.refresh();
        assert!(!state.in_progress());
        assert!(state.failure().is_none());
        let success = state.last_success().expect("the refresh collected");
        let age = SystemTime::now()
            .duration_since(success.at)
            .expect("the check time is not in the future");
        assert!(age < Duration::from_secs(30));
        assert!(success.at >= before);
        expect_row(&success.records, Protocol::Tcp, &address);
        let kept_at = success.at;
        let kept_len = success.records.len();

        // The socket is still open, so the stored rows are a real collection.
        // The failure is injected: the OS call cannot be forced to fail, and
        // this must not restamp that collection as if it had just succeeded.
        state.begin();
        assert!(state.in_progress());
        state.complete(Attempt::Failed(CollectError::Table {
            protocol: Protocol::Tcp,
            family: IpFamily::V4,
            code: 5,
        }));
        assert!(!state.in_progress());
        assert!(state.failure().is_some());
        let stale = state.last_success().expect("the old list is still held");
        assert_eq!(stale.at, kept_at);
        assert_eq!(stale.records.len(), kept_len);
        expect_row(&stale.records, Protocol::Tcp, &address);

        drop(listener);
        assert_gone(Protocol::Tcp, &address);

        // The stored list still describes the open socket. A failed attempt did
        // not replace it with the closed one, and did not call that old list current.
        expect_row(
            &state.last_success().unwrap().records,
            Protocol::Tcp,
            &address,
        );
        assert_eq!(state.last_success().unwrap().at, kept_at);

        state.refresh();
        assert!(state.failure().is_none());
        let fresh = state.last_success().unwrap();
        assert_ne!(fresh.at, kept_at);
        assert!(
            row(&fresh.records, Protocol::Tcp, &address).is_none(),
            "the closed socket is gone from the new success"
        );
    }

    #[cfg(windows)]
    fn expect_row<'a>(
        records: &'a [ListenRecord],
        protocol: Protocol,
        address: &SocketAddr,
    ) -> &'a ListenRecord {
        row(records, protocol, address).unwrap_or_else(|| {
            let same_port: Vec<_> = records
                .iter()
                .filter(|record| record.port == address.port())
                .map(|record| format!("{record:?}"))
                .collect();
            panic!(
                "missing {protocol} {address} among {} rows; same port: {}",
                records.len(),
                same_port.join(" | ")
            )
        })
    }

    #[cfg(windows)]
    fn row<'a>(
        records: &'a [ListenRecord],
        protocol: Protocol,
        address: &SocketAddr,
    ) -> Option<&'a ListenRecord> {
        records.iter().find(|record| {
            record.protocol == protocol
                && record.address.ip == address.ip()
                && record.port == address.port()
        })
    }

    #[cfg(windows)]
    fn assert_ours(record: &ListenRecord) {
        let pid = std::process::id();
        assert_eq!(record.pid, Readable::Known(pid), "owner pid");
        let exe = std::env::current_exe().expect("this test process has a path");
        let file_name = exe
            .file_name()
            .expect("the test executable has a file name")
            .to_string_lossy();
        match &record.process_name {
            Readable::Known(name) => assert!(
                name.eq_ignore_ascii_case(&file_name),
                "process name {name}, executable {file_name}"
            ),
            Readable::Unavailable => panic!("this process's name should be readable"),
        }
        match &record.program_path {
            Readable::Known(path) => {
                assert!(path.is_absolute(), "program path {path:?}");
                let got = path
                    .file_name()
                    .expect("program path has a file name")
                    .to_string_lossy();
                assert!(
                    got.eq_ignore_ascii_case(&file_name),
                    "program path {path:?}, executable {file_name}"
                );
            }
            Readable::Unavailable => {
                panic!("this process's path should be readable without administrator")
            }
        }
    }

    #[cfg(windows)]
    fn assert_gone(protocol: Protocol, address: &SocketAddr) {
        let pid = Readable::Known(std::process::id());
        for _ in 0..20 {
            let records = collect().expect("reading the owner tables after close");
            let still = records.iter().any(|record| {
                record.protocol == protocol
                    && record.address.ip == address.ip()
                    && record.port == address.port()
                    && record.pid == pid
            });
            if !still {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("{protocol} {address} was still attributed to this process after the socket closed");
    }
}
