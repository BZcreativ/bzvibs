//! A YouTube video id, validated at construction. Everything that builds a
//! watch URL or a resolver argv takes this type, never a raw string.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoId(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid video id: {0:?}")]
pub struct InvalidVideoId(String);

impl VideoId {
    /// Accept exactly 11 characters of `[A-Za-z0-9_-]`, the full YouTube id
    /// alphabet.
    pub fn parse(raw: &str) -> Result<Self, InvalidVideoId> {
        let valid = raw.len() == 11
            && raw
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
        if valid {
            Ok(VideoId(raw.to_string()))
        } else {
            Err(InvalidVideoId(raw.to_string()))
        }
    }

    pub fn watch_url(&self) -> String {
        format!("https://www.youtube.com/watch?v={}", self.0)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_real_ids_including_dash_and_underscore() {
        for raw in ["lYBUbBu4W08", "pHWj-hRP-cY", "uSFXOBaqiR8", "abcDEF123-_"] {
            let id = VideoId::parse(raw).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(id.as_str(), raw);
            assert_eq!(
                id.watch_url(),
                format!("https://www.youtube.com/watch?v={raw}")
            );
        }
    }

    #[test]
    fn rejects_wrong_length_bad_alphabet_and_empty() {
        for raw in [
            "",
            "short",
            "waytoolongidentifier",
            "bad chars!!",
            "lYBUbBu4W08\n",
        ] {
            assert!(VideoId::parse(raw).is_err(), "{raw:?} should be rejected");
        }
    }
}
