//! CPU and memory of a managed session's own process (#108).
//!
//! The processes come from [`SessionCore::listening_processes`](crate::session::core::SessionCore::listening_processes):
//! the session id, the process identity, and the tree. This module does not
//! start, stop, signal, or replace a run, and it does not walk the machine's
//! process table looking for names.
//!
//! A pid is not an identity. Windows reuses the number, so a reading is
//! returned only when the creation time still matches. A field that could not
//! be read is absent. It is not stored as zero. A tree of `None` is an unread
//! tree, not an empty one and not a guess at whichever processes happen to
//! share a name.

use std::time::{Duration, Instant, SystemTime};

use crate::listen::SessionProcess;
use crate::process::ProcessIdentity;

#[cfg(not(windows))]
mod unsupported;
#[cfg(windows)]
mod win;

#[cfg(not(windows))]
use unsupported as backend;
#[cfg(windows)]
use win as backend;

/// How long one check waits between the two CPU samples.
///
/// CPU use is a rate. One sample is a clock reading, and reporting that as 0%
/// would be the guess this module refuses. The wait happens once per check,
/// after the session's own process has already matched, not once per process
/// and not at all when the identity does not match.
const SAMPLE_GAP: Duration = Duration::from_millis(200);

/// One process whose creation time was just confirmed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfirmedReading {
    pid: u32,
    created_at: u64,
    /// Kernel plus user time, in the 100-ns units `GetProcessTimes` reports.
    /// `None` only when a test double supplies no clock. A Windows reading
    /// that confirmed the creation time has this, because both come from the
    /// same call.
    cpu_time_100ns: Option<u64>,
    /// Working set, in bytes. `None` when the memory counters could not be read.
    memory_bytes: Option<u64>,
}

/// Where a confirmed process sits in the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberRole {
    /// The session's own process.
    Own,
    /// A member of the listed tree, other than the session's own process.
    Tree,
}

impl MemberRole {
    /// The word the window's DTO uses.
    pub fn as_str(self) -> &'static str {
        match self {
            MemberRole::Own => "own",
            MemberRole::Tree => "tree",
        }
    }
}

/// One process included in a check that finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberReading {
    pub pid: u32,
    pub created_at: u64,
    pub role: MemberRole,
    /// Hundredths of one percent of this machine's processors over the sample
    /// interval. `10_000` is the whole machine. `None` when the rate could not
    /// be computed. `Some(0)` is a measured zero, not a missing field.
    pub cpu_percent_hundredths: Option<u32>,
    /// Working set in bytes at the later sample. `None` when it could not be read.
    pub memory_bytes: Option<u64>,
}

/// The check for one session.
///
/// `checked_at` is set only when the session's own process still matched at
/// the end of the check. Without it, `members` is empty: an earlier sample is
/// not a check that just finished, and a pid Windows has reused is not this
/// session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReading {
    pub session_id: String,
    pub checked_at: Option<SystemTime>,
    /// The tree could not be listed. The members are then only the session's
    /// own process, when that process still matched. This is not a claim that
    /// the tree was empty.
    pub tree_unavailable: bool,
    pub members: Vec<MemberReading>,
}

impl SessionReading {
    fn unconfirmed(session_id: String) -> Self {
        Self {
            session_id,
            checked_at: None,
            tree_unavailable: false,
            members: Vec::new(),
        }
    }
}

/// Numbers an observer is willing to attribute to one exact identity.
struct Observed {
    pid: u32,
    created_at: u64,
    cpu_percent_hundredths: Option<u32>,
    memory_bytes: Option<u64>,
}

/// Read one process if, and only if, it is still the process `created_at` names.
///
/// `None` means the pid is gone or the process now under that pid was created
/// at a different time. The counters of whichever process is there now are not
/// returned.
fn read_once(pid: u32, created_at: u64) -> Option<ConfirmedReading> {
    backend::read_once(pid, created_at)
}

/// Share of the machine's processors, in hundredths of a percent.
///
/// `earlier` and `later` are kernel-plus-user time in 100-ns units. `wall_100ns`
/// is how long passed between those samples, on the same scale. `None` when
/// the rate cannot be computed: no wall time, no processor count, or a CPU
/// clock that moved backwards. A process that used no CPU in the interval is
/// `Some(0)`.
fn cpu_percent_hundredths(
    earlier: u64,
    later: u64,
    wall_100ns: u128,
    processors: u32,
) -> Option<u32> {
    if wall_100ns == 0 || processors == 0 {
        return None;
    }
    let cpu_delta = later.checked_sub(earlier)?;
    let denom = wall_100ns.checked_mul(u128::from(processors))?;
    let numer = u128::from(cpu_delta).checked_mul(10_000)?;
    u32::try_from(numer / denom).ok()
}

fn processor_count() -> u32 {
    std::thread::available_parallelism()
        .ok()
        .and_then(|count| u32::try_from(count.get()).ok())
        .unwrap_or(0)
}

/// Assemble one session from an observer that already checked creation times.
///
/// The observer is asked for the session's own identity first. If it declines,
/// the tree is not consulted and nothing is published: a tree is not a way to
/// keep another process's numbers under a session whose own pid no longer
/// matches. An observer that answers with a different pid or creation time is
/// declined the same way. `None` for the tree means the tree was not listed;
/// this function does not invent members to fill it.
fn describe_session(
    session: &SessionProcess,
    now: SystemTime,
    mut observe: impl FnMut(u32, u64) -> Option<Observed>,
) -> SessionReading {
    let Some(own) = confirmed_observation(
        &mut observe,
        session.identity.pid(),
        session.identity.created_at(),
    ) else {
        return SessionReading::unconfirmed(session.id.clone());
    };
    let mut members = vec![member(own, MemberRole::Own)];
    match &session.tree {
        None => SessionReading {
            session_id: session.id.clone(),
            checked_at: Some(now),
            tree_unavailable: true,
            members,
        },
        Some(tree) => {
            for identity in tree {
                if same_identity(
                    identity,
                    session.identity.pid(),
                    session.identity.created_at(),
                ) {
                    continue;
                }
                if members
                    .iter()
                    .any(|existing| same_identity(identity, existing.pid, existing.created_at))
                {
                    continue;
                }
                let Some(reading) =
                    confirmed_observation(&mut observe, identity.pid(), identity.created_at())
                else {
                    continue;
                };
                members.push(member(reading, MemberRole::Tree));
            }
            SessionReading {
                session_id: session.id.clone(),
                checked_at: Some(now),
                tree_unavailable: false,
                members,
            }
        }
    }
}

fn confirmed_observation(
    observe: &mut impl FnMut(u32, u64) -> Option<Observed>,
    pid: u32,
    created_at: u64,
) -> Option<Observed> {
    let reading = observe(pid, created_at)?;
    if reading.pid == pid && reading.created_at == created_at {
        Some(reading)
    } else {
        None
    }
}

fn same_identity(identity: &ProcessIdentity, pid: u32, created_at: u64) -> bool {
    identity.pid() == pid && identity.created_at() == created_at
}

fn member(reading: Observed, role: MemberRole) -> MemberReading {
    MemberReading {
        pid: reading.pid,
        created_at: reading.created_at,
        role,
        cpu_percent_hundredths: reading.cpu_percent_hundredths,
        memory_bytes: reading.memory_bytes,
    }
}

/// Read the session's own process and the tree members that still match.
///
/// The session's own creation time is checked before any tree member is
/// opened. When it does not match, the result carries no time and no members,
/// so a sample taken earlier cannot be presented as this check. CPU percent
/// comes from two samples separated by [`SAMPLE_GAP`]. Memory is the working
/// set at the later sample.
pub fn read_session(session: &SessionProcess) -> SessionReading {
    let own_pid = session.identity.pid();
    let own_created = session.identity.created_at();
    let Some(first_own) = read_once(own_pid, own_created) else {
        return SessionReading::unconfirmed(session.id.clone());
    };
    let mut first = vec![first_own];
    remember_tree(session, own_pid, own_created, &mut first);

    let started = Instant::now();
    std::thread::sleep(SAMPLE_GAP);
    let wall_100ns = started.elapsed().as_nanos() / 100;

    let Some(second_own) = read_once(own_pid, own_created) else {
        return SessionReading::unconfirmed(session.id.clone());
    };
    let mut second = vec![second_own];
    remember_tree(session, own_pid, own_created, &mut second);

    let processors = processor_count();
    describe_session(session, SystemTime::now(), |pid, created| {
        let later = second
            .iter()
            .find(|reading| reading.pid == pid && reading.created_at == created)?;
        let earlier = first
            .iter()
            .find(|reading| reading.pid == pid && reading.created_at == created);
        Some(Observed {
            pid: later.pid,
            created_at: later.created_at,
            cpu_percent_hundredths: earlier.and_then(|prior| {
                match (prior.cpu_time_100ns, later.cpu_time_100ns) {
                    (Some(before), Some(after)) => {
                        cpu_percent_hundredths(before, after, wall_100ns, processors)
                    }
                    _ => None,
                }
            }),
            memory_bytes: later.memory_bytes,
        })
    })
}

fn remember_tree(
    session: &SessionProcess,
    own_pid: u32,
    own_created: u64,
    into: &mut Vec<ConfirmedReading>,
) {
    let Some(tree) = &session.tree else {
        return;
    };
    for identity in tree {
        if same_identity(identity, own_pid, own_created) {
            continue;
        }
        if into.iter().any(|reading| {
            reading.pid == identity.pid() && reading.created_at == identity.created_at()
        }) {
            continue;
        }
        if let Some(reading) = read_once(identity.pid(), identity.created_at()) {
            into.push(reading);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed(
        pid: u32,
        created_at: u64,
        cpu_percent_hundredths: Option<u32>,
        memory_bytes: Option<u64>,
    ) -> Observed {
        Observed {
            pid,
            created_at,
            cpu_percent_hundredths,
            memory_bytes,
        }
    }

    fn session(pid: u32, created_at: u64, tree: Option<Vec<ProcessIdentity>>) -> SessionProcess {
        SessionProcess {
            id: "session".to_owned(),
            identity: ProcessIdentity::for_test(pid, created_at),
            tree,
        }
    }

    #[test]
    fn a_measured_zero_is_not_a_missing_cpu_sample() {
        let one_second = 10_000_000_u64;
        assert_eq!(
            cpu_percent_hundredths(0, one_second, u128::from(one_second), 4),
            Some(2500),
            "one full core on a four-processor machine is a quarter of it"
        );
        assert_eq!(
            cpu_percent_hundredths(5, 5, u128::from(one_second), 4),
            Some(0)
        );
        assert_eq!(cpu_percent_hundredths(0, one_second, 0, 4), None);
        assert_eq!(
            cpu_percent_hundredths(0, one_second, u128::from(one_second), 0),
            None
        );
        assert_eq!(
            cpu_percent_hundredths(8, 3, u128::from(one_second), 4),
            None
        );
    }

    #[test]
    fn an_unread_tree_is_not_filled_with_other_processes() {
        let mut asked = Vec::new();
        let report = describe_session(
            &session(4, 9, None),
            SystemTime::UNIX_EPOCH,
            |pid, created| {
                asked.push((pid, created));
                if (pid, created) == (4, 9) {
                    Some(observed(pid, created, None, None))
                } else {
                    Some(observed(pid, created, Some(9), Some(9)))
                }
            },
        );
        assert_eq!(asked, vec![(4, 9)]);
        assert!(report.tree_unavailable);
        assert!(report.checked_at.is_some());
        assert_eq!(report.members.len(), 1);
        assert_eq!(report.members[0].pid, 4);
        assert_eq!(report.members[0].created_at, 9);
        assert_eq!(report.members[0].role, MemberRole::Own);
        assert_eq!(report.members[0].cpu_percent_hundredths, None);
        assert_eq!(report.members[0].memory_bytes, None);
    }

    #[test]
    fn a_listed_empty_tree_is_not_reported_as_unread() {
        let report = describe_session(
            &session(4, 9, Some(Vec::new())),
            SystemTime::UNIX_EPOCH,
            |pid, created| Some(observed(pid, created, Some(0), Some(1))),
        );
        assert!(!report.tree_unavailable);
        assert_eq!(report.members.len(), 1);
        assert_eq!(report.members[0].role, MemberRole::Own);
    }

    #[test]
    fn a_tree_member_whose_creation_time_does_not_match_is_omitted() {
        let report = describe_session(
            &session(4, 9, Some(vec![ProcessIdentity::for_test(5, 8)])),
            SystemTime::UNIX_EPOCH,
            |pid, created| {
                if (pid, created) == (4, 9) {
                    Some(observed(pid, created, Some(1), Some(10)))
                } else {
                    None
                }
            },
        );
        assert_eq!(report.members.len(), 1);
        assert_eq!(report.members[0].pid, 4);
        assert_eq!(report.members[0].memory_bytes, Some(10));
        assert!(report.members.iter().all(|member| member.pid != 5));
    }

    #[test]
    fn numbers_for_a_different_process_are_not_kept() {
        let report = describe_session(
            &session(4, 9, None),
            SystemTime::UNIX_EPOCH,
            |_pid, _created| Some(observed(99, 1, Some(50), Some(50))),
        );
        assert!(report.checked_at.is_none());
        assert!(report.members.is_empty());
        assert!(!report.tree_unavailable);
    }

    #[test]
    fn a_session_whose_own_identity_does_not_match_does_not_keep_its_tree() {
        let mut asked = Vec::new();
        let report = describe_session(
            &session(4, 9, Some(vec![ProcessIdentity::for_test(5, 8)])),
            SystemTime::UNIX_EPOCH,
            |pid, created| {
                asked.push((pid, created));
                None
            },
        );
        assert_eq!(asked, vec![(4, 9)]);
        assert!(report.checked_at.is_none());
        assert!(report.members.is_empty());
    }

    /// The process this test starts is the only one it reads. A creation time
    /// that is not the one Windows reports for that pid must not come back as
    /// the session, including when the tree still names the live process.
    #[cfg(windows)]
    #[test]
    fn a_live_process_this_test_owns_is_read_only_while_its_creation_time_matches() {
        use std::process::{Child, Command, Stdio};

        struct Stop(Child);
        impl Drop for Stop {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        use std::os::windows::process::CommandExt;

        // `ping` itself, not a shell that starts it: `Child::kill` ends this
        // process and not a grandchild left running after the test.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let child = Command::new("ping.exe")
            .args(["-n", "30", "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .expect("spawn a process this test owns");
        let mut child = Stop(child);
        let pid = child.0.id();
        let mut created = None;
        for _ in 0..50 {
            if let Some(found) = crate::process::creation_time_of(pid) {
                created = Some(found);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let created = created.expect("creation time of the process this test owns");

        let reading = read_once(pid, created).expect("the live process matches its creation time");
        assert_eq!(reading.pid, pid);
        assert_eq!(reading.created_at, created);
        assert_eq!(
            crate::process::creation_time_of(reading.pid),
            Some(reading.created_at)
        );
        assert!(
            reading.cpu_time_100ns.is_some(),
            "CPU time of a process this test owns should be readable"
        );
        let memory = reading
            .memory_bytes
            .expect("working set of a process this test owns should be readable");
        assert!(memory > 0);

        assert!(
            read_once(pid, created.wrapping_add(1)).is_none(),
            "a mismatched creation time must not report this process's numbers"
        );

        let reused = session(
            pid,
            created.wrapping_add(1),
            Some(vec![ProcessIdentity::for_test(pid, created)]),
        );
        let rejected = read_session(&reused);
        assert!(rejected.checked_at.is_none());
        assert!(
            rejected.members.is_empty(),
            "the live process's numbers must not stay attached to a session whose creation time does not match, saw {rejected:?}"
        );

        let owned = session(
            pid,
            created,
            Some(vec![ProcessIdentity::for_test(pid, created)]),
        );
        let accepted = read_session(&owned);
        assert!(accepted.checked_at.is_some());
        assert!(!accepted.tree_unavailable);
        assert_eq!(accepted.members.len(), 1);
        assert_eq!(accepted.members[0].pid, pid);
        assert_eq!(accepted.members[0].created_at, created);
        assert_eq!(accepted.members[0].role, MemberRole::Own);
        assert!(accepted.members[0].memory_bytes.unwrap_or(0) > 0);
        assert!(
            accepted.members[0].cpu_percent_hundredths.is_some(),
            "two samples of a confirmed process produce a CPU rate, including a measured zero"
        );

        let _ = child.0.kill();
        let _ = child.0.wait();
    }
}
