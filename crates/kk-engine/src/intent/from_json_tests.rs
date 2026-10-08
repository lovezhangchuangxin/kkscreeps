use super::*;

/// 无效对象键、未知动作和错误容器格式被丢弃，合法 move 保留。
#[test]
fn from_json_adversarial_inputs() {
    let list = serde_json::json!({
        "kk0000000000000001": { "move": { "direction": 3 }, "explode": {}, "5": {} },
        "not-an-id": { "move": { "direction": 3 } },
        "room": "forged-non-object",
        "kk0000000000000002": 7,
    });
    let batch = IntentBatch::from_json("p1", &list);
    let user = &batch.users["p1"];
    assert_eq!(user.objects.len(), 1, "只有合法对象键存活");
    let obj = kk_world::ObjId::parse("kk0000000000000001").unwrap();
    let actions = &user.objects[&obj];
    assert_eq!(actions.len(), 1, "未知动作名 'explode' 丢弃");
    assert_eq!(actions[&Action::Move].direction, Some(3));
    assert!(user.room.is_empty(), "'room' 键非对象丢弃");
}

/// 扩展意图收集：冒号形状的合法键进 ext 容器并绑定玩家身份；
/// 键形非法丢弃；载荷原样保留（schema/接收者/所有权由结算侧校验）；
/// 同键后发覆盖；与 vanilla 动作可共存于同一对象键。
#[test]
fn from_json_collects_extension_intents() {
    let obj = kk_world::ObjId::parse("kk0000000000000031").unwrap();
    let key = "kk0000000000000031";
    let list = serde_json::json!({
        key: {
            "demo-counter:increment": { "amount": 3 },
            "demo-counter:increment ": { "amount": 1 },
            "not-a-key": { "amount": 2 },
            ":leading": {},
            "demo-counter:increment": { "amount": 7 },
        },
        "kk0000000000000032": {
            "other:act": null,
        },
    });
    let batch = IntentBatch::from_json("p1", &list);
    let user = &batch.users["p1"];
    assert_eq!(user.ext.len(), 2, "两个对象键各持有扩展意图");
    let ext = &user.ext[&obj];
    assert_eq!(ext.len(), 1, "同键后发覆盖 + 尾空白键形/非法键丢弃");
    let intent_key = kk_mod_api::Key::new("demo-counter:increment").unwrap();
    assert_eq!(ext[&intent_key], serde_json::json!({ "amount": 7 }));
    let other = kk_world::ObjId::parse("kk0000000000000032").unwrap();
    assert_eq!(
        user.ext[&other][&kk_mod_api::Key::new("other:act").unwrap()],
        serde_json::Value::Null,
        "null 载荷原样保留（nullable schema 的合法形态）"
    );
    // 身份绑定：发射者自报的玩家名不进入读取路径。
    let forged = serde_json::json!({
        key: { "demo-counter:increment": { "user": "p9", "amount": 1 } }
    });
    let batch = IntentBatch::from_json("p2", &forged);
    let user = &batch.users["p2"];
    assert!(user.ext.contains_key(&obj), "扩展意图归调用方给定的玩家");
    assert_eq!(user.ext[&obj].len(), 1);
    // 合并语义与对象动作一致：同键覆盖、异键共存。
    let mut merged = IntentBatch::from_json(
        "p1",
        &serde_json::json!({ key: { "demo-counter:increment": { "amount": 1 } } }),
    );
    let second = IntentBatch::from_json(
        "p1",
        &serde_json::json!({
            key: {
                "demo-counter:increment": { "amount": 5 },
                "demo-counter:reset": { "to": 0 },
            }
        }),
    );
    merged.extend_from(second);
    let ext = &merged.users["p1"].ext[&obj];
    assert_eq!(ext.len(), 2, "异键共存");
    assert_eq!(
        ext[&kk_mod_api::Key::new("demo-counter:increment").unwrap()],
        serde_json::json!({ "amount": 5 }),
        "同键后发覆盖"
    );
}

/// 缺失能量结构列表与显式空列表必须保持区别，创建执行器据此选择扣费方式。
#[test]
fn from_json_create_creep_roundtrip_and_presence() {
    let list = serde_json::json!({
        "kk0000000000000003": {
            "createCreep": { "body": ["move", "work"], "name": "c1", "energyStructures": [] }
        },
        "kk0000000000000004": {
            "createCreep": { "body": ["move"], "name": "c2" }
        }
    });
    let batch = IntentBatch::from_json("p1", &list);
    let user = &batch.users["p1"];
    let s1 = kk_world::ObjId::parse("kk0000000000000003").unwrap();
    let s2 = kk_world::ObjId::parse("kk0000000000000004").unwrap();
    assert_eq!(
        user.objects[&s1][&Action::CreateCreep].energy_structures,
        Some(vec![]),
        "空数组保持 Some（显式列表模式）"
    );
    assert_eq!(
        user.objects[&s2][&Action::CreateCreep].energy_structures,
        None,
        "缺键读 None（缺省全房扣费）"
    );
    assert_eq!(user.objects[&s1][&Action::CreateCreep].body.len(), 2);
}

/// room 级数组通道：createFlag/createConstructionSite 解析 + 未知名丢弃。
#[test]
fn from_json_room_intents() {
    let list = serde_json::json!({
        "room": {
            "createFlag": [
                { "roomName": "W1N1", "x": 10, "y": 11, "name": "f1", "color": 1, "secondaryColor": 1 },
                { "roomName": "W1N1", "x": 12, "y": 13, "name": "f2" }
            ],
            "teleport": [{ "roomName": "W1N1", "x": 0, "y": 0 }]
        }
    });
    let batch = IntentBatch::from_json("p1", &list);
    assert_eq!(batch.users["p1"].room.len(), 2, "未知 room intent 名丢弃");
}

/// 解析 spawn 动作、转换并去重方向；这些动作不属于 creep 执行序列。
/// 这里仅检查给定输入的结果，不证明按动作分别执行了字段白名单。
#[test]
fn from_json_spawn_family_whitelist() {
    let spawn = "kk0000000000000021";
    let creep = "kk0000000000000022";
    let list = serde_json::json!({
        spawn: {
            "renewCreep": { "id": creep, "forged": 1 },
            "recycleCreep": { "id": creep },
            "setSpawnDirections": { "directions": [3, 5, "7tail", 3] },
            "cancelSpawning": { "forged": "field" }
        }
    });
    let batch = IntentBatch::from_json("p1", &list);
    let user = &batch.users["p1"];
    let spawn_id = ObjId::parse(spawn).unwrap();
    let creep_id = ObjId::parse(creep).unwrap();
    let actions = &user.objects[&spawn_id];
    assert_eq!(actions.len(), 4);
    assert_eq!(
        actions[&Action::RenewCreep].id,
        Some(creep_id),
        "renewCreep 只留 id"
    );
    assert_eq!(
        actions[&Action::RecycleCreep].id,
        Some(creep_id),
        "recycleCreep 只留 id"
    );
    assert_eq!(
        actions[&Action::SetSpawnDirections].directions,
        vec![3, 5, 7],
        "number[]：parseInt 截断（\"7tail\"→7）+ 保序去重"
    );
    let cancel = &actions[&Action::CancelSpawning];
    assert_eq!(cancel.id, None, "cancelSpawning 空白名单——无字段");
    // 名字与 parse 往返 + 不在 creep 执行序
    for (action, name) in [
        (Action::RenewCreep, "renewCreep"),
        (Action::RecycleCreep, "recycleCreep"),
        (Action::SetSpawnDirections, "setSpawnDirections"),
        (Action::CancelSpawning, "cancelSpawning"),
    ] {
        assert_eq!(Action::parse(name), Some(action));
        assert_eq!(action.name(), name);
        assert!(
            !EXECUTION_ORDER.contains(&action),
            "{name} 不在 creepActions 序列（spawns/intents.js 的 spawn 分派序）"
        );
    }
}

/// 解析控制器相关动作，签名截为 100 个字符，未知字段不进入载荷。
#[test]
fn from_json_controller_family_whitelist() {
    let creep = "kk0000000000000009";
    let ctl = "kk0000000000000010";
    let list = serde_json::json!({
        creep: {
            "claimController": { "id": ctl, "unknown": 1 },
            "reserveController": { "id": ctl },
            "attackController": { "id": ctl },
            "generateSafeMode": { "id": ctl },
            "signController": { "id": ctl, "sign": "x".repeat(150) }
        },
        ctl: {
            "activateSafeMode": { "forged": "field" },
            "unclaim": { "forged": 1 }
        }
    });
    let batch = IntentBatch::from_json("p1", &list);
    let user = &batch.users["p1"];
    let creep = ObjId::parse(creep).unwrap();
    let ctl_id = ObjId::parse(ctl).unwrap();
    for action in [
        Action::ClaimController,
        Action::ReserveController,
        Action::AttackController,
        Action::GenerateSafeMode,
    ] {
        assert_eq!(
            user.objects[&creep][&action].id,
            Some(ctl_id),
            "{:?} 白名单只留 id",
            action.name()
        );
    }
    let sign = &user.objects[&creep][&Action::SignController];
    assert_eq!(sign.id, Some(ctl_id));
    assert_eq!(
        sign.sign.as_ref().map(|s| s.chars().count()),
        Some(100),
        "sign 走 userString 截 100"
    );
    // controller 对象的 activateSafeMode（system.js:34 空白名单——无字段）
    let activate = &user.objects[&ctl_id][&Action::ActivateSafeMode];
    assert_eq!(activate.id, None, "activateSafeMode 无字段");
    assert_eq!(
        Action::parse("activateSafeMode"),
        Some(Action::ActivateSafeMode)
    );
    assert_eq!(Action::ActivateSafeMode.name(), "activateSafeMode");
    // unclaim（system.js:78 空白名单——无字段；processor.js:307-309 分派）
    let unclaim = &user.objects[&ctl_id][&Action::Unclaim];
    assert_eq!(unclaim.id, None, "unclaim 无字段");
    // 缺省 → None / 显式空串 → Some("")：清洗层保留 presence 区分，
    // 语义分叉在 signController 结算层（两者都落"清除签名"）
    let d = sanitize_object_intent(Action::SignController, &serde_json::json!({ "id": ctl }));
    assert_eq!(d.sign, None, "缺省 sign → None");
    let d2 = sanitize_object_intent(Action::SignController, &serde_json::json!({ "sign": "" }));
    assert_eq!(d2.sign, Some(String::new()), "显式空串保留 Some(\"\")");
}

/// 检查 say、pull 和通知开关的载荷，以及 creep 执行顺序。
/// 字符串 "false" 在 JavaScript 真值规则下为 true。
#[test]
fn from_json_misc_family_whitelist_and_order() {
    let creep = "kk0000000000000011";
    let target = "kk0000000000000012";
    let list = serde_json::json!({
        creep: {
            "say": { "message": "hello", "isPublic": "yes", "unknown": 1 },
            "pull": { "id": target, "extra": true },
            "suicide": { "unknown": "dropped" },
        },
        target: {
            "notifyWhenAttacked": { "enabled": "false", "junk": 0 },
        },
    });
    let batch = IntentBatch::from_json("p1", &list);
    let u = &batch.users["p1"];
    let c_intents = &u.objects[&ObjId::parse(creep).unwrap()];
    // say：message 经 userString（截 100）；isPublic 经 to_bool——JS !! 真值表
    //（"yes" 非空串 → true，"false" 同样 true——字符串除 "" 外恒真）
    let say = &c_intents[&Action::Say];
    assert_eq!(say.message.as_deref(), Some("hello"));
    assert_eq!(
        say.is_public,
        Some(true),
        "to_bool('yes') === true（JS !! 真值表）"
    );
    assert_eq!(say.id, None, "say 无 id 字段，未知字段丢弃");
    // pull：只认 id
    let pull = &c_intents[&Action::Pull];
    assert_eq!(pull.id, Some(ObjId::parse(target).unwrap()));
    // suicide：空白名单（未知字段丢弃）
    assert_eq!(c_intents[&Action::Suicide].id, None);
    // notifyWhenAttacked：enabled 折叠（"false" 字符串在 JS !! 下是 true！）
    let t_intents = &u.objects[&ObjId::parse(target).unwrap()];
    assert_eq!(
        t_intents[&Action::NotifyWhenAttacked].enabled,
        Some(true),
        "to_bool('false') === true——官方 transform 是 !!value 非 JSON 解析"
    );
    // EXECUTION_ORDER 位次（官方 creepActions：harvest(8) < say(14) <
    // suicide(15) < pull(22)）
    let pos = |a: Action| EXECUTION_ORDER.iter().position(|x| *x == a).unwrap();
    assert!(pos(Action::Harvest) < pos(Action::Say));
    assert!(pos(Action::Say) < pos(Action::Suicide));
    assert!(pos(Action::Suicide) < pos(Action::Pull));
}

#[test]
fn global_container_collects_power_intents() {
    // global 容器的 power 六动作按官方处理序收集；与市场动作共用容器。
    let raw = serde_json::json!({
        "global": {
            "createPowerCreep": [{ "name": "PC1", "className": "operator" }],
            "createOrder": [{ "type": "buy", "resourceType": "energy", "price": 1, "totalAmount": 100, "roomName": "W1N1" }],
            "renamePowerCreep": [{ "id": "kk1", "name": "PC2" }],
        }
    });
    let batch = IntentBatch::from_json("p1", &raw);
    let u = &batch.users["p1"];
    assert_eq!(u.power.len(), 2, "create 与 rename 均收集");
    assert!(matches!(
        &u.power[0],
        PowerIntent::CreatePowerCreep { name, class_name }
            if name == "PC1" && class_name == "operator"
    ));
    assert!(matches!(
        &u.power[1],
        PowerIntent::RenamePowerCreep { id, name } if name == "PC2"
    ));
    assert_eq!(u.market.len(), 1, "市场动作不因 power 并列遍历丢失");
}
