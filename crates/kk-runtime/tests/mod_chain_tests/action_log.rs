//! 房间动作记录导出（captureActionLog 读面）集成：与套件
//! 27.10-actionlog 同形的坐标/生命期/覆盖语义，另钉套件外的
//! 私有 say 裁剪、条目元数据（type/structureType/name/pos/roomName）
//! 与 tick 值口径。形状来源：官方 processor.js 各 intent 写入点，
//! 经 settle::action_log 旁路通道（见模块注释）。
use super::*;

const ROOM: &str = "W1N1";

fn engine_on(binary: bool, world: Value) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&world)?;
    e.tick(1)?;
    Ok(e)
}

fn entry<'a>(capture: &'a Value, id: &str) -> Option<&'a Value> {
    capture["objects"]
        .as_array()?
        .iter()
        .find(|entry| entry["id"] == json!(id))
}

/// 套件 CREEP/TARGET-001 同形：attack 记目标坐标、attacked 记攻击者
/// 坐标；条目元数据与 tick 口径（tick=当前世界时间，产物属上一拍）。
#[test]
fn action_log_creep_source_and_target_entries() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(
            binary,
            json!({ "players": ["p1", "p2"], "rooms": [{ "name": ROOM, "rcl": 1, "owner": "p1" }] }),
        )?;
        let attacker = e.place_object(
            ROOM,
            "creep",
            &json!({"pos":[25,25],"owner":"p1","body":["attack","move"],"name":"a1"}),
        )?;
        let target = e.place_object(
            ROOM,
            "creep",
            &json!({"pos":[25,26],"owner":"p2","body":["tough","move"],"name":"t1"}),
        )?;
        e.run_player(
            "p1",
            &format!("Game.getObjectById('{attacker}').attack(Game.getObjectById('{target}'))"),
        )?;

        let capture = e.capture_action_log(ROOM);
        assert_eq!(capture["room"], json!(ROOM), "{binary}");
        assert_eq!(
            capture["tick"],
            json!(e.get_game_time()),
            "{binary}: tick 口径"
        );
        assert_eq!(
            capture["objects"].as_array().map(Vec::len),
            Some(2),
            "{binary}: 只有动作对象"
        );

        let src = entry(&capture, &attacker).expect("源侧条目");
        assert_eq!(src["type"], json!("creep"), "{binary}");
        assert_eq!(src["name"], json!("a1"), "{binary}");
        assert_eq!(
            src["pos"],
            json!({"x":25,"y":25,"roomName":ROOM}),
            "{binary}: 源自身位置"
        );
        assert_eq!(
            src["actionLog"]["attack"],
            json!({"x":25,"y":26}),
            "{binary}"
        );
        // objects 序显性化：ObjId 定宽零填充十六进制使字典序==数值序，
        // 与 vanilla 适配器的 id.localeCompare 序等价（套件不锁序）。
        let ids: Vec<&str> = capture["objects"]
            .as_array()
            .expect("objects 数组")
            .iter()
            .map(|e| e["id"].as_str().expect("id 字符串"))
            .collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted, "{binary}: 条目按 id 升序");

        let dst = entry(&capture, &target).expect("目标侧条目");
        assert_eq!(dst["type"], json!("creep"), "{binary}");
        assert_eq!(
            dst["actionLog"]["attacked"],
            json!({"x":25,"y":25}),
            "{binary}: 坐标=攻击者"
        );

        // 冷恢复：检查点往返后上一拍动作导出保留（与 last_events 同形
        // 保存恢复——restore 后未结算新拍，capture 仍返回保存时产物）。
        let checkpoint = e.checkpoint(json!({"case": "action_log"}))?;
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(checkpoint, None)?;
        let after = restored.capture_action_log(ROOM);
        let src = entry(&after, &attacker).expect("冷恢复后源侧条目仍在");
        assert_eq!(
            src["actionLog"]["attack"],
            json!({"x":25,"y":26}),
            "{binary}"
        );

        // 未知房间与未结算房间返回空 objects（不报错）。
        let empty = e.capture_action_log("E1N1");
        assert_eq!(
            empty["objects"].as_array().map(Vec::len),
            Some(0),
            "{binary}"
        );
    }
    Ok(())
}

/// 套件 SAY-001 同形 + 套件外：非公开 say 不进导出（官方历史快照
/// processor.js:436-438 删除非公开 say），公开 say 带截断文本与标志。
#[test]
fn action_log_say_public_only() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(
            binary,
            json!({ "players": ["p1"], "rooms": [{ "name": ROOM, "rcl": 1, "owner": "p1" }] }),
        )?;
        let public = e.place_object(
            ROOM,
            "creep",
            &json!({"pos":[25,25],"owner":"p1","body":["move"],"name":"c1"}),
        )?;
        let private = e.place_object(
            ROOM,
            "creep",
            &json!({"pos":[26,25],"owner":"p1","body":["move"],"name":"c2"}),
        )?;
        e.run_player(
            "p1",
            &format!(
                "Game.getObjectById('{public}').say('hello', true); Game.getObjectById('{private}').say('secret'); null"
            ),
        )?;
        let capture = e.capture_action_log(ROOM);
        let src = entry(&capture, &public).expect("公开 say 条目");
        assert_eq!(
            src["actionLog"]["say"],
            json!({"message":"hello","isPublic":true}),
            "{binary}"
        );
        assert!(
            entry(&capture, &private).is_none(),
            "{binary}: 非公开 say 不进导出（对象无任何动作，不出现）"
        );
    }
    Ok(())
}

/// 套件 TICK-001/DEDUP-001 同形：塔同拍两次 attack（两次调用都 OK）
/// 只暴露后者坐标且 attack 键唯一；随后一拍无动作后键消失。
#[test]
fn action_log_tick_clear_and_last_wins() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(
            binary,
            json!({ "players": ["p1", "p2"], "rooms": [{ "name": ROOM, "rcl": 3, "owner": "p1" }] }),
        )?;
        let tower = e.place_object(
            ROOM,
            "structure",
            &json!({"pos":[25,25],"structureType":"tower","owner":"p1","store":{"energy":1000}}),
        )?;
        let first = e.place_object(
            ROOM,
            "creep",
            &json!({"pos":[25,28],"owner":"p2","body":["tough","tough","tough","tough","tough","move"]}),
        )?;
        let second = e.place_object(
            ROOM,
            "creep",
            &json!({"pos":[26,28],"owner":"p2","body":["tough","tough","tough","tough","tough","move"]}),
        )?;
        let rc = e.run_player(
            "p1",
            &format!(
                "const t = Game.getObjectById('{tower}'); [t.attack(Game.getObjectById('{first}')), t.attack(Game.getObjectById('{second}'))]"
            ),
        )?;
        assert_eq!(rc.expr["value"], json!([0, 0]), "{binary}: 两次调用都 OK");

        let capture = e.capture_action_log(ROOM);
        let src = entry(&capture, &tower).expect("塔条目");
        assert_eq!(
            src["actionLog"]["attack"],
            json!({"x":26,"y":28}),
            "{binary}: 后写覆盖"
        );
        let keys = src["actionLog"]
            .as_object()
            .expect("actionLog 对象")
            .keys()
            .filter(|k| *k == "attack")
            .count();
        assert_eq!(keys, 1, "{binary}: attack 键唯一");
        // 塔条目的结构元数据：type/structureType 同官方具体类型名。
        assert_eq!(src["type"], json!("tower"), "{binary}");
        assert_eq!(src["structureType"], json!("tower"), "{binary}");

        e.tick(1)?;
        let later = e.capture_action_log(ROOM);
        assert!(
            entry(&later, &tower).is_none(),
            "{binary}: 下一拍无动作，条目消失"
        );
    }
    Ok(())
}

/// 套件 STRUCT-001 同形抽样：link transferEnergy（目标坐标）与 lab
/// runReaction（两个输入 lab 四坐标）。
#[test]
fn action_log_link_and_lab_entries() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(
            binary,
            json!({ "players": ["p1"], "rooms": [{ "name": ROOM, "rcl": 6, "owner": "p1" }] }),
        )?;
        let link_a = e.place_object(
            ROOM,
            "structure",
            &json!({"pos":[25,25],"structureType":"link","owner":"p1","store":{"energy":400}}),
        )?;
        let link_b = e.place_object(
            ROOM,
            "structure",
            &json!({"pos":[25,35],"structureType":"link","owner":"p1","store":{"energy":0}}),
        )?;
        let rc = e.run_player(
            "p1",
            &format!(
                "Game.getObjectById('{link_a}').transferEnergy(Game.getObjectById('{link_b}'), 100)"
            ),
        )?;
        assert_eq!(rc.expr["value"], json!(0), "{binary}: link 传输 OK");
        let capture = e.capture_action_log(ROOM);
        let src = entry(&capture, &link_a).expect("link 条目");
        assert_eq!(
            src["actionLog"]["transferEnergy"],
            json!({"x":25,"y":35}),
            "{binary}: 目标 link 坐标"
        );

        let lab = e.place_object(
            ROOM,
            "structure",
            &json!({"pos":[25,25],"structureType":"lab","owner":"p1","store":{"energy":2000,"OH":0}}),
        )?;
        let lab1 = e.place_object(
            ROOM,
            "structure",
            &json!({"pos":[25,27],"structureType":"lab","owner":"p1","store":{"energy":2000,"H":5}}),
        )?;
        let lab2 = e.place_object(
            ROOM,
            "structure",
            &json!({"pos":[27,25],"structureType":"lab","owner":"p1","store":{"energy":2000,"O":5}}),
        )?;
        let rc = e.run_player(
            "p1",
            &format!(
                "Game.getObjectById('{lab}').runReaction(Game.getObjectById('{lab1}'), Game.getObjectById('{lab2}'))"
            ),
        )?;
        assert_eq!(rc.expr["value"], json!(0), "{binary}: 反应 OK");
        let capture = e.capture_action_log(ROOM);
        let src = entry(&capture, &lab).expect("lab 条目");
        assert_eq!(
            src["actionLog"]["runReaction"],
            json!({"x1":25,"y1":27,"x2":27,"y2":25}),
            "{binary}: 两个输入 lab 四坐标"
        );
    }
    Ok(())
}

/// 套件外战斗边角：近战反伤的攻击者 attacked（坐标=受击目标）、
/// rangedAttack 键（区别于塔远程的 attack 键）、rangedMassAttack 空对象
/// 载荷、同拍死亡对象的条目不进导出（collect 存活过滤）。
#[test]
fn action_log_combat_edges() -> anyhow::Result<()> {
    for binary in [false, true] {
        let world = json!({ "players": ["p1", "p2"], "rooms": [{ "name": ROOM, "rcl": 1, "owner": "p1" }] });
        // 反伤：近战打带 ATTACK 的目标，native 反击使攻击者记 attacked
        //（坐标=目标位置，官方 _damage.js:86-91）。
        {
            let mut e = engine_on(binary, world.clone())?;
            let attacker = e.place_object(
                ROOM,
                "creep",
                &json!({"pos":[25,25],"owner":"p1","body":["attack","move"]}),
            )?;
            let defender = e.place_object(
                ROOM,
                "creep",
                &json!({"pos":[25,26],"owner":"p2","body":["attack","move"]}),
            )?;
            e.run_player(
                "p1",
                &format!(
                    "Game.getObjectById('{attacker}').attack(Game.getObjectById('{defender}'))"
                ),
            )?;
            let capture = e.capture_action_log(ROOM);
            let src = entry(&capture, &attacker).expect("攻击者条目");
            assert_eq!(
                src["actionLog"]["attacked"],
                json!({"x":25,"y":26}),
                "{binary}: 反伤 attacked 坐标=受击目标"
            );
        }
        // rangedAttack 键 + rangedMassAttack 空载荷：分两拍（creep 每拍
        // 一个动作）。
        {
            let mut e = engine_on(binary, world.clone())?;
            let actor = e.place_object(
                ROOM,
                "creep",
                &json!({"pos":[25,25],"owner":"p1","body":["ranged_attack","move"]}),
            )?;
            let target = e.place_object(
                ROOM,
                "creep",
                &json!({"pos":[26,26],"owner":"p2","body":["move"]}),
            )?;
            e.run_player(
                "p1",
                &format!(
                    "Game.getObjectById('{actor}').rangedAttack(Game.getObjectById('{target}'))"
                ),
            )?;
            let capture = e.capture_action_log(ROOM);
            let src = entry(&capture, &actor).expect("远程攻击者条目");
            assert_eq!(
                src["actionLog"]["rangedAttack"],
                json!({"x":26,"y":26}),
                "{binary}: creep 远程记 rangedAttack 键"
            );
            assert!(
                src["actionLog"].get("attack").is_none(),
                "{binary}: 不与近战 attack 键混写"
            );
            e.run_player(
                "p1",
                &format!("Game.getObjectById('{actor}').rangedMassAttack()"),
            )?;
            let capture = e.capture_action_log(ROOM);
            let src = entry(&capture, &actor).expect("群体攻击者条目");
            assert_eq!(
                src["actionLog"]["rangedMassAttack"],
                json!({}),
                "{binary}: 群体攻击空对象载荷（非坐标）"
            );
        }
        // 同拍死亡：4×ATTACK（120）一击致死 [move]（100）目标——目标的
        // attacked 记录在结算尾被存活过滤丢弃（官方 saveRoomHistory 只快
        // 照最终 roomObjects）。
        {
            let mut e = engine_on(binary, world)?;
            let attacker = e.place_object(
                ROOM,
                "creep",
                &json!({"pos":[25,25],"owner":"p1","body":["attack","attack","attack","attack","move"]}),
            )?;
            let victim = e.place_object(
                ROOM,
                "creep",
                &json!({"pos":[25,26],"owner":"p2","body":["move"]}),
            )?;
            e.run_player(
                "p1",
                &format!("Game.getObjectById('{attacker}').attack(Game.getObjectById('{victim}'))"),
            )?;
            let capture = e.capture_action_log(ROOM);
            assert!(
                entry(&capture, &victim).is_none(),
                "{binary}: 同拍死亡对象不进导出"
            );
            let src = entry(&capture, &attacker).expect("击杀者条目仍在");
            assert_eq!(
                src["actionLog"]["attack"],
                json!({"x":25,"y":26}),
                "{binary}"
            );
        }
    }
    Ok(())
}

/// 套件外 powerCreep 导出：usePower 的 power 载荷 {id,x,y}（目标坐标）
/// 与 enableRoom 的 attack 键（官方 enableRoom.js:21，坐标=控制器）；
/// 条目 type=powerCreep。
#[test]
fn action_log_power_creep_entries() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(
            binary,
            json!({ "players": ["p1"], "rooms": [{ "name": ROOM, "rcl": 6, "owner": "p1" }] }),
        )?;
        let spawn = e.place_object(
            ROOM,
            "structure",
            &json!({"pos":[25,26],"structureType":"spawn","owner":"p1","store":{"energy":300}}),
        )?;
        let operator = e.place_object(
            ROOM,
            "powerCreep",
            &json!({
                "name":"operator","owner":"p1","pos":[25,25],
                "powers":{ kk_core::constants::PWR_OPERATE_SPAWN.to_string(): 1 },
                "store":{"ops":100}
            }),
        )?;
        // enableRoom 先行（usePower 的官方 isPowerEnabled 门）：pc 紧邻
        // 控制器放置，键复用 attack、坐标=控制器。
        let (cx, cy) = e
            .get_controller_pos(ROOM)
            .map(|(x, y)| (x as i64, y as i64))
            .expect("控制器存在");
        let enabler = e.place_object(
            ROOM,
            "powerCreep",
            &json!({
                "name":"enabler","owner":"p1","pos":[(cx + 1).clamp(0, 49), cy],
                "powers":{}
            }),
        )?;
        let rc = e.run_player(
            "p1",
            &format!("Game.powerCreeps.enabler.enableRoom(Game.rooms.{ROOM}.controller)"),
        )?;
        assert_eq!(rc.expr["value"], json!(0), "{binary}: enableRoom OK");
        let capture = e.capture_action_log(ROOM);
        let src = entry(&capture, &enabler).expect("pc enableRoom 条目");
        assert_eq!(
            src["actionLog"]["attack"],
            json!({"x": cx, "y": cy}),
            "{binary}: enableRoom 复用 attack 键、坐标=控制器"
        );

        // usePower：power 载荷 {id,x,y}（目标坐标）。
        let rc = e.run_player(
            "p1",
            &format!(
                "Game.powerCreeps.operator.usePower(PWR_OPERATE_SPAWN, Game.getObjectById('{spawn}'))"
            ),
        )?;
        assert_eq!(rc.expr["value"], json!(0), "{binary}: usePower OK");
        let capture = e.capture_action_log(ROOM);
        let src = entry(&capture, &operator).expect("pc 条目");
        assert_eq!(src["type"], json!("powerCreep"), "{binary}");
        assert_eq!(
            src["actionLog"]["power"],
            json!({"id": kk_core::constants::PWR_OPERATE_SPAWN, "x":25,"y":26}),
            "{binary}: power 载荷含目标坐标"
        );
    }
    Ok(())
}

/// 套件外通道语义：公开 say 超 10 个 UTF-16 码元截断进导出（锁第二次
/// utf16_truncate 调用）；结算失败轮保留上一拍的导出不清空。
#[test]
fn action_log_say_truncation_and_failed_tick_retention() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = engine_on(
            binary,
            json!({ "players": ["p1"], "rooms": [{ "name": ROOM, "rcl": 1, "owner": "p1" }] }),
        )?;
        let talker = e.place_object(
            ROOM,
            "creep",
            &json!({"pos":[25,25],"owner":"p1","body":["move"],"name":"c1"}),
        )?;
        e.run_player(
            "p1",
            &format!("Game.getObjectById('{talker}').say('0123456789XY', true)"),
        )?;
        let capture = e.capture_action_log(ROOM);
        let src = entry(&capture, &talker).expect("say 条目");
        assert_eq!(
            src["actionLog"]["say"]["message"],
            json!("0123456789"),
            "{binary}: 导出面同截 10 个 UTF-16 码元"
        );

        // 结算失败轮（故障注入）不清空上一拍导出。
        let before = e.capture_action_log(ROOM);
        assert!(entry(&before, &talker).is_some(), "{binary}: 前置");
        e.fail_next_settle_for_test();
        let err = e.run_player(
            "p1",
            &format!("Game.getObjectById('{talker}').say('dead', true)"),
        );
        assert!(err.is_err(), "{binary}: 注入失败轮");
        let after = e.capture_action_log(ROOM);
        let src = entry(&after, &talker).expect("失败轮保留上一拍条目");
        assert_eq!(
            src["actionLog"]["say"]["message"],
            json!("0123456789"),
            "{binary}: 失败轮导出保持上一成功拍"
        );
    }
    Ok(())
}
