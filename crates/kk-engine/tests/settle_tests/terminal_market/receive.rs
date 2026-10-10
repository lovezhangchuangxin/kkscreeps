//! 从父测试模块拆出的领域验收，复用原夹具。
use super::*;

/// 复验拒：库存不足时不转移、不挂冷却。
#[test]
fn send_recheck_resource_in_global_phase() {
    let mut f = MarketFixture::new();
    let from = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 100}));
    let _to = f.terminal("W5N1", "p2", json!({"energy": 500}));
    // 直注 stow 1000（>实际 100）：房间相校验同拒（本测试只锁 global 相）
    f.stow_send(from, "W5N1", "H", 1000);
    f.tick();

    assert_eq!(f.data_of(from).pointer("/cooldownTime"), Some(&json!(0)));
    assert_eq!(market(&f).transactions.len(), 0);
}

/// 接收方空间钳：目标只剩 500 空间 → amount 钳 500，成本按**钳后**量算
///（官方 :34-35 序），交易记钳后量。
#[test]
fn send_clamps_to_receiver_free_space() {
    let mut f = MarketFixture::new();
    let from = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 1000}));
    let to = f.terminal("W5N1", "p2", json!({"energy": 500, "H": 299000}));
    // 目标空余容量为 300000-(500+299000)=500。
    f.send_intent("p1", from, "W5N1", "H", 1000);

    let td = f.data_of(to);
    assert_eq!(
        td.pointer("/store/H"),
        Some(&json!(299500)),
        "钳后送达 500（299000+500）"
    );
    let fd = f.data_of(from);
    // 成本按钳后 500、环绕 range=3 算：ceil(500×(1−e^(−3/30)))=ceil(47.58)=48
    assert_eq!(fd.pointer("/store/energy"), Some(&json!(100000 - 48)));
    assert_eq!(
        fd.pointer("/store/H"),
        Some(&json!(500)),
        "发送方也只扣钳后量"
    );
    assert_eq!(market(&f).transactions[0].amount, 500, "交易记钳后量");
}

/// 直接保存资源足额、energy=cost-1 的请求，验证全局阶段仍拒绝费用不足。
/// 房间动作可能已消耗能量，所以不能只依赖提交意图时的检查。
#[test]
fn send_global_phase_energy_gate_rejects() {
    let mut f = MarketFixture::new();
    // cost(环绕 range=3, amount=1000)=96 → energy=95
    let from = f.terminal("W1N1", "p1", json!({"energy": 95, "H": 1000}));
    let _to = f.terminal("W5N1", "p2", json!({"energy": 500}));
    // 直注 stow（绕过房间相能量门——只锁全局相门）
    f.stow_send(from, "W5N1", "H", 1000);
    f.tick();

    let fd = f.data_of(from);
    assert_eq!(fd.pointer("/store/H"), Some(&json!(1000)), "能量不足不转移");
    assert_eq!(fd.pointer("/store/energy"), Some(&json!(95)), "能量不动");
    assert_eq!(fd.pointer("/cooldownTime"), Some(&json!(0)), "不挂冷却");
    assert_eq!(market(&f).transactions.len(), 0);
}

/// 发送 energy 时须同时持有 amount 和 cost（环绕 range=3 → cost=96），
/// 本例 1096 恰好成功，1095 拒绝。
#[test]
fn send_energy_type_amount_plus_cost_branch() {
    let mut f = MarketFixture::new();
    let from = f.terminal("W1N1", "p1", json!({"energy": 1096}));
    let to = f.terminal("W5N1", "p2", json!({"energy": 0}));
    f.send_intent("p1", from, "W5N1", "energy", 1000);

    let fd = f.data_of(from);
    assert_eq!(
        fd.pointer("/store/energy"),
        Some(&json!(0)),
        "1096−(1000+96)"
    );
    let td = f.data_of(to);
    assert_eq!(td.pointer("/store/energy"), Some(&json!(1000)), "送达 1000");
    assert_eq!(market(&f).transactions[0].resource_type, "energy");

    // 边界下侧：1095 拒（amount+cost 差 1）
    let mut f2 = MarketFixture::new();
    let from2 = f2.terminal("W1N1", "p1", json!({"energy": 1095}));
    let _to2 = f2.terminal("W5N1", "p2", json!({"energy": 0}));
    f2.send_intent("p1", from2, "W5N1", "energy", 1000);
    assert_eq!(
        f2.data_of(from2).pointer("/store/energy"),
        Some(&json!(1095)),
        "拒"
    );
    assert_eq!(f2.data_of(from2).pointer("/cooldownTime"), Some(&json!(0)));
}

/// 交易 description 中每个 < 都替换为 &lt;。来源：上游 global-intents/market.js。
#[test]
fn send_description_lt_escaping_in_transaction() {
    let mut f = MarketFixture::new();
    let from = f.terminal("W1N1", "p1", json!({"energy": 100000, "H": 1000}));
    let _to = f.terminal("W5N1", "p2", json!({"energy": 500}));
    // stow 带 description（绕 API 层——直注载荷形态与房间相 stow 相同）
    {
        f.engine
            .edit_object(from, |object| {
                let RoomObject::Generic(g) = object else {
                    panic!()
                };
                let mut v: Value = serde_json::from_str(&g.data).unwrap();
                v["send"] = json!({
                    "targetRoomName": "W5N1", "resourceType": "H", "amount": 100,
                    "description": "a<b<c",
                });
                g.data = v.to_string().into();
                Ok(())
            })
            .unwrap();
    }
    f.tick();
    let tx = &market(&f).transactions[0];
    assert_eq!(
        tx.description.as_deref(),
        Some("a&lt;b&lt;c"),
        "`<` 全量转义"
    );
}

/// Terminal 无控制器时保留容量；同主 RCL 5 无配额或异主时容量归 0，
/// 同主 RCL 8 时容量恢复为 300000。
#[test]
fn terminal_store_capacity_normalizes_by_rcl() {
    let mut f = MarketFixture::new();
    let t = f.terminal("W1N1", "p1", json!({"energy": 1000}));
    // 无 controller 房：terminal_capacity_tick 的 w.controller() None → 保持
    f.engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(
        f.data_of(t).pointer("/storeCapacity"),
        Some(&json!(300000)),
        "无 controller 保持放置值（官方 roomController 假门整段跳过）"
    );

    // 放 controller level 5 owner p1：terminal 允许数为 0 → 归 0
    f.engine
        .insert_object(
            "W1N1",
            RoomObject::Controller(kk_schema::room::ControllerObj {
                id: 0,
                x: 40,
                y: 40,
                level: 5,
                safe_mode: 0,
                safe_mode_available: 0,
                progress: 0,
                downgrade_time: None,
                owner: Some("p1".into()),
                owner_cleared: false,
                is_power_enabled: false,
                reservation: None,
                sign: None,
                upgrade_blocked: None,
                safe_mode_cooldown: None,
            }),
        )
        .unwrap();
    f.engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(
        f.data_of(t).pointer("/storeCapacity"),
        Some(&json!(0)),
        "RCL 5（terminal 配额 0）归 0"
    );

    // 异主：p2 主控（RCL 8）下 p1 的 terminal 归 0（owner 等值门判别）
    set_controller(&mut f, 8, "p2");
    f.engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(
        f.data_of(t).pointer("/storeCapacity"),
        Some(&json!(0)),
        "异主 controller（p2 RCL 8）下 p1 terminal 归 0"
    );

    // 同主 RCL 8：300000（level>0+owner 相等+配额 1 全真——保持/回升侧）
    set_controller(&mut f, 8, "p1");
    f.engine.tick_with_intents(IntentBatch::default()).unwrap();
    assert_eq!(
        f.data_of(t).pointer("/storeCapacity"),
        Some(&json!(300000)),
        "同主 RCL 8 回升 300000（全真路径）"
    );
}

/// 改 W1N1 controller 的 level/owner（capacity 测试的三态构造）。
fn set_controller(f: &mut MarketFixture, level: u8, owner: &str) {
    let cid = f
        .engine
        .world()
        .room("W1N1")
        .unwrap()
        .objects
        .keys()
        .copied()
        .find(|k| {
            f.engine
                .world()
                .get(*k)
                .is_some_and(|(_, o)| matches!(o, RoomObject::Controller(_)))
        })
        .expect("controller 不存在");
    f.engine
        .edit_object(cid, |object| {
            if let Some(RoomObject::Controller(c)) = Some(object) {
                c.level = level;
                c.owner = Some(owner.into());
            }
            Ok(())
        })
        .unwrap();
}
