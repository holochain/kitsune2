#[derive(thiserror::Error, Debug, Clone)]
pub enum Kitsune2MemoryError {}

pub type Kitsune2MemoryResult<T> = Result<T, Kitsune2MemoryError>;
