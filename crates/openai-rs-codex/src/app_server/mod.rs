//! Spawn-and-supervise client for the Codex `app-server` subprocess.
//!
//! [`AppServerConfig`]'s `arguments` field has no public setter: the sealing
//! is deliberate, and exposing additional command-line flags later is a
//! conscious API change, not an oversight.

mod client;
mod codec;

pub use client::{
    AppServerClient, AppServerConfig, AppServerEvent, AppServerLimits, CodexAppServerClient,
    RawResponse, RawServerRequest,
};
