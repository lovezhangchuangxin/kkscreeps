//! FORTIFY 墙体免伤与 SHIELD 临时 rampart 的真实玩家链。
use super::*;
use kk_core::constants as c;

/// POWER_INFO[17]：duration [1..5] / cooldown 5 / range 3 / ops 5；
/// [12]：effect [5000..25000] / duration 50 / cooldown 20，无 ops（energy
/// 字段官方无消费者）。免伤门见 damage/combat：_damage 严格 >、dismantle
/// 与 rangedMassAttack >=、预检 ticksRemaining > 0。
fn terrain_with(walls: &[(usize, usize)]) -> Vec<u8> {
    let mut t = vec![0u8; 2500];
    for &(x, y) in walls {
        t[y * 50 + x] = 1;
    }
    t
}

fn fixture_with_terrain(binary: bool, terrain: Vec<u8>) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(
        &json!({"players":[{"name":"p1","power":100000},{"name":"p2"}],
        "rooms":[{"name":"W1N1","owner":"p1","rcl":8,"powerEnabled":true,
                  "terrain":terrain}]}),
    )?;
    Ok(e)
}

fn fixture(binary: bool) -> anyhow::Result<EmbedEngine> {
    fixture_with_terrain(binary, vec![0u8; 2500])
}

fn actor(e: &mut EmbedEngine, name: &str, powers: Value) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "powerCreep",
        &json!({"name":name,"owner":"p1","pos":[24,25],
        "powers":powers,"store":{"ops":50}}),
    )
}

fn attacker(e: &mut EmbedEngine, name: &str, body: Vec<&str>) -> anyhow::Result<String> {
    e.place_object(
        "W1N1",
        "creep",
        &json!({"name":name,"owner":"p2","pos":[22,23],
            "body":body,"ticksToLive":1500}),
    )
}

fn cast(name: &str, power: i64, target: &str) -> String {
    format!("Game.powerCreeps.{name}.usePower({power},{target})")
}

/// FORTIFY：施放期内（t1..t4）attack/rangedAttack/dismantle 预检
/// ERR_INVALID_TARGET（wall 与 rampart 两腿）、massAttack 跳过、塔攻击
/// （无预检门的唯一免伤正路径）被拦；到期拍 t5（end_time == time）：
/// _damage 严格 > 恢复伤害、dismantle >= 回收仍 0、massAttack >= 仍跳过；
/// 过期拍 t6 起回收恢复（>= 与 < 的分叉）。
#[test]
fn fortify_blocks_structured_damage_until_strict_expiry() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let wall = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[23,23],"structureType":"constructedWall","hits":10000}),
        )?;
        let rampart = e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[23,24],"structureType":"rampart","owner":"p1","hits":10000}),
        )?;
        e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[23,22],"structureType":"tower","owner":"p1",
                "store":{"energy":1000}}),
        )?;
        let pc = actor(&mut e, "op", json!({"17":5}))?;
        actor(&mut e, "op2", json!({"17":5}))?;
        attacker(&mut e, "atk", vec!["attack"; 5])?; // [22,23]
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"wrk","owner":"p2","pos":[24,23],
                "body":["work","work","work","work","work",
                        "carry","carry","carry","carry","carry"],
                "ticksToLive":1500}),
        )?;
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"rng","owner":"p2","pos":[24,22],
                "body":["ranged_attack","ranged_attack","ranged_attack",
                        "ranged_attack","ranged_attack"],"ticksToLive":1500}),
        )?;
        let tw = format!("Game.getObjectById('{wall}')");
        let rp = format!("Game.getObjectById('{rampart}')");
        let tower =
            "Game.rooms.W1N1.find(FIND_MY_STRUCTURES).find(s=>s.structureType===STRUCTURE_TOWER)";

        // t0：双施放同拍结算（均 end 5）。
        e.run_player(
            "p1",
            &format!(
                "[{},{}]",
                cast("op", c::PWR_FORTIFY, &tw),
                cast("op2", c::PWR_FORTIFY, &rp)
            ),
        )?;
        assert_eq!(
            e.get_object(&wall).unwrap()["effects"][0]["power"],
            c::PWR_FORTIFY
        );
        assert_eq!(e.get_object(&pc).unwrap()["store"]["ops"], 45, "ops 5");

        // t1：预检三拒（wall 两动作 + rampart 一动作）+ massAttack 跳过。
        let rc = e.run_player(
            "p2",
            &format!(
                "[Game.creeps.atk.attack({tw}),Game.creeps.wrk.dismantle({tw}),\
                 Game.creeps.rng.rangedAttack({rp}),Game.creeps.rng.rangedMassAttack()]"
            ),
        )?;
        assert_eq!(rc.expr["value"][0], c::ERR_INVALID_TARGET, "attack 预检拒");
        assert_eq!(
            rc.expr["value"][1],
            c::ERR_INVALID_TARGET,
            "dismantle 预检拒"
        );
        assert_eq!(
            rc.expr["value"][2],
            c::ERR_INVALID_TARGET,
            "rampart 腿预检拒"
        );
        assert_eq!(
            e.get_object(&wall).unwrap()["hits"],
            10000,
            "期内免伤（预检拒绝，无伤害意图）"
        );
        assert_eq!(
            e.get_object(&rampart).unwrap()["hits"],
            10000,
            "期内 massAttack 跳过"
        );

        // t2：塔攻击自有 FORTIFY 墙——无预检门，唯一免伤正路径。
        let rc = e.run_player("p1", &format!("{tower}.attack({tw})"))?;
        assert_eq!(rc.expr["value"], 0);
        assert_eq!(
            e.get_object(&wall).unwrap()["hits"],
            10000,
            "塔伤害被免伤门拦截"
        );

        // t5（end_time == time）：attack 伤害恢复（> 为假）、dismantle 伤害
        // 照常回收仍 0（>= 为真）、massAttack 仍跳过（>= 为真）。
        e.tick(2)?; // time 3 → 5（结算 t3、t4）
        let rc = e.run_player(
            "p2",
            &format!(
                "[Game.creeps.atk.attack({tw}),Game.creeps.wrk.dismantle({tw}),\
                 Game.creeps.rng.rangedMassAttack()]"
            ),
        )?;
        assert_eq!(rc.expr["value"][0], 0, "到期拍预检放行");
        assert_eq!(rc.expr["value"][1], 0);
        assert_eq!(e.get_object(&wall).unwrap()["hits"], 9600, "150+250");
        assert_eq!(
            e.get_object(&rampart).unwrap()["hits"],
            10000,
            "到期拍 massAttack 仍跳过（>=）"
        );
        let wrk = e.run_player("p2", "(Game.creeps.wrk.store||{})[RESOURCE_ENERGY]||0")?;
        assert_eq!(wrk.expr["value"], 0, "到期拍回收仍 0（>=）");

        // t6（过期）：回收恢复 1（>= 与 < 分叉），massAttack 落伤 60。
        let rc = e.run_player(
            "p2",
            &format!(
                "[Game.creeps.atk.attack({tw}),Game.creeps.wrk.dismantle({tw}),\
                 Game.creeps.rng.rangedMassAttack()]"
            ),
        )?;
        assert_eq!(rc.expr["value"][0], 0);
        assert_eq!(e.get_object(&wall).unwrap()["hits"], 9200, "再 150+250");
        assert_eq!(
            e.get_object(&rampart).unwrap()["hits"],
            9980,
            "massAttack 落伤 round(5×10×0.4)"
        );
        let wrk = e.run_player("p2", "(Game.creeps.wrk.store||{})[RESOURCE_ENERGY]||0")?;
        assert_eq!(wrk.expr["value"], 1, "过期后回收恢复 floor(250/200)");

        // t7：塔伤害恢复（免伤门正路径的过期腿）。
        let rc = e.run_player("p1", &format!("{tower}.attack({tw})"))?;
        assert_eq!(rc.expr["value"], 0);
        assert_eq!(
            e.get_object(&wall).unwrap()["hits"],
            8600,
            "塔 600（射程 1 无衰减）"
        );
    }
    Ok(())
}

/// SHIELD：原地删工地、插入临时 rampart（hits=effect、hitsMax=0、带
/// PWR_SHIELD 效果）；护盾本体可受伤但 dismantle 无回收；到期由衰减门
/// 删除；放置门（同类 rampart / 边界墙规则）静默拒绝。
#[test]
fn shield_creates_and_expires_temporary_rampart() -> anyhow::Result<()> {
    let find_rampart =
        "Game.rooms.W1N1.find(FIND_STRUCTURES).filter(s=>s.structureType==='rampart')";
    for binary in [false, true] {
        // 正常链：工地先删、rampart 落位、可受伤、dismantle 无回收、到期删。
        let mut e = fixture(binary)?;
        let pc = actor(&mut e, "op", json!({"12":1}))?;
        e.place_object(
            "W1N1",
            "constructionSite",
            &json!({"pos":[24,25],"structureType":"road","owner":"p1"}),
        )?;
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"atk2","owner":"p2","pos":[24,24],
                "body":["attack","attack","attack","attack","attack"],"ticksToLive":1500}),
        )?;
        e.place_object(
            "W1N1",
            "creep",
            &json!({"name":"wrk2","owner":"p2","pos":[25,25],
                "body":["work","work","work","work","work"],"ticksToLive":1500}),
        )?;
        e.run_player("p1", &cast("op", c::PWR_SHIELD, "undefined"))?;
        // 事件 1-tick 生命期：施放后立即核对，再读工地与 rampart。
        assert_eq!(harness::events(&e, c::EVENT_POWER).len(), 1);
        let sites = e.run_player("p1", "Game.rooms.W1N1.find(FIND_CONSTRUCTION_SITES).length")?;
        assert_eq!(sites.expr["value"], 0, "原地工地被删除");
        let r = e.run_player(
            "p1",
            &format!(
                "[{find_rampart}.length,{find_rampart}[0]&&{find_rampart}[0].hits,\
                 {find_rampart}[0]&&{find_rampart}[0].hitsMax,\
                 {find_rampart}[0]&&{find_rampart}[0].effects[0].power,\
                 {find_rampart}[0]&&{find_rampart}[0].effects[0].effect===undefined]"
            ),
        )?;
        assert_eq!(r.expr["value"][0], 1, "临时 rampart 落位");
        assert_eq!(r.expr["value"][1], 5000, "L1 hits=effect");
        assert_eq!(r.expr["value"][2], 0, "hitsMax=0");
        assert_eq!(r.expr["value"][3], c::PWR_SHIELD);
        assert_eq!(
            r.expr["value"][4], true,
            "SHIELD 条目无 effect 键（官方专用分支）"
        );
        assert_eq!(
            e.get_object(&pc).unwrap()["powers"]["12"]["cooldownTime"],
            20,
            "冷却 20（施放结算于 tick 0）"
        );

        // 护盾本体不在 _damage 免伤门：attack 正常扣血。
        let rc = e.run_player("p2", &format!("Game.creeps.atk2.attack({find_rampart}[0])"))?;
        assert_eq!(rc.expr["value"], 0, "SHIELD 不在预检门");
        let r = e.run_player("p1", &format!("{find_rampart}[0].hits"))?;
        assert_eq!(r.expr["value"], 4850, "护盾可受伤");

        // dismantle 护盾：伤害照常、能量回收 0（SHIELD 在 dismantle 门）。
        let rc = e.run_player(
            "p2",
            &format!("Game.creeps.wrk2.dismantle({find_rampart}[0])"),
        )?;
        assert_eq!(rc.expr["value"], 0);
        let r = e.run_player("p1", &format!("{find_rampart}[0].hits"))?;
        assert_eq!(r.expr["value"], 4600, "dismantle 250");
        let wrk = e.run_player("p2", "(Game.creeps.wrk2.store||{})[RESOURCE_ENERGY]||0")?;
        assert_eq!(wrk.expr["value"], 0, "SHIELD dismantle 无回收");

        // 到期拍用绝对时间钉住：施放结算于 t0 → end 50；删除结算于 t50，
        // 首个读到 0 的世界时间为 51（此前每拍都存活——衰减门 <= 在 t49
        // 为假、跳过分支承担该拍）。
        let start = e.get_game_time();
        let mut gone_at: Option<u64> = None;
        for i in 0..60u64 {
            let r = e.run_player("p1", &format!("{find_rampart}.length"))?;
            if r.expr["value"] == 0 {
                gone_at = Some(start + i);
                break;
            }
        }
        assert_eq!(
            gone_at,
            Some(51),
            "到期拍 t50 删除（首个 0 读取时 time 51）"
        );

        // 放置门一：同格已有 rampart → 静默拒绝（无冷却、无事件）。
        let mut e = fixture(binary)?;
        let pc = actor(&mut e, "op", json!({"12":1}))?;
        e.place_object(
            "W1N1",
            "structure",
            &json!({"pos":[24,25],"structureType":"rampart","owner":"p1","hits":1000}),
        )?;
        e.run_player("p1", &cast("op", c::PWR_SHIELD, "undefined"))?;
        assert!(
            e.get_object(&pc).unwrap()["powers"]["12"]["cooldownTime"].is_null(),
            "拒绝不进冷却"
        );
        assert!(harness::events(&e, c::EVENT_POWER).is_empty());

        // 放置门二：贴边位置边界格非墙 → 拒绝；边界格全墙 → 成功。
        let mut e = fixture_with_terrain(binary, vec![0u8; 2500])?;
        let edge = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"name":"edge","owner":"p1","pos":[1,25],
            "powers":{"12":1},"store":{"ops":50}}),
        )?;
        e.place_object(
            "W1N1",
            "constructionSite",
            &json!({"pos":[1,25],"structureType":"road","owner":"p1"}),
        )?;
        e.run_player("p1", &cast("edge", c::PWR_SHIELD, "undefined"))?;
        assert!(
            e.get_object(&edge).unwrap()["powers"]["12"]["cooldownTime"].is_null(),
            "边界非墙拒绝"
        );
        let sites = e.run_player("p1", "Game.rooms.W1N1.find(FIND_CONSTRUCTION_SITES).length")?;
        assert_eq!(sites.expr["value"], 0, "拒绝路径工地仍被删（官方同形）");
        let mut e = fixture_with_terrain(binary, terrain_with(&[(0, 24), (0, 25), (0, 26)]))?;
        let edge2 = e.place_object(
            "W1N1",
            "powerCreep",
            &json!({"name":"edge2","owner":"p1","pos":[1,25],
            "powers":{"12":1},"store":{"ops":50}}),
        )?;
        e.run_player("p1", &cast("edge2", c::PWR_SHIELD, "undefined"))?;
        assert_eq!(
            e.get_object(&edge2).unwrap()["powers"]["12"]["cooldownTime"],
            20,
            "边界全墙放行"
        );
    }
    Ok(())
}

/// 施放轮写回失败缓存整轮候选（含 SHIELD 的对象插入）；重试原子提交；
/// 检查点冷恢复与继续推进逐拍一致。
#[test]
fn shield_retry_and_cold_restore_atomically() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        actor(&mut e, "op", json!({"12":2}))?;
        let before = e.world_snapshot();
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        assert!(
            e.run_player(
                "p1",
                &format!(
                    "Memory.runs=(Memory.runs||0)+1;{}",
                    cast("op", c::PWR_SHIELD, "undefined")
                )
            )
            .is_err()
        );
        assert_eq!(e.world_snapshot(), before, "对象插入随候选回滚");
        e.run_player("p1", "throw Error('不得重跑玩家')")?;
        let r = e.run_player(
            "p1",
            "Game.rooms.W1N1.find(FIND_STRUCTURES).filter(s=>s.structureType==='rampart').length",
        )?;
        assert_eq!(r.expr["value"], 1, "重试轮 rampart 落位");
        let r = e.run_player(
            "p1",
            "Game.rooms.W1N1.find(FIND_STRUCTURES).filter(s=>s.structureType==='rampart')[0].hits",
        )?;
        assert_eq!(r.expr["value"], 10000, "L2 hits");
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(e.checkpoint(Value::Null)?, None)?;
        for ticks in [1, 5, 25, 30] {
            e.tick(ticks)?;
            restored.tick(ticks)?;
            assert_eq!(e.world_snapshot(), restored.world_snapshot());
        }
        assert_eq!(e.run_player("p1", "Memory.runs")?.expr["value"], 1);
    }
    Ok(())
}
