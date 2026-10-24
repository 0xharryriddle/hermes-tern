use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    Config(&'static str),
    #[error("gateway discovery failed: {0}")]
    Bootstrap(&'static str),
    #[error("protocol error: {0}")]
    Protocol(&'static str),
    #[error("transport error: {0}")]
    Transport(&'static str),
    #[error("{0} timed out; submitted work may have an unknown outcome")]
    Timeout(&'static str),
    #[error("gateway disconnected; outcome unknown. Resume the session; do not blindly resend")]
    Disconnected,
    #[error("runtime refused request: {0}")]
    Refused(SafeReason),
    #[error("unsupported capability: {0}")]
    Unsupported(&'static str),
    #[error("local I/O error ({0})")]
    Io(std::io::ErrorKind),
    #[error("storage error: {0}")]
    Store(&'static str),
}

#[derive(Debug, Clone)]
pub struct SafeReason(String);
impl SafeReason {
    pub fn new(value: &str) -> Self {
        let valid = !value.is_empty()
            && value.len() <= 80
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_');
        Self(if valid {
            value.to_owned()
        } else {
            "request_failed".to_owned()
        })
    }
}
impl fmt::Display for SafeReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.kind())
    }
}
pub type Result<T> = std::result::Result<T, Error>;
