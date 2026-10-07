//! Mod 整槽执行接线：可信 realm 组合处理器，native 经普通 op 进入
//! link 候选；Mod 状态统一发布、失败回滚，未接线的作用域装配即拒。

use std::path::PathBuf;

use serde_json::Value;

use super::*;
use kk_mod_host::{RegisterOptions, discover_packages};

/// 写一个临时 Mod 根（单包），返回根路径。
fn write_mod_root(label: &str, server_js: &str, manifest: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "kk-mod-settle-tests-{}-{}",
        label,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let pkg = dir.join("settle-mod");
    std::fs::create_dir_all(&pkg).expect("建包目录");
    let manifest =
        manifest.unwrap_or(r#"{"id":"settle-mod","version":"1.0.0","apiVersion":"^1.0.0"}"#);
    std::fs::write(pkg.join("mod.json"), manifest).expect("写 manifest");
    std::fs::write(pkg.join("server.js"), server_js).expect("写入口");
    dir
}

fn spec() -> Value {
    json!({
        "players": ["p1"],
        "rooms": [{ "name": "W1N1", "rcl": 6, "owner": "p1" }],
    })
}

fn engine_with_link(mods_root: &std::path::Path) -> anyhow::Result<EmbedEngine> {
    let packages = discover_packages(mods_root)?;
    let mut e = EmbedEngine::new();
    e.create_shard_with_mods(&spec(), &packages, RegisterOptions::default())?;
    e.place_object(
        "W1N1",
        "structure",
        &json!({ "pos": [25, 25], "structureType": "link", "owner": "p1", "store": {"energy": 100} }),
    )?;
    Ok(e)
}

/// link tick 槽的 after 处理器累积状态：每 tick 每 link 计数一次，
/// 状态跨 tick 提交续传。
pub(super) fn c_mod_settle_state_accumulates_impl() -> anyhow::Result<()> {
    let root = write_mod_root(
        "tally",
        r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.handlers.after('builtin:link.tick', { id: 'tally' }, function tally(ctx) {
    const count = (ctx.state && ctx.state.count) || 0;
    ctx.state = { count: count + 1, room: ctx.room, tick: ctx.tick };
  });
  api.rules.define('settle-mod:display-cap', {
    type: 'i32', min: 1, max: 10000, default: 100,
    consumers: ['settle-mod:handler.tally'],
  });
}
"#,
        None,
    );
    let mut e = engine_with_link(&root)?;
    e.tick(3)?;
    let state = e.engine_mod_state("settle-mod");
    assert_eq!(
        state["count"],
        serde_json::json!(3),
        "3 tick × 1 link 的 after 计数；实得 {state}"
    );
    assert_eq!(state["room"], serde_json::json!("W1N1"));
    assert_eq!(state["tick"], serde_json::json!(2), "末次处理的 tick 序号");
    // 再来两 tick：状态从已提交值续传。
    e.tick(2)?;
    let state = e.engine_mod_state("settle-mod");
    assert_eq!(state["count"], serde_json::json!(5));
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

/// 处理器异常使本轮候选失败：世界与 Mod 状态都保持上一 tick，
/// 重试同因失败（不静默跳过 Mod 后提交不同规则的世界）。
pub(super) fn c_mod_settle_failure_rolls_back_state_impl() -> anyhow::Result<()> {
    let root = write_mod_root(
        "flaky",
        r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.handlers.after('builtin:link.tick', { id: 'gate' }, function gate(ctx) {
    const count = (ctx.state && ctx.state.count) || 0;
    if (count >= 3) {
      throw new Error('状态门拒绝第 ' + (ctx.tick + 1) + ' tick');
    }
    ctx.state = { count: count + 1 };
  });
}
"#,
        None,
    );
    let mut e = engine_with_link(&root)?;
    e.tick(3)?;
    let before = e.engine_mod_state("settle-mod");
    assert_eq!(before["count"], serde_json::json!(3));
    let time_before = e.get_game_time();
    let generation_before = e.engine_generation();

    let failure = e.tick(1);
    assert!(failure.is_err(), "状态门在 count=3 后必须使 tick 失败");
    assert_eq!(
        e.engine_mod_state("settle-mod")["count"],
        serde_json::json!(3),
        "失败的候选不得改动已提交 Mod 状态"
    );
    assert_eq!(e.get_game_time(), time_before, "世界保持上一 tick");
    let failure = e.tick(1);
    assert!(failure.is_err(), "重试同样失败（同输入同规则）");
    assert_eq!(e.get_game_time(), time_before);
    assert_eq!(e.engine_generation(), generation_before, "失败轮不消耗代际");
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

/// replace 中心执行与 wrap 续行均可装配。native 被替换语义由
/// 引擎级 m2b 用例锁定，此处不重复断言冷却字段。
pub(super) fn c_mod_settle_replace_and_wrap_gate_impl() -> anyhow::Result<()> {
    let root = write_mod_root(
        "replacer",
        r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.handlers.replace('builtin:link.tick', { id: 'noop-tick' }, function noop(ctx) {
    ctx.state = { replaced: true };
  });
}
"#,
        None,
    );
    let mut e = engine_with_link(&root)?;
    e.tick(2)?;
    let state = e.engine_mod_state("settle-mod");
    assert_eq!(state["replaced"], serde_json::json!(true));
    let _ = std::fs::remove_dir_all(&root);

    let root = write_mod_root(
        "wrapper",
        r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.handlers.wrap('builtin:link.tick', { id: 'audit' }, function audit(ctx, next) { next(); });
}
"#,
        None,
    );
    let mut e = engine_with_link(&root)?;
    e.tick(2)?;
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}

/// 装配失败（房间域动作的执行通道未接线）时分片按 vanilla 建立：引擎
/// 保持默认计划、Mod 面不可用——与 create_shard 的“后续失败不恢复旧分片”
/// 语义一致，且 vanilla tick 仍可推进。
pub(super) fn c_mod_settle_registration_failure_impl() -> anyhow::Result<()> {
    let root = write_mod_root(
        "orphan",
        r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.intents.define('settle-mod:boom', { scope: 'room', args: { type: 'bool' }, process() {} });
}
"#,
        None,
    );
    let packages = discover_packages(&root)?;
    let mut e = EmbedEngine::new();
    let rejection = e.create_shard_with_mods(&spec(), &packages, RegisterOptions::default());
    assert!(
        rejection.is_err(),
        "房间域动作的执行通道未接线，装配必须显式拒绝"
    );
    let message = rejection.unwrap_err().to_string();
    assert!(
        message.contains("settle-mod:boom") && message.contains("房间"),
        "错误应指向动作键与未接线的作用域，实得 {message}"
    );
    e.tick(1)?;
    assert_eq!(e.get_game_time(), 1, "装配失败的 vanilla 分片仍可运行");
    let _ = std::fs::remove_dir_all(&root);
    Ok(())
}
