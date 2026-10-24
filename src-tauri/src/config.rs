//! Runtime config: config.json in the app config dir, one file for the icon
//! and the profiles. Read on demand (it is tiny) and always written whole,
//! so every writer goes through `update` and never clobbers another key.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// The profile that owns the pre-profiles data directory, so an existing
/// sign-in carries over without a new login.
pub const DEFAULT_PROFILE: &str = "default";
const DEFAULT_UNLOAD_MINUTES: u32 = 15;

#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Directory name under profiles/, never shown.
    pub id: String,
    pub name: String,
    /// Stay loaded while deselected, whatever `unload_after_minutes` says.
    #[serde(default)]
    pub keep_awake: bool,
    /// Forward this profile's new-mail notifications to the desktop.
    #[serde(default = "yes")]
    pub notifications: bool,
}

impl Profile {
    pub fn new(id: String, name: String) -> Self {
        Self { id, name, keep_awake: false, notifications: true }
    }
}

fn yes() -> bool {
    true
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Notifications {
    /// Master switch for desktop notifications.
    pub enabled: bool,
    /// Drop notifications while the main window is focused.
    pub only_when_hidden: bool,
    /// Include the message preview; off keeps sender and subject only.
    pub preview: bool,
    /// Let the desktop play its notification sound.
    pub sound: bool,
    /// Red dot on the tray icon while there is unread mail.
    pub tray_badge: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Self { enabled: true, only_when_hidden: false, preview: true, sound: true, tray_badge: true }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Path of the custom icon (PNG or SVG); empty means the bundled icon.
    pub app_icon: String,
    /// Minutes a deselected profile stays loaded: 0 unloads it at once,
    /// null never unloads. A missing key means the default.
    pub unload_after_minutes: Option<u32>,
    pub active_profile: String,
    pub profiles: Vec<Profile>,
    pub notifications: Notifications,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            app_icon: String::new(),
            unload_after_minutes: Some(DEFAULT_UNLOAD_MINUTES),
            active_profile: DEFAULT_PROFILE.into(),
            profiles: vec![Profile::new(DEFAULT_PROFILE.into(), "Default".into())],
            notifications: Notifications::default(),
        }
    }
}

impl Config {
    pub fn profile(&self, id: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.id == id)
    }

    /// Whether a notification from `profile` reaches the desktop.
    pub fn notifies(&self, profile: &str) -> bool {
        self.notifications.enabled && self.profile(profile).is_none_or(|p| p.notifications)
    }

    /// None means never unload.
    pub fn unload_after(&self) -> Option<Duration> {
        self.unload_after_minutes.map(|m| Duration::from_secs(u64::from(m) * 60))
    }

    /// A hand-edited file can leave no profiles or a stale active id.
    fn normalized(mut self) -> Self {
        if self.profiles.is_empty() {
            self.profiles = Config::default().profiles;
        }
        if self.profile(&self.active_profile).is_none() {
            self.active_profile = self.profiles[0].id.clone();
        }
        self
    }
}

fn file(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("config.json"))
}

pub fn load(app: &AppHandle) -> Config {
    file(app)
        .and_then(|f| fs::read(f).ok())
        .and_then(|b| serde_json::from_slice::<Config>(&b).ok())
        .unwrap_or_default()
        .normalized()
}

fn save(app: &AppHandle, config: &Config) {
    let Some(file) = file(app) else { return };
    let write = || -> std::io::Result<()> {
        fs::create_dir_all(file.parent().unwrap_or(Path::new(".")))?;
        fs::write(&file, serde_json::to_vec_pretty(config)?)
    };
    if let Err(e) = write() {
        eprintln!("could not save config: {e}");
    }
}

/// Read, change, write. Returns the saved config.
pub fn update(app: &AppHandle, change: impl FnOnce(&mut Config)) -> Config {
    let mut config = load(app);
    change(&mut config);
    let config = config.normalized();
    save(app, &config);
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_null_and_zero_unload_values() {
        let missing: Config = serde_json::from_str(r#"{"app_icon": ""}"#).unwrap();
        assert_eq!(missing.unload_after(), Some(Duration::from_secs(15 * 60)));
        let never: Config = serde_json::from_str(r#"{"unload_after_minutes": null}"#).unwrap();
        assert_eq!(never.unload_after(), None);
        let now: Config = serde_json::from_str(r#"{"unload_after_minutes": 0}"#).unwrap();
        assert_eq!(now.unload_after(), Some(Duration::ZERO));
    }

    #[test]
    fn old_config_keeps_notifying() {
        let cfg: Config = serde_json::from_str(r#"{"profiles": [{"id": "a", "name": "A"}]}"#).unwrap();
        assert!(cfg.notifies("a"));
        assert!(cfg.notifications.tray_badge);
        let muted: Config = serde_json::from_str(r#"{"profiles": [{"id": "a", "name": "A", "notifications": false}]}"#).unwrap();
        assert!(!muted.notifies("a"));
    }

    #[test]
    fn normalize_repairs_empty_profiles_and_stale_active() {
        let cfg: Config = serde_json::from_str(r#"{"active_profile": "gone", "profiles": []}"#).unwrap();
        let cfg = cfg.normalized();
        assert_eq!(cfg.profiles.len(), 1);
        assert_eq!(cfg.active_profile, DEFAULT_PROFILE);
    }
}
