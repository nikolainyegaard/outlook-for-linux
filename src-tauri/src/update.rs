//! Update check and Update Now, after OxTree's. The check is one GET of the
//! GitHub releases API through `curl`, at start on the schedule in config
//! (every launch unless changed) or from Settings. Updating is a second
//! explicit press, and then this copy is replaced the way it was installed:
//!
//! | Installed as | Update |
//! |---|---|
//! | a package from a repository | `dnf upgrade` or `apt-get install` of the package, as root through polkit |
//! | a .rpm or .deb from the release page | download the new one, check its SHA-256, `dnf install` or `apt-get install` it as root through polkit |
//! | an AppImage | download the new one, check it, rename it over this one |
//! | anything else (the tarball, a development build) | the link |
//!
//! `pkexec` rather than `sudo`: it shows the desktop's own prompt and
//! follows the session's polkit rules. The package manager does every
//! install, so its database stays right. The file names must match what
//! release.yml publishes.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};

use crate::{config, settings};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RELEASES_URL: &str = "https://github.com/nikolainyegaard/outlook-for-linux/releases/latest";
const API_URL: &str = "https://api.github.com/repos/nikolainyegaard/outlook-for-linux/releases/latest";
const DOWNLOAD_URL: &str = "https://github.com/nikolainyegaard/outlook-for-linux/releases/download";
const PACKAGE: &str = "outlook-for-linux";

/// What a check found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateResult {
    /// The latest tag, newer than this build.
    Newer(String),
    /// This build is the latest release, or newer than it.
    Current,
    Failed(String),
}

/// How this running copy was installed, which decides how it updates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    /// From a repository the package manager knows; it has the update.
    Repo(Pkg),
    /// A package installed from a file; the next file goes the same way.
    Package(Pkg),
    /// The AppImage file.
    AppImage(PathBuf),
    /// The tarball, a development build: nothing to do by hand but the link.
    Other,
}

/// The package manager that owns the install.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pkg {
    Dnf,
    Apt,
}

impl Install {
    pub fn label(&self) -> String {
        match self {
            Install::Repo(Pkg::Dnf) => "installed from a dnf repository".into(),
            Install::Repo(Pkg::Apt) => "installed from an apt repository".into(),
            Install::Package(Pkg::Dnf) => "installed from an .rpm file".into(),
            Install::Package(Pkg::Apt) => "installed from a .deb file".into(),
            Install::AppImage(p) => format!("the AppImage at {}", p.display()),
            Install::Other => "not installed by a package manager".into(),
        }
    }

    pub fn can_update(&self) -> bool {
        !matches!(self, Install::Other)
    }
}

/// What Settings shows. Lives on the main thread; the worker threads hand
/// their results over through `run_on_main_thread`.
#[derive(Clone, Default)]
pub struct State {
    pub checking: bool,
    pub result: Option<UpdateResult>,
    /// Detected once a newer release is found.
    pub install: Option<Install>,
    pub installing: bool,
    pub progress: Option<String>,
    pub error: Option<String>,
}

impl State {
    /// The tag Update Now can install: a newer release, unless it is the
    /// one just installed whose binary reports an older version.
    pub fn available(&self, config: &config::Config) -> Option<&str> {
        match &self.result {
            Some(UpdateResult::Newer(tag)) if *tag != config.updates.last_installed => Some(tag),
            _ => None,
        }
    }

    pub fn busy(&self) -> bool {
        self.checking || self.installing
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

pub fn state() -> State {
    STATE.with(|s| s.borrow().clone())
}

fn set(f: impl FnOnce(&mut State)) {
    STATE.with(|s| f(&mut s.borrow_mut()));
}

fn unix_now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// At start: check when the automatic check is on and the interval has
/// passed (0 days is every launch).
pub fn check_at_start(app: &AppHandle) {
    let u = config::load(app).updates;
    if u.check && unix_now().saturating_sub(u.last_check) >= i64::from(u.interval_days) * 86_400 {
        start_check(app, true);
    }
}

/// Check for Updates, by hand or on schedule. `automatic` announces a
/// newer release with a desktop notification, since no window asked.
pub fn start_check(app: &AppHandle, automatic: bool) {
    if state().busy() {
        return;
    }
    config::update(app, |c| c.updates.last_check = unix_now());
    set(|s| {
        s.checking = true;
        s.result = None;
        s.error = None;
    });
    settings::redraw(app, settings::UPDATES);
    let handle = app.clone();
    std::thread::Builder::new()
        .name("update-check".into())
        .spawn(move || {
            let result = check_blocking(VERSION);
            // Detection runs the package managers' query commands, which
            // can take a moment on dnf; keep that off the main thread.
            let install = matches!(result, UpdateResult::Newer(_)).then(detect);
            let app = handle.clone();
            let _ = handle.run_on_main_thread(move || {
                set(|s| {
                    s.checking = false;
                    s.result = Some(result);
                    if let Some(i) = install {
                        s.install = Some(i);
                    }
                });
                settings::redraw(&app, settings::UPDATES);
                if automatic {
                    if let Some(tag) = state().available(&config::load(&app)) {
                        announce(&app, tag);
                    }
                }
            });
        })
        .ok();
}

/// Desktop notification for an automatic find; the click opens Settings
/// on the Updates page.
fn announce(app: &AppHandle, tag: &str) {
    let handle = app.clone();
    let body = format!("Version {} is available, this is {VERSION}.", tag.trim_start_matches('v'));
    std::thread::spawn(move || {
        let shown = notify_rust::Notification::new()
            .summary("Outlook for Linux update")
            .body(&body)
            .appname("Outlook")
            .hint(notify_rust::Hint::DesktopEntry(PACKAGE.into()))
            .action("default", "Open Settings")
            .show();
        if let Ok(h) = shown {
            h.wait_for_action(|action| {
                if action == "default" {
                    let app = handle.clone();
                    let _ = handle.run_on_main_thread(move || settings::open_at(&app, Some(settings::UPDATES)));
                }
            });
        }
    });
}

fn check_blocking(current: &str) -> UpdateResult {
    let out = Command::new("curl")
        .args(["-fsSL", "--max-time", "15", "-A", &format!("{PACKAGE}/{current}"), "-H", "Accept: application/vnd.github+json", API_URL])
        .stdin(Stdio::null())
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => return UpdateResult::Failed(format!("Cannot run curl: {e}")),
    };
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let err = err.lines().last().unwrap_or("").trim();
        return UpdateResult::Failed(if err.is_empty() { "The release check failed".into() } else { err.to_string() });
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let Some(tag) = release_tag(&text) else {
        return UpdateResult::Failed("No release tag in the response".into());
    };
    match (parse_version(&tag), parse_version(current)) {
        (Some(latest), Some(mine)) if latest > mine => UpdateResult::Newer(tag),
        (Some(_), Some(_)) => UpdateResult::Current,
        _ => UpdateResult::Failed(format!("Cannot read the version in {tag}")),
    }
}

fn parse_version(tag: &str) -> Option<(u64, u64, u64)> {
    let mut it = tag.trim().trim_start_matches('v').split('.').map(|p| p.parse::<u64>());
    Some((it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?))
}

/// The `tag_name` of a GitHub releases API response.
fn release_tag(json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(json).ok()?.get("tag_name")?.as_str().map(str::to_owned)
}

/// Detect the install. Runs the package managers' query commands.
fn detect() -> Install {
    if let Some(p) = std::env::var_os("APPIMAGE") {
        return Install::AppImage(PathBuf::from(p));
    }
    if !has("pkexec") {
        return Install::Other;
    }
    if has("rpm") && capture("rpm", &["-q", PACKAGE]).is_some() {
        let from = capture("dnf", &["repoquery", "--installed", "--queryformat", "%{from_repo}\n", PACKAGE]).unwrap_or_default();
        return if repo_from_dnf(&from) { Install::Repo(Pkg::Dnf) } else { Install::Package(Pkg::Dnf) };
    }
    if has("dpkg-query") && capture("dpkg-query", &["-W", "-f", "${db:Status-Status}", PACKAGE]).is_some_and(|s| s.trim() == "installed") {
        let policy = capture("apt-cache", &["policy", PACKAGE]).unwrap_or_default();
        return if repo_from_apt(&policy) { Install::Repo(Pkg::Apt) } else { Install::Package(Pkg::Apt) };
    }
    Install::Other
}

/// `dnf repoquery --installed --queryformat %{from_repo}`: `@commandline`
/// for a file install, `@System` on dnf4 for one it cannot tell, else the
/// repository id.
fn repo_from_dnf(from_repo: &str) -> bool {
    let r = from_repo.trim();
    !(r.is_empty() || r == "@commandline" || r == "@System")
}

/// `apt-cache policy`: a repository install lists the installed version
/// with a source line beyond `/var/lib/dpkg/status`.
fn repo_from_apt(policy: &str) -> bool {
    // Source lines are `PRIORITY URL suite/component arch Packages`.
    policy.lines().any(|l| {
        let mut f = l.split_whitespace();
        f.next().is_some_and(|p| p.parse::<u32>().is_ok()) && f.next().is_some_and(|src| src.contains("://") || src.starts_with("file:"))
    })
}

fn has(cmd: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| std::env::split_paths(&path).any(|d| d.join(cmd).is_file()))
}

fn capture(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

enum Event {
    Progress(String),
    Done(Result<PathBuf, String>),
}

/// Update Now: replace this copy the way it was installed, on a thread,
/// then start the new program and quit.
pub fn start_update(app: &AppHandle) {
    let s = state();
    let Some(tag) = s.available(&config::load(app)).map(str::to_string) else { return };
    let Some(install) = s.install.clone().filter(Install::can_update) else { return };
    if s.busy() {
        return;
    }
    set(|s| {
        s.installing = true;
        s.error = None;
        s.progress = Some("Starting\u{2026}".into());
    });
    settings::redraw(app, settings::UPDATES);
    let cache_dir = app.path().app_cache_dir().unwrap_or_else(|_| std::env::temp_dir()).join("update");
    let handle = app.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let pump = handle.clone();
    std::thread::Builder::new()
        .name("update-install".into())
        .spawn(move || {
            let result = update_blocking(&install, &tag, &cache_dir, &tx);
            let _ = tx.send(Event::Done(result));
        })
        .ok();
    // The receiving end lives on its own thread too, so the main thread
    // only ever runs the short closures it is handed.
    std::thread::spawn(move || {
        for event in rx {
            let app = pump.clone();
            let _ = pump.run_on_main_thread(move || match event {
                Event::Progress(p) => {
                    set(|s| s.progress = Some(p));
                    settings::redraw(&app, settings::UPDATES);
                }
                Event::Done(Ok(exe)) => {
                    if let Some(UpdateResult::Newer(tag)) = state().result {
                        config::update(&app, |c| c.updates.last_installed = tag);
                    }
                    set(|s| {
                        s.installing = false;
                        s.progress = None;
                    });
                    match relaunch(&exe) {
                        Ok(()) => app.exit(0),
                        Err(e) => {
                            set(|s| s.error = Some(e));
                            settings::redraw(&app, settings::UPDATES);
                        }
                    }
                }
                Event::Done(Err(e)) => {
                    set(|s| {
                        s.installing = false;
                        s.progress = None;
                        s.error = Some(e);
                    });
                    settings::redraw(&app, settings::UPDATES);
                }
            });
        }
    });
}

fn update_blocking(install: &Install, tag: &str, cache_dir: &Path, tx: &Sender<Event>) -> Result<PathBuf, String> {
    let say = |s: &str| {
        let _ = tx.send(Event::Progress(s.to_string()));
    };
    let version = tag.trim_start_matches('v');
    let arch = std::env::consts::ARCH;
    let deb_arch = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };
    match install {
        Install::Repo(Pkg::Dnf) => {
            say("Asking dnf for the update\u{2026}");
            run_as_root(&["dnf", "upgrade", "-y", "--refresh", PACKAGE])?;
            installed_binary()
        }
        Install::Repo(Pkg::Apt) => {
            say("Asking apt for the update\u{2026}");
            run_as_root(&["sh", "-c", &format!("apt-get update && apt-get install -y {PACKAGE}")])?;
            installed_binary()
        }
        Install::Package(manager) => {
            let name = match manager {
                Pkg::Dnf => format!("{PACKAGE}-{version}-1.{arch}.rpm"),
                Pkg::Apt => format!("{PACKAGE}_{version}_{deb_arch}.deb"),
            };
            let file = download_verified(tag, &name, cache_dir, &say)?;
            say("Installing the package\u{2026}");
            let path = file.to_string_lossy().into_owned();
            let result = match manager {
                Pkg::Dnf => run_as_root(&["dnf", "install", "-y", &path]),
                Pkg::Apt => run_as_root(&["apt-get", "install", "-y", &path]),
            };
            let _ = std::fs::remove_file(&file);
            result?;
            installed_binary()
        }
        Install::AppImage(current) => {
            // The AppImage keeps the deb's amd64 but the rpm's aarch64.
            let name = format!("{PACKAGE}_{version}_{}.AppImage", if arch == "x86_64" { "amd64" } else { arch });
            let file = download_verified(tag, &name, cache_dir, &say)?;
            say("Replacing the AppImage\u{2026}");
            let dir = current.parent().ok_or("The AppImage is not in a directory")?;
            let staged = dir.join(format!(".{name}.new"));
            std::fs::copy(&file, &staged).map_err(|e| format!("Cannot write next to the AppImage: {e}"))?;
            let _ = std::fs::remove_file(&file);
            #[cfg(unix)]
            let _ = std::fs::set_permissions(&staged, std::os::unix::fs::PermissionsExt::from_mode(0o755));
            // The running copy is mounted from its own file and keeps
            // working after the rename.
            std::fs::rename(&staged, current).map_err(|e| format!("Cannot replace the AppImage: {e}"))?;
            Ok(current.clone())
        }
        Install::Other => Err("This copy cannot update itself".into()),
    }
}

/// The package's binary after an install, for the relaunch.
fn installed_binary() -> Result<PathBuf, String> {
    let p = PathBuf::from("/usr/bin/outlook-for-linux");
    if p.is_file() {
        Ok(p)
    } else {
        std::env::current_exe().map_err(|e| format!("Cannot find the new binary: {e}"))
    }
}

/// Download one release file and its SHA256SUMS into the cache directory
/// and verify the file with `sha256sum`; the file's path on success.
fn download_verified(tag: &str, name: &str, cache_dir: &Path, say: &dyn Fn(&str)) -> Result<PathBuf, String> {
    std::fs::create_dir_all(cache_dir).map_err(|e| format!("Cannot create {}: {e}", cache_dir.display()))?;
    say(&format!("Downloading {name}\u{2026}"));
    let file = cache_dir.join(name);
    let sums = cache_dir.join("SHA256SUMS");
    for (url_name, dest) in [(name, &file), ("SHA256SUMS", &sums)] {
        let url = format!("{DOWNLOAD_URL}/{tag}/{url_name}");
        let status = Command::new("curl")
            .args(["-fsSL", "--max-time", "600", "-o"])
            .arg(dest)
            .arg(&url)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| format!("Cannot run curl: {e}"))?;
        if !status.success() {
            let _ = std::fs::remove_file(&file);
            let _ = std::fs::remove_file(&sums);
            return Err(format!("Download of {url_name} failed (curl exit {})", status.code().unwrap_or(-1)));
        }
    }
    say("Checking the download\u{2026}");
    let ok = Command::new("sha256sum")
        .args(["--check", "--ignore-missing", "--status", "SHA256SUMS"])
        .current_dir(cache_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| format!("Cannot run sha256sum: {e}"))?
        .success();
    let _ = std::fs::remove_file(&sums);
    if !ok {
        let _ = std::fs::remove_file(&file);
        return Err(format!("{name} did not match the release's checksum"));
    }
    Ok(file)
}

/// Run a command as root through polkit, which shows the desktop's prompt.
fn run_as_root(args: &[&str]) -> Result<(), String> {
    let status = Command::new("pkexec").args(args).stdin(Stdio::null()).status().map_err(|e| format!("Cannot run pkexec: {e}"))?;
    match status.code() {
        Some(0) => Ok(()),
        Some(126) => Err("Authorization dismissed".into()),
        Some(127) => Err("Not authorized".into()),
        Some(c) => Err(format!("The package manager stopped with exit code {c}")),
        None => Err("The package manager was interrupted".into()),
    }
}

/// Start the new program detached, so this one can close.
fn relaunch(exe: &Path) -> Result<(), String> {
    Command::new(exe)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Cannot start {}: {e}", exe.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_and_tags_parse() {
        assert_eq!(parse_version("v0.7.0"), Some((0, 7, 0)));
        assert!(parse_version("v0.10.0") > parse_version("v0.9.9"));
        assert_eq!(parse_version("nightly"), None);
        assert_eq!(release_tag(r#"{"url": "x", "tag_name": "v0.8.1", "name": "v0.8.1"}"#).as_deref(), Some("v0.8.1"));
        assert_eq!(release_tag("not json"), None);
    }

    #[test]
    fn repository_installs_are_told_apart_from_file_installs() {
        assert!(!repo_from_dnf("@commandline\n"));
        assert!(!repo_from_dnf("@System"));
        assert!(!repo_from_dnf(""));
        assert!(repo_from_dnf("copr:copr.fedorainfracloud.org:nikolainyegaard:outlook-for-linux\n"));

        let file_install = "outlook-for-linux:\n  Installed: 0.7.0\n  Candidate: 0.7.0\n  Version table:\n *** 0.7.0 100\n        100 /var/lib/dpkg/status\n";
        assert!(!repo_from_apt(file_install));
        let repo_install = "outlook-for-linux:\n  Installed: 0.7.0\n  Candidate: 0.7.1\n  Version table:\n     0.7.1 500\n        500 https://example.org/apt stable/main amd64 Packages\n *** 0.7.0 500\n        100 /var/lib/dpkg/status\n";
        assert!(repo_from_apt(repo_install));
    }

    #[test]
    fn just_installed_tag_is_not_offered_again() {
        let s = State { result: Some(UpdateResult::Newer("v0.8.0".into())), ..State::default() };
        let mut config = config::Config::default();
        assert_eq!(s.available(&config), Some("v0.8.0"));
        config.updates.last_installed = "v0.8.0".into();
        assert_eq!(s.available(&config), None);
        assert!(!Install::Other.can_update());
        assert!(Install::AppImage(PathBuf::from("/x")).can_update());
    }
}
