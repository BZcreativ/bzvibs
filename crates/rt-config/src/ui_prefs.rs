//! UI appearance preferences: font family, global text scale. Persisted as
//! one JSON file under the config dir; loaded at boot so settings survive
//! restarts.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum FontChoice {
    #[serde(rename = "system")]
    System,
    #[serde(rename = "inter")]
    Inter,
}

impl FontChoice {
    pub fn all() -> [FontChoice; 2] {
        [FontChoice::System, FontChoice::Inter]
    }

    pub fn label(self) -> String {
        match self {
            FontChoice::System => "System default".to_string(),
            FontChoice::Inter => "Inter".to_string(),
        }
    }

    /// The font family name iced resolves at render time. Inter is embedded
    /// in the binary (see rt-ui) and registered under this family name.
    pub fn family(self) -> Option<&'static str> {
        match self {
            FontChoice::System => None,
            FontChoice::Inter => Some("Inter"),
        }
    }
}

impl std::fmt::Display for FontChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum TextSize {
    #[serde(rename = "small")]
    Small,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "large")]
    Large,
    #[serde(rename = "xl")]
    ExtraLarge,
}

impl TextSize {
    pub fn all() -> [TextSize; 4] {
        [
            TextSize::Small,
            TextSize::Medium,
            TextSize::Large,
            TextSize::ExtraLarge,
        ]
    }

    pub fn label(self) -> String {
        match self {
            TextSize::Small => "Small".to_string(),
            TextSize::Medium => "Medium".to_string(),
            TextSize::Large => "Large".to_string(),
            TextSize::ExtraLarge => "Extra large".to_string(),
        }
    }

    /// Multiplier applied to the whole interface via the window scale
    /// factor (text, padding, and controls scale together).
    pub fn scale(self) -> f32 {
        match self {
            TextSize::Small => 0.85,
            TextSize::Medium => 1.0,
            TextSize::Large => 1.15,
            TextSize::ExtraLarge => 1.3,
        }
    }
}

impl std::fmt::Display for TextSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct UiPrefs {
    pub font: FontChoice,
    pub text_size: TextSize,
}

impl Default for UiPrefs {
    fn default() -> Self {
        UiPrefs {
            font: FontChoice::Inter,
            text_size: TextSize::Medium,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PrefsError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bad prefs file {path}: {reason}")]
    BadFile { path: PathBuf, reason: String },
}

fn prefs_file() -> PathBuf {
    let base = if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("bzvibs")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".config").join("bzvibs")
    } else {
        PathBuf::from(".bzvibs")
    };
    base.join("ui-prefs.json")
}

/// Load saved prefs, or defaults when absent. A corrupt file falls back to
/// defaults rather than failing the app boot.
pub fn load() -> UiPrefs {
    let path = prefs_file();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return UiPrefs::default();
    };
    match serde_json::from_str(&text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ignoring bad ui-prefs {}: {e}", path.display());
            UiPrefs::default()
        }
    }
}

/// Persist prefs. Called on every change from the settings screen.
pub fn store(prefs: &UiPrefs) -> Result<(), super::PresetError> {
    let path = prefs_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_string_pretty(prefs).map_err(|e| super::PresetError::BadFile {
        path: path.clone(),
        reason: e.to_string(),
    })?;
    std::fs::write(&path, json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn with_temp_dir(f: impl FnOnce(&PathBuf)) {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rt-prefs-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // SAFETY: confined to this test process; each test gets a unique dir.
        unsafe {
            std::env::set_var("APPDATA", &dir);
        }
        f(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn defaults_are_inter_medium() {
        let d = UiPrefs::default();
        assert_eq!(d.font, FontChoice::Inter);
        assert_eq!(d.text_size, TextSize::Medium);
        assert_eq!(d.text_size.scale(), 1.0);
    }

    #[test]
    fn store_then_load_round_trips() {
        with_temp_dir(|_| {
            let p = UiPrefs {
                font: FontChoice::System,
                text_size: TextSize::ExtraLarge,
            };
            store(&p).unwrap();
            assert_eq!(load(), p);
        });
    }

    #[test]
    fn missing_file_yields_defaults() {
        with_temp_dir(|_| {
            assert_eq!(load(), UiPrefs::default());
        });
    }

    #[test]
    fn corrupt_file_yields_defaults_not_a_crash() {
        with_temp_dir(|dir| {
            let file = dir.join("bzvibs").join("ui-prefs.json");
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "{ broken").unwrap();
            assert_eq!(load(), UiPrefs::default());
        });
    }

    #[test]
    fn scales_cover_the_reported_range() {
        assert_eq!(TextSize::Small.scale(), 0.85);
        assert_eq!(TextSize::ExtraLarge.scale(), 1.3);
    }
}
