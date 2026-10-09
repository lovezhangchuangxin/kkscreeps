//! 数据模型与管理入口的单元行为验证。
use super::*;
use kk_schema::room::{Reservation, Sign};

fn ctx() -> PlaceCtx<'static> {
    PlaceCtx {
        time: 100,
        links: builtin_link_rules(),
        stores: builtin_store_rules(),
        buildings: Default::default(),
        controller: None,
        name_seq: 7,
    }
}

fn spec(v: Value) -> Value {
    v
}

#[test]
fn creep_defaults_match_vanilla() {
    let s = spec(json!({
        "pos": [10, 11], "owner": "p1",
        "body": ["work", "carry", "move"],
        "boosts": { "0": "XUHO2" },
        "store": { "energy": 30 },
        "ticksToLive": 500,
    }));
    let RoomObject::Creep(c) = normalize("creep", &s, &ctx()).unwrap() else {
        panic!("应产出 creep");
    };
    assert_eq!((c.x, c.y), (10, 11));
    assert_eq!(c.body.len(), 3);
    assert_eq!(c.body[0].boost.as_deref(), Some("XUHO2"));
    assert_eq!(c.body[1].boost, None);

    assert_eq!(c.hits, 300);
    assert_eq!(c.age_time, 600);
    assert_eq!(c.store.energy, 30);
    assert_eq!(c.fatigue, 0);
    assert!(!c.spawning);
    // 无名稳定名
    assert_eq!(c.name, "creep_7");
}

/// 放置 CLAIM creep 时，显式 TTL 不受正常孵化寿命上限限制；默认仍为 1500。
#[test]
fn claim_body_ttl_follows_vanilla_placement() {
    let s = spec(json!({
        "pos": [0, 0], "owner": "p1", "body": ["claim"], "ticksToLive": 5000,
    }));
    let RoomObject::Creep(c) = normalize("creep", &s, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(c.age_time, 100 + 5000);
    // 缺省恒 1500（与 body 无关）
    let s2 = spec(json!({ "pos": [0, 0], "owner": "p1", "body": ["claim"] }));
    let RoomObject::Creep(c2) = normalize("creep", &s2, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(c2.age_time, 100 + c::CREEP_LIFE_TIME as u64);
}

#[test]
fn source_capacity_by_controller_state() {
    // 无 controller → keeper 4000
    let s = spec(json!({ "pos": [5, 5], "structureType": "source" }));
    let RoomObject::Source(src) = normalize("structure", &s, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(src.energy_capacity, 4000);
    assert_eq!(src.energy, 4000);
    assert_eq!(src.next_regeneration_time, None);
    // 归属 controller → 3000
    let ctl = ControllerObj {
        id: 0,
        x: 1,
        y: 1,
        level: 3,
        safe_mode: 0,
        safe_mode_available: 1,
        progress: 0,
        downgrade_time: None,
        owner: Some("p1".into()),
        owner_cleared: false,
        is_power_enabled: false,
        reservation: None,
        sign: None,
        upgrade_blocked: None,
        safe_mode_cooldown: None,
    };
    let c2 = PlaceCtx {
        controller: Some(&ctl),
        ..ctx()
    };
    let RoomObject::Source(src2) = normalize("structure", &s, &c2).unwrap() else {
        panic!()
    };
    assert_eq!(src2.energy_capacity, 3000);
    // 未满挂 300 tick 重生钟（vanilla placeSource）
    let s3 = spec(json!({ "pos": [5, 5], "structureType": "source", "energy": 100 }));
    let RoomObject::Source(src3) = normalize("structure", &s3, &c2).unwrap() else {
        panic!()
    };
    assert_eq!(src3.next_regeneration_time, Some(100 + 300));
    // spec.energyCapacity 覆盖
    let s4 = spec(json!({ "pos": [5, 5], "structureType": "source", "energyCapacity": 1500 }));
    let RoomObject::Source(src4) = normalize("structure", &s4, &c2).unwrap() else {
        panic!()
    };
    assert_eq!(src4.energy_capacity, 1500);
}

#[test]
fn structure_defaults() {
    // spawn：无名稳定名、能量起点、hits
    let s = spec(json!({ "pos": [25, 25], "owner": "p1" }));
    let RoomObject::Spawn(sp) = normalize("spawn", &s, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(sp.name, "Spawn_7");
    assert_eq!(sp.store.energy, c::SPAWN_ENERGY_START as u32);
    assert_eq!(sp.hits, c::SPAWN_HITS as u32);
    assert!(sp.spawning.is_none());
    // 墙：hits=1（可修到 300M）
    let s = spec(json!({ "pos": [1, 1], "structureType": "constructedWall" }));
    let RoomObject::ConstructedWall(w) = normalize("structure", &s, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(w.hits, 1);
    // 工地：progressTotal 派生 + progress 缺省 0
    let s = spec(json!({ "pos": [2, 2], "structureType": "extension", "owner": "p1" }));
    let RoomObject::ConstructionSite(site) = normalize("constructionSite", &s, &ctx()).unwrap()
    else {
        panic!()
    };
    assert_eq!(site.structure_type, "extension");
    assert_eq!(site.progress, 0);
    // flag：color 缺省 1
    let s = spec(json!({ "pos": [3, 3], "name": "f1" }));
    let RoomObject::Flag(f) = normalize("flag", &s, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!((f.color, f.secondary_color), (1, 1));
    // resource
    let s = spec(json!({ "pos": [4, 4], "amount": 55 }));
    let RoomObject::Energy(e) = normalize("resource", &s, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(e.amount, 55);
}

#[test]
fn oversized_store_energy_saturates_instead_of_wrapping() {
    let s = json!({
        "pos": [10, 10], "body": ["carry"],
        "store": { "energy": u32::MAX as u64 + 1 },
    });
    let RoomObject::Creep(creep) = normalize("creep", &s, &ctx()).unwrap() else {
        panic!("应产出 creep");
    };
    assert_eq!(creep.store.energy, u32::MAX);
}

/// structureType=spawn 须产生 Spawn 变体，使玩家集合和类型判断识别为 spawn。
#[test]
fn spawn_via_structure_channel_routes_to_spawn() {
    let s = spec(json!({
        "pos": [20, 20], "structureType": "spawn", "owner": "p1",
    }));
    let RoomObject::Spawn(sp) = normalize("structure", &s, &ctx()).unwrap() else {
        panic!("structure+spawn 必须产出 typed Spawn，不能落 Generic")
    };
    assert_eq!(sp.owner, "p1");
    assert_eq!(sp.store.energy, c::SPAWN_ENERGY_START as u32);
    // 显式 store 原样（不钳）
    let s2 = spec(json!({
        "pos": [20, 20], "structureType": "spawn", "owner": "p1",
        "store": { "energy": 5000 },
    }));
    let RoomObject::Spawn(sp2) = normalize("structure", &s2, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(sp2.store.energy, 5000);
    // 空 store → 0（vanilla 语义：有 store 原样）
    let s3 = spec(json!({
        "pos": [20, 20], "structureType": "spawn", "owner": "p1", "store": {},
    }));
    let RoomObject::Spawn(sp3) = normalize("structure", &s3, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(sp3.store.energy, 0);
}

/// 控制器接收绝对计时和完整的签名、预订记录；creep 将 G 保存到专用资源字段。
#[test]
fn controller_b2_fields_and_ghodium_passthrough() {
    let s = spec(json!({
        "pos": [9, 9], "structureType": "controller", "level": 4, "owner": "p1",
        "upgradeBlocked": 1200,
        "safeModeCooldown": 50100,
        "reservation": { "user": "p2", "endTime": 4321 },
        "sign": { "user": "p1", "text": "hi", "time": 77 }
    }));
    let RoomObject::Controller(c) = normalize("structure", &s, &ctx()).unwrap() else {
        panic!("应产出 controller");
    };
    assert_eq!(c.upgrade_blocked, Some(1200));
    assert_eq!(c.safe_mode_cooldown, Some(50_100));
    assert_eq!(
        c.reservation,
        Some(Reservation {
            user: "p2".into(),
            end_time: 4321,
        })
    );
    assert_eq!(
        c.sign,
        Some(Sign {
            user: "p1".into(),
            text: "hi".into(),
            time: 77,
            datetime: None,
        })
    );
    // 缺省形态：四新字段全 None
    let s2 = spec(json!({ "pos": [9, 9], "structureType": "controller" }));
    let RoomObject::Controller(c2) = normalize("structure", &s2, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(
        (
            c2.reservation,
            c2.sign,
            c2.upgrade_blocked,
            c2.safe_mode_cooldown
        ),
        (None, None, None, None)
    );
    // 畸形嵌套对象（缺键）→ None（不部分落地）
    let s3 = spec(json!({
        "pos": [9, 9], "structureType": "controller",
        "reservation": { "user": "p2" }
    }));
    let RoomObject::Controller(c3) = normalize("structure", &s3, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(c3.reservation, None, "缺 endTime 的预约整票丢弃");
    // creep store 的 ghodium 槽（官方键 "G"）
    let s4 = spec(json!({
        "pos": [5, 5], "owner": "p1", "body": ["move"],
        "store": { "energy": 30, "G": 1200 }
    }));
    let RoomObject::Creep(cr) = normalize("creep", &s4, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(cr.store.ghodium, 1200);
}

/// 墓碑、废墟采用默认衰减时间，并保存专用字段之外的资源。
#[test]
fn tombstone_and_ruin_placement_defaults() {
    let s = spec(json!({
        "pos": [8, 8],
        "creepName": "w1",
        "store": { "energy": 30, "XUHO2": 6 },
        "ticksToDecay": 100,
    }));
    let RoomObject::Tombstone(t) = normalize("tombstone", &s, &ctx()).unwrap() else {
        panic!("tombstone 直通道进 typed 变体");
    };
    assert_eq!(t.death_time, 100, "缺省 deathTime = gameTime");
    assert_eq!(t.decay_time, 200, "deathTime + ticksToDecay");
    assert_eq!(t.creep_name.as_deref(), Some("w1"));
    assert_eq!(t.creep_id, None, "vanilla 缺省 null");
    assert_eq!(t.creep_ticks_to_live, 0);
    assert_eq!(t.creep_body, Vec::<u8>::new());
    assert_eq!(t.owner, None, "vanilla 放置不落 user");
    assert_eq!(t.store.energy, 30);
    assert_eq!(
        t.store_extra,
        vec![ResourceStack {
            resource: "XUHO2".into(),
            amount: 6,
        }],
        "额外资源进列表槽"
    );
    // 全缺省形态
    let s2 = spec(json!({ "pos": [9, 9], "creepName": "x" }));
    let RoomObject::Tombstone(t2) = normalize("tombstone", &s2, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(t2.decay_time, 100 + 500, "ticksToDecay 缺省 500（vanilla）");

    let r = spec(json!({
        "pos": [10, 10], "structureType": "spawn", "structureOwner": "p1",
        "structureId": "kk0000000000000004", "structureHitsMax": 5000,
        "destroyTime": 90, "ticksToDecay": 50,
        "store": { "energy": 300 },
    }));
    let RoomObject::Ruin(ru) = normalize("ruin", &r, &ctx()).unwrap() else {
        panic!("ruin 直通道进 typed 变体");
    };
    assert_eq!(ru.structure_type, "spawn");
    assert_eq!(ru.structure_id, 4, "kk id 字符串解析");
    assert_eq!(ru.structure_hits_max, Some(5000));
    assert_eq!(ru.owner.as_deref(), Some("p1"));
    assert_eq!(ru.destroy_time, 90);
    assert_eq!(ru.decay_time, 140);
    assert_eq!(ru.store.energy, 300);
    // structureOwner 缺省（vanilla 无 user 键）
    let r2 = spec(json!({ "pos": [11, 11], "structureType": "wall" }));
    let RoomObject::Ruin(ru2) = normalize("ruin", &r2, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(ru2.owner, None);
    assert_eq!(ru2.decay_time, 100 + 500);
}

/// 检查矿物默认密度、存量和相对再生时间，以及提取器的默认值与冷却计数。
#[test]
fn mineral_and_extractor_placement_defaults() {
    // mineral：density 缺省 DENSITY_HIGH(3)、amount 缺省 MINERAL_DENSITY[3]
    let s = spec(json!({
        "pos": [10, 10], "structureType": "mineral", "mineralType": "X",
    }));
    let RoomObject::Mineral(m) = normalize("structure", &s, &ctx()).unwrap() else {
        panic!("structure 通道的 mineral 进 typed 变体");
    };
    assert_eq!(m.mineral_type, "X");
    assert_eq!(m.density, 3);
    assert_eq!(m.mineral_amount, 70_000, "MINERAL_DENSITY[DENSITY_HIGH]");
    assert_eq!(m.next_regeneration_time, None);
    // 显式 density/amount + ticksToRegeneration 绝对钟（vanilla 只判
    // !== undefined——无 source 的 >0 过滤）
    let s2 = spec(json!({
        "pos": [10, 10], "structureType": "mineral", "mineralType": "U",
        "density": 1, "mineralAmount": 123, "ticksToRegeneration": 45,
    }));
    let RoomObject::Mineral(m2) = normalize("structure", &s2, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(m2.density, 1);
    assert_eq!(m2.mineral_amount, 123);
    assert_eq!(
        m2.next_regeneration_time,
        Some(100 + 45),
        "gameTime + ticks"
    );
    // 缺 mineralType 拒（vanilla 的 MineralSpec 必填字段）
    let bad = spec(json!({ "pos": [10, 10], "structureType": "mineral" }));
    assert!(normalize("structure", &bad, &ctx()).is_err());

    // extractor：hits 缺省 EXTRACTOR_HITS、cooldown 透传、owner 可缺省
    let e = spec(json!({
        "pos": [10, 10], "structureType": "extractor", "owner": "p1", "cooldown": 4,
    }));
    let RoomObject::Extractor(ex) = normalize("structure", &e, &ctx()).unwrap() else {
        panic!("structure 通道的 extractor 进 typed 变体");
    };
    assert_eq!(ex.hits, c::EXTRACTOR_HITS as u32);
    assert_eq!(ex.owner.as_deref(), Some("p1"));
    assert_eq!(ex.cooldown, 4);
    let e2 = spec(json!({ "pos": [10, 10], "structureType": "extractor", "hits": 111 }));
    let RoomObject::Extractor(ex2) = normalize("structure", &e2, &ctx()).unwrap() else {
        panic!()
    };
    assert_eq!(ex2.hits, 111);
    assert_eq!(ex2.owner, None, "无主 extractor（harvest 无主放行分支）");
    assert_eq!(ex2.cooldown, 0);
}

#[test]
fn unknown_types_go_generic_passthrough() {
    // tower 使用 Generic 保存数据，仍可被结构查询和结算识别。
    let s = spec(json!({
        "pos": [7, 7], "structureType": "tower", "owner": "p1", "hits": 999,
    }));
    let RoomObject::Generic(g) = normalize("structure", &s, &ctx()).unwrap() else {
        panic!()
    };
    let data: Value = serde_json::from_str(&g.data).unwrap();
    assert_eq!(data["structureType"], "tower");
    assert_eq!(data["hits"], 999);
    // 非法输入拒绝
    let bad = spec(json!({ "pos": [50, 0] }));
    assert!(normalize("creep", &bad, &ctx()).is_err());
    // 旗名含引擎数据分隔符拒绝（官方 |~ 拼串存储的放置契约对齐）。
    // 断言绑定输入名而非分隔符本身——错误模板恒含 |~，后者无法判别门是否触发。
    for name in ["Bad|Flag", "Bad~Flag"] {
        let bad = spec(json!({ "pos": [1, 2], "name": name }));
        let err = normalize("flag", &bad, &ctx()).unwrap_err();
        assert!(err.contains(name), "错误消息含被拒旗名：{err}");
    }
    // spawn 缺 owner 拒绝（必有主结构）。
    let bad = spec(json!({ "pos": [1, 2], "structureType": "spawn" }));
    let err = normalize("structure", &bad, &ctx()).unwrap_err();
    assert!(err.contains("owner"), "错误消息含 owner：{err}");
    // 公开 object-only 类型经 structure 通道拒绝并提示 placeObject。
    for ty in [
        "deposit",
        "keeperLair",
        "invaderCore",
        "portal",
        "powerBank",
    ] {
        let bad = spec(json!({ "pos": [1, 2], "structureType": ty }));
        let err = normalize("structure", &bad, &ctx()).unwrap_err();
        assert!(
            err.contains("placeObject"),
            "{ty} 拒绝并提示对象通道：{err}"
        );
    }
    // deposit 走专用 kind 通道不受 object-only 门影响。
    assert!(normalize("deposit", &spec(json!({ "pos": [1, 2] })), &ctx()).is_ok());
    // deposit：绝对权威键原样透传；相对 cooldown/ticksToDecay 按放置
    // 时刻换算为 cooldownTime/decayTime（适配器契约，与 nuke 同形）。
    let s2 = spec(json!({ "pos": [1, 1], "decayTime": 88 }));
    let RoomObject::Generic(g2) = normalize("deposit", &s2, &ctx()).unwrap() else {
        panic!()
    };
    let d2: Value = serde_json::from_str(&g2.data).unwrap();
    assert_eq!(d2["type"], "deposit");
    assert_eq!(d2["decayTime"], 88, "绝对键透传不换算");
    let s2 = spec(json!({ "pos": [1, 1], "cooldown": 25, "ticksToDecay": 100 }));
    let RoomObject::Generic(g2) = normalize("deposit", &s2, &ctx()).unwrap() else {
        panic!()
    };
    let d2: Value = serde_json::from_str(&g2.data).unwrap();
    assert_eq!(d2["cooldownTime"], 125, "相对冷却 → ctx.time(100)+25");
    assert_eq!(d2["decayTime"], 200, "相对寿命 → ctx.time(100)+100");
    assert!(d2.get("cooldown").is_none() && d2.get("ticksToDecay").is_none());
    // structure 通道的 tower 透传也带 type
    let RoomObject::Generic(g3) = normalize("structure", &s, &ctx()).unwrap() else {
        panic!()
    };
    let d3: Value = serde_json::from_str(&g3.data).unwrap();
    assert_eq!(d3["type"], "structure");
    assert_eq!(d3["structureType"], "tower");
}

/// powerBank 放置归一：power 进 store、相对 ticksToDecay 换算绝对
/// decayTime、hits 可覆盖而 hitsMax 恒 POWER_BANK_HITS、缺相对键不写
/// 权威键（永不衰变移除，与 deposit 缺键形态一致）。
#[test]
fn power_bank_placement_normalizes_relative_keys_and_store() {
    let s = spec(json!({
        "pos": [25, 25], "power": 2500, "hits": 1000000, "ticksToDecay": 500
    }));
    let RoomObject::Generic(g) = normalize("powerBank", &s, &ctx()).unwrap() else {
        panic!()
    };
    let d: Value = serde_json::from_str(&g.data).unwrap();
    assert_eq!(d["type"], "structure");
    assert_eq!(d["structureType"], "powerBank");
    assert_eq!(d["store"]["power"], 2500, "power 进官方 store 形态");
    assert_eq!(d["hits"], 1000000, "显式 hits 覆盖");
    assert_eq!(d["hitsMax"], 2_000_000, "hitsMax 恒 POWER_BANK_HITS");
    assert_eq!(d["decayTime"], 600, "相对寿命 → ctx.time(100)+500");
    assert!(d.get("ticksToDecay").is_none() && d.get("power").is_none());

    // 缺 ticksToDecay 默认 POWER_BANK_DECAY（适配器契约；RUIN-002 夹具
    // 形态——vanilla 下该银行 5000 拍后衰变移除）。
    let s = spec(json!({ "pos": [25, 25], "power": 100, "hits": 100 }));
    let RoomObject::Generic(g) = normalize("powerBank", &s, &ctx()).unwrap() else {
        panic!()
    };
    let d: Value = serde_json::from_str(&g.data).unwrap();
    assert_eq!(
        d["decayTime"],
        100 + 5000,
        "缺省 = ctx.time+POWER_BANK_DECAY"
    );
    assert_eq!(d["hits"], 100);

    // 缺省 hits 用 POWER_BANK_HITS、缺省 power 为 0。
    let s = spec(json!({ "pos": [25, 25] }));
    let RoomObject::Generic(g) = normalize("powerBank", &s, &ctx()).unwrap() else {
        panic!()
    };
    let d: Value = serde_json::from_str(&g.data).unwrap();
    assert_eq!(d["hits"], 2_000_000);
    assert_eq!(d["store"]["power"], 0);
}

/// powerBank 放置拒绝负数、小数与超安全整数的 power/hits/ticksToDecay；
/// hits=0、绝对 decayTime 非法、spec 自带 store/owner 整单拒绝。
#[test]
fn power_bank_placement_rejects_invalid_numbers() {
    for key in ["power", "hits", "ticksToDecay"] {
        for value in [json!(-1), json!(1.5), json!("3"), json!(9.1e15)] {
            let s = spec(json!({ "pos": [25, 25], key: value.clone() }));
            let err = normalize("powerBank", &s, &ctx()).unwrap_err();
            assert!(err.contains("非负安全整数"), "{key}={value}: {err}");
        }
    }
    let err = normalize(
        "powerBank",
        &spec(json!({ "pos": [25, 25], "hits": 0 })),
        &ctx(),
    )
    .unwrap_err();
    assert!(err.contains("正整数"), "hits=0 无敌银行拒绝：{err}");
    // 换算溢出分支：ctx.time=100 + 9_007_199_254_740_991 超安全整数。
    let err = normalize(
        "powerBank",
        &spec(json!({ "pos": [25, 25], "ticksToDecay": 9_007_199_254_740_991_u64 })),
        &ctx(),
    )
    .unwrap_err();
    assert!(err.contains("安全整数范围"), "{err}");
    for bad in [json!(-5), json!(1.5), json!(9.1e15)] {
        let err = normalize(
            "powerBank",
            &spec(json!({ "pos": [25, 25], "decayTime": bad.clone() })),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.contains("非负安全整数"), "decayTime={bad}: {err}");
    }
    for key in ["store", "owner"] {
        let err = normalize(
            "powerBank",
            &spec(json!({ "pos": [25, 25], key: json!({"power": 1}) })),
            &ctx(),
        )
        .unwrap_err();
        assert!(err.contains(key), "spec 键 {key} 整单拒绝：{err}");
    }
}

/// deposit 放置拒绝携带 structureType（混合类型串扰：type_name_of 优先
/// 读 structureType 会让"deposit"串享 powerBank 反射，对抗评审 P2-5）。
#[test]
fn deposit_placement_rejects_structure_type_key() {
    let err = normalize(
        "deposit",
        &spec(json!({ "pos": [1, 1], "structureType": "powerBank" })),
        &ctx(),
    )
    .unwrap_err();
    assert!(err.contains("structureType"), "{err}");
}

/// 相对时钟在输入值和加法结果两处校验，缺省与零仍保持原语义。
#[test]
fn deposit_rejects_invalid_numbers_and_deadline_overflow() {
    for key in [
        "cooldown",
        "ticksToDecay",
        "cooldownTime",
        "decayTime",
        "harvested",
    ] {
        for value in [
            json!(-1),
            json!(1.5),
            json!("10"),
            Value::Null,
            json!(u64::MAX),
            json!(9_007_199_254_740_992u64),
        ] {
            let mut spec = json!({"pos":[20,20]});
            spec[key] = value;
            assert!(
                normalize("deposit", &spec, &ctx()).is_err(),
                "{key}: {spec}"
            );
        }
    }
    for key in ["cooldown", "ticksToDecay"] {
        let mut spec = json!({"pos":[20,20]});
        spec[key] = json!(crate::domain::deposit::MAX_SAFE_INTEGER);
        assert!(
            normalize("deposit", &spec, &ctx()).is_err(),
            "有效输入换算后超限"
        );
        let mut overflow_ctx = ctx();
        overflow_ctx.time = u64::MAX;
        spec[key] = json!(1);
        assert!(
            normalize("deposit", &spec, &overflow_ctx).is_err(),
            "u64 加法溢出"
        );
    }
    let RoomObject::Generic(g) = normalize(
        "deposit",
        &json!({"pos":[20,20],
        "cooldown":0,"ticksToDecay":0,"harvested":0}),
        &ctx(),
    )
    .unwrap() else {
        panic!()
    };
    let data: Value = serde_json::from_str(&g.data).unwrap();
    assert_eq!(data["cooldownTime"], 100);
    assert_eq!(data["decayTime"], 100);
}
