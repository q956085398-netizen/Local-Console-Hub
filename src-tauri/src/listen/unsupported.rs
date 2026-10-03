//! No listening-port table on this platform.
//!
//! The product collects listeners on Windows. A build for anywhere else still
//! compiles, and collection reports that it is unsupported. An empty list would
//! claim that nothing is listening, which this platform did not check.

use super::{CollectError, ListenRecord};

pub(super) fn collect() -> Result<Vec<ListenRecord>, CollectError> {
    Err(CollectError::Unsupported)
}
