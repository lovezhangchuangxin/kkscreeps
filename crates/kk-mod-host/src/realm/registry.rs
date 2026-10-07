//! 注册状态及声明首故障；调用方只能经阶段入口操作，不能直接改字段。
use super::*;

#[derive(Default)]
enum Phase {
    #[default]
    Idle,
    Package(String),
    Sealed,
}

#[derive(Default)]
pub(crate) struct Registry {
    phase: Phase,
    declarations: Vec<Declaration>,
    fault: Option<String>,
}

impl Registry {
    pub(super) fn is_sealed(&self) -> bool {
        matches!(self.phase, Phase::Sealed)
    }

    pub(crate) fn active_namespace(&self) -> Result<String, JsErrorBox> {
        if self.is_sealed() {
            return Err(JsErrorBox::generic("注册已密封"));
        }
        if let Some(fault) = &self.fault {
            return Err(JsErrorBox::generic(fault.clone()));
        }
        match &self.phase {
            Phase::Package(namespace) => Ok(namespace.clone()),
            _ => Err(JsErrorBox::generic("注册不在进行中（宿主未开始或已收尾）")),
        }
    }

    pub(super) fn begin_package(&mut self, namespace: &str) -> ModHostResult<()> {
        self.check_fault(namespace)?;
        if !matches!(self.phase, Phase::Idle) {
            return Err(ModHostError::new(
                namespace,
                "注册包阶段不可重入或在密封后重开",
            ));
        }
        self.phase = Phase::Package(namespace.to_string());
        Ok(())
    }

    pub(super) fn finish_package(&mut self) -> ModHostResult<Vec<Declaration>> {
        let namespace = self
            .active_namespace()
            .map_err(|error| ModHostError::new("register", error.to_string()))?;
        self.check_fault(&namespace)?;
        self.phase = Phase::Idle;
        Ok(std::mem::take(&mut self.declarations))
    }

    /// getter 转换结束后重新验证能力；被内层捕获的错误不能清除首故障。
    pub(super) fn record(
        &mut self,
        namespace: String,
        kind: String,
        value: Result<Value, String>,
    ) -> Result<(), JsErrorBox> {
        self.active_namespace()?;
        let value = value.map_err(|error| {
            self.fault.get_or_insert(error.clone());
            JsErrorBox::generic(error)
        })?;
        self.declarations.push(Declaration {
            namespace,
            kind,
            value,
        });
        Ok(())
    }

    pub(super) fn check_fault(&self, namespace: &str) -> ModHostResult<()> {
        match &self.fault {
            Some(fault) => Err(ModHostError::new(namespace, fault.clone())),
            None => Ok(()),
        }
    }

    pub(super) fn seal(&mut self) {
        self.phase = Phase::Sealed;
    }

    pub(super) fn poison(&mut self) {
        self.seal();
        self.declarations.clear();
    }
}
