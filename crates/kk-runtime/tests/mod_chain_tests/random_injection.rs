//! tick({random}) 注入域集成（套件 00-adapter-contract execution 与
//! 17.3 MINERAL-REGEN-009 同形）：序列整单校验（越界拒收不推进时间）、
//! 处理器随机调用逐值消费（矿物再密化门 + 密度选择）、耗尽报错回滚该拍
//! 候选（此前已成功拍保留）、多余注入值无害、密度门矩阵（注入 gate <
//! MINERAL_DENSITY_CHANGE 触发重掷、>= 保持原密度；选择值 0.05→LOW——
//! 官方 MINERAL_DENSITY_PROBABILITY[1]=0.1 同分支）。
use super::*;

const ROOM: &str = "W1N1";

fn engine_on(binary: bool) -> EmbedEngine {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e
}

fn owned_world() -> Value {
    json!({ "players": ["p1"], "rooms": [{ "name": ROOM, "rcl": 1, "owner": "p1" }] })
}

/// 放置耗尽矿物（套件 placeMineral 同形：density 可调、量 0、3 拍后再生）。
fn place_mineral(e: &mut EmbedEngine, density: i64) -> anyhow::Result<String> {
    e.place_object(
        ROOM,
        "structure",
        &json!({
            "structureType": "mineral", "pos": [25, 25],
            "mineralType": "H", "mineralAmount": 0,
            "density": density, "ticksToRegeneration": 3,
        }),
    )
}

fn mineral_density(e: &EmbedEngine, id: &str) -> i64 {
    e.get_object(id).unwrap()["density"].as_i64().unwrap()
}

/// 套件 adapter-contract 同形：越界值（1.0）点名拒收，时间不推进。
#[test]
fn out_of_range_rejects_without_advancing_time() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&owned_world())?;
        let before = e.get_game_time();
        let err = e
            .tick_with_random(1, Some(vec![1.0]))
            .expect_err("1.0 越界须拒收");
        assert!(
            err.to_string().contains("random[0]"),
            "报错点名 random[0]：{err}"
        );
        assert_eq!(e.get_game_time(), before, "校验失败不推进时间");
        // 负值同样越界。
        let err = e
            .tick_with_random(1, Some(vec![0.5, -0.1]))
            .expect_err("-0.1 越界须拒收");
        assert!(
            err.to_string().contains("random[1]"),
            "报错点名 random[1]：{err}"
        );
        assert_eq!(e.get_game_time(), before);
    }
    Ok(())
}

/// 套件 adapter-contract 同形：MODERATE 矿再生拍消耗两个随机值（门 +
/// 密度选择），只注入一个门通过值即中途回滚报错。
#[test]
fn exhaustion_mid_tick_rejects() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&owned_world())?;
        place_mineral(&mut e, 2)?;
        let err = e
            .tick_with_random(5, Some(vec![0.04]))
            .expect_err("选择值缺失须报耗尽");
        assert!(
            err.to_string().contains("exhausted"),
            "报错含 exhausted：{err}"
        );
    }
    Ok(())
}

/// 官方 minerals/tick.js:25-26 的 || 短路：LOW/ULTRA 强制轮换**不消耗
/// 门值**（ kk Rust || 同构）；do-while 排除旧密度的继续分支（:33）注入
/// 后首次可确定性触发。
#[test]
fn forced_rotation_skips_gate_and_exclusion_rerolls() -> anyhow::Result<()> {
    for binary in [false, true] {
        // LOW：短路不吃门值，单值 0.5 直达选择（0.5→density 2）。若错误
        // 先吃门值，0.5>=0.05 门不过、密度保持 1——可区分。
        let mut e = engine_on(binary);
        e.create_shard(&owned_world())?;
        let id = place_mineral(&mut e, 1)?;
        e.tick_with_random(5, Some(vec![0.5]))?;
        assert_eq!(mineral_density(&e, &id), 2, "LOW 短路不消耗门值");

        // ULTRA：同样短路，单值 0.05 选 1。
        let mut e = engine_on(binary);
        e.create_shard(&owned_world())?;
        let id = place_mineral(&mut e, 4)?;
        e.tick_with_random(5, Some(vec![0.05]))?;
        assert_eq!(mineral_density(&e, &id), 1, "ULTRA 短路不消耗门值");

        // do-while 排除旧密度：0.7 选 3 与旧密度同被排除、重掷 0.95 选 4。
        let mut e = engine_on(binary);
        e.create_shard(&owned_world())?;
        let id = place_mineral(&mut e, 3)?;
        e.tick_with_random(5, Some(vec![0.04, 0.7, 0.95]))?;
        assert_eq!(mineral_density(&e, &id), 4, "排除旧密度后重掷 0.95 选 4");
    }
    Ok(())
}

/// 套件 adapter-contract 同形：空房一拍不消耗随机值，多余注入值无害。
#[test]
fn extra_values_are_harmless() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(binary);
        e.create_shard(&owned_world())?;
        e.tick_with_random(1, Some(vec![0.5, 0.5, 0.5]))?;
        assert_eq!(e.get_game_time(), 2);
    }
    Ok(())
}

/// 套件 MINERAL-REGEN-009 同形矩阵：注入 gate 0.04（< 0.05）触发重掷、
/// 选择值 0.05 落 LOW（≤ 概率表 [1]=0.1）；gate 0.06（>= 0.05）保持
/// 原 density（MODERATE/HIGH 均不强制轮换）。
#[test]
fn redensify_gate_matrix() -> anyhow::Result<()> {
    for binary in [false, true] {
        for density in [2i64, 3] {
            // 门通过 → 重掷 → LOW。
            let mut e = engine_on(binary);
            e.create_shard(&owned_world())?;
            let id = place_mineral(&mut e, density)?;
            e.tick_with_random(5, Some(vec![0.04, 0.05]))?;
            assert_eq!(mineral_density(&e, &id), 1, "density={density} 重掷为 LOW");
            // 官方 minerals/tick.js:17：补量按旧 density 容量（先补量后轮换）。
            let old_capacity = match density {
                2 => 35000,
                _ => 70000,
            };
            assert_eq!(
                e.get_object(&id).unwrap()["mineralAmount"].as_i64(),
                Some(old_capacity),
                "补量按旧 density={density} 容量"
            );

            // 门未过 → 保持原 density（不消耗选择值）。
            let mut e = engine_on(binary);
            e.create_shard(&owned_world())?;
            let id = place_mineral(&mut e, density)?;
            e.tick_with_random(5, Some(vec![0.06]))?;
            assert_eq!(
                mineral_density(&e, &id),
                density,
                "gate >= 0.05 保持原 density"
            );
        }
    }
    Ok(())
}
