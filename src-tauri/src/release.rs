//! Release-manifest guards for the Windows packaging (T12, #13).
//!
//! Everything here is a guard rather than a test of behaviour. The release
//! candidate is described by four surfaces that nothing compares with each
//! other: `tauri.conf.json` (what the installer is built from), `Cargo.toml`
//! (the version `ipc::ping` reports to the window), `package.json` (the
//! frontend's version), and the app-data layout constants in `config::paths`.
//! A drift between them stays silent until the worst moment — the bundle
//! builds, the app launches, and only then does the release disagree with
//! itself about which version, name or directory it is.
//!
//! ## Why a test-only module
//!
//! The facts below are not any single module's property: they are about the
//! IPC identity, the config layer, and two files outside the crate at once. A
//! `tests/` integration test would need `ipc::APP_NAME` and
//! `config::APP_DIR_NAME` to be public — widening the API for a guard's sake —
//! so they live here instead, compiled only under `cargo test`.
//!
//! The manifests are read from disk instead of being embedded with
//! `include_str!` on purpose: the guard is about *the file that is about to be
//! bundled*, and reading it at test time means the assertion is about the
//! working tree rather than about a copy frozen at compile time.

#![cfg(test)]

use std::path::{Path, PathBuf};

use serde_json::Value;

/// A JSON manifest beside the crate directory (`src-tauri/`).
///
/// `name` is relative to the crate root, so a file one level up — the
/// repository's `package.json` — is `"../package.json"`. A missing or
/// malformed file is a failure of the guard itself, not a passing test.
fn manifest(name: &str) -> Value {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is valid JSON: {error}", path.display()))
}

/// One version, three files.
///
/// `tauri.conf.json` is what the bundle is stamped with and what an MSI/NSIS
/// upgrade compares against; `Cargo.toml` is what `ping` answers and what the
/// window's own footer reports; `package.json` is the frontend's. When they
/// disagree the release installs as one version and reports itself as another,
/// which is exactly the kind of difference a user finds before the developer
/// does.
#[test]
fn the_three_manifests_agree_on_one_version() {
    let version = env!("CARGO_PKG_VERSION");

    assert_eq!(
        manifest("tauri.conf.json")["version"],
        version,
        "src-tauri/tauri.conf.json and src-tauri/Cargo.toml disagree on the version"
    );
    assert_eq!(
        manifest("../package.json")["version"],
        version,
        "package.json and src-tauri/Cargo.toml disagree on the version"
    );
}

/// The name on the installer, the tray tooltip and the exit confirmation is
/// one name: `bundle.productName` and `ipc::APP_NAME` are two spellings of the
/// same application.
#[test]
fn the_bundled_product_name_is_the_name_the_app_reports() {
    let product_name = manifest("tauri.conf.json")["productName"].clone();

    assert_eq!(
        product_name,
        Value::from(crate::ipc::APP_NAME),
        "tauri.conf.json's productName and ipc::APP_NAME must be the same string"
    );
}

/// The install directory must never be the app-data directory.
///
/// The NSIS installer puts the application in `%LOCALAPPDATA%\<productName>`
/// and its uninstaller removes that whole directory; the app puts the user's
/// config, logs and run metadata in `%LOCALAPPDATA%\<APP_DIR_NAME>`. Today the
/// two differ by the spaces alone — `Local Console Hub` against
/// `LocalConsoleHub` — which is a thin margin for a rule whose violation
/// deletes the user's log history on uninstall. Shortening the product name to
/// the bare `LocalConsoleHub` is the natural tidy-up that would cross the line,
/// so it is refused here rather than noticed in review.
///
/// See `docs/RELEASE.md` for the layout this protects and the install this was
/// verified against.
#[test]
fn the_install_directory_can_never_be_the_app_data_directory() {
    let value = manifest("tauri.conf.json")["productName"].clone();
    let product_name = value.as_str().expect("bundle.productName is a string");

    assert_ne!(
        product_name.to_ascii_lowercase(),
        crate::config::APP_DIR_NAME.to_ascii_lowercase(),
        "productName and the app-data directory name must stay different: an NSIS install into \
         %LOCALAPPDATA%\\{product_name} would sit on top of the config and logs of the same name, \
         and uninstalling would take them with it"
    );
}

/// The bundle identifier is the install identity, and it is frozen.
///
/// Tauri derives the WiX upgrade code from it and builds the NSIS uninstall
/// registry key out of it. Changing it does not rename anything — it publishes
/// a second application that cannot upgrade the first, and leaves the first
/// installed under a key nothing points at any more. A deliberate change
/// belongs in `docs/DECISIONS.md` next to this test, not in a quiet edit.
#[test]
fn the_identifier_that_upgrades_are_keyed_on_is_frozen() {
    assert_eq!(
        manifest("tauri.conf.json")["identifier"],
        "com.localconsolehub.hub"
    );
}

/// T12 delivers an installer, not only an executable: both Windows formats are
/// produced so a fresh machine has something to run.
#[test]
fn the_bundle_still_produces_both_windows_installers() {
    let bundle = manifest("tauri.conf.json")["bundle"].clone();
    assert_eq!(bundle["active"], true, "bundling is switched on");

    let targets = bundle["targets"]
        .as_array()
        .expect("bundle.targets is a list")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();

    for expected in ["msi", "nsis"] {
        assert!(
            targets.contains(&expected),
            "bundle.targets no longer builds {expected}: {targets:?}"
        );
    }
}

/// The installer downloads WebView2 instead of bundling it.
///
/// `offlineInstaller` would add roughly 130 MB to every download and `skip`
/// would turn a machine without the runtime into a window that never opens.
/// `downloadBootstrapper` costs the network only where there is no runtime to
/// find, which is the trade `docs/RELEASE_NOTES_v0.1.0.md` records as a known
/// limitation — this keeps the config from drifting out from under that
/// sentence.
#[test]
fn the_installer_downloads_webview2_rather_than_bundling_it() {
    let mode = manifest("tauri.conf.json")["bundle"]["windows"]["webviewInstallMode"].clone();

    assert_eq!(mode["type"], "downloadBootstrapper");
    assert_eq!(mode["silent"], true);
}
