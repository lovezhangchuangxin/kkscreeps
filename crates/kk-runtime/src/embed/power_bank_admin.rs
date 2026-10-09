//! powerBank 管理命令：显式生成器扫描与调度时间种子。
//! 套件无生成器 fixture（消费者用 placeObject 直放），本命令面服务
//! Rust 测试与运维：`runPowerBankSpawner({random})` 注入确定性序列，
//! `setPowerBankState({room, powerBankTime})` 直接调度。
use super::*;

impl EmbedEngine {
    /// 显式执行一次官方 genPowerBanks 扫描。`random` 为注入序列（每值
    /// 须落在 [0,1)，耗尽报错）；缺省用世界种子派生流（确定性回放，
    /// 官方为真随机，登记分歧）。随机源复用 raid 的注入/种子双形态。
    pub fn run_power_bank_spawner(&mut self, random: Option<Vec<f64>>) -> anyhow::Result<()> {
        self.no_open_round("runPowerBankSpawner")?;
        let random = match random {
            Some(values) => {
                for (index, value) in values.iter().enumerate() {
                    if !value.is_finite() || *value < 0.0 || *value >= 1.0 {
                        anyhow::bail!("random[{index}] 必须落在 [0,1): {value}");
                    }
                }
                kk_engine::settle::RaidRandom::sequence(values)
            }
            None => kk_engine::settle::power_bank_seeded_random(
                self.engine.world().seed(),
                self.engine.world().time(),
            ),
        };
        self.engine
            .run_power_bank_spawner(random)
            .map_err(anyhow::Error::msg)?;
        Ok(())
    }

    /// 种子 powerBank 生成调度：powerBankTime 为非负整数 tick 或 null
    ///（清除调度，回到"首轮 cron 只写时间"形态）。未知键整单拒绝，
    /// 与 raid 状态种子同一严格度。0 与 null 同义——官方
    /// `!room.powerBankTime` 的 JS falsy 把 0 视为未调度（自然路径写入
    /// 值 ≥37500 使 0 不可达，管理面显式归一保持两路同义）。
    pub fn set_power_bank_state(&mut self, room: &str, spec: &Value) -> anyhow::Result<()> {
        self.no_open_round("setPowerBankState")?;
        if self.engine.world().room(room).is_none() {
            anyhow::bail!("房间 {room} 不存在");
        }
        let Some(fields) = spec.as_object() else {
            anyhow::bail!("powerBank 状态种子必须是对象");
        };
        for key in fields.keys() {
            if key != "powerBankTime" {
                anyhow::bail!("powerBank 状态种子未知键: {key}");
            }
        }
        let value = match fields.get("powerBankTime") {
            None | Some(Value::Null) => None,
            // 整数经 as_f64 读取再取整（JSON 100.0 与 100 等价）。
            Some(v) => {
                let n = v
                    .as_f64()
                    .filter(|n| n.fract() == 0.0 && *n >= 0.0 && *n <= 9.0e15)
                    .ok_or_else(|| anyhow::anyhow!("powerBankTime 必须是非负整数或 null: {v}"))?
                    as u64;
                // 0 ≡ null（官方 falsy 归一）。
                (n > 0).then_some(n)
            }
        };
        self.engine
            .set_power_bank_time(room, value)
            .map_err(anyhow::Error::msg)?;
        Ok(())
    }
}
