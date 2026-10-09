//! 渲染器夹具导出（F2 前端批唯一的引擎侧触点，方案 docs/plans/f2-renderer.md §4）。
//!
//! 只读公开 API：`World` 构建（settle_tests Fixture 同款摆法——脚本对象摆放）
//! → `Engine` 空意图推进 N tick（零 V8：sandboxes 空 + 默认 IntentBatch，走
//! 与 embed/测试同一条 settle_room 结算链）→ 逐 tick 落 `world_json()`。
//! 输出 JSONL（`{"tick":n,"world":{…}}` 每行一条，含 tick 0 初始态），由
//! 前端 `packages/renderer/scripts/build-fixtures.mjs` 经 differ 转成
//! `{room, ticks:[snapshot, …deltas]}` 夹具入库。
//!
//! 确定性（方案 §9）：固定 seed + 固定 [`TickInput`] 墙钟注入（ADR-013 决策 5）
//! + 结算全程 BTreeMap 序 + 无时间戳——两次导出逐字节一致（本模块测试断言）。

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use kk_engine::intent::IntentBatch;
use kk_engine::settle::{Engine, TickInput};
use kk_world::ObjId;
use kk_world::World;

use motion_scenes::{action_tick_script, build_actions, build_creeps, creep_tick_script};
use static_scenes::{build_dense_edge, build_plains, build_standard, build_swamp_walls};

mod factories;
mod motion_scenes;
mod static_scenes;

/// 墙钟固定值（2023-11-14T22:13:20Z 量级整数）：ADR-013 决策 5 的显式注入面，
/// 夹具重放/回归的唯一时间真值——绝不在导出路径使用 `TickInput::now`。
const FIXED_NOW_MS: u64 = 1_700_000_000_000;

/// 世界构建器（settle_tests Fixture 同款摆法的脚本摆放函数）。
/// 返回需要意图脚本驱动的对象句柄（名 → id；静态夹具返回空表）。
type WorldBuilder = fn(&mut World) -> BTreeMap<String, ObjId>;

/// 逐 tick 意图脚本（运动/战斗夹具——R1 §12 决策①登记的本文件扩展点）。
type IntentScript = fn(tick: u32, handles: &BTreeMap<String, ObjId>) -> IntentBatch;

/// 夹具清单：名（=输出文件名/前端夹具键）、seed、世界构建器、意图脚本。
struct FixtureSpec {
    name: &'static str,
    seed: u64,
    build: WorldBuilder,
    script: Option<IntentScript>,
}

const fn fixture(name: &'static str, seed: u64, build: WorldBuilder) -> FixtureSpec {
    FixtureSpec {
        name,
        seed,
        build,
        script: None,
    }
}

const FIXTURES: &[FixtureSpec] = &[
    fixture("plains", 1, build_plains),
    fixture("standard", 2, build_standard),
    fixture("swamp-walls", 3, build_swamp_walls),
    fixture("dense-edge", 4, build_dense_edge),
    FixtureSpec {
        name: "creeps",
        seed: 5,
        build: build_creeps,
        script: Some(creep_tick_script),
    },
    FixtureSpec {
        name: "actions",
        seed: 6,
        build: build_actions,
        script: Some(action_tick_script),
    },
];

/// 单夹具导出为 JSONL 行（tick 0 初始态 + 每 tick 推进后各一条）。
/// IO 与渲染分离：确定性测试直接对比本函数两次输出。
fn render_fixture(
    world: World,
    handles: BTreeMap<String, ObjId>,
    script: Option<IntentScript>,
    ticks: u32,
) -> Vec<String> {
    let mut engine = Engine::new(world);
    let mut lines = Vec::with_capacity(ticks as usize + 1);
    let mut emit = |tick: u32, world: &World| {
        let mut line = String::new();
        // serde_json::to_string 写 Value（确定序），String::from_utf8 恒成功
        let json = world.world_json().to_string();
        let _ = write!(line, r#"{{"tick":{tick},"world":{json}}}"#);
        lines.push(line);
    };
    emit(0, engine.world());
    for t in 1..=ticks {
        let batch = script.map(|f| f(t, &handles)).unwrap_or_default();
        engine
            .tick_with_intents_with_input(
                batch,
                TickInput {
                    now_ms: FIXED_NOW_MS,
                    warmup: false,
                },
            )
            .expect("夹具世界结算不应失败（空意图或静态脚本）");
        emit(t, engine.world());
    }
    lines
}

/// 导出全部夹具到目录（每夹具一个 `<名>.jsonl`）。返回成功写出的文件数。
pub fn run(out_dir: &str, ticks: u32) -> i32 {
    std::fs::create_dir_all(out_dir).unwrap_or_else(|e| panic!("创建输出目录 {out_dir} 失败: {e}"));
    for spec in FIXTURES {
        let mut world = World::new(spec.seed);
        let handles = (spec.build)(&mut world);
        let lines = render_fixture(world, handles, spec.script, ticks);
        let path = Path::new(out_dir).join(format!("{}.jsonl", spec.name));
        std::fs::write(&path, lines.join("\n") + "\n")
            .unwrap_or_else(|e| panic!("写入 {} 失败: {e}", path.display()));
        println!("✓ {} ({} ticks)", path.display(), ticks);
    }
    println!("导出完成：{} 夹具 × {}+1 tick 快照", FIXTURES.len(), ticks);
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 确定性（方案 §9）：同 seed 同 TickInput 两次导出逐字节一致。
    #[test]
    fn 导出确定性_两次逐字节一致() {
        for spec in FIXTURES {
            let build_world = || {
                let mut w = World::new(spec.seed);
                let handles = (spec.build)(&mut w);
                (w, handles)
            };
            let (wa, ha) = build_world();
            let (wb, hb) = build_world();
            // 30 tick = build-fixtures.mjs 默认导出长度，确定性检查覆盖入库全长。
            let la = render_fixture(wa, ha, spec.script, 30);
            let lb = render_fixture(wb, hb, spec.script, 30);
            assert_eq!(la, lb, "夹具 {} 两次导出应逐字节一致", spec.name);
            assert_eq!(
                la.len(),
                31,
                "夹具 {} 应为 tick0+30 tick 共 31 条",
                spec.name
            );
        }
    }

    /// 时间序：world_json.time 随 tick 单调推进（回放链的时间轴依据）。
    #[test]
    fn 导出时间序单调推进() {
        let mut world = World::new(1);
        let handles = build_plains(&mut world);
        let lines = render_fixture(world, handles, None, 3);
        let times: Vec<u64> = lines
            .iter()
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).unwrap();
                v["world"]["time"].as_u64().unwrap()
            })
            .collect();
        assert_eq!(times, vec![0, 1, 2, 3], "time 应逐 tick +1");
    }

    #[test]
    fn actions_真实事件及完工目标坐标() {
        let mut world = World::new(6);
        let handles = build_actions(&mut world);
        let site = format!("kk{:016x}", handles["site"].0);
        let lines = render_fixture(world, handles, Some(action_tick_script), 1);
        let v: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        let events = v["world"]["rooms"]["W6N6"]["events"].as_array().unwrap();
        for code in [1, 4, 5, 6, 7, 9] {
            assert!(events.iter().any(|e| e["event"] == code), "缺少事件 {code}");
        }
        let build = events.iter().find(|e| e["event"] == 4).unwrap();
        assert_eq!(build["data"]["x"], 14);
        assert_eq!(build["data"]["y"], 17);
        assert_eq!(build["data"]["incomplete"], false);
        assert!(
            v["world"]["objects"]
                .as_array()
                .unwrap()
                .iter()
                .all(|o| o["id"] != site)
        );
        let heal = events.iter().find(|e| e["event"] == 6).unwrap();
        assert_eq!(heal["data"]["healType"], 2);
    }

    /// R4 运动夹具行为面：意图注入后走位者位移、victimA 死亡移除、
    /// 攻击/治疗事件入投影——渲染端运动/受击罩/死亡淡出的数据依据。
    #[test]
    fn creeps_夹具_位移_死亡_事件() {
        let mut world = World::new(5);
        let handles = build_creeps(&mut world);
        let lines = render_fixture(world, handles, Some(creep_tick_script), 10);
        let parse = |i: usize| -> serde_json::Value { serde_json::from_str(&lines[i]).unwrap() };
        let obj_at = |v: &serde_json::Value, name: &str| -> Option<serde_json::Value> {
            v["world"]["objects"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["name"] == name)
                .cloned()
        };
        // 走位者 tick1 各进一格（walker1 方向 5=下、walker2 方向 7=左——
        // phase=tick%4 的首步）
        let w0 = obj_at(&parse(0), "walker1").unwrap();
        let w1 = obj_at(&parse(1), "walker1").unwrap();
        assert_eq!((w0["x"].as_i64(), w0["y"].as_i64()), (Some(10), Some(10)));
        assert_eq!((w1["x"].as_i64(), w1["y"].as_i64()), (Some(10), Some(11)));
        let v2 = obj_at(&parse(1), "walker2").unwrap();
        assert_eq!((v2["x"].as_i64(), v2["y"].as_i64()), (Some(13), Some(10)));
        // victimA tick0 在场、前 5 tick 内死亡移除（30×4≥100 hits）
        assert!(obj_at(&parse(0), "victimA").is_some());
        let died = (1..=10)
            .find(|t| obj_at(&parse(*t), "victimA").is_none())
            .unwrap_or_else(|| panic!("victimA 应在夹具期间死亡移除"));
        assert!(died <= 5, "victimA 应在前 5 tick 内死亡，实际 tick {died}");
        // 攻击/治疗事件进投影（EVENT_ATTACK=1 / EVENT_HEAL=6）
        let has_event = |v: &serde_json::Value, code: i64| {
            v["world"]["rooms"]["W5N5"]["events"]
                .as_array()
                .map(|evs| evs.iter().any(|e| e["event"].as_i64() == Some(code)))
                .unwrap_or(false)
        };
        assert!(has_event(&parse(1), 1), "tick1 应有 EVENT_ATTACK");
        assert!(has_event(&parse(1), 6), "tick1 应有 EVENT_HEAL");
    }
}
