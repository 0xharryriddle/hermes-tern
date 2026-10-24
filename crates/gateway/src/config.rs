use hermes_tern_core::error::{Error, Result};
use std::{path::PathBuf, time::Duration};

#[derive(Debug, Clone)]
pub struct Limits {
    pub max_frame_bytes: usize,
    pub max_events: usize,
    pub max_pending_requests: usize,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_frame_bytes: 8 * 1024 * 1024,
            max_events: 64,
            max_pending_requests: 64,
            connect_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(30),
        }
    }
}
impl Limits {
    pub fn validate(&self) -> Result<()> {
        if !(1024..=8 * 1024 * 1024).contains(&self.max_frame_bytes)
            || !(1..=256).contains(&self.max_events)
            || !(1..=256).contains(&self.max_pending_requests)
            || self.connect_timeout.is_zero()
            || self.connect_timeout > Duration::from_secs(60)
            || self.request_timeout.is_zero()
            || self.request_timeout > Duration::from_secs(600)
        {
            return Err(Error::Config("limits outside supported finite bounds"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct AttachOptions {
    pub hermes_root: PathBuf,
    pub python: PathBuf,
    pub home: Option<PathBuf>,
}
impl AttachOptions {
    pub fn new(
        root: impl Into<PathBuf>,
        python: impl Into<PathBuf>,
        home: Option<PathBuf>,
    ) -> Result<Self> {
        let result = Self {
            hermes_root: root.into(),
            python: python.into(),
            home,
        };
        if !result.hermes_root.is_absolute()
            || !result.python.is_absolute()
            || result.home.as_ref().is_some_and(|p| !p.is_absolute())
        {
            return Err(Error::Config("runtime paths must be absolute"));
        }
        Ok(result)
    }
}

#[derive(Debug, Clone)]
pub struct SessionOptions {
    pub cwd: PathBuf,
    pub resume: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
}
