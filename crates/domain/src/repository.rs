/// A persistence adapter failed. The cause is kept for logs; the UI shows a
/// generic message because the user cannot act on SQLite details.
#[derive(Debug, thiserror::Error)]
#[error("storage failed: {0}")]
pub struct RepositoryError(#[from] pub Box<dyn std::error::Error + Send + Sync>);
