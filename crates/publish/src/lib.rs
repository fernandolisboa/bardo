//! Adapters per network: sign-in, upload, schedule, metrics and export.
//!
//! Every adapter implements a domain port and goes through the HTTP seam of
//! `bardo_ai::http`, so it is tested against recorded responses and fake
//! servers, never live calls.

pub mod instagram;
pub mod instagram_insights;
pub mod instagram_upload;
pub mod loopback;
pub mod oauth;
pub mod tiktok;
pub mod tiktok_insights;
pub mod tiktok_upload;
pub mod youtube;
pub mod youtube_analytics;
pub mod youtube_upload;

mod text;

pub use instagram::{InstagramSignIn, MetaEndpoints};
pub use instagram_insights::InstagramInsights;
pub use instagram_upload::InstagramUploader;
pub use loopback::LoopbackReceiver;
pub use tiktok::{TikTokEndpoints, TikTokSignIn};
pub use tiktok_insights::TikTokInsights;
pub use tiktok_upload::TikTokUploader;
pub use youtube::{GoogleEndpoints, YouTubeSignIn};
pub use youtube_analytics::YouTubeAnalytics;
pub use youtube_upload::YouTubeUploader;
