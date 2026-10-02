use std::fmt;

use kk_mod_api::FreezeError;

/// 装载与注册期错误；`context` 定位出错的包、键或阶段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModHostError {
    pub context: String,
    pub message: String,
}

impl ModHostError {
    pub fn new(context: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            context: context.into(),
            message: message.into(),
        }
    }
}

impl fmt::Display for ModHostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.context, self.message)
    }
}

impl std::error::Error for ModHostError {}

impl From<FreezeError> for ModHostError {
    fn from(error: FreezeError) -> Self {
        Self {
            context: error.context,
            message: error.message,
        }
    }
}

pub type ModHostResult<T> = Result<T, ModHostError>;
