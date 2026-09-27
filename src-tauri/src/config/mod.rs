//! Config layer — schema, validation, defaults.
//!
//! Owned by T01 (#2, configuration schema and session model): YAML config
//! loading, Service/Interactive-Terminal session schemas, logging config,
//! per-session actionable validation, and app-data path resolution.
//! Validation must reject duplicate ids and invalid port/URL/cwd without
//! hiding unrelated valid sessions. No process is launched from here.
