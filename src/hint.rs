use std::borrow::Cow;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct ArcError(Arc<anyhow::Error>);

#[derive(Debug)]
pub struct HintedError {
    pub error: anyhow::Error,
    pub hint: Cow<'static, str>,
}

pub trait HintExt {
    type Result: Sized;
    fn hint(self, text: &'static str) -> Self::Result;
    fn with_hint<F>(self, f: F) -> Self::Result
        where F: FnOnce() -> String;
}

impl<T> HintExt for Result<T, anyhow::Error> {
    type Result = Result<T, HintedError>;
    fn hint(self, text: &'static str) -> Self::Result
    {
        self.map_err(|error| HintedError {
            error,
            hint: text.into(),
        })
    }
    fn with_hint<F>(self, f: F) -> Self::Result
        where F: FnOnce() -> String
    {
        self.map_err(|error| HintedError {
            hint: f().into(),
            error,
        })
    }
}

impl HintExt for anyhow::Error {
    type Result = HintedError;
    fn hint(self, text: &'static str) -> Self::Result
    {
        HintedError {
            error: self,
            hint: text.into(),
        }
    }
    fn with_hint<F>(self, f: F) -> Self::Result
        where F: FnOnce() -> String
    {
        HintedError {
            hint: f().into(),
            error: self,
        }
    }
}

impl Error for HintedError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.error.source()
    }
}

impl fmt::Display for HintedError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl ArcError {
    pub fn inner(&self) -> &anyhow::Error {
        &*self.0
    }
}

impl Error for ArcError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&**self.0)
    }
}

impl fmt::Display for ArcError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<anyhow::Error> for ArcError {
    fn from(err: anyhow::Error) -> ArcError {
        ArcError(Arc::new(err))
    }
}
