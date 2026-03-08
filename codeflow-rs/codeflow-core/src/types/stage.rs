use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING-KEBAB-CASE")]
pub enum WorkStage {
    WsDev,
    WsRev,
    WsQa,
    WsTest,
    WsPlan,
    WsDocs,
}
