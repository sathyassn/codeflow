use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum Phase {
    Pf1Init,
    Pf2Context,
    Pf3Classify,
    Pf4Execute,
    Pf5Verify,
    Pf6Complete,
    Pf7End,
}
