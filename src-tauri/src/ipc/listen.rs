//! The one window command for listening ports (#98).
//!
//! It reads. It collects the OS owner tables, snapshots running sessions, and
//! attributes each row. It does not bind a socket, send HTTP, or stop a
//! process. A failed collection is part of the payload: it carries no success
//! time and no rows, so a caller cannot present an older list as a check that
//! just finished. The previous list, if the window still has one, stays there.

use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::State;

use crate::listen::{
    attribute, Attribution, CollectError, ListenAddress, ListenRecord, Protocol, Readable,
};
use crate::session::core::SessionCore;

/// One attributed listener, as the window renders it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenerRowDto {
    /// `"TCP"` or `"UDP"`.
    pub protocol: String,
    pub address: String,
    pub port: u16,
    /// `None` when the owner pid could not be read.
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub program_path: Option<String>,
    /// `"session"`, `"external"`, or `"unavailable"`.
    pub attribution: &'static str,
    /// Set only for [`Attribution::Session`]. The window looks the name up.
    pub session_id: Option<String>,
}

/// This attempt's outcome.
///
/// `checked_at_ms` is set only when the collection succeeded, including when
/// nothing is listening. `failure` is set only when it did not. `rows` on a
/// failure is empty, and that empty list is not a successful check.
/// `in_progress` is false on return: the command finishes the attempt it started.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenerListDto {
    pub checked_at_ms: Option<u64>,
    pub in_progress: bool,
    pub failure: Option<String>,
    pub rows: Vec<ListenerRowDto>,
}

/// List listening ports and who holds each one.
#[tauri::command]
pub fn list_listeners(core: State<'_, SessionCore>) -> ListenerListDto {
    listeners_from(core.inner())
}

fn listeners_from(core: &SessionCore) -> ListenerListDto {
    match crate::listen::collect() {
        Ok(records) => {
            let sessions = core.listening_processes();
            let attributed = attribute(&records, &sessions);
            ListenerListDto {
                checked_at_ms: Some(unix_millis(SystemTime::now())),
                in_progress: false,
                failure: None,
                rows: attributed
                    .into_iter()
                    .map(|row| row_dto(row.record, row.attribution))
                    .collect(),
            }
        }
        Err(error) => failed_attempt(&error),
    }
}

fn failed_attempt(error: &CollectError) -> ListenerListDto {
    ListenerListDto {
        checked_at_ms: None,
        in_progress: false,
        failure: Some(error.to_string()),
        rows: Vec::new(),
    }
}

fn row_dto(record: ListenRecord, attribution: Attribution) -> ListenerRowDto {
    let (kind, session_id) = match attribution {
        Attribution::Session(id) => ("session", Some(id)),
        Attribution::External => ("external", None),
        Attribution::Unavailable => ("unavailable", None),
    };
    ListenerRowDto {
        protocol: match record.protocol {
            Protocol::Tcp => "TCP".to_owned(),
            Protocol::Udp => "UDP".to_owned(),
        },
        address: format_address(&record.address),
        port: record.port,
        pid: known(record.pid),
        process_name: known(record.process_name),
        program_path: known(record.program_path).map(|path| path.to_string_lossy().into_owned()),
        attribution: kind,
        session_id,
    }
}

fn known<T>(value: Readable<T>) -> Option<T> {
    match value {
        Readable::Known(value) => Some(value),
        Readable::Unavailable => None,
    }
}

fn format_address(address: &ListenAddress) -> String {
    match address.ip {
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) if address.scope_id == 0 => ip.to_string(),
        IpAddr::V6(ip) => format!("{ip}%{}", address.scope_id),
    }
}

fn unix_millis(at: SystemTime) -> u64 {
    at.duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    use crate::listen::ListenAddress;

    #[test]
    fn a_failed_attempt_has_no_success_time_and_no_rows() {
        let dto = failed_attempt(&CollectError::Unsupported);
        assert!(dto.checked_at_ms.is_none());
        assert!(dto.rows.is_empty());
        assert!(!dto.in_progress);
        assert_eq!(
            dto.failure.as_deref(),
            Some("listening-port collection is not supported on this platform")
        );

        let value = serde_json::to_value(&dto).expect("the failure serializes");
        assert!(value.get("checkedAtMs").unwrap().is_null());
        assert!(value.get("failure").unwrap().is_string());
        assert!(value.get("checked_at_ms").is_none(), "snake_case leaked");
        assert!(value.get("rows").unwrap().as_array().unwrap().is_empty());
    }

    #[test]
    fn an_address_keeps_family_and_a_non_zero_scope() {
        let v4 = ListenAddress {
            ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            scope_id: 0,
        };
        let v6 = ListenAddress {
            ip: IpAddr::V6(Ipv6Addr::LOCALHOST),
            scope_id: 0,
        };
        let scoped = ListenAddress {
            ip: IpAddr::V6(Ipv6Addr::LOCALHOST),
            scope_id: 12,
        };
        assert_eq!(format_address(&v4), "127.0.0.1");
        assert_eq!(format_address(&v6), "::1");
        assert_eq!(format_address(&scoped), "::1%12");
    }

    #[test]
    fn unavailable_fields_stay_absent_and_external_has_no_session() {
        let record = ListenRecord {
            protocol: Protocol::Udp,
            address: ListenAddress {
                ip: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                scope_id: 0,
            },
            port: 1,
            pid: Readable::Unavailable,
            process_name: Readable::Unavailable,
            program_path: Readable::Unavailable,
        };
        let dto = row_dto(record, Attribution::External);
        assert_eq!(dto.protocol, "UDP");
        assert!(dto.pid.is_none());
        assert!(dto.process_name.is_none());
        assert!(dto.program_path.is_none());
        assert_eq!(dto.attribution, "external");
        assert!(dto.session_id.is_none());
        assert_ne!(dto.attribution, "session");
    }

    #[cfg(windows)]
    #[test]
    fn a_socket_with_no_managed_session_is_listed_as_external_and_then_closed() {
        use std::net::{TcpListener, UdpSocket};

        let tcp = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind tcp");
        let udp = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind udp");
        let tcp_port = tcp.local_addr().expect("tcp address").port();
        let udp_port = udp.local_addr().expect("udp address").port();

        let core = SessionCore::without_listener();
        let dto = listeners_from(&core);
        assert!(
            dto.failure.is_none(),
            "the tables were readable: {:?}",
            dto.failure
        );
        assert!(dto.checked_at_ms.is_some());
        assert!(!dto.in_progress);
        assert!(
            core.listening_processes().is_empty(),
            "this test does not start a session"
        );

        let tcp_row = dto
            .rows
            .iter()
            .find(|row| row.protocol == "TCP" && row.port == tcp_port && row.address == "127.0.0.1")
            .expect("the tcp socket is in the list");
        let udp_row = dto
            .rows
            .iter()
            .find(|row| row.protocol == "UDP" && row.port == udp_port && row.address == "127.0.0.1")
            .expect("the udp socket is in the list");
        assert_eq!(tcp_row.attribution, "external");
        assert!(tcp_row.session_id.is_none());
        assert_eq!(udp_row.attribution, "external");
        assert_eq!(tcp_row.pid, Some(std::process::id()));
        assert!(tcp_row.process_name.is_some());

        drop(tcp);
        drop(udp);
    }
}
