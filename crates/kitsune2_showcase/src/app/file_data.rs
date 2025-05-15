use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileData {
    /// The name of the file to be stored
    pub name: String,

    // The contents of the file
    pub contents: String,
}
