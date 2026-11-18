//! Stream resolution and guarded fetching for itag-140 audio.

pub mod fetch;
pub mod id;
pub mod proc;
pub mod ytdlp;

pub use fetch::{FetchError, fetch_full};
pub use id::{InvalidVideoId, VideoId};
pub use ytdlp::{ResolveError, ResolvedStream, StreamResolver, YtDlpResolver};
