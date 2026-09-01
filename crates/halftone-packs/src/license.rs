//! Offline license verification. Placeholder until the key ceremony exists.

use serde::{Deserialize, Serialize};

/// Signed license payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct License {
    /// Customer identifier.
    pub customer: String,
    /// Plan name.
    pub plan: String,
    /// RFC 3339 expiry for pack updates (the binary keeps working after).
    pub updates_until: String,
    /// Enabled feature flags.
    pub features: Vec<String>,
}
