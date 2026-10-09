//! 官方后端 `genInvaders` cron 的 raid 生成器：管理命令显式触发一次
//! 全房扫描，逐房过资格门后在合格出口格插入 Invader creep 并重掷阈值。
//! cron 墙钟调度（官方 5 分钟一次）不在本片——套件以
//! `runInvaderRaidSpawner({random})` 注入确定性随机序列驱动。
//! 来源：backend-npm/lib/cronjobs.js 的 `genInvaders`（245-441）。

use kk_core::constants as c;
use kk_schema::room::{BodyPart, CreepObj, RoomObject, Store};
use kk_world::World;
use kk_world::domain::invader_raid;
use kk_world::domain::npc::INVADER_OWNER;

/// 生成器随机源。`官方 Math.random` 在套件里被注入序列替换（vanilla
/// 适配器同形）；kk 未注入时退回世界种子派生流（同输入同输出，回放
/// 可复现——官方此处是真随机，登记为有意分歧）。
pub struct RaidRandom {
    inner: RaidRandomInner,
}

enum RaidRandomInner {
    /// 注入序列：消耗殆尽即报错（套件契约：序列不足须失败）。
    Sequence(Vec<f64>, usize),
    /// 种子流：FNV(seed#invader-raid) + SplitMix64 终结器，53 位 [0,1)。
    Seeded(u64),
}

impl RaidRandom {
    pub fn sequence(values: Vec<f64>) -> Self {
        Self {
            inner: RaidRandomInner::Sequence(values, 0),
        }
    }

    pub fn seeded(seed: u64, time: u64) -> Self {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for b in format!("invader-raid#{seed}#{time}").as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100_0000_01b3);
        }
        Self {
            inner: RaidRandomInner::Seeded(h),
        }
    }

    fn unit(&mut self) -> Result<f64, String> {
        match &mut self.inner {
            RaidRandomInner::Sequence(values, index) => {
                let v = values
                    .get(*index)
                    .copied()
                    .ok_or("随机序列耗尽：注入的 random 数组不足以覆盖本次生成器消耗")?;
                *index += 1;
                Ok(v)
            }
            RaidRandomInner::Seeded(state) => {
                *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
                let mut z = *state;
                z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
                z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
                z ^= z >> 31;
                Ok((z >> 11) as f64 / (1u64 << 53) as f64)
            }
        }
    }
}

/// 房间名快照 + 逐房（读检查 → 变更）执行官方扫描循环。
/// 遍历序是 kk 房名排序，官方为 db 自然序（插入序）；多候选房并存时
/// 随机流交织次序不同（登记分歧；单候选房等价）。
pub fn run_spawner(world: &mut World, random: &mut RaidRandom) -> Result<(), String> {
    let names: Vec<String> = world.rooms().keys().cloned().collect();
    for name in names {
        let Some(room) = world.room(&name) else {
            continue;
        };
        if room.raid.status != "normal" || room.raid.active {
            continue;
        }
        let has_invader = room
            .objects
            .values()
            .any(|o| matches!(o, RoomObject::Creep(cr) if cr.owner == INVADER_OWNER));
        if has_invader {
            continue;
        }
        // 官方 `room.invaderGoal || C.INVADERS_ENERGY_GOAL` 的 JS 真值回退：
        // 存储的 0（乘数半量分支的写入值）读回时回落默认阈值，不是免门槛。
        let goal = room
            .raid
            .invader_goal
            .filter(|goal| *goal != 0)
            .unwrap_or(c::INVADERS_ENERGY_GOAL);
        if goal != 1 && room.raid.harvested_energy < goal {
            continue;
        }
        let Some(sector) = invader_raid::sector_key(&name) else {
            continue;
        };
        let core_in_sector = world.rooms().iter().any(|(other, other_room)| {
            invader_raid::sector_key(other) == Some(sector)
                && other_room.objects.values().any(|o| match o {
                    RoomObject::Generic(g) => invader_core_level(g) > 0,
                    _ => false,
                })
        });
        if !core_in_sector {
            continue;
        }

        // 出口扫描逐字保留官方 0..49 循环上限：(49,49) 永不被扫到、
        // (49,0) 归 right 组、(0,49) 归 bottom 组、(0,0) 同时进 top 与
        // left 组——kk 地形归一恒置四角墙，角格实际不可成为出口。
        let terrain = room.terrain().clone();
        let mut groups: Vec<(&'static str, Vec<(u8, u8)>)> = vec![
            ("top", Vec::new()),
            ("bottom", Vec::new()),
            ("left", Vec::new()),
            ("right", Vec::new()),
        ];
        for i in 0..49usize {
            if !terrain.is_wall(i, 0) {
                groups[0].1.push((i as u8, 0));
            }
            if !terrain.is_wall(i, 49) {
                groups[1].1.push((i as u8, 49));
            }
            if !terrain.is_wall(0, i) {
                groups[2].1.push((0, i as u8));
            }
            if !terrain.is_wall(49, i) {
                groups[3].1.push((49, i as u8));
            }
        }
        // 方向选择数组序是官方 exits 对象的键插入序——各方向**首个开口
        // 格的发现先後**（i 升序，同 i 按官方代码检查序 top/bottom/
        // left/right），不是固定方位序（cronjobs.js:401-420、L420/L429）。
        groups.sort_by_key(|(dir, squares)| {
            // top/bottom 的开口沿 x 递增发现，left/right 沿 y；角格双归
            // 属取本组坐标即可（仅作次序比较）。
            let discovery = match *dir {
                "top" | "bottom" => squares.first().map(|s| s.0),
                _ => squares.first().map(|s| s.1),
            };
            let rank = match *dir {
                "top" => 0,
                "bottom" => 1,
                "left" => 2,
                _ => 3,
            };
            (discovery, rank)
        });
        // 邻房控制器有 owner 或 reservation 的方向 disqualified
        //（cronjobs.js:254-259；邻房不存在于世界即无控制器，放行）。
        groups.retain(|(dir, squares)| !squares.is_empty() && !exit_blocked(world, &name, dir));
        if groups.is_empty() {
            continue;
        }

        let exit_index = (random.unit()? * groups.len() as f64).floor() as usize;
        let (_, squares) = groups.swap_remove(exit_index.min(groups.len() - 1));
        // 官方 controllerLevel = controller && controller.user && controller.level：
        // JS 真值链使 RCL0 有主房等同无主（level 0 falsy）。
        let controller_level = room
            .controller()
            .filter(|ctl| ctl.owner.is_some())
            .map(|ctl| i64::from(ctl.level));
        create_raid(world, random, &name, controller_level, squares)?;

        // 成功后重掷阈值、清预算、置活跃（官方 433-439）。
        // 乘数 Math.floor(rand>0.5?2:0.5)：半量分支 floor(0.5)=0 把阈值
        // 写成 0——读回侧经上方真值回退回落默认阈值（官方固有怪癖：
        // 本意减半实际等效重置为默认）。
        let mut new_goal =
            (c::INVADERS_ENERGY_GOAL as f64 * (random.unit()? * 0.6 + 0.7)).floor() as i64;
        if random.unit()? < 0.1 {
            let multiplier = if random.unit()? > 0.5 { 2 } else { 0 };
            new_goal *= multiplier;
        }
        world.set_raid_post_spawn(&name, new_goal);
    }
    Ok(())
}

/// Generic 对象是 invaderCore 时读其 level，其余返回 0。
fn invader_core_level(g: &kk_schema::room::GenericObj) -> i64 {
    serde_json::from_str::<serde_json::Value>(&g.data)
        .ok()
        .and_then(|v| {
            (v.get("structureType").and_then(serde_json::Value::as_str) == Some("invaderCore"))
                .then(|| {
                    v.get("level")
                        .and_then(serde_json::Value::as_i64)
                        .unwrap_or(0)
                })
        })
        .unwrap_or(0)
}

/// 出口方向的邻房控制器是否被占用（有 owner 或 reservation）。
/// 房名解析失败按阻塞处理——官方 roomNameToXY 对怪名抛错中断整轮，
/// kk 保守单房跳过。
fn exit_blocked(world: &World, name: &str, dir: &str) -> bool {
    let Ok(parsed) = kk_core::RoomName::parse(name) else {
        return true;
    };
    let (mut xx, mut yy) = (parsed.xx, parsed.yy);
    match dir {
        "top" => yy -= 1,
        "right" => xx += 1,
        "bottom" => yy += 1,
        _ => xx -= 1,
    }
    let adjacent = kk_core::RoomName::from_xy(xx, yy).as_str();
    world
        .room(&adjacent)
        .and_then(|r| r.controller())
        .is_some_and(|ctl| ctl.owner.is_some() || ctl.reservation.is_some())
}

/// 官方 createRaid（cronjobs.js:320-375）：升级掷骰决定规模，逐 creep
/// 按下标规则定子型、按切比雪夫最近出口落格，全部从注入随机流取值。
fn create_raid(
    world: &mut World,
    random: &mut RaidRandom,
    room: &str,
    controller_level: Option<i64>,
    mut squares: Vec<(u8, u8)>,
) -> Result<(), String> {
    let center = invader_raid::is_center_room(room);
    let is_big = controller_level.is_some_and(|level| level >= 4);
    let mut count = 1usize;
    let mut boost_chance = 0.5f64;

    if random.unit()? > 0.9 || center {
        // 官方先给 max=2，二次掷骰或中心房再给 max=5；big 身体按 RCL
        // 收敛上限（<5/<6 收 2、<7 收 3、<8 与 8 为 3/5 且 boost 0.4）。
        let mut max = 2usize;
        boost_chance = if is_big { 0.0 } else { 0.5 };
        if random.unit()? > 0.8 || center {
            max = 5;
            if is_big && let Some(level) = controller_level {
                // 官方 <5 与 <6 是两个同体分支，语义等价于合并的 <6。
                if level < 6 {
                    max = 2;
                } else if level < 7 {
                    max = 3;
                } else if level < 8 {
                    boost_chance = 0.4;
                    max = 3;
                } else {
                    boost_chance = 0.4;
                    max = 5;
                }
            }
        }
        // count 掷骰在外层升级分支内、内层 if 之后：外层升级但内层未
        // 升级时 max=2、count 恒 2（官方 352-355 的缩进层级）。
        count = (random.unit()? * (max - 1) as f64).floor() as usize + 2;
        count = count.min(squares.len());
    }

    let mut last_square: Option<(u8, u8)> = None;
    for i in 0..count {
        // 官方三目优先级：i==0 非中心恒 Melee；中心 i==0 恒 Ranged；
        // 其余仅 i==1 或（i==2 且 count==5）消耗一次随机定 Ranged/Healer。
        let subtype = if i == 0 && !center {
            "Melee"
        } else {
            let roll_needed = i == 1 || (i == 2 && count == 5);
            if i == 0 || (roll_needed && random.unit()? > 0.5) {
                "Ranged"
            } else {
                "Healer"
            }
        };
        let square = match last_square {
            Some(last) => {
                // 稳定最近：切比雪夫距离最小者，平局取扫描序靠前（同
                // lodash sortBy 稳定排序）。
                squares
                    .iter()
                    .enumerate()
                    .min_by_key(|(idx, s)| {
                        (
                            (s.0 as i32 - last.0 as i32)
                                .abs()
                                .max((s.1 as i32 - last.1 as i32).abs()),
                            *idx,
                        )
                    })
                    .map(|(_, s)| *s)
            }
            None => {
                let index = (random.unit()? * squares.len() as f64).floor() as usize;
                squares.get(index.min(squares.len() - 1)).copied()
            }
        };
        let Some(square) = square else {
            break;
        };
        let boosted = random.unit()? < boost_chance;
        let name_roll = (random.unit()? * 1000.0).floor() as i64;
        insert_raider(
            world,
            &Raider {
                room,
                is_big,
                subtype,
                square,
                boosted,
                center,
                name_roll,
            },
        )?;
        squares.retain(|s| *s != square);
        last_square = Some(square);
    }
    Ok(())
}

/// 单只 raid creep 的落格参数（打包以满足参数上限）。
struct Raider<'a> {
    room: &'a str,
    is_big: bool,
    subtype: &'a str,
    square: (u8, u8),
    boosted: bool,
    center: bool,
    name_roll: i64,
}

/// 官方 createCreep（cronjobs.js:262-312）：body 按官方表逐件 hits:100，
/// 归属 Invader，寿命由 creep tick 首拍初始化（官方 cron 不写 ageTime，
/// 引擎 creeps/tick.js:88 补设——kk 同构，age_time=0 表未初始化）。
/// boost 按部件类型映射，中心房用 X 级。
fn insert_raider(world: &mut World, raider: &Raider<'_>) -> Result<(), String> {
    let Raider {
        room,
        is_big,
        subtype,
        square,
        boosted,
        center,
        name_roll,
    } = *raider;
    let key = format!("{}{}", if is_big { "big" } else { "small" }, subtype);
    let Some(parts) = invader_raid::raid_body(&key) else {
        return Err(format!("官方 raid 身体表缺键: {key}"));
    };
    let mut body = Vec::with_capacity(parts.len());
    for part in parts {
        let kind = kk_world::types::BodyKind::parse(part)
            .ok_or_else(|| format!("官方 raid 身体表含未知部件: {part}"))?
            .code();
        let boost = if boosted {
            invader_raid::raid_boost(part, center).map(str::to_string)
        } else {
            None
        };
        body.push(BodyPart {
            kind,
            hits: 100,
            boost,
        });
    }
    let hits = (body.len() as u32).saturating_mul(100);
    let creep = CreepObj {
        id: 0,
        x: square.0,
        y: square.1,
        fatigue: 0,
        age_time: 0,
        hits,
        spawning: false,
        action_log: 0,
        name: format!("invader_{room}_{name_roll}"),
        owner: INVADER_OWNER.to_string(),
        body,
        store: Store::default(),
        store_extra: Vec::new(),
        say_message: None,
        say_public: None,
        notify_when_attacked: None,
    };
    world.insert(room, RoomObject::Creep(creep))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequence_random_reports_exhaustion_and_seeded_is_reproducible() {
        let mut a = RaidRandom::sequence(vec![0.25, 0.5]);
        assert_eq!(a.unit().unwrap(), 0.25);
        assert_eq!(a.unit().unwrap(), 0.5);
        assert!(a.unit().is_err());
        let mut s1 = RaidRandom::seeded(7, 3);
        let mut s2 = RaidRandom::seeded(7, 3);
        for _ in 0..16 {
            let (x, y) = (s1.unit().unwrap(), s2.unit().unwrap());
            assert_eq!(x, y);
            assert!((0.0..1.0).contains(&x));
        }
        // 不同 tick 派生不同流，避免同拍重复扫描同值。
        let mut s3 = RaidRandom::seeded(7, 4);
        assert_ne!(s1.unit().unwrap(), s3.unit().unwrap());
    }
}
