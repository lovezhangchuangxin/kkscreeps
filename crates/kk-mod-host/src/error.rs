use std::fmt;

use kk_mod_api::{FreezeError, Key};
use serde::Serialize;

/// 错误发生的边界；共享 VM 的资源故障不能据此归责到某个 Mod。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModFaultKind {
    Handler,
    Control,
    Context,
    Read,
    WriteConversion,
    Write,
    Native,
    Capability,
    Resource,
    Chain,
}

/// 可信执行路径留存的来源，不从异常文本解析绑定或命名空间。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModFault {
    pub slot: Option<String>,
    pub binding: Option<Key>,
    pub kind: ModFaultKind,
}

/// 装载、注册与执行错误；执行边界另保留结构化来源。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModHostError {
    pub context: String,
    pub message: String,
    pub fault: Option<ModFault>,
}

impl ModHostError {
    pub fn new(context: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            context: context.into(),
            message: message.into(),
            fault: None,
        }
    }

    pub(crate) fn execution(
        context: impl Into<String>,
        message: impl Into<String>,
        fault: ModFault,
    ) -> Self {
        Self {
            context: context.into(),
            message: message.into(),
            fault: Some(fault),
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
            fault: None,
        }
    }
}

pub type ModHostResult<T> = Result<T, ModHostError>;
