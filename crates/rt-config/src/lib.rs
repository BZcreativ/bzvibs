//! EQ presets: one JSON file per named preset under the config dir, plus
//! the built-ins. The engine consumes plain `[f32; 8]` gains; this crate
//! owns persistence so the UI never touches files.

use rt_audio::BAND_FREQS_HZ;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Preset {
    pub name: String,
    #[serde(rename = "gains_db")]
    pub gains: [f32; 8],
}

#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("bad preset file {path}: {reason}")]
    BadFile { path: PathBuf, reason: String },
}

/// `%APPDATA%\bzvibs\presets` on Windows, `~/.config/bzvibs/presets`
/// elsewhere.
pub fn presets_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("bzvibs").join("presets")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join(".config")
            .join("bzvibs")
            .join("presets")
    } else {
        PathBuf::from(".bzvibs").join("presets")
    }
}

pub fn builtin_presets() -> Vec<Preset> {
    vec![
        Preset {
            name: "Flat".into(),
            gains: [0.0; 8],
        },
        // The PRD's bass emphasis: 60/250 shelves plus a touch of air.
        Preset {
            name: "Bass".into(),
            gains: [6.0, 4.0, 2.0, 0.0, 0.0, 0.0, 1.0, 2.0],
        },
        // The PRD's vocal-boost preset: dip the low mids, lift presence.
        Preset {
            name: "Vocal".into(),
            gains: [2.0, 0.0, -1.0, -2.0, 1.5, 3.5, 3.0, 2.0],
        },
        Preset {
            name: "Treble".into(),
            gains: [0.0, 0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 4.5],
        },
        Preset {
            name: "Rock".into(),
            gains: [4.5, 3.0, -1.5, -2.0, 1.5, 3.5, 4.0, 4.0],
        },
        Preset {
            name: "Classical".into(),
            gains: [3.5, 2.0, 0.0, 0.0, 0.0, -1.0, -2.0, -2.5],
        },
    ]
}

/// The band label shown next to a slider ("60 Hz" … "16 kHz").
pub fn band_label(i: usize) -> String {
    let hz = BAND_FREQS_HZ[i.min(7)];
    if hz >= 1000.0 {
        format!("{:.0} kHz", hz / 1000.0)
    } else {
        format!("{hz:.0} Hz")
    }
}

fn file_stem(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Persist a preset in the presets dir. Idempotent: saving the same name
/// again overwrites the same file.
pub fn save(preset: &Preset) -> Result<PathBuf, PresetError> {
    save_to(&presets_dir(), preset)
}

/// Every user preset in the presets dir, sorted by name. Unreadable files
/// are skipped, not fatal: a preset list must never take the app down.
pub fn list_user() -> Vec<Preset> {
    list_from(&presets_dir())
}

pub fn save_to(dir: &std::path::Path, preset: &Preset) -> Result<PathBuf, PresetError> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", file_stem(&preset.name)));
    let json = serde_json::to_string_pretty(preset).map_err(|e| PresetError::BadFile {
        path: path.clone(),
        reason: e.to_string(),
    })?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// Delete a user preset by name. Missing files are not an error.
pub fn delete_from(dir: &std::path::Path, name: &str) -> Result<(), PresetError> {
    let path = dir.join(format!("{}.json", file_stem(name)));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Rename a user preset: save under the new name, remove the old file.
pub fn rename_in(dir: &std::path::Path, old: &str, new: &str) -> Result<Preset, PresetError> {
    let presets = list_from(dir);
    let Some(mut preset) = presets.into_iter().find(|p| p.name == old) else {
        return Err(PresetError::BadFile {
            path: dir.to_path_buf(),
            reason: format!("preset {old:?} not found"),
        });
    };
    preset.name = new.to_string();
    save_to(dir, &preset)?;
    delete_from(dir, old)?;
    Ok(preset)
}

pub fn list_from(dir: &std::path::Path) -> Vec<Preset> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut presets: Vec<Preset> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let path = e.path();
            let text = std::fs::read_to_string(&path).ok()?;
            match serde_json::from_str::<Preset>(&text) {
                Ok(p) => Some(p),
                Err(reason) => {
                    eprintln!("skipping bad preset {}: {reason}", path.display());
                    None
                }
            }
        })
        .collect();
    presets.sort_by(|a, b| a.name.cmp(&b.name));
    presets
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    // Tests pass explicit dirs (unique per test) so parallel tests never
    // share the real config dir or a mutated environment.
    fn with_temp_dir(f: impl FnOnce(&PathBuf)) {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rt-config-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        f(&dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_then_list_round_trips_every_field() {
        with_temp_dir(|dir| {
            let p = Preset {
                name: "My Vocal+".into(),
                gains: [1.5, -2.0, 0.0, 3.0, -4.5, 6.0, 0.5, -1.0],
            };
            save_to(dir, &p).expect("save");
            let listed = list_from(dir);
            assert_eq!(listed, vec![p]);
        });
    }

    #[test]
    fn delete_removes_only_the_named_preset() {
        with_temp_dir(|dir| {
            save_to(
                dir,
                &Preset {
                    name: "A".into(),
                    gains: [1.0; 8],
                },
            )
            .unwrap();
            save_to(
                dir,
                &Preset {
                    name: "B".into(),
                    gains: [2.0; 8],
                },
            )
            .unwrap();
            delete_from(dir, "A").unwrap();
            // Missing is not an error (idempotent).
            delete_from(dir, "A").unwrap();
            let listed = list_from(dir);
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].name, "B");
        });
    }

    #[test]
    fn rename_moves_gains_under_new_name() {
        with_temp_dir(|dir| {
            save_to(
                dir,
                &Preset {
                    name: "Old".into(),
                    gains: [3.0; 8],
                },
            )
            .unwrap();
            let renamed = rename_in(dir, "Old", "New Name").unwrap();
            assert_eq!(renamed.name, "New Name");
            assert_eq!(renamed.gains, [3.0; 8]);
            let listed = list_from(dir);
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].name, "New Name");
        });
    }

    #[test]
    fn rename_of_missing_preset_errors() {
        with_temp_dir(|dir| {
            assert!(rename_in(dir, "ghost", "x").is_err());
        });
    }

    #[test]
    fn saving_twice_overwrites_and_keeps_one_file() {
        with_temp_dir(|dir| {
            save_to(
                dir,
                &Preset {
                    name: "A".into(),
                    gains: [1.0; 8],
                },
            )
            .unwrap();
            save_to(
                dir,
                &Preset {
                    name: "A".into(),
                    gains: [2.0; 8],
                },
            )
            .unwrap();
            let listed = list_from(dir);
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].gains, [2.0; 8]);
        });
    }

    #[test]
    fn bad_json_is_skipped_not_fatal() {
        with_temp_dir(|dir| {
            std::fs::write(dir.join("broken.json"), "{ not json").unwrap();
            std::fs::write(
                dir.join("good.json"),
                r#"{ "name": "Good", "gains_db": [0,0,0,0,0,0,0,0] }"#,
            )
            .unwrap();
            let listed = list_from(dir);
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].name, "Good");
        });
    }

    #[test]
    fn band_labels_read_naturally() {
        assert_eq!(band_label(0), "60 Hz");
        assert_eq!(band_label(3), "1 kHz");
        assert_eq!(band_label(7), "16 kHz");
    }

    #[test]
    fn builtins_are_eight_bands_and_named() {
        for p in builtin_presets() {
            assert_eq!(p.gains.len(), 8, "{}", p.name);
        }
        assert!(builtin_presets().iter().any(|p| p.name == "Vocal"));
    }
}
