//! Owned domain types crossing the API boundary. The UI never sees
//! ytmapi-rs types.

use ytmapi_rs::common::YoutubeID as _;
use ytmapi_rs::parse::SearchResultSong;

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub duration: String,
}

impl From<SearchResultSong> for Track {
    fn from(s: SearchResultSong) -> Self {
        Track {
            video_id: s.video_id.get_raw().to_string(),
            title: s.title,
            artist: s.artist,
            album: s.album.map(|a| a.name),
            duration: s.duration,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // SearchResultSong is #[non_exhaustive], so it cannot be constructed with
    // a struct expression outside its crate. Deserializing a fixture JSON is
    // the boundary this test actually wants: raw API shape in, Track out.
    const FIXTURE: &str = r#"{
        "title": "Never Gonna Give You Up",
        "artist": "Rick Astley",
        "album": { "name": "Whenever You Need Somebody", "id": "MPREb_fX4Yw8ZbmnM" },
        "duration": "3:34",
        "plays": "1.4B",
        "explicit": "NotExplicit",
        "video_id": "lYBUbBu4W08",
        "thumbnails": [
            { "width": 48, "height": 48, "url": "https://i.ytimg.com/vi/lYBUbBu4W08/s48.jpg" }
        ]
    }"#;

    #[test]
    fn fixture_json_parses_into_track_fields() {
        let song: SearchResultSong = serde_json::from_str(FIXTURE).expect("fixture parses");
        let t = Track::from(song);
        assert_eq!(t.video_id, "lYBUbBu4W08");
        assert_eq!(t.title, "Never Gonna Give You Up");
        assert_eq!(t.artist, "Rick Astley");
        assert_eq!(t.album.as_deref(), Some("Whenever You Need Somebody"));
        assert_eq!(t.duration, "3:34");
    }

    #[test]
    fn fixture_without_album_maps_to_none() {
        let song: SearchResultSong = serde_json::from_str(
            r#"{
                "title": "t", "artist": "a", "album": null, "duration": "1:00",
                "plays": "0", "explicit": "NotExplicit", "video_id": "dQw4w9WgXcQ",
                "thumbnails": []
            }"#,
        )
        .expect("fixture parses");
        assert!(Track::from(song).album.is_none());
    }
}
