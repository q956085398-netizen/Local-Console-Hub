//! Windows listening-port collector.
//!
//! TCP listeners come from `GetExtendedTcpTable` with
//! `TCP_TABLE_OWNER_PID_LISTENER`, so an established connection is not reported
//! as something listening. UDP has no listen state; `GetExtendedUdpTable` with
//! `UDP_TABLE_OWNER_PID` is every bound UDP endpoint, and a bound port is the
//! occupancy a UDP record is for.
//!
//! Both calls are reads. Nothing here terminates a process, connects a socket,
//! or sends HTTP. A row the OS returned is kept even when the owner's name or
//! path cannot be opened: that field is [`Readable::Unavailable`](super::Readable::Unavailable).

use std::collections::HashMap;
use std::mem::{size_of, size_of_val};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::PathBuf;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_INSUFFICIENT_BUFFER, ERROR_NO_MORE_FILES, HANDLE,
    INVALID_HANDLE_VALUE, NO_ERROR,
};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, GetExtendedUdpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID,
    MIB_UDP6ROW_OWNER_PID, MIB_UDPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER, UDP_TABLE_OWNER_PID,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};

use super::{CollectError, IpFamily, ListenAddress, ListenRecord, Protocol, Readable};

/// `AF_INET`. Numeric so this module does not pull in the WinSock feature.
const AF_INET: u32 = 2;
/// `AF_INET6`.
const AF_INET6: u32 = 23;

/// How many times to re-read a table that grew between the size query and the copy.
const TABLE_ATTEMPTS: usize = 5;

/// How many times to re-take a process snapshot that came back short.
const SNAPSHOT_ATTEMPTS: usize = 3;

struct RawRow {
    protocol: Protocol,
    address: ListenAddress,
    port: u16,
    pid: u32,
}

pub(super) fn collect() -> Result<Vec<ListenRecord>, CollectError> {
    let mut raw = Vec::new();
    raw.extend(tcp_rows(IpFamily::V4)?);
    raw.extend(tcp_rows(IpFamily::V6)?);
    raw.extend(udp_rows(IpFamily::V4)?);
    raw.extend(udp_rows(IpFamily::V6)?);

    let names = process_names();
    let mut owners = HashMap::new();
    let mut records = Vec::with_capacity(raw.len());
    for row in raw {
        let (pid, process_name, program_path) = owner_of(row.pid, &names, &mut owners);
        records.push(ListenRecord {
            protocol: row.protocol,
            address: row.address,
            port: row.port,
            pid,
            process_name,
            program_path,
        });
    }
    Ok(records)
}

/// PID, process name, and program path for `pid`, as far as this process may look.
///
/// A pid of zero is not an owner the table could name. Any other pid is kept
/// even when the process has already exited or this process may not open it:
/// the missing name or path is unavailable, and the row stays.
///
/// Test-only: production collection goes through [`collect`], which uses the
/// same helper. This entry exists so a missing field can be checked without
/// inventing a listening row.
#[cfg(test)]
pub(super) fn owner_reading(pid: u32) -> (Readable<u32>, Readable<String>, Readable<PathBuf>) {
    let names = process_names();
    let mut owners = HashMap::new();
    owner_of(pid, &names, &mut owners)
}

fn owner_of(
    pid: u32,
    names: &HashMap<u32, String>,
    cache: &mut HashMap<u32, (Readable<String>, Readable<PathBuf>)>,
) -> (Readable<u32>, Readable<String>, Readable<PathBuf>) {
    if pid == 0 {
        return (
            Readable::Unavailable,
            Readable::Unavailable,
            Readable::Unavailable,
        );
    }
    let (name, path) = cache.entry(pid).or_insert_with(|| {
        let path = program_path(pid);
        let name = process_name(pid, names, &path);
        (name, path)
    });
    (Readable::Known(pid), name.clone(), path.clone())
}

fn process_name(
    pid: u32,
    names: &HashMap<u32, String>,
    path: &Readable<PathBuf>,
) -> Readable<String> {
    if let Some(name) = names.get(&pid).filter(|name| !name.is_empty()) {
        return Readable::Known(name.clone());
    }
    // The image path's file name is still the process name when the snapshot
    // missed this pid. It is not a guess at a name the OS never gave us.
    if let Readable::Known(path) = path {
        if let Some(name) = path.file_name() {
            let name = name.to_string_lossy();
            if !name.is_empty() {
                return Readable::Known(name.into_owned());
            }
        }
    }
    Readable::Unavailable
}

/// Win32 path of `pid`, or unavailable when the process cannot be opened or the
/// query fails. Both are the same field state: the path was not read.
fn program_path(pid: u32) -> Readable<PathBuf> {
    // SAFETY: `OpenProcess` either returns a handle this function closes, or null.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return Readable::Unavailable;
    }
    let _opened = Opened(handle);

    // `MAX_PATH` is not the limit here. The documented bound for a long path is
    // 32_767 wide characters, and the size is passed in and out.
    let mut buffer = vec![0u16; 32_768];
    let mut size = buffer.len() as u32;
    // SAFETY: `handle` is open, `buffer` is writable for `size` wide characters,
    // and `size` is in/out as the API requires.
    let read = unsafe {
        QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, buffer.as_mut_ptr(), &mut size)
    };
    if read == 0 {
        return Readable::Unavailable;
    }
    buffer.truncate(size as usize);
    let path = PathBuf::from(String::from_utf16_lossy(&buffer));
    if path.as_os_str().is_empty() {
        Readable::Unavailable
    } else {
        Readable::Known(path)
    }
}

/// One complete process snapshot, keyed by pid. Empty when no complete snapshot
/// could be taken: names are then unavailable, and the port rows are still returned.
fn process_names() -> HashMap<u32, String> {
    for _ in 0..SNAPSHOT_ATTEMPTS {
        if let Some(names) = one_process_snapshot() {
            return names;
        }
    }
    HashMap::new()
}

fn one_process_snapshot() -> Option<HashMap<u32, String>> {
    // SAFETY: a snapshot handle is closed by `Snapshot`'s drop, including when
    // the walk below returns early.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot.is_null() || snapshot == INVALID_HANDLE_VALUE {
        return None;
    }
    let _snapshot = Snapshot(snapshot);

    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
    // SAFETY: `entry.dwSize` is set, and `snapshot` is a process snapshot.
    if unsafe { Process32FirstW(snapshot, &mut entry) } == 0 {
        return None;
    }

    let mut names = HashMap::new();
    loop {
        let name = exe_name(&entry.szExeFile);
        if !name.is_empty() {
            names.insert(entry.th32ProcessID, name);
        }
        // SAFETY: same snapshot and entry as `Process32FirstW`.
        if unsafe { Process32NextW(snapshot, &mut entry) } == 0 {
            // `ERROR_NO_MORE_FILES` is the end of a complete list. Anything else,
            // including `ERROR_BAD_LENGTH` when a process appears or leaves
            // mid-copy, is a short map. A short map would mark a live owner
            // unavailable, so the caller retries instead of keeping it.
            let ended = unsafe { GetLastError() } == ERROR_NO_MORE_FILES;
            return ended.then_some(names);
        }
    }
}

fn exe_name(file: &[u16]) -> String {
    let end = file
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(file.len());
    String::from_utf16_lossy(&file[..end])
}

fn tcp_rows(family: IpFamily) -> Result<Vec<RawRow>, CollectError> {
    let af = family_af(family);
    let buf = read_table(|ptr, size| {
        // SAFETY: `read_table` passes either null with a size pointer, or a
        // buffer at least `*size` bytes long. The class is the listener table.
        unsafe { GetExtendedTcpTable(ptr, size, 0, af, TCP_TABLE_OWNER_PID_LISTENER, 0) }
    })
    .map_err(|code| CollectError::Table {
        protocol: Protocol::Tcp,
        family,
        code,
    })?;

    match family {
        IpFamily::V4 => map_rows(&buf, Protocol::Tcp, family, |row: MIB_TCPROW_OWNER_PID| {
            RawRow {
                protocol: Protocol::Tcp,
                address: ListenAddress {
                    ip: IpAddr::V4(ipv4(row.dwLocalAddr)),
                    scope_id: 0,
                },
                port: tcp_port(row.dwLocalPort),
                pid: row.dwOwningPid,
            }
        }),
        IpFamily::V6 => map_rows(&buf, Protocol::Tcp, family, |row: MIB_TCP6ROW_OWNER_PID| {
            RawRow {
                protocol: Protocol::Tcp,
                address: ListenAddress {
                    ip: IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
                    scope_id: row.dwLocalScopeId,
                },
                port: tcp_port(row.dwLocalPort),
                pid: row.dwOwningPid,
            }
        }),
    }
}

fn udp_rows(family: IpFamily) -> Result<Vec<RawRow>, CollectError> {
    let af = family_af(family);
    let buf = read_table(|ptr, size| {
        // SAFETY: same contract as the TCP read. The class is the owner-PID
        // table, which is every bound UDP endpoint.
        unsafe { GetExtendedUdpTable(ptr, size, 0, af, UDP_TABLE_OWNER_PID, 0) }
    })
    .map_err(|code| CollectError::Table {
        protocol: Protocol::Udp,
        family,
        code,
    })?;

    match family {
        IpFamily::V4 => map_rows(&buf, Protocol::Udp, family, |row: MIB_UDPROW_OWNER_PID| {
            RawRow {
                protocol: Protocol::Udp,
                address: ListenAddress {
                    ip: IpAddr::V4(ipv4(row.dwLocalAddr)),
                    scope_id: 0,
                },
                port: tcp_port(row.dwLocalPort),
                pid: row.dwOwningPid,
            }
        }),
        IpFamily::V6 => map_rows(&buf, Protocol::Udp, family, |row: MIB_UDP6ROW_OWNER_PID| {
            RawRow {
                protocol: Protocol::Udp,
                address: ListenAddress {
                    ip: IpAddr::V6(Ipv6Addr::from(row.ucLocalAddr)),
                    scope_id: row.dwLocalScopeId,
                },
                port: tcp_port(row.dwLocalPort),
                pid: row.dwOwningPid,
            }
        }),
    }
}

fn family_af(family: IpFamily) -> u32 {
    match family {
        IpFamily::V4 => AF_INET,
        IpFamily::V6 => AF_INET6,
    }
}

fn map_rows<T: Copy>(
    buf: &[u32],
    protocol: Protocol,
    family: IpFamily,
    to_raw: impl FnMut(T) -> RawRow,
) -> Result<Vec<RawRow>, CollectError> {
    parse_rows(buf)
        .map(|rows| rows.into_iter().map(to_raw).collect())
        .map_err(|_| CollectError::Truncated { protocol, family })
}

/// Copy `T` rows out of an owner table.
///
/// The buffer is a `u32` count followed by that many `T` rows. An empty buffer
/// is an empty table, which is a successful read of nothing — not a truncated one.
fn parse_rows<T: Copy>(buf: &[u32]) -> Result<Vec<T>, ()> {
    if buf.is_empty() {
        return Ok(Vec::new());
    }
    let count = buf[0] as usize;
    let header = size_of::<u32>();
    let row = size_of::<T>();
    let need = count
        .checked_mul(row)
        .and_then(|rows| rows.checked_add(header))
        .ok_or(())?;
    if size_of_val(buf) < need {
        return Err(());
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: `buf` is the bytes `GetExtendedTcpTable` / `GetExtendedUdpTable`
    // wrote: a `u32` count, then `count` rows of `T`. The byte length covers
    // those rows. `buf` is aligned for `u32`, and every row type here is a
    // `repr(C)` struct whose alignment is at most that.
    let rows = unsafe {
        let ptr = (buf.as_ptr() as *const u8).add(header) as *const T;
        std::slice::from_raw_parts(ptr, count)
    };
    Ok(rows.to_vec())
}

/// Read one extended TCP or UDP table, retrying when it grows mid-call.
fn read_table(fill: impl Fn(*mut core::ffi::c_void, *mut u32) -> u32) -> Result<Vec<u32>, u32> {
    let mut size = 0u32;
    let mut code = fill(std::ptr::null_mut(), &mut size);
    if code == NO_ERROR {
        return Ok(Vec::new());
    }
    if code != ERROR_INSUFFICIENT_BUFFER {
        return Err(code);
    }

    for _ in 0..TABLE_ATTEMPTS {
        let words = (size as usize).div_ceil(size_of::<u32>());
        if words == 0 {
            return Err(code);
        }
        let mut buf = vec![0u32; words];
        let mut capacity = (buf.len() * size_of::<u32>()) as u32;
        code = fill(buf.as_mut_ptr() as *mut core::ffi::c_void, &mut capacity);
        if code == NO_ERROR {
            return Ok(buf);
        }
        if code != ERROR_INSUFFICIENT_BUFFER {
            return Err(code);
        }
        // `capacity` is the size the call asked for on the way out.
        size = capacity.max(size.saturating_add(size_of::<u32>() as u32));
    }
    Err(code)
}

/// IPv4 address from an owner-table `DWORD`.
///
/// The field is network-order octets stored in the integer's memory. On
/// little-endian Windows, `127.0.0.1` therefore has the numeric value
/// `0x0100_007F`, and the address is those native bytes.
pub(super) fn ipv4(addr: u32) -> Ipv4Addr {
    Ipv4Addr::from(addr.to_ne_bytes())
}

/// Port from an owner-table `DWORD`. The port is network-order in the low 16 bits.
pub(super) fn tcp_port(value: u32) -> u16 {
    u16::from_be(value as u16)
}

struct Opened(HANDLE);

impl Drop for Opened {
    fn drop(&mut self) {
        // SAFETY: the handle was a non-null `OpenProcess` result, closed once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct Snapshot(HANDLE);

impl Drop for Snapshot {
    fn drop(&mut self) {
        // SAFETY: the handle was a successful toolhelp snapshot, closed once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
