//! Release-manifest guards for the Windows packaging (T12, #13; #47).
//!
//! Everything here is a guard rather than a test of behaviour. The release
//! candidate is described by five surfaces that nothing else compares with
//! each other: `tauri.conf.json` (what the installer is built from),
//! `Cargo.toml` (the version `ipc::ping` reports to the window),
//! `package.json` (the frontend's version), the app-data layout constants in
//! `config::paths`, and the NSIS language file the uninstaller's checkbox is
//! labelled from. A drift between them stays silent until the worst moment —
//! the bundle builds, the app launches, and only then does the release
//! disagree with itself about which version, name or directory it is, or
//! promise a user something it does not do.
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
/// one name: the `productName` at the top of `tauri.conf.json` and
/// `ipc::APP_NAME` are two spellings of the same application.
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
    let product_name = value.as_str().expect("productName is a string");

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

/// A JSON array of strings, read as `&str`s.
///
/// A missing array is a failure of the guard itself, not a passing test; an
/// entry that is not a string is dropped, because every list these guards read
/// is one the bundler writes itself and the strings in it are what is under
/// test.
fn string_list<'a>(value: &'a Value, what: &str) -> Vec<&'a str> {
    value
        .as_array()
        .unwrap_or_else(|| panic!("{what} is a list: {value}"))
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

/// T12 delivers an installer, not only an executable: both Windows formats are
/// produced so a fresh machine has something to run.
#[test]
fn the_bundle_still_produces_both_windows_installers() {
    let bundle = manifest("tauri.conf.json")["bundle"].clone();
    assert_eq!(bundle["active"], true, "bundling is switched on");

    let targets = string_list(&bundle["targets"], "bundle.targets");

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

/// The sentence the uninstaller's checkbox has to show.
///
/// It describes what the checkbox actually removes — the WebView2 user-data
/// directory under `%LOCALAPPDATA%\<identifier>` — and says nothing about the
/// app's own data, which the checkbox does not touch. `docs/RELEASE.md` §2.2
/// is the long form.
const NSIS_CHECKBOX_LABEL: &str = "Delete WebView2 browser profile (not your config or logs)";

/// How many `LangString`s the bundler's own `English.nsh` declares, for the
/// bundler version `installer/languages/English.nsh` was copied from.
const NSIS_EXPECTED_LANGSTRINGS: usize = 27;

/// The NSIS language file `tauri.conf.json` points at, and its text.
fn nsis_language_file() -> (PathBuf, String) {
    let value = manifest("tauri.conf.json")["bundle"]["windows"]["nsis"]["customLanguageFiles"]
        ["English"]
        .clone();
    let name = value.as_str().unwrap_or_else(|| {
        panic!("bundle.windows.nsis.customLanguageFiles.English is a path: {value}")
    });

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{} is readable: {error}", path.display()));

    (path, text)
}

/// The `LangString deleteAppData` our NSIS language file declares.
///
/// That string is the label Tauri puts on the checkbox `un.ConfirmShow` adds
/// to the uninstaller's confirm page (the generated installer carries it as
/// `w "$(deleteAppData)"`). Reading it from the file rather than restating it
/// keeps the guard about the wording that will actually be bundled.
fn uninstaller_checkbox_label() -> String {
    let (path, text) = nsis_language_file();

    let declaration = text
        .lines()
        .find_map(|line| line.strip_prefix("LangString deleteAppData "))
        .unwrap_or_else(|| panic!("{} defines no `LangString deleteAppData`", path.display()));

    // `LANG_ENGLISH} "the label"` — the label is the first quoted run.
    let mut quoted = declaration.split('"').skip(1);
    let label = quoted.next().unwrap_or_else(|| {
        panic!(
            "{}: the `LangString deleteAppData` value is not quoted",
            path.display()
        )
    });

    label.to_owned()
}

/// The uninstaller's checkbox is labelled from our own language file.
///
/// Two halves of the wiring, and one honest caveat about the first:
///
/// `bundle.windows.nsis.customLanguageFiles` mounts the file, and Tauri's
/// config schema says such a key "must be added to the `languages` array".
/// On the bundler version in use the file is mounted **whether or not** the
/// language is listed — measured 2026-09-29 by removing `languages` and
/// building: the label from our file still landed in the generated
/// `English.nsh`, exit code 0. So this assertion is conformance to the
/// documented contract, not a check on a measured dependency; it is here
/// because a future bundler that enforces the schema would otherwise fall
/// back to upstream's wording in silence.
///
/// The second half is the read itself, through `customLanguageFiles.English`:
/// `uninstaller_checkbox_label` panics when that path is unreadable or
/// declares no `deleteAppData`.
#[test]
fn the_uninstaller_checkbox_is_labelled_from_our_language_file() {
    let nsis = manifest("tauri.conf.json")["bundle"]["windows"]["nsis"].clone();

    let languages = string_list(&nsis["languages"], "bundle.windows.nsis.languages");
    assert!(
        languages.contains(&"English"),
        "bundle.windows.nsis.customLanguageFiles is documented as requiring its language to be \
         listed in bundle.windows.nsis.languages too; this bundler mounts it either way, but a \
         later one may not: {languages:?}"
    );

    // Absent or unquoted panics inside the helper; what is left for this test
    // is the file declaring a *blank* label, which the wording guard would
    // report as a mismatch rather than as the broken wiring it is.
    let label = uninstaller_checkbox_label();
    assert!(
        !label.is_empty(),
        "{} declares an empty deleteAppData: the installer would draw a blank checkbox",
        nsis["customLanguageFiles"]["English"]
    );
}

/// Every `LangString` the bundler ships is still declared in our copy.
///
/// `customLanguageFiles` REPLACES the bundler's `English.nsh` rather than
/// merging with it, so this file has to stay complete. A string the installer
/// template asks for and this file does not declare is **not** a build
/// failure: `makensis` prints one warning — `LangString "deleteAppData" is not
/// set in language table of language English` — and exits 0, verified on
/// 2026-09-29 by removing the string from the generated script and compiling
/// it again. The installer would ship with a blank label, and CI would not
/// notice, because the build job stops at a debug `cargo build` and never runs
/// the bundler. This count is what stands between a bundler upgrade and a
/// silently empty control.
///
/// It is a count and not a comparison: the guard has no copy of Tauri's own
/// file to compare against, so what it can say is that the file has neither
/// lost a string nor gained one since it was copied. The number is upstream's
/// for the version this copy came from; when Tauri's file changes shape,
/// re-copy it and re-apply the wording — the failure is the reminder, not a
/// defect in itself.
#[test]
fn the_uninstaller_language_file_declares_the_expected_string_count() {
    let (path, text) = nsis_language_file();

    let declared = text
        .lines()
        .filter(|line| line.starts_with("LangString "))
        .count();

    assert_eq!(
        declared,
        NSIS_EXPECTED_LANGSTRINGS,
        "{} declares {declared} LangStrings, not the {NSIS_EXPECTED_LANGSTRINGS} the bundler's \
         own English.nsh declares for the version this was copied from — re-copy that file and \
         re-apply the deleteAppData wording (`src-tauri/installer/languages/English.nsh`), or a \
         label in the installer goes blank without failing the build",
        path.display()
    );
}

/// The checkbox's label describes what the uninstaller removes with it.
///
/// The defect behind #47 was a control that said "Delete the application data"
/// and removed the WebView2 user-data directory instead, leaving `config.yaml`
/// and `logs\` in place — a user who ticked it believed their run history was
/// gone. The sentence may be reworded, but it must keep describing the
/// *browser* data the uninstaller actually deletes, and it must not call
/// itself the app's data again. The equality pins the wording that was
/// reviewed (and the run recorded in `docs/RELEASE.md` §5); the loop states
/// the rule a reword has to keep satisfying.
///
/// See `docs/DECISIONS.md` D-026 for the layout this protects: the app's own
/// data lives in `%APPDATA%\LocalConsoleHub` and
/// `%LOCALAPPDATA%\LocalConsoleHub`, outside the installer's reach by design.
#[test]
fn the_uninstaller_checkbox_label_describes_what_it_deletes() {
    let label = uninstaller_checkbox_label();
    let lower = label.to_ascii_lowercase();

    for claim in ["application data", "app data", "user data"] {
        assert!(
            !lower.contains(claim),
            "the uninstaller's checkbox removes the WebView2 user-data directory, not the app's \
             config and logs, so its label must not claim \"{claim}\": {lower:?}"
        );
    }

    assert_eq!(
        label, NSIS_CHECKBOX_LABEL,
        "the uninstaller's checkbox label and the wording this guard was written for disagree"
    );
}

// ---------------------------------------------------------------------------
// The window's title bar and its icon (issue #68, docs/DECISIONS.md D-028)
// ---------------------------------------------------------------------------
//
// Three surfaces have to agree for the merged title bar to be the window's
// only one, and none of them fails loudly when they drift apart: the window's
// config (undecorated? resizable?), the capability the controls' commands are
// gated by, and the icon the window, taskbar, tray and installers all show.
// The first two are checked at click time in a running app — too late to be a
// build failure; the third is a picture nothing compares.

/// The main window's entry in `tauri.conf.json`, wherever it sits in the list.
fn main_window() -> Value {
    let windows = manifest("tauri.conf.json")["app"]["windows"].clone();

    windows
        .as_array()
        .unwrap_or_else(|| panic!("app.windows is a list: {windows}"))
        .iter()
        .find(|window| window["label"] == "main")
        .cloned()
        .unwrap_or_else(|| panic!("app.windows has an entry labelled `main`: {windows}"))
}

/// The window is undecorated, and it stays resizable.
///
/// Both halves matter together. `decorations: false` is what makes the dark V2
/// title bar the window's only title bar; an undecorated window has no system
/// frame of its own, so the resize borders are the ones Tauri attaches when
/// `resizable` is on (`tauri-runtime-wry`'s undecorated resizing). Setting
/// `resizable: false` would produce a window with no frame at all: nothing to
/// drag for a resize, no system menu, and a close button as the only way out
/// besides the tray.
#[test]
fn the_main_window_carries_its_own_title_bar() {
    let window = main_window();

    assert_eq!(
        window["decorations"], false,
        "the main window is undecorated so the dark title bar is the only one — a system title \
         above it is the duplicate title #68 removed (docs/DECISIONS.md D-028)"
    );
    assert_ne!(
        window["resizable"], false,
        "an undecorated window takes its resize borders from Tauri's undecorated resizing, which \
         only attaches while the window is resizable — turning it off leaves no way to resize"
    );
}

/// The controls' window commands are allowed for the window that uses them.
///
/// Tauri gates every `plugin:window|*` call on a capability permission and
/// refuses the call when it is missing — at click time, in the running app,
/// with nothing failing at build time and no test red. `core:window:default`
/// covers the readings (`is-maximized`) but not the actions, so minimize,
/// toggle-maximize, close and the drag region's `start-dragging` are granted
/// explicitly in `capabilities/default.json`.
#[test]
fn the_title_bar_controls_are_granted_what_they_ask_for() {
    let capabilities = manifest("capabilities/default.json");
    let permissions = string_list(&capabilities["permissions"], "permissions");
    let windows = string_list(&capabilities["windows"], "capabilities.windows");

    assert_eq!(
        windows,
        vec!["main"],
        "the capability is about the main window: that is the window the title bar controls"
    );
    for permission in [
        "core:default",
        "core:window:allow-minimize",
        "core:window:allow-toggle-maximize",
        "core:window:allow-close",
        "core:window:allow-start-dragging",
    ] {
        assert!(
            permissions.contains(&permission),
            "capabilities/default.json no longer grants `{permission}`: the merged title bar's \
             control that uses it is refused at runtime, with nothing else failing"
        );
    }
}

/// One icon, generated from the one file the window also renders.
///
/// #68 unified the app's identity: the Hub mark the V2 title bar draws is now
/// the source the whole Windows icon set is rasterized from
/// (`assets/brand/hub-mark.svg` → `scripts/generate-icon.mjs` → the checked-in
/// `src-tauri/icons/`), and the title bar renders that file rather than a copy
/// of it. Nothing can compare the raster with the vector, so what is guarded
/// here is the *linkage*: the bundle lists icons that exist, the source they
/// are generated from exists, and the title bar still imports that same source.
/// Delete the source or re-inline a private copy of the mark, and this fails
/// while `cargo build` and the bundle would both have been happy.
#[test]
fn the_icon_set_comes_from_the_one_hub_mark() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = manifest("tauri.conf.json");
    let icons = string_list(&config["bundle"]["icon"], "bundle.icon");

    for icon in &icons {
        assert!(
            crate_dir.join(icon).is_file(),
            "bundle.icon lists `{icon}`, which is not in src-tauri — regenerate with \
             `node scripts/generate-icon.mjs && npx tauri icon assets/brand/hub-mark-1024.png`"
        );
    }
    assert!(
        icons.iter().any(|icon| icon.ends_with("icon.ico")),
        "the Windows icon format is missing from bundle.icon: that is the file the executable's \
         resource and every installed shortcut take their icon from: {icons:?}"
    );

    let source = crate_dir.join("../assets/brand/hub-mark.svg");
    assert!(
        source.is_file(),
        "the icon source `{}` is gone — it is the file both the native icon set and the title \
         bar's mark come from (D-028)",
        source.display()
    );

    let title_bar =
        std::fs::read_to_string(crate_dir.join("../src/components/title-bar/TitleBar.tsx"))
            .expect("the title bar component is readable");
    assert!(
        title_bar.contains("assets/brand/hub-mark.svg"),
        "the title bar no longer renders the icon asset: a second drawing of the same mark is \
         the drift #68 removed (docs/DECISIONS.md D-028)"
    );
}
