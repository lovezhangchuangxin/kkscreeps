//! 玩家面安装全链：FIND/LOOK 查询常量、玩家类装配与字段 getter、
//! 类方法 emit 进扩展意图、两通道一致、可见性裁切与装配门。

use serde_json::json;

use super::ext_intent_counter::{CHAIN_JS, chain_engine, place_counter, value_of};
use super::*;

/// 带归属裁切字段的 counter 变体：secret 仅归属者可见。
const SECRET_JS: &str = r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  const counter = api.objects.define('chainmod:counter', {
    version: 1, playerClass: 'Counter', visibility: 'room-visible',
    ownerField: 'owner', positionFields: ['x', 'y'],
    fields: {
      owner: { type: 'string', maxBytes: 128, nullable: true },
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'i32', default: 0, visibility: 'public' },
      secret: { type: 'i32', default: 42, visibility: 'owner' },
    },
  });
  api.intents.define('chainmod:increment', {
    scope: 'object', target: counter,
    args: { type: 'record', fields: { amount: { type: 'i32', min: 1, max: 10 } } },
    process(ctx) { ctx.target.fields.value += ctx.args.amount; },
  });
}
"#;

const SECRET_PLAYER_JS: &str = r#"
export default function install(api) {
  const { RoomObject } = api.builtins;
  class Counter extends RoomObject {
    get value() { return api.view.field(this, 'value'); }
    get secret() { return api.view.field(this, 'secret'); }
    increment(amount = 1) {
      return api.intents.emit('chainmod:increment', this, { amount });
    }
  }
  api.objects.bind('chainmod:counter', Counter);
  api.globals.define('Counter', Counter);
  const q = api.queries.install('chainmod:counter');
  api.globals.define('FIND_COUNTERS', q.find);
}
"#;

fn counter_expr(id: &str) -> String {
    format!(
        r#"(function() {{
            const room = Game.rooms.W1N1;
            const found = room.find(FIND_COUNTERS);
            const direct = Game.getObjectById('{id}');
            const look = room.lookForAt(LOOK_COUNTERS, 10, 12);
            return [
                found.length, found[0] === direct,
                direct instanceof Counter, direct.value, direct.owner,
                direct.pos.x, direct.pos.y, direct.pos.roomName,
                look.length, look[0] === direct,
            ];
        }})()"#
    )
}

/// FIND/LOOK 常量 + 类装配 + getter + 类方法 emit + 跨 tick 投影刷新。
pub(super) fn player_chain_full_impl() -> anyhow::Result<()> {
    let mut e = chain_engine("c4full", CHAIN_JS, None)?;
    let id = place_counter(&mut e, "p1")?;
    let run = e.run_player("p1", &counter_expr(&id))?;
    assert_eq!(
        run.expr["value"],
        json!([1, true, true, 0, "p1", 10, 12, "W1N1", 1, true]),
        "FIND/LOOK/类装配/getter 全链：{:?}",
        run.expr
    );
    // 类方法 emit → 结算 → 下一轮投影读到新值。
    let run = e.run_player("p1", &format!(r#"Game.getObjectById('{id}').increment(3)"#))?;
    assert_eq!(run.expr["value"], json!(0), "emit 返回 OK");
    assert_eq!(value_of(&e, &id)["value"], json!(3), "process 写入生效");
    let run = e.run_player("p1", &format!(r#"Game.getObjectById('{id}').value"#))?;
    assert_eq!(run.expr["value"], json!(3), "getter 读到当轮已提交值");
    Ok(())
}

/// 两通道一致：FIND/实例/getter/look 在 JSON 与 KKPV 通道同结果。
pub(super) fn dual_channel_parity_impl() -> anyhow::Result<()> {
    let mut e = chain_engine("c4parity", CHAIN_JS, None)?;
    let id = place_counter(&mut e, "p1")?;
    let mut results = Vec::new();
    for channel in [false, true] {
        e.set_kkpv_channel(channel);
        let run = e.run_player("p1", &counter_expr(&id))?;
        results.push(run.expr["value"].clone());
    }
    assert_eq!(results[0], results[1], "两通道玩家面一致");
    assert_eq!(
        results[0],
        json!([1, true, true, 0, "p1", 10, 12, "W1N1", 1, true])
    );
    Ok(())
}

/// 字段可见性裁切：public 对所有人，owner 仅归属者（两通道）。
pub(super) fn field_visibility_cut_impl() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("kk-c4-vis-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let pkg = dir.join("chainmod");
    std::fs::create_dir_all(&pkg)?;
    std::fs::write(
        pkg.join("mod.json"),
        r#"{"id":"chainmod","version":"1.0.0","apiVersion":"^1.0.0","player":"player.js"}"#,
    )?;
    std::fs::write(pkg.join("server.js"), SECRET_JS)?;
    std::fs::write(pkg.join("player.js"), SECRET_PLAYER_JS)?;
    let packages = kk_mod_host::discover_packages(&dir)?;
    let mut e = EmbedEngine::new();
    e.create_shard_with_mods(
        &json!({ "players": ["p1", "p2"], "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }] }),
        &packages,
        kk_mod_host::RegisterOptions::default(),
    )?;
    let _ = std::fs::remove_dir_all(&dir);
    let id = place_counter(&mut e, "p1")?;
    // p2 的 creep 授予房间视野（Mod 对象自身不授予视野——RoomVisible
    // 对象对看得见房间的他人导出）。
    e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [20, 20], "name": "w2", "owner": "p2", "body": ["move"], "ageTime": 999999 }),
    )?;
    for channel in [false, true] {
        e.set_kkpv_channel(channel);
        // 归属者：secret=42 可见。
        let run = e.run_player("p1", &format!(r#"Game.getObjectById('{id}').secret"#))?;
        assert_eq!(run.expr["value"], json!(42), "归属者读 secret（{channel}）");
        // 他人：secret 裁掉（getter 读 undefined），value 公开。
        let run = e.run_player(
            "p2",
            &format!(
                r#"(function() {{ const c = Game.getObjectById('{id}'); return [c.value, c.secret === undefined]; }})()"#
            ),
        )?;
        assert_eq!(
            run.expr["value"],
            json!([0, true]),
            "他人视角 owner 字段裁掉（{channel}）：{:?}",
            run.expr
        );
    }
    Ok(())
}

/// 装配门与防篡改：缺绑定失败、vanilla 分片无玩家面、全局不可改写、
/// 类方法 emit 过结算侧归属门。
pub(super) fn install_gates_impl() -> anyhow::Result<()> {
    // 玩家面类型未被任何 player.js 绑定 → 分片创建失败。
    let dir = std::env::temp_dir().join(format!("kk-c4-gate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let pkg = dir.join("chainmod");
    std::fs::create_dir_all(&pkg)?;
    std::fs::write(
        pkg.join("mod.json"),
        r#"{"id":"chainmod","version":"1.0.0","apiVersion":"^1.0.0","player":"player.js"}"#,
    )?;
    std::fs::write(pkg.join("server.js"), CHAIN_JS)?;
    std::fs::write(
        pkg.join("player.js"),
        "export default function install() {}",
    )?;
    let packages = kk_mod_host::discover_packages(&dir)?;
    let mut e = EmbedEngine::new();
    let err = e
        .create_shard_with_mods(
            &json!({ "players": ["p1"], "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }] }),
            &packages,
            kk_mod_host::RegisterOptions::default(),
        )
        .map(|_| ())
        .expect_err("缺绑定须使分片创建失败");
    assert!(
        err.to_string().contains("未绑定玩家面类型"),
        "错误指向缺绑定：{err}"
    );
    let _ = std::fs::remove_dir_all(&dir);

    // vanilla 分片：玩家面全局缺席。
    let mut plain = EmbedEngine::new();
    plain.create_shard(
        &json!({ "players": ["p1"], "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }] }),
    )?;
    let run = plain.run_player("p1", "typeof Counter + ':' + typeof FIND_COUNTERS")?;
    assert_eq!(run.expr["value"], json!("undefined:undefined"));

    // Mod 分片：安装全局不可改写/删除；原型补丁可见（官方生态形态）。
    let mut e = chain_engine("c4tamper", CHAIN_JS, None)?;
    let id = place_counter(&mut e, "p1")?;
    let run = e.run_player(
        "p1",
        r#"(function() {
            const before = globalThis.Counter;
            globalThis.Counter = 1;
            const afterAssign = globalThis.Counter === before;
            const delOk = delete globalThis.Counter;
            Counter.prototype.tag = 'patched';
            const patchVisible = Game.getObjectById === null ? false
                : (function() { return true; })();
            return [afterAssign, delOk, globalThis.Counter === before, patchVisible];
        })()"#,
    )?;
    assert_eq!(
        run.expr["value"],
        json!([true, false, true, true]),
        "安装全局不可改写（原型补丁不破坏装配）：{:?}",
        run.expr
    );
    // 类方法 emit 的归属门与扩展意图原语同口径：p2 经类方法发射被丢弃。
    e.run_player("p2", &format!(r#"Game.getObjectById('{id}').increment(5)"#))?;
    assert_eq!(
        value_of(&e, &id)["value"],
        json!(0),
        "非归属经类方法同样丢弃"
    );
    Ok(())
}

/// 服务器私有类型（server-only）不要求玩家绑定，也不进玩家投影。
pub(super) fn server_only_type_impl() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("kk-c4-srv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let pkg = dir.join("chainmod");
    std::fs::create_dir_all(&pkg)?;
    std::fs::write(
        pkg.join("mod.json"),
        r#"{"id":"chainmod","version":"1.0.0","apiVersion":"^1.0.0"}"#,
    )?;
    std::fs::write(
        pkg.join("server.js"),
        r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.objects.define('chainmod:ledger', {
    version: 1, playerClass: 'Ledger', visibility: 'server-only',
    fields: { total: { type: 'i64', min: 0, max: 1000000, default: 0, visibility: 'server-only' } },
  });
}
"#,
    )?;
    let packages = kk_mod_host::discover_packages(&dir)?;
    let mut e = EmbedEngine::new();
    e.create_shard_with_mods(
        &json!({ "players": ["p1"], "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }] }),
        &packages,
        kk_mod_host::RegisterOptions::default(),
    )?;
    let _ = std::fs::remove_dir_all(&dir);
    // 无 player.js 也能创建（server-only 免绑），对象可经管理面创建，
    // 但不进玩家投影。
    let id = e.place_mod_object("W1N1", "chainmod:ledger", &json!({ "total": 7 }))?;
    assert_eq!(e.get_object(&id).unwrap()["fields"]["total"], json!(7));
    let run = e.run_player("p1", &format!(r#"Game.getObjectById('{id}') === null"#))?;
    assert_eq!(
        run.expr["value"],
        json!(true),
        "server-only 类型不进玩家投影"
    );
    Ok(())
}

/// Float 域字段混合 backing（管理面整数 JSON 与小数）经写入口规范化后，
/// KKPV 列式编码不因列型混派失败（清偿 R3 P1 的 e2e 验收）。
pub(super) fn float_field_dual_channel_impl() -> anyhow::Result<()> {
    let dir = std::env::temp_dir().join(format!("kk-c4-f64-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let pkg = dir.join("chainmod");
    std::fs::create_dir_all(&pkg)?;
    std::fs::write(
        pkg.join("mod.json"),
        r#"{"id":"chainmod","version":"1.0.0","apiVersion":"^1.0.0","player":"player.js"}"#,
    )?;
    std::fs::write(
        pkg.join("server.js"),
        r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  const gauge = api.objects.define('chainmod:gauge', {
    version: 1, playerClass: 'Gauge', visibility: 'room-visible',
    ownerField: 'owner', positionFields: ['x', 'y'],
    fields: {
      owner: { type: 'string', maxBytes: 128, nullable: true },
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      ratio: { type: 'f64', min: 0, max: 100 },
    },
  });
}
"#,
    )?;
    std::fs::write(
        pkg.join("player.js"),
        r#"
export default function install(api) {
  class Gauge extends api.builtins.RoomObject {
    get ratio() { return api.view.field(this, 'ratio'); }
  }
  api.objects.bind('chainmod:gauge', Gauge);
}
"#,
    )?;
    let packages = kk_mod_host::discover_packages(&dir)?;
    let mut e = EmbedEngine::new();
    e.create_shard_with_mods(
        &json!({ "players": ["p1"], "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }] }),
        &packages,
        kk_mod_host::RegisterOptions::default(),
    )?;
    let _ = std::fs::remove_dir_all(&dir);
    // 同房同类型两个对象：整数 backing（json!(2)）与小数（2.5）——写入口
    // 规范化后同桶列型一致。
    let g1 = e.place_mod_object(
        "W1N1",
        "chainmod:gauge",
        &json!({ "owner": "p1", "x": 5, "y": 5, "ratio": 2 }),
    )?;
    let g2 = e.place_mod_object(
        "W1N1",
        "chainmod:gauge",
        &json!({ "owner": "p1", "x": 6, "y": 6, "ratio": 2.5 }),
    )?;
    for channel in [false, true] {
        e.set_kkpv_channel(channel);
        let run = e.run_player(
            "p1",
            &format!(
                r#"(function() {{ return [Game.getObjectById('{g1}').ratio, Game.getObjectById('{g2}').ratio]; }})()"#
            ),
        )?;
        let values = run.expr["value"].as_array().unwrap();
        let ratios: Vec<f64> = values.iter().map(|v| v.as_f64().unwrap()).collect();
        assert_eq!(
            ratios,
            vec![2.0, 2.5],
            "混合 backing 的 f64 字段两通道读值一致（{channel}）：{:?}",
            run.expr
        );
    }
    Ok(())
}
