//! VM 串行场景的定向执行与逐场景诊断，仅由开发依赖启用。
use anyhow::{Context, Result, ensure};
use std::time::Instant;

pub struct ScenarioSuite {
    filter: Option<String>,
    passed: usize,
}

impl ScenarioSuite {
    pub fn from_env() -> Result<Self> {
        let filter = match std::env::var("KKSCREEPS_SCENARIO") {
            Ok(filter) => {
                ensure!(!filter.is_empty(), "场景过滤条件不能为空");
                Some(filter)
            }
            Err(std::env::VarError::NotPresent) => None,
            Err(error) => return Err(error.into()),
        };
        Ok(Self { filter, passed: 0 })
    }

    /// 默认保持声明顺序；定向执行只匹配名称子串，不运行未选场景。
    pub fn run(&mut self, name: &str, scenario: impl FnOnce() -> Result<()>) -> Result<()> {
        if self
            .filter
            .as_ref()
            .is_some_and(|filter| !name.contains(filter))
        {
            return Ok(());
        }
        eprintln!("scenario START {name}");
        let start = Instant::now();
        scenario().with_context(|| format!("串行场景失败：{name}"))?;
        self.passed += 1;
        eprintln!("scenario PASS {name} ({:?})", start.elapsed());
        Ok(())
    }

    /// 空匹配必须失败；选择场景不能被当成完整套件验收。
    pub fn finish(&self, total: usize) -> Result<()> {
        ensure!(self.passed > 0, "场景过滤未匹配任何场景：{:?}", self.filter);
        if self.filter.is_some() {
            eprintln!("场景子集通过：{}/{total}；不是完整套件验收", self.passed);
        } else {
            ensure!(
                self.passed == total,
                "场景注册数不一致：{}/{total}",
                self.passed
            );
            eprintln!("完整串行场景通过：{total}/{total}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_skips_unselected_and_rejects_empty_match() {
        let mut suite = ScenarioSuite {
            filter: Some("selected".into()),
            passed: 0,
        };
        suite.run("other", || panic!("未选场景不应执行")).unwrap();
        assert!(suite.finish(2).is_err());
        suite.run("selected", || Ok(())).unwrap();
        suite.finish(2).unwrap();
    }

    #[test]
    fn failure_identifies_scenario_and_does_not_count_as_passed() {
        let mut suite = ScenarioSuite {
            filter: None,
            passed: 0,
        };
        let error = suite
            .run("restore", || anyhow::bail!("broken"))
            .unwrap_err();
        assert!(format!("{error:#}").contains("restore: broken"));
        assert!(suite.finish(1).is_err());
    }
}
