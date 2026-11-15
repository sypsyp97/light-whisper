#![allow(dead_code)]

#[path = "../../../src-tauri/src/agent_api/secure_files.rs"]
pub mod secure_files;
#[path = "../../../src-tauri/src/agent_api/transport.rs"]
pub mod transport;

// Both production module paths are available without importing Tauri.
pub mod agent_api {
    pub use crate::{secure_files, transport};
}
