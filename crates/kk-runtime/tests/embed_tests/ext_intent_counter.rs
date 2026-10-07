//! counter 对象与动作全链：管理命令创建 Mod 对象（默认值落位、
//! 校验拒绝）、玩家面原语发射扩展意图、结算侧校验与自有槽执行链
//! （process 中心 + after 处理器 + Mod 状态）、权限与异常回滚、
//! 未知键/越界参数丢弃、双通道一致性（Mod 对象不进玩家投影）。

use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::*;
use kk_mod_host::{RegisterOptions, discover_packages};

fn spec() -> Value {
    json!({
        "players": ["p1", "p2"],
        "rooms": [{ "name": "W1N1", "rcl": 3, "owner": "p1" }],
    })
}

pub(super) fn chain_engine(
    label: &str,
    server_js: &str,
    configs: Option<Value>,
) -> anyhow::Result<EmbedEngine> {
    chain_engine_with_player(label, server_js, PLAYER_JS, configs)
}

/// 同 chain_engine 但保留包目录并返回包表，供冷恢复用同包重装。
pub(super) fn chain_engine_kept(
    label: &str,
    server_js: &str,
) -> anyhow::Result<(EmbedEngine, BTreeMap<String, kk_mod_host::ModPackage>)> {
    let dir = std::env::temp_dir().join(format!(
        "kk-c3-chain-tests-{}-{}",
        label,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pkg = dir.join("chainmod");
    std::fs::create_dir_all(&pkg).expect("建包目录");
    std::fs::write(
        pkg.join("mod.json"),
        r#"{"id":"chainmod","version":"1.0.0","apiVersion":"^1.0.0","player":"player.js"}"#,
    )
    .expect("写 manifest");
    std::fs::write(pkg.join("server.js"), server_js).expect("写入口");
    std::fs::write(pkg.join("player.js"), PLAYER_JS).expect("写玩家模块");
    let packages = discover_packages(&dir)?;
    let mut e = EmbedEngine::new();
    e.create_shard_with_mods(&spec(), &packages, RegisterOptions::default())?;
    Ok((e, packages))
}

/// counter 的玩家面模块：类/getter/emit 与查询常量（FIND/LOOK）。
pub(super) const PLAYER_JS: &str = r#"
export default function install(api) {
  const { RoomObject } = api.builtins;
  class Counter extends RoomObject {
    get value() { return api.view.field(this, 'value'); }
    get owner() { return api.view.field(this, 'owner'); }
    increment(amount = 1) {
      return api.intents.emit('chainmod:increment', this, { amount });
    }
  }
  api.objects.bind('chainmod:counter', Counter);
  api.globals.define('Counter', Counter);
  const q = api.queries.install('chainmod:counter');
  api.globals.define('FIND_COUNTERS', q.find);
  api.globals.define('LOOK_COUNTERS', q.look);
}
"#;

fn chain_engine_with_player(
    label: &str,
    server_js: &str,
    player_js: &str,
    configs: Option<Value>,
) -> anyhow::Result<EmbedEngine> {
    let dir = std::env::temp_dir().join(format!(
        "kk-c3-chain-tests-{}-{}",
        label,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pkg = dir.join("chainmod");
    std::fs::create_dir_all(&pkg).expect("建包目录");
    std::fs::write(
        pkg.join("mod.json"),
        r#"{"id":"chainmod","version":"1.0.0","apiVersion":"^1.0.0","player":"player.js"}"#,
    )
    .expect("写 manifest");
    std::fs::write(pkg.join("server.js"), server_js).expect("写入口");
    std::fs::write(pkg.join("player.js"), player_js).expect("写玩家模块");
    let mut options = RegisterOptions::default();
    if let Some(config) = configs {
        options.configs.insert("chainmod".into(), config);
    }
    let packages = discover_packages(&dir)?;
    let mut e = EmbedEngine::new();
    e.create_shard_with_mods(&spec(), &packages, options)?;
    let _ = std::fs::remove_dir_all(&dir);
    Ok(e)
}

/// counter 包：类型 + increment 动作（process 更新字段）+ after 审计
/// （Mod 状态计数）。
pub(super) const CHAIN_JS: &str = r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  const counter = api.objects.define('chainmod:counter', {
    version: 1,
    playerClass: 'Counter',
    visibility: 'room-visible',
    ownerField: 'owner',
    positionFields: ['x', 'y'],
    fields: {
      owner: { type: 'string', maxBytes: 128, nullable: true },
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'i32', default: 0, visibility: 'public' },
    },
  });
  api.intents.define('chainmod:increment', {
    scope: 'object',
    target: counter,
    args: { type: 'record', fields: { amount: { type: 'i32', min: 1, max: 10 } } },
    process(ctx) {
      ctx.target.fields.value += ctx.args.amount;
    },
  });
  api.handlers.after('chainmod:increment.slot', { id: 'audit' }, function audit(ctx) {
    const seen = (ctx.state && ctx.state.seen) || 0;
    ctx.state = { seen: seen + 1 };
  });
}
"#;

pub(super) fn place_counter(e: &mut EmbedEngine, owner: &str) -> anyhow::Result<String> {
    e.place_mod_object(
        "W1N1",
        "chainmod:counter",
        &json!({ "owner": owner, "x": 10, "y": 12 }),
    )
}

pub(super) fn value_of(e: &EmbedEngine, id: &str) -> Value {
    e.get_object(id).unwrap()["fields"].clone()
}

/// 管理创建 + 玩家发射 + 结算执行 + after 审计全链；跨 tick 续传。
pub(super) fn counter_full_chain_impl() -> anyhow::Result<()> {
    let mut e = chain_engine("full", CHAIN_JS, None)?;
    let id = place_counter(&mut e, "p1")?;
    // 创建读面：诊断快照带类型键与字段；默认 value=0 已落位。
    let snap = e.get_object(&id).unwrap();
    assert_eq!(snap["kind"], json!("mod"));
    assert_eq!(snap["typeKey"], json!("chainmod:counter"));
    assert_eq!(snap["fields"]["value"], json!(0), "无默认值字段落默认");
    assert_eq!(snap["pos"]["x"], json!(10));

    // 玩家发射 → 结算 → 值变化。
    let run = e.run_player(
        "p1",
        &format!(r#"__kkEmitExtensionIntent('chainmod:increment', '{id}', {{ amount: 3 }}); 'ok'"#),
    )?;
    assert_eq!(run.expr["value"], json!("ok"), "原语可用：{:?}", run.expr);
    assert_eq!(value_of(&e, &id)["value"], json!(3), "process 写入生效");
    assert_eq!(
        e.engine_mod_state("chainmod")["seen"],
        json!(1),
        "after 审计跑 Mod 状态"
    );

    // 下一 tick 再加：对象字段与状态从已提交值续传。
    e.run_player(
        "p1",
        &format!(r#"__kkEmitExtensionIntent('chainmod:increment', '{id}', {{ amount: 7 }}); 'ok'"#),
    )?;
    assert_eq!(value_of(&e, &id)["value"], json!(10), "跨 tick 续传");
    assert_eq!(e.engine_mod_state("chainmod")["seen"], json!(2));
    Ok(())
}

/// 校验矩阵：非归属/越界参数/未知键/非 Mod 目标全部丢弃且结算成功；
/// 同键两次发射 last-wins。
pub(super) fn counter_gates_impl() -> anyhow::Result<()> {
    let mut e = chain_engine("gates", CHAIN_JS, None)?;
    let id = place_counter(&mut e, "p1")?;
    let creep = e.place_object(
        "W1N1",
        "creep",
        &json!({ "pos": [20, 20], "name": "w1", "owner": "p1", "body": ["move"], "ageTime": 999999 }),
    )?;
    // 非归属（p2 发射）。
    e.run_player(
        "p2",
        &format!(r#"__kkEmitExtensionIntent('chainmod:increment', '{id}', {{ amount: 5 }}); 'ok'"#),
    )?;
    assert_eq!(value_of(&e, &id)["value"], json!(0), "非归属丢弃");
    assert_eq!(
        e.engine_mod_state("chainmod"),
        json!(null),
        "未进链不跑 after"
    );
    // 越界参数（11 > max 10）与缺字段。
    e.run_player(
        "p1",
        &format!(
            r#"__kkEmitExtensionIntent('chainmod:increment', '{id}', {{ amount: 11 }}); 'ok'"#
        ),
    )?;
    e.run_player(
        "p1",
        &format!(r#"__kkEmitExtensionIntent('chainmod:increment', '{id}', {{}}); 'ok'"#),
    )?;
    // 未知意图键与非 Mod 目标。
    e.run_player(
        "p1",
        &format!(r#"__kkEmitExtensionIntent('chainmod:ghost', '{id}', {{}}); 'ok'"#),
    )?;
    e.run_player(
        "p1",
        &format!(
            r#"__kkEmitExtensionIntent('chainmod:increment', '{creep}', {{ amount: 1 }}); 'ok'"#
        ),
    )?;
    assert_eq!(value_of(&e, &id)["value"], json!(0), "全部丢弃");
    // 同键两次发射：last-wins（+2 后 +9 → 9）。
    e.run_player(
        "p1",
        &format!(
            r#"__kkEmitExtensionIntent('chainmod:increment', '{id}', {{ amount: 2 }});
               __kkEmitExtensionIntent('chainmod:increment', '{id}', {{ amount: 9 }}); 'ok'"#
        ),
    )?;
    assert_eq!(value_of(&e, &id)["value"], json!(9), "last-wins");
    assert_eq!(e.engine_mod_state("chainmod")["seen"], json!(1), "链恰一次");
    Ok(())
}

/// process 异常使候选失败：世界、对象字段与 Mod 状态保持上一 tick，
/// 重试同因失败。
pub(super) fn counter_failure_rolls_back_impl() -> anyhow::Result<()> {
    let server_js = r#"
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
    },
  });
  api.intents.define('chainmod:increment', {
    scope: 'object', target: counter,
    args: { type: 'record', fields: { amount: { type: 'i32', min: 1, max: 10 } } },
    process(ctx) {
      ctx.target.fields.value += ctx.args.amount;
      if (ctx.target.fields.value >= 5) throw new Error('counter 上限拒绝');
    },
  });
}
"#;
    let mut e = chain_engine("flaky", server_js, None)?;
    let id = place_counter(&mut e, "p1")?;
    let emit = |amount: i64| {
        format!(
            r#"__kkEmitExtensionIntent('chainmod:increment', '{id}', {{ amount: {amount} }}); 'ok'"#
        )
    };
    e.run_player("p1", &emit(3))?;
    assert_eq!(value_of(&e, &id)["value"], json!(3));
    let time_before = e.get_game_time();
    let generation_before = e.engine_generation();
    let failure = e.run_player("p1", &emit(4));
    assert!(failure.is_err(), "value 到 7 越过门槛 5，process 抛错");
    assert_eq!(value_of(&e, &id)["value"], json!(3), "对象字段保持");
    assert_eq!(e.get_game_time(), time_before, "世界保持上一 tick");
    let failure = e.run_player("p1", "''"); // 重试缓存轮（不再执行玩家代码）
    assert!(failure.is_err(), "重试同因失败");
    assert_eq!(e.engine_generation(), generation_before, "失败轮不消耗代际");
    e.discard_pending_round();
    // 放弃恢复后同输入再次失败（门在字段值上，提交值未变）。
    assert!(e.run_player("p1", &emit(4)).is_err());
    Ok(())
}

/// 创建命令的输入校验：未知类型、缺无默认字段、未声明字段、越界值、
/// 位置字段同步；vanilla 分片无 Mod 类型可用。
pub(super) fn counter_creation_gates_impl() -> anyhow::Result<()> {
    let mut e = chain_engine("create", CHAIN_JS, None)?;
    let err = e
        .place_mod_object("W1N1", "chainmod:ghost", &json!({}))
        .unwrap_err();
    assert!(err.to_string().contains("不在已装配计划"), "{err}");
    // 缺无默认字段（owner/x/y 任一）。
    let err = e
        .place_mod_object(
            "W1N1",
            "chainmod:counter",
            &json!({ "owner": "p1", "x": 1 }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("未提供且无默认值"), "{err}");
    // 未声明字段。
    let err = e
        .place_mod_object(
            "W1N1",
            "chainmod:counter",
            &json!({ "owner": "p1", "x": 1, "y": 1, "rogue": 2 }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("未在类型"), "{err}");
    // 越界位置（x=50）。
    let err = e
        .place_mod_object(
            "W1N1",
            "chainmod:counter",
            &json!({ "owner": "p1", "x": 50, "y": 1 }),
        )
        .unwrap_err();
    assert!(err.to_string().contains("schema"), "{err}");
    // 显式 value 覆盖默认。
    let id = e.place_mod_object(
        "W1N1",
        "chainmod:counter",
        &json!({ "owner": "p1", "x": 3, "y": 4, "value": 42 }),
    )?;
    assert_eq!(value_of(&e, &id)["value"], json!(42));
    // vanilla 分片：无 Mod 类型。
    let mut plain = EmbedEngine::new();
    plain.create_shard(&spec())?;
    let err = plain
        .place_mod_object("W1N1", "chainmod:counter", &json!({}))
        .unwrap_err();
    assert!(err.to_string().contains("不在已装配计划"), "{err}");
    Ok(())
}

/// 配置默认值到达创建面（value 默认读 api.config.start）；Mod 对象
/// 不进玩家投影（JSON 与 KKPV 两通道一致——玩家执行正常、对象不可见）。
pub(super) fn counter_config_and_projection_impl() -> anyhow::Result<()> {
    let config_js = r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  const start = (api.config && api.config.start) || 0;
  const counter = api.objects.define('chainmod:counter', {
    version: 1, playerClass: 'Counter', visibility: 'room-visible',
    ownerField: 'owner', positionFields: ['x', 'y'],
    fields: {
      owner: { type: 'string', maxBytes: 128, nullable: true },
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'i32', default: start, visibility: 'public' },
    },
  });
  api.intents.define('chainmod:increment', {
    scope: 'object', target: counter,
    args: { type: 'record', fields: { amount: { type: 'i32', min: 1, max: 10 } } },
    process(ctx) { ctx.target.fields.value += ctx.args.amount; },
  });
}
"#;
    let mut e = chain_engine("config", config_js, Some(json!({ "start": 5 })))?;
    let id = place_counter(&mut e, "p1")?;
    assert_eq!(value_of(&e, &id)["value"], json!(5), "配置默认值落位");
    // 两通道：counter 经玩家面目录进入投影，值经 getter
    // 读出，两通道一致。
    for channel in [false, true] {
        e.set_kkpv_channel(channel);
        let run = e.run_player(
            "p1",
            &format!(
                r#"(function() {{ const c = Game.getObjectById('{id}'); return c === null ? null : [c.value, c instanceof Counter, c.pos.roomName]; }})()"#
            ),
        )?;
        assert_eq!(
            run.expr["value"],
            json!([5, true, "W1N1"]),
            "counter 对玩家可见（{channel} 通道）：{:?}",
            run.expr
        );
    }
    let snapshot = e.get_object(&id).unwrap();
    assert_eq!(snapshot["fields"]["value"], json!(5));
    Ok(())
}

/// 创建与失败轮门的补充：builtin 类型键拒绝（builtin 类型有 Rust 权威
/// 表示，不得以 Mod 信封再造）；失败轮 pending 时创建被拒。
pub(super) fn counter_creation_and_round_gates_impl() -> anyhow::Result<()> {
    let mut e = chain_engine("roundgate", CHAIN_JS, None)?;
    // builtin 命名空间类型键：管理面拒绝。
    let err = e
        .place_mod_object(
            "W1N1",
            "builtin:link",
            &json!({ "owner": "p1", "x": 1, "y": 1 }),
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("不在已装配计划"),
        "builtin 类型不得以 Mod 信封创建：{err}"
    );
    // 失败轮 pending：世界修改门拒绝创建。
    let id = place_counter(&mut e, "p1")?;
    let sabotage = r#"
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
    },
  });
  api.intents.define('chainmod:increment', {
    scope: 'object', target: counter,
    args: { type: 'record', fields: { amount: { type: 'i32', min: 1, max: 10 } } },
    process(ctx) { throw new Error('一轮即炸'); },
  });
}
"#;
    // 用同一引擎的失败轮：直接制造结算失败（settle_external 不可用——
    // 经玩家发射触发 process 异常）。
    let _ = sabotage;
    let emit_fail =
        format!(r#"__kkEmitExtensionIntent('chainmod:increment', '{id}', {{ amount: 1 }}); 'ok'"#);
    // CHAIN_JS 的 process 正常；改用注入房间故障钩子制造失败轮。
    e.fail_next_settle_for_test();
    let failure = e.run_player("p1", &emit_fail);
    assert!(failure.is_err(), "注入的结算失败");
    assert!(e.has_pending_round(), "失败轮已缓存");
    let err = e
        .place_mod_object(
            "W1N1",
            "chainmod:counter",
            &json!({ "owner": "p2", "x": 8, "y": 8 }),
        )
        .unwrap_err();
    assert!(
        err.to_string().contains("被拒"),
        "失败轮 pending 时创建必须被拒：{err}"
    );
    e.discard_pending_round();
    let id2 = e.place_mod_object(
        "W1N1",
        "chainmod:counter",
        &json!({ "owner": "p2", "x": 8, "y": 8 }),
    )?;
    assert_ne!(id, id2, "放弃恢复后创建恢复");
    Ok(())
}

/// 只含 link 处理器（无 Mod 意图）的 Mod 分片：玩家面原语不装配——
/// 门控按"计划含 Mod 意图"推导，不是按"是否 Mod 分片"。
pub(super) fn counter_link_only_shard_skips_primitive_impl() -> anyhow::Result<()> {
    let server_js = r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.handlers.after('builtin:link.tick', { id: 'audit' }, function audit(ctx) {
    ctx.state = { ticks: (ctx.state && ctx.state.ticks || 0) + 1 };
  });
}
"#;
    let mut e = chain_engine_with_player(
        "linkonly",
        server_js,
        "export default function install() {}",
        None,
    )?;
    e.run_player("p1", "typeof __kkEmitExtensionIntent")?;
    let logs = e.capture_console_logs("p1");
    let _ = logs;
    let run = e.run_player("p1", "typeof __kkEmitExtensionIntent")?;
    assert_eq!(
        run.expr["value"],
        json!("undefined"),
        "无 Mod 意图的分片不装配原语：{:?}",
        run.expr
    );
    Ok(())
}

/// 压制包：increment 声明被 reset 压制（SDK suppressedBy），reset 把
/// value 置为 mark（G07.b）。
const SUPPRESSION_JS: &str = r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  const counter = api.objects.define('chainmod:counter', {
    version: 1,
    playerClass: 'Counter',
    visibility: 'room-visible',
    ownerField: 'owner',
    positionFields: ['x', 'y'],
    fields: {
      owner: { type: 'string', maxBytes: 128, nullable: true },
      x: { type: 'u32', min: 0, max: 49 },
      y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'i32', default: 0, visibility: 'public' },
    },
  });
  api.intents.define('chainmod:increment', {
    scope: 'object',
    target: counter,
    args: { type: 'record', fields: { amount: { type: 'i32', min: 1, max: 10 } } },
    suppressedBy: ['chainmod:reset'],
    process(ctx) {
      ctx.target.fields.value += ctx.args.amount;
    },
  });
  api.intents.define('chainmod:reset', {
    scope: 'object',
    target: counter,
    args: { type: 'record', fields: { mark: { type: 'i32', min: 0, max: 9 } } },
    process(ctx) {
      ctx.target.fields.value = ctx.args.mark;
    },
  });
}
"#;

/// SDK 压制声明端到端：齐发只执行压制者、压制者缺席则执行、压制按
/// 键存在性（压制者参数越界被丢弃仍压制）、跨对象隔离；两通道一致。
pub(super) fn counter_suppression_impl() -> anyhow::Result<()> {
    let (mut e, kept_packages) = chain_engine_kept("suppress", SUPPRESSION_JS)?;
    for channel in [false, true] {
        e.set_kkpv_channel(channel);
        let a = place_counter(&mut e, "p1")?;
        let b = place_counter(&mut e, "p1")?;

        // 齐发：increment 被 reset 压制，只有 reset 落地。发射原语恒返
        // true（记录即 OK——压制对玩家不可见，官方静默形态）。
        let run = e.run_player(
            "p1",
            &format!(
                r#"__kkEmitExtensionIntent('chainmod:increment', '{a}', {{ amount: 3 }}) &&
                   __kkEmitExtensionIntent('chainmod:reset', '{a}', {{ mark: 7 }})"#
            ),
        )?;
        assert_eq!(
            run.expr["value"],
            json!(true),
            "{channel}: 发射原语记录即返 OK"
        );
        assert_eq!(
            value_of(&e, &a)["value"],
            json!(7),
            "{channel}: 齐发只执行压制者"
        );

        // 压制者缺席：increment 照常执行。
        e.run_player(
            "p1",
            &format!(
                r#"__kkEmitExtensionIntent('chainmod:increment', '{a}', {{ amount: 3 }}); 'ok'"#
            ),
        )?;
        assert_eq!(
            value_of(&e, &a)["value"],
            json!(10),
            "{channel}: 无压制者则执行"
        );

        // 存在性语义：reset 参数越界被丢弃，increment 仍被压制。
        e.run_player(
            "p1",
            &format!(
                r#"__kkEmitExtensionIntent('chainmod:increment', '{a}', {{ amount: 3 }});
                   __kkEmitExtensionIntent('chainmod:reset', '{a}', {{ mark: 99 }}); 'ok'"#
            ),
        )?;
        assert_eq!(
            value_of(&e, &a)["value"],
            json!(10),
            "{channel}: 压制只看键存在不看压制者成败"
        );

        // 跨对象隔离：reset 发往另一对象不压制本对象的 increment。
        e.run_player(
            "p1",
            &format!(
                r#"__kkEmitExtensionIntent('chainmod:increment', '{a}', {{ amount: 2 }});
                   __kkEmitExtensionIntent('chainmod:reset', '{b}', {{ mark: 5 }}); 'ok'"#
            ),
        )?;
        assert_eq!(
            value_of(&e, &a)["value"],
            json!(12),
            "{channel}: 跨对象不压制"
        );
        assert_eq!(
            value_of(&e, &b)["value"],
            json!(5),
            "{channel}: 另一对象效果落地"
        );

        // 冷恢复：检查点恢复（带同包）后压制语义仍在（描述符在计划身份内）。
        let snapshot = e.checkpoint(serde_json::Value::Null)?;
        let mut restored = EmbedEngine::new();
        restored.restore_checkpoint(
            snapshot,
            Some((&kept_packages, &RegisterOptions::default())),
        )?;
        restored.set_kkpv_channel(channel);
        // 恢复后发射 increment(4)+reset(mark=99 越界)：压制语义保持时
        // increment 被 reset 的存在压制、reset 被 schema 丢弃 → 值不变
        //（12）；压制若在重装中丢失则 increment 执行 → 16。顺序敏感。
        restored.run_player(
            "p1",
            &format!(
                r#"__kkEmitExtensionIntent('chainmod:increment', '{a}', {{ amount: 4 }});
                   __kkEmitExtensionIntent('chainmod:reset', '{a}', {{ mark: 99 }}); 'ok'"#
            ),
        )?;
        assert_eq!(
            value_of(&restored, &a)["value"],
            json!(12),
            "{channel}: 冷恢复后压制语义保持（丢失则为 16）"
        );
    }
    Ok(())
}

/// 冻结校验经 SDK 面可见：压制动作不在计划（未知键）拒绝注册。
pub(super) fn counter_suppression_freeze_gates_impl() -> anyhow::Result<()> {
    let server_js = r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  const counter = api.objects.define('chainmod:counter', {
    version: 1, playerClass: 'Counter', visibility: 'room-visible',
    ownerField: 'owner', positionFields: ['x', 'y'],
    fields: {
      owner: { type: 'string', maxBytes: 128, nullable: true },
      x: { type: 'u32', min: 0, max: 49 }, y: { type: 'u32', min: 0, max: 49 },
      value: { type: 'i32', default: 0, visibility: 'public' },
    },
  });
  api.intents.define('chainmod:increment', {
    scope: 'object',
    target: counter,
    args: { type: 'record', fields: { amount: { type: 'i32', min: 1, max: 10 } } },
    suppressedBy: ['chainmod:ghost'],
    process(ctx) { ctx.target.fields.value += ctx.args.amount; },
  });
}
"#;
    let result = chain_engine("suppress-bad", server_js, None);
    let error = format!("{:#}", result.err().expect("未知压制动作必须拒绝注册"));
    // chain_engine 的清理只在成功路径执行；错误路径手工清临时目录。
    let _ = std::fs::remove_dir_all(std::env::temp_dir().join(format!(
        "kk-c3-chain-tests-suppress-bad-{}",
        std::process::id()
    )));
    assert!(error.contains("未知压制动作"), "冻结校验文案：{error}");
    Ok(())
}
