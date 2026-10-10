//! 官方后端 `genPowerBanks` cron 的 powerBank 生成器：管理命令显式触发
//! 一次全房扫描，合格高速路房先写入到期 tick、到期后在墙体目标格生成
//! powerBank 对象。cron 墙钟调度（官方 5 分钟一次）不在本片——套件无
//! 生成器用例，kk 以 `runPowerBankSpawner({random})` 注入确定性随机
//! 序列供 Rust 测试驱动（随机源复用 raid 的注入/种子双形态）。
//! 来源：backend-npm/lib/cronjobs.js 的 `genPowerBanks`（194-243）。

use kk_core::constants as c;
use kk_schema::room::{GenericObj, RoomObject};
use kk_world::World;
use kk_world::domain::power_bank;
use kk_world::domain::room_status::{self, RoomStatusKind};

use super::invader_raid::RaidRandom;

/// 种子流域标签：与 raid 生成器区分，同 tick/seed 不产生相同序列。
pub fn seeded_random(seed: u64, time: u64) -> RaidRandom {
    RaidRandom::seeded_domain("power-bank", seed, time)
}

/// 官方 respawnTime 公式（cronjobs.js:199）。
fn respawn_ticks(unit: f64) -> u64 {
    (unit * c::POWER_BANK_RESPAWN_TIME as f64 / 2.0 + c::POWER_BANK_RESPAWN_TIME as f64 * 0.75)
        .round() as u64
}

/// 逐高速路房执行官方调度循环。遍历序是 kk 房名排序，官方为 db
/// 自然序（插入序）；多候选房并存时随机流交织次序不同（登记分歧；
/// 单候选房等价，与 raid 生成器同款口径）。
pub fn run_spawner(world: &mut World, random: &mut RaidRandom) -> Result<(), String> {
    let time = world.time();
    let now_ms = room_status::wall_now_ms();
    let names: Vec<String> = world.rooms().keys().cloned().collect();
    for name in names {
        // 官方查询 {bus: true, status: 'normal'}：bus 是 world-gen 写入的
        // 高速路标志，kk 无该标志域，按房名编号推导（编号任一轴 ≡0
        // (mod 10)）；novice/respawn 的 db status 仍是 normal，只有
        // 有效 closed 房被查询排除。到期 closed 恢复 normal。
        if !power_bank::is_highway_room(&name) {
            continue;
        }
        if world
            .room(&name)
            .and_then(|room| room.room_status.as_ref())
            .is_some_and(|status| status.active_kind(now_ms) == Some(RoomStatusKind::Closed))
        {
            continue;
        }
        // 官方 L199 对每个合格房都先掷 respawnTime（不进任何分支的
        // 房也消耗随机）：round(r*RESPAWN/2 + RESPAWN*0.75) ∈
        // [0.75, 1.25]×RESPAWN；到期生成时复用同一掷值（L210）。
        let respawn = respawn_ticks(random.unit()?);
        let (scheduled, terrain) = {
            let Some(room) = world.room(&name) else {
                continue;
            };
            (room.power_bank_time, room.terrain().clone())
        };
        match scheduled {
            // 首轮只写到期时间，不生成对象（官方 L200-204）。
            None => {
                world.set_power_bank_time(&name, Some(time.saturating_add(respawn)))?;
                continue;
            }
            Some(due) if time < due => continue,
            Some(_) => {}
        }

        // 位置 do/while（官方 L213-226）：x,y ∈ [5,44]（floor(r*40+5)），
        // 重掷直到中心格是墙且 3×3 邻域存在非墙格（官方 9 格循环含
        // 中心，中心恒墙不贡献，等价 8 邻域）。注入序列耗尽即报错保留
        // 原世界。官方依赖 world-gen 保证存在合格格而可安全无限重掷；
        // kk 地形是管理面任意输入（全平原/全墙高速路房都可能），重掷
        // 上限 10_000 次后报错——报错路径由候选整体回滚覆盖，真实地图
        // 行为不变（合格格密度下限远高于命中 10_000 次全miss 的概率）。
        let (x, y) = {
            let mut attempts = 0u32;
            loop {
                attempts += 1;
                if attempts > 10_000 {
                    return Err(format!(
                        "powerBank 生成器重掷耗尽：房 {name} 无「中心墙且邻域有出口」格"
                    ));
                }
                let x = (random.unit()? * 40.0 + 5.0).floor().min(44.0) as u8;
                let y = (random.unit()? * 40.0 + 5.0).floor().min(44.0) as u8;
                let center_wall = terrain.is_wall(x as usize, y as usize);
                let mut has_exit = false;
                for dy in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let nx = (x as i32 + dx) as usize;
                        let ny = (y as i32 + dy) as usize;
                        if !terrain.is_wall(nx, ny) {
                            has_exit = true;
                        }
                    }
                }
                if center_wall && has_exit {
                    break (x, y);
                }
            }
        };
        // 容量掷骰（官方 L227-230）：floor(r*(MAX-MIN)+MIN)，暴击掷
        // r<CRIT 再加一份 MAX。
        let mut power = (random.unit()?
            * (c::POWER_BANK_CAPACITY_MAX - c::POWER_BANK_CAPACITY_MIN) as f64
            + c::POWER_BANK_CAPACITY_MIN as f64)
            .floor() as u64;
        if random.unit()? < c::POWER_BANK_CAPACITY_CRIT {
            power += c::POWER_BANK_CAPACITY_MAX as u64;
        }
        let data = serde_json::json!({
            "type": "structure",
            "structureType": "powerBank",
            "store": { "power": power },
            "hits": c::POWER_BANK_HITS,
            "hitsMax": c::POWER_BANK_HITS,
            "decayTime": time.saturating_add(c::POWER_BANK_DECAY as u64),
        });
        world.insert(
            &name,
            RoomObject::Generic(GenericObj {
                id: 0,
                x,
                y,
                data: data.to_string().into(),
            }),
        )?;
        world.set_power_bank_time(&name, Some(time.saturating_add(respawn)))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respawn_roll_bounds_match_official_formula() {
        // round 边界：0 → 0.75×RESPAWN；0.999_96 → 62_499.0 → 62_499；
        // 0.999_999 → 62_499.975 → 62_500（round 半远离零，Math.round 同形）。
        assert_eq!(respawn_ticks(0.0), 37_500);
        assert_eq!(respawn_ticks(0.999_96), 62_499);
        assert_eq!(respawn_ticks(0.999_999), 62_500);
    }

    #[test]
    fn sequence_random_rejects_out_of_domain_values() {
        // embed 层已拒，此处是引擎级构造 RaidRandom 的纵深防御。
        for bad in [1.0, -0.5, f64::NAN, f64::INFINITY] {
            let mut r = RaidRandom::sequence(vec![bad]);
            assert!(r.unit().is_err(), "{bad} 须拒");
        }
    }

    #[test]
    fn seeded_domain_labels_diverge_from_raid_stream() {
        // 同 seed/time 下 power-bank 域标签与 raid 流不同源。
        let mut raid = RaidRandom::seeded(7, 3);
        let mut bank = seeded_random(7, 3);
        let r: Vec<f64> = (0..8).map(|_| raid.unit().unwrap()).collect();
        let b: Vec<f64> = (0..8).map(|_| bank.unit().unwrap()).collect();
        assert_ne!(r, b);
        // 同域标签重放确定。
        let mut again = seeded_random(7, 3);
        let b2: Vec<f64> = (0..8).map(|_| again.unit().unwrap()).collect();
        assert_eq!(b, b2);
    }
}
