//! Which managed session owns a listening row (#97).
//!
//! [`attribute`] is a pure reading over two inputs the caller already has: the
//! listening rows, and a snapshot of each managed session's process. The only
//! operating-system queries it adds are the listener's creation time and
//! [`ProcessIdentity::matches`](crate::process::ProcessIdentity::matches) on
//! each session. It does not signal or terminate a process, and it does not
//! read or rewrite [`crate::health`]. A health reading is still only whether
//! the configured port accepts a TCP connection.

use crate::process::{creation_time_of, ProcessIdentity};

use super::{ListenRecord, Readable, UNAVAILABLE_LABEL};

/// Words a later view shows for [`Attribution::External`].
///
/// A confirmed process that no managed session owns. It is not a process name,
/// and it is not what [`UNAVAILABLE_LABEL`] means.
pub const EXTERNAL_LABEL: &str = "外部";

/// Who owns one listening row, after the listener's process identity and the
/// tree it belongs to have been checked.
///
/// A port number, a process name, or a port written in config cannot produce
/// [`Attribution::Session`]. Those are not inputs of [`attribute`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Attribution {
    /// The listener is this session's process, or a member of its tree, and
    /// that session's [`ProcessIdentity`] still matches the process Windows
    /// reports under the session's pid.
    Session(String),
    /// The listener's creation time was read, and no managed session whose
    /// identity still matches owns the process.
    External,
    /// The pid was not read, its creation time could not be read, the tree
    /// could not be checked enough to decide, or more than one session owns
    /// the pid. Not a guess that the listener is external, and not a guess
    /// that it belongs to a session. A view spells this [`UNAVAILABLE_LABEL`].
    Unavailable,
}

impl Attribution {
    /// How a later view spells this reading.
    ///
    /// A session is named by the id the caller supplied. The other two
    /// readings are fixed words.
    pub fn label(&self) -> &str {
        match self {
            Attribution::Session(id) => id.as_str(),
            Attribution::External => EXTERNAL_LABEL,
            Attribution::Unavailable => UNAVAILABLE_LABEL,
        }
    }
}

/// One managed session, as the caller observed it.
///
/// `identity` is that session's own process. `tree` is the pids in its
/// process tree at the same moment: a job's membership when the Hub hosts
/// one, which is the stronger list, or the parent-pid walk otherwise. `None`
/// means the tree could not be listed. A configured port is deliberately not
/// a field — a port is not ownership.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionProcess {
    pub id: String,
    pub identity: ProcessIdentity,
    /// `None` when the tree could not be listed. An empty list means it was
    /// listed and had no members beyond the session's own process. The
    /// session's own pid is recognized from [`ProcessIdentity`] whether or
    /// not it appears here: a job lists it, and a parent-pid walk does not.
    pub tree: Option<Vec<u32>>,
}

/// A listening row plus the attribution decided for it.
///
/// The row is the same value [`super::collect`] would have returned. Attribution
/// does not add, drop, or rewrite rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributedRecord {
    pub record: ListenRecord,
    pub attribution: Attribution,
}

/// Attribute each listening row to the managed session that owns its process.
///
/// A row points at a session only when both of these hold:
///
/// - the listener pid's creation time can be read, and
/// - that pid is the session's own process and this creation time is the one
///   [`ProcessIdentity`] stores — [`ProcessIdentity::matches`] is how a later
///   reading is checked — or the pid is in the session's listed tree while
///   that identity still matches.
///
/// A confirmed process that no such session owns is [`Attribution::External`].
/// A pid that more than one live session owns is [`Attribution::Unavailable`]:
/// the reading does not pick one. A tree whose session identity does not match
/// does not count, so a pid Windows has reused is not still that session.
///
/// The result has one entry per input row, in the same order.
pub fn attribute(records: &[ListenRecord], sessions: &[SessionProcess]) -> Vec<AttributedRecord> {
    records
        .iter()
        .map(|record| AttributedRecord {
            record: record.clone(),
            attribution: attribute_one(record, sessions),
        })
        .collect()
}

fn attribute_one(record: &ListenRecord, sessions: &[SessionProcess]) -> Attribution {
    let Some(pid) = record.pid.as_ref().copied() else {
        return Attribution::Unavailable;
    };
    // A pid without a creation time is not a confirmed process. Membership in
    // a tree is not enough to guess a session, and it is not enough to call
    // the row external.
    let Some(created) = creation_time_of(pid) else {
        return Attribution::Unavailable;
    };

    let mut owners: Vec<String> = Vec::new();
    // Some other live session might contain this pid, and its tree was not
    // listed, so a single positive match is not yet "the one session".
    let mut unchecked = false;

    for session in sessions {
        // The tree was collected for a process. If that process is no longer
        // the one the session holds, the list does not describe the session —
        // including the case where Windows has handed the pid to someone else.
        if !session.identity.matches() {
            continue;
        }

        if session.identity.pid() == pid {
            // The listener is the session's own number. The creation time has
            // to be the one the identity stores; the tree is not a second vote
            // for a number that failed that check.
            if session.identity.created_at() == created {
                remember(&mut owners, &session.id);
            }
            continue;
        }

        match &session.tree {
            Some(members) if members.contains(&pid) => remember(&mut owners, &session.id),
            Some(_) => {}
            None => unchecked = true,
        }
    }

    match (owners.as_slice(), unchecked) {
        ([owner], false) => Attribution::Session(owner.clone()),
        ([], false) => Attribution::External,
        _ => Attribution::Unavailable,
    }
}

fn remember(owners: &mut Vec<String>, id: &str) {
    if !owners.iter().any(|owner| owner == id) {
        owners.push(id.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    use crate::listen::{ListenAddress, Protocol};
    use crate::process::ProcessIdentity;

    fn row(pid: Readable<u32>, port: u16, name: &str) -> ListenRecord {
        ListenRecord {
            protocol: Protocol::Tcp,
            address: ListenAddress {
                ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
                scope_id: 0,
            },
            port,
            pid,
            process_name: Readable::Known(name.to_owned()),
            program_path: Readable::Unavailable,
        }
    }

    #[test]
    fn an_unconfirmed_listener_is_unavailable_not_external_or_a_session() {
        // The session names this pid and this port and this process name.
        // None of those is an identity, so none of them may attribute the row.
        let session = SessionProcess {
            id: "svc".to_owned(),
            identity: ProcessIdentity::for_test(7, 11),
            tree: Some(vec![7, u32::MAX]),
        };
        let rows = [
            row(Readable::Unavailable, 8080, "svc.exe"),
            row(Readable::Known(u32::MAX), 8080, "svc.exe"),
        ];

        let got = attribute(&rows, &[session]);

        assert_eq!(got.len(), rows.len(), "attribution does not drop rows");
        assert_eq!(got[0].record, rows[0]);
        assert_eq!(got[1].record, rows[1]);
        assert_eq!(got[0].attribution, Attribution::Unavailable);
        assert_eq!(got[1].attribution, Attribution::Unavailable);
        assert_eq!(got[0].attribution.label(), UNAVAILABLE_LABEL);
        assert_eq!(Attribution::External.label(), EXTERNAL_LABEL);
        assert_ne!(got[0].attribution, Attribution::External);
        assert_ne!(got[1].attribution, Attribution::Session("svc".to_owned()));
    }

    #[cfg(windows)]
    mod windows {
        use super::super::{attribute, Attribution, SessionProcess, EXTERNAL_LABEL};
        use super::row;
        use std::process::{Child, Command, Stdio};
        use std::thread;
        use std::time::{Duration, Instant};

        use crate::listen::Readable;
        use crate::process::{creation_time_of, descendants, ProcessIdentity};

        struct KillOnDrop(Child);

        impl KillOnDrop {
            fn spawn() -> Self {
                let child = Command::new("cmd.exe")
                    .args(["/c", "ping -n 60 127.0.0.1 > NUL"])
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .expect("spawn a sleeper");
                KillOnDrop(child)
            }

            fn pid(&self) -> u32 {
                self.0.id()
            }

            fn identity(&self) -> ProcessIdentity {
                let created = creation_time_of(self.pid())
                    .unwrap_or_else(|| panic!("creation time of {}", self.pid()));
                let identity = ProcessIdentity::for_test(self.pid(), created);
                assert!(
                    identity.matches(),
                    "the identity taken from the live process matches it"
                );
                identity
            }

            fn alive(&mut self) -> bool {
                self.0
                    .try_wait()
                    .expect("the sleeper is waitable")
                    .is_none()
            }
        }

        impl Drop for KillOnDrop {
            fn drop(&mut self) {
                let _ = Command::new("taskkill")
                    .args(["/PID", &self.0.id().to_string(), "/T", "/F"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                let _ = self.0.wait();
            }
        }

        /// A process `cmd` has actually started, so the tree is not a list the
        /// test invented. `ping` is that child; a console host may appear too.
        fn tree_member(parent: u32) -> u32 {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let kids = descendants(parent);
                if let Some(pid) = kids
                    .into_iter()
                    .find(|pid| *pid != parent && creation_time_of(*pid).is_some())
                {
                    return pid;
                }
                if Instant::now() >= deadline {
                    panic!("no live descendant of {parent}");
                }
                thread::sleep(Duration::from_millis(50));
            }
        }

        fn session(id: &str, process: &KillOnDrop, tree: Option<Vec<u32>>) -> SessionProcess {
            SessionProcess {
                id: id.to_owned(),
                identity: process.identity(),
                tree,
            }
        }

        #[test]
        fn own_process_and_a_tree_member_name_that_session() {
            let mut owner = KillOnDrop::spawn();
            let child = tree_member(owner.pid());
            let tree = descendants(owner.pid());
            assert!(
                tree.contains(&child),
                "the child is in the process tree that attribution is given"
            );
            let other = KillOnDrop::spawn();
            let other_tree = descendants(other.pid());
            assert!(
                !other_tree.contains(&child) && !other_tree.contains(&owner.pid()),
                "the other session's tree does not contain this one"
            );

            let sessions = [
                session("svc", &owner, Some(tree)),
                session("other", &other, Some(other_tree)),
            ];
            // Same port and the same process name on every row. Attribution
            // still follows the process, and a configured port is not an
            // input the snapshot even has.
            let rows = [
                row(Readable::Known(owner.pid()), 8080, "cmd.exe"),
                row(Readable::Known(child), 8080, "cmd.exe"),
            ];

            let got = attribute(&rows, &sessions);

            assert_eq!(got.len(), 2);
            assert_eq!(got[0].attribution, Attribution::Session("svc".to_owned()));
            assert_eq!(got[1].attribution, Attribution::Session("svc".to_owned()));
            assert_ne!(got[0].attribution, Attribution::Session("other".to_owned()));
            assert_ne!(got[1].attribution, Attribution::Session("other".to_owned()));
            assert_eq!(got[0].attribution.label(), "svc");
            assert!(
                owner.alive(),
                "attribution does not stop the session process"
            );
            assert!(
                creation_time_of(child).is_some(),
                "attribution does not stop a process in the tree"
            );
        }

        #[test]
        fn another_sessions_verified_process_names_that_session() {
            let first = KillOnDrop::spawn();
            let second = KillOnDrop::spawn();
            let second_child = tree_member(second.pid());
            let sessions = [
                session("first", &first, Some(descendants(first.pid()))),
                session("second", &second, Some(descendants(second.pid()))),
            ];
            let rows = [
                row(Readable::Known(second.pid()), 8080, "cmd.exe"),
                row(Readable::Known(second_child), 9, "ping.exe"),
            ];

            let got = attribute(&rows, &sessions);

            assert_eq!(
                got[0].attribution,
                Attribution::Session("second".to_owned())
            );
            assert_eq!(
                got[1].attribution,
                Attribution::Session("second".to_owned())
            );
            assert_ne!(got[0].attribution, Attribution::Session("first".to_owned()));
            assert_ne!(got[1].attribution, Attribution::Session("first".to_owned()));
        }

        #[test]
        fn a_confirmed_process_outside_every_managed_tree_is_external() {
            let mut managed = KillOnDrop::spawn();
            let mut outside = KillOnDrop::spawn();
            let sessions = [session("svc", &managed, Some(descendants(managed.pid())))];
            // Same executable name as the managed process, and a port the
            // caller might have configured. The snapshot has no port to match.
            let rows = [row(Readable::Known(outside.pid()), 3000, "cmd.exe")];

            let got = attribute(&rows, &sessions);

            assert_eq!(got[0].attribution, Attribution::External);
            assert_eq!(got[0].attribution.label(), EXTERNAL_LABEL);
            assert_ne!(got[0].attribution, Attribution::Session("svc".to_owned()));
            assert!(outside.alive(), "an external process is not terminated");
            assert!(
                managed.alive(),
                "attribution does not stop the managed process"
            );
        }

        #[test]
        fn the_same_pid_with_a_different_creation_time_drops_the_old_session() {
            let live = KillOnDrop::spawn();
            let created =
                creation_time_of(live.pid()).expect("the live process has a creation time");
            let reused = ProcessIdentity::for_test(live.pid(), created.wrapping_add(1));
            assert!(
                !reused.matches(),
                "a different creation time is not this process"
            );
            let bystander = KillOnDrop::spawn();
            // Both the reused pid and an unrelated pid are listed in the old
            // tree. The identity does not match, so the list does not count.
            let stale = SessionProcess {
                id: "old".to_owned(),
                identity: reused,
                tree: Some(vec![live.pid(), bystander.pid()]),
            };
            let rows = [
                row(Readable::Known(live.pid()), 8080, "cmd.exe"),
                row(Readable::Known(bystander.pid()), 8080, "cmd.exe"),
            ];

            let got = attribute(&rows, &[stale]);

            assert_eq!(got[0].attribution, Attribution::External);
            assert_eq!(got[1].attribution, Attribution::External);
            assert_ne!(got[0].attribution, Attribution::Session("old".to_owned()));
            assert_ne!(got[1].attribution, Attribution::Session("old".to_owned()));
            assert_ne!(got[0].attribution, Attribution::Unavailable);
        }

        #[test]
        fn a_pid_claimed_by_two_live_sessions_is_unavailable() {
            let first = KillOnDrop::spawn();
            let second = KillOnDrop::spawn();
            let sessions = [
                session("first", &first, Some(vec![second.pid()])),
                session("second", &second, Some(descendants(second.pid()))),
            ];
            let rows = [row(Readable::Known(second.pid()), 8080, "cmd.exe")];

            let got = attribute(&rows, &sessions);

            assert_eq!(got[0].attribution, Attribution::Unavailable);
            assert_ne!(got[0].attribution, Attribution::Session("first".to_owned()));
            assert_ne!(
                got[0].attribution,
                Attribution::Session("second".to_owned())
            );
            assert_ne!(got[0].attribution, Attribution::External);
        }

        #[test]
        fn an_unlisted_tree_is_not_treated_as_external_or_as_a_session() {
            let managed = KillOnDrop::spawn();
            let outside = KillOnDrop::spawn();
            // The session process is alive, and its tree could not be read.
            // The outside process is confirmed, but it might still be a member.
            let sessions = [session("svc", &managed, None)];
            let rows = [
                row(Readable::Known(outside.pid()), 8080, "cmd.exe"),
                row(Readable::Known(managed.pid()), 8080, "cmd.exe"),
            ];

            let got = attribute(&rows, &sessions);

            assert_eq!(
                got[0].attribution,
                Attribution::Unavailable,
                "an unchecked tree cannot be called external"
            );
            assert_eq!(
                got[1].attribution,
                Attribution::Session("svc".to_owned()),
                "the session's own process does not need the tree"
            );
        }

        #[test]
        fn another_unlisted_tree_blocks_a_unique_guess() {
            let first = KillOnDrop::spawn();
            let second = KillOnDrop::spawn();
            let sessions = [
                session("first", &first, Some(descendants(first.pid()))),
                session("second", &second, None),
            ];
            let rows = [row(Readable::Known(first.pid()), 8080, "cmd.exe")];

            let got = attribute(&rows, &sessions);

            assert_eq!(got[0].attribution, Attribution::Unavailable);
            assert_ne!(got[0].attribution, Attribution::Session("first".to_owned()));
            assert_ne!(got[0].attribution, Attribution::External);
        }
    }
}
