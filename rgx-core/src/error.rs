use thiserror::Error;

pub type Result<T> = std::result::Result<T, RgxError>;

#[derive(Debug, Error)]
pub enum RgxError {
    #[error("pattern compile error: {0}")]
    Compile(String),

    #[error("engine error: {0}")]
    Engine(String),
}
