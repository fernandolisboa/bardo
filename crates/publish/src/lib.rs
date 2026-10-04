//! Adapters per network: sign-in, upload, schedule, metrics and export.
//!
//! Every adapter implements a domain port and goes through the HTTP seam of
//! `bardo_ai::http`, so it is tested against recorded responses and fake
//! servers, never live calls.

pub mod instagram;
pub mod instagram_upload;
pub mod loopback;
pub mod oauth;
pub mod tiktok;
pub mod youtube;
pub mod youtube_analytics;
pub mod youtube_upload;

pub use instagram::{InstagramSignIn, MetaEndpoints};
pub use instagram_upload::InstagramUploader;
pub use loopback::LoopbackReceiver;
pub use tiktok::{TikTokEndpoints, TikTokSignIn};
pub use youtube::{GoogleEndpoints, YouTubeSignIn};
pub use youtube_analytics::YouTubeAnalytics;
pub use youtube_upload::YouTubeUploader;
