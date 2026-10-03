//! Adapters per network: sign-in, upload, schedule, metrics and export.
//!
//! Every adapter implements a domain port and goes through the HTTP seam of
//! `bardo_ai::http`, so it is tested against recorded responses and fake
//! servers, never live calls.

pub mod loopback;
pub mod oauth;
pub mod youtube;

pub use loopback::LoopbackReceiver;
pub use youtube::{GoogleEndpoints, YouTubeSignIn};
