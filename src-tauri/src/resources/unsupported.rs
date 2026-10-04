//! Platforms without this reading. Declining is not a zero.

use super::ConfirmedReading;

pub(super) fn read_once(_pid: u32, _created_at: u64) -> Option<ConfirmedReading> {
    None
}
