//! Adapters for the decision engine, generative providers, voices, market
//! data, public video statistics and Bardo's own releases.

pub mod claude;
pub mod elevenlabs;
pub mod gemini;
pub mod github_releases;
pub mod google_clips;
pub mod higgsfield;
pub mod http;
pub mod jev;
pub mod key_check;
pub mod kling;
pub mod market;
pub mod retry;
pub mod youtube_stats;

pub use claude::ClaudeTextGenerator;
pub use elevenlabs::{
    ElevenLabsAlignment, ElevenLabsMusic, ElevenLabsPreviews, ElevenLabsSpeech,
    ElevenLabsTranscription, ElevenLabsVoiceChanger, ElevenLabsVoiceIsolator, ElevenLabsVoices,
};
pub use gemini::GeminiImages;
pub use github_releases::GitHubReleases;
pub use google_clips::GoogleClips;
pub use higgsfield::{HiggsfieldClips, HiggsfieldTakes};
pub use jev::JevDecisionEngine;
pub use key_check::HttpKeyChecker;
pub use kling::KlingClips;
pub use market::YouTubeMarketData;
pub use youtube_stats::YouTubeStats;
