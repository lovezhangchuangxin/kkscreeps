//! 已注册处理器的调用、字段/state 读写和玩家模块输出。
use super::*;

#[test]
fn invoke_dispatches_handler_with_state_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_root("invoke");
    write_pkg(
        &root,
        "tally",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.handlers.after('builtin:link.tick', { id: 'tally' }, function tally(ctx) {
    const count = (ctx.state && ctx.state.count) || 0;
    ctx.state = { count: count + 1, room: ctx.room, tick: ctx.tick };
    return 'seen';
  });
  api.handlers.after('builtin:link.tick', { id: 'async-bad' }, function bad() {
    return { then() {} };
  });
  api.handlers.after('builtin:link.tick', { id: 'throwing' }, function boom() {
    throw new Error('处理器抛错');
  });
}
"#,
        )],
    );
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    let registered = realm
        .register(&packages, RegisterOptions::default())
        .expect("可注册");

    let mut state = serde_json::Value::Null;
    for tick in [7u64, 8] {
        let ctx = json!({"tick": tick, "room": "E1N1", "state": state});
        let outcome = realm
            .invoke("tally:handler.tally", ctx)
            .expect("处理器可调用");
        state = outcome["state"].clone();
        assert_eq!(outcome["result"], json!("seen"));
        assert_eq!(state["count"], json!(tick - 6), "状态在调用间由调用方续传");
        assert_eq!(state["room"], json!("E1N1"));
    }

    let promise = realm
        .invoke(
            "tally:handler.async-bad",
            json!({"tick": 1, "room": "E1N1", "state": null}),
        )
        .expect_err("thenable 结果必须被拒");
    assert!(promise.to_string().contains("同步完成"), "实得 {promise}");
    let thrown = realm
        .invoke(
            "tally:handler.throwing",
            json!({"tick": 1, "room": "E1N1", "state": null}),
        )
        .expect_err("处理器异常必须传播");
    assert!(thrown.to_string().contains("处理器抛错"), "实得 {thrown}");
    let unknown = realm
        .invoke("tally:handler.ghost", json!({}))
        .expect_err("未知绑定必须失败");
    assert!(
        unknown.to_string().contains("没有注册函数"),
        "实得 {unknown}"
    );
    assert!(
        registered.executables.contains(&key("tally:handler.tally")),
        "派发键来自密封绑定表"
    );
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn invoke_null_result_and_state_clear() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_root("invoke-null");
    write_pkg(
        &root,
        "nullable",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.handlers.after('builtin:link.tick', { id: 'echo' }, function echo(ctx) {
    if (ctx.tick === 2) {
      ctx.state = null;
      return null;
    }
    ctx.state = { seen: ctx.tick };
    return null;
  });
}
"#,
        )],
    );
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    realm
        .register(&packages, RegisterOptions::default())
        .expect("可注册");
    let out = realm
        .invoke(
            "nullable:handler.echo",
            json!({"tick": 1, "room": "W1N1", "state": null}),
        )
        .expect("null 返回可用");
    assert_eq!(out["result"], serde_json::Value::Null);
    assert_eq!(out["state"]["seen"], serde_json::json!(1));
    let cleared = realm
        .invoke(
            "nullable:handler.echo",
            json!({"tick": 2, "room": "W1N1", "state": json!({"seen": 1})}),
        )
        .expect("清空调用可用");
    assert_eq!(cleared["state"], serde_json::Value::Null, "写 null 即清空");
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

/// 动作处理链的 target 往返：ctx.target.fields 原地改写后随结果返回；
/// 未触碰（undefined）与显式 null 都归一为 null（不改动）。
#[test]
fn invoke_target_fields_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let root = temp_root("invoke-target");
    write_pkg(
        &root,
        "bump",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  const thing = api.objects.define('bump:thing', {
    version: 1,
    playerClass: 'Thing',
    ownerField: 'owner',
    positionFields: ['x', 'y'],
    fields: {
      owner: { type: 'string', maxBytes: 128, nullable: true },
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'i32', default: 0, visibility: 'public' },
    },
  });
  api.intents.define('bump:bump', {
    scope: 'object',
    target: thing,
    args: { type: 'record', fields: { amount: { type: 'i32', min: 1, max: 10 } } },
    process(ctx) {
      ctx.target.fields.value += ctx.args.amount;
      ctx.state = { bumps: (ctx.state && ctx.state.bumps || 0) + 1 };
    },
  });
}
"#,
        )],
    );
    let packages = discover_packages(&root).expect("可装载");
    let mut realm = ModRealm::new().expect("realm 可创建");
    realm
        .register(&packages, RegisterOptions::default())
        .expect("可注册");
    let ctx = json!({
        "tick": 3,
        "room": "W1N1",
        "state": null,
        "target": { "id": "kk0000000000000011", "fields": {
            "owner": "p1", "x": 10, "y": 12, "value": 5
        } },
        "args": { "amount": 4 },
    });
    let out = realm
        .invoke("bump:bump.process", ctx)
        .expect("动作处理可调用");
    assert_eq!(
        out["target"]["value"],
        serde_json::json!(9),
        "target 改写随结果返回"
    );
    assert_eq!(
        out["target"]["owner"],
        serde_json::json!("p1"),
        "未触碰字段保持"
    );
    assert_eq!(out["state"]["bumps"], serde_json::json!(1));
    // 无 target 的处理器（link 槽桥形态）：target 归一 null。
    let out = realm.invoke(
        "bump:bump.process",
        json!({ "tick": 4, "room": "W1N1", "state": null,
                "target": { "id": "kk0000000000000011", "fields": { "owner": "p1", "x": 10, "y": 12, "value": 1 } },
                "args": { "amount": 0 } }),
    );
    // amount 0 低于 schema 下限不影响 dispatch 层——参数校验属宿主结算侧。
    assert!(out.is_ok(), "dispatch 不做参数校验：{:?}", out.err());
    let out = out.unwrap();
    assert_eq!(
        out["target"]["value"],
        serde_json::json!(1),
        "amount 0 仍执行加法"
    );
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

#[test]
fn player_modules_exported_in_dependency_order() {
    let (_realm, registered) = register_fixtures(RegisterOptions::default());
    // 声明 player 面的包依依赖拓扑序导出模块集（demo-base 在前）。
    let names: Vec<&str> = registered
        .player_modules
        .iter()
        .map(|m| m.namespace.as_str())
        .collect();
    assert_eq!(names, vec!["demo-base", "demo-counter"]);
    let module = registered
        .player_modules
        .iter()
        .find(|m| m.namespace == "demo-counter")
        .unwrap();
    assert_eq!(module.entry, "player.js");
    // 包快照内的 UTF-8 文件全部可用（mod.json/server.js/util 也在其中，
    // 玩家模块按需导入；入口不需要的文件不会装载）。
    assert!(module.files.contains_key("player.js"));
    assert!(module.files.contains_key("util/label.js"));
    assert!(module.files["player.js"].contains("api.objects.bind('demo-counter:counter'"));
    // 玩家面目录：FIND 分配与必需绑定集来自同一冻结计划；两类型按键序
    // 分配相邻常量。
    let surface = kk_mod_api::player_surface::PlayerSurface::from_plan(&registered.plan)
        .expect("夹具计划可导出玩家面");
    assert_eq!(surface.types.len(), 2);
    assert_eq!(surface.types[0].key.as_str(), "demo-base:widget");
    assert_eq!(
        surface.types[0].find_const,
        kk_mod_api::player_surface::MOD_FIND_BASE
    );
    assert_eq!(surface.types[1].key.as_str(), "demo-counter:counter");
    assert_eq!(
        surface.types[1].find_const,
        kk_mod_api::player_surface::MOD_FIND_BASE + 1
    );
    assert_eq!(surface.types[1].player_class, "Counter");
    assert_eq!(
        surface.intents,
        vec![key("demo-base:recharge"), key("demo-counter:increment")],
        "夹具声明的 Mod 意图进入玩家 emit 目录"
    );
}

// ---------------------------------------------------------------------------
// wrap 续行：JS 处理器经 reentrant op 同步进入内层链。
//

#[test]
fn runtime_fault_policy_does_not_change_frozen_plan_identity() {
    let (_, original) = register_fixtures(RegisterOptions::default());
    let (_, configured) = register_fixtures(RegisterOptions {
        fault_disable_threshold: std::num::NonZeroU32::new(2),
        ..Default::default()
    });
    assert_eq!(original.plan.hash(), configured.plan.hash());
    assert_eq!(original.contributions, configured.contributions);
    assert_eq!(original.executables, configured.executables);
}
