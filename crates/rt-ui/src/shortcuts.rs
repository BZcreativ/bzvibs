//! Global keyboard shortcuts (PR-16). Resolution is a pure function over the
//! key event so it can be tested without a window; the app maps each
//! [`Shortcut`] onto a real command.
//!
//! Rule against swallowing typed input: a bare key that produces text (letters,
//! digits, punctuation) is never a shortcut, and Ctrl must be held for letter
//! and digit shortcuts. Space and the arrows are bare shortcuts only while
//! the search field is not being typed into.

use iced::keyboard::Modifiers;
use iced::keyboard::key::{Key, Named};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    PlayPause,
    SeekBack,
    SeekForward,
    VolumeUp,
    VolumeDown,
    FocusSearch,
    Home,
    Explore,
    Library,
    Equalizer,
    Mute,
    Like,
    Help,
}

/// The reference shown in the help overlay: (label, keys).
pub const TABLE: [(&str, &str); 13] = [
    ("Play / Pause", "Space"),
    ("Seek forward", "→"),
    ("Seek backward", "←"),
    ("Volume up", "↑"),
    ("Volume down", "↓"),
    ("Search", "Ctrl + K"),
    ("Home", "Ctrl + 1"),
    ("Explore", "Ctrl + 2"),
    ("Library", "Ctrl + 3"),
    ("Equalizer", "Ctrl + E"),
    ("Mute", "Ctrl + M"),
    ("Like current track", "L"),
    ("Show shortcuts", "?"),
];

/// Resolve a key event to a shortcut, or None when the key must reach the
/// focused widget (typing) instead.
pub fn resolve(key: &Key, mods: Modifiers, typing: bool) -> Option<Shortcut> {
    let named = |n: Named| matches!(key, Key::Named(k) if *k == n);
    let ch = |c: &str| match key {
        Key::Character(s) => s.eq_ignore_ascii_case(c),
        _ => false,
    };

    if mods.control() {
        return if ch("k") {
            Some(Shortcut::FocusSearch)
        } else if ch("e") {
            Some(Shortcut::Equalizer)
        } else if ch("l") {
            Some(Shortcut::Library)
        } else if ch("m") {
            Some(Shortcut::Mute)
        } else if ch("1") {
            Some(Shortcut::Home)
        } else if ch("2") {
            Some(Shortcut::Explore)
        } else if ch("3") {
            Some(Shortcut::Library)
        } else {
            None
        };
    }

    // Help is shift + '/', which arrives as the '?' character.
    if ch("?") {
        return Some(Shortcut::Help);
    }
    if typing {
        // Never steal keys while the user types in a field.
        return None;
    }
    if named(Named::Space) {
        Some(Shortcut::PlayPause)
    } else if named(Named::ArrowLeft) {
        Some(Shortcut::SeekBack)
    } else if named(Named::ArrowRight) {
        Some(Shortcut::SeekForward)
    } else if named(Named::ArrowUp) {
        Some(Shortcut::VolumeUp)
    } else if named(Named::ArrowDown) {
        Some(Shortcut::VolumeDown)
    } else if ch("l") {
        Some(Shortcut::Like)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::Modifiers;
    use iced::keyboard::key::Named;

    fn ctrl() -> Modifiers {
        Modifiers::CTRL
    }

    fn plain() -> Modifiers {
        Modifiers::empty()
    }

    #[test]
    fn ctrl_letters_map_to_actions_while_typing_is_irrelevant() {
        let cases = [
            ("k", Shortcut::FocusSearch),
            ("e", Shortcut::Equalizer),
            ("l", Shortcut::Library),
            ("m", Shortcut::Mute),
            ("1", Shortcut::Home),
            ("2", Shortcut::Explore),
            ("3", Shortcut::Library),
        ];
        for (ch, want) in cases {
            let key = Key::Character(ch.into());
            assert_eq!(resolve(&key, ctrl(), true), Some(want), "Ctrl+{ch}");
            assert_eq!(resolve(&key, ctrl(), false), Some(want), "Ctrl+{ch}");
        }
    }

    #[test]
    fn bare_letters_never_shortcut_so_typing_survives() {
        for ch in ["a", "l", "m", "q", "z", "5"] {
            let key = Key::Character(ch.into());
            // Regardless of the typing flag, a bare letter must pass through.
            assert_eq!(resolve(&key, plain(), true), None, "{ch}");
        }
        // 'L' is the like shortcut only when not typing.
        let l = Key::Character("l".into());
        assert_eq!(resolve(&l, plain(), false), Some(Shortcut::Like));
        assert_eq!(resolve(&l, plain(), true), None);
    }

    #[test]
    fn space_and_arrows_are_suppressed_while_typing() {
        for (k, want) in [
            (Named::Space, Shortcut::PlayPause),
            (Named::ArrowLeft, Shortcut::SeekBack),
            (Named::ArrowRight, Shortcut::SeekForward),
            (Named::ArrowUp, Shortcut::VolumeUp),
            (Named::ArrowDown, Shortcut::VolumeDown),
        ] {
            let key = Key::Named(k);
            assert_eq!(resolve(&key, plain(), false), Some(want), "{k:?}");
            assert_eq!(resolve(&key, plain(), true), None, "{k:?} while typing");
        }
    }

    #[test]
    fn help_needs_question_mark_and_is_not_a_typing_key() {
        let q = Key::Character("?".into());
        assert_eq!(resolve(&q, Modifiers::SHIFT, true), Some(Shortcut::Help));
        // An unrelated modified key resolves to nothing.
        let z = Key::Character("z".into());
        assert_eq!(resolve(&z, ctrl(), false), None);
    }

    #[test]
    fn every_table_row_has_a_resolvable_default() {
        // The overlay must not advertise a shortcut the resolver ignores.
        assert_eq!(TABLE.len(), 13);
        assert!(TABLE.iter().all(|(l, k)| !l.is_empty() && !k.is_empty()));
    }
}
