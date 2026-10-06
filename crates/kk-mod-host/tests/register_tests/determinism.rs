//! G22.b 玩法 realm 确定性时间/随机协议：流推导、轮重置、域独立（含
//! wrap 链跨层与跨包）、init 流、Date 映射、原生 Date 不可取回与注册
//! 窗外随机报错。
use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use kk_mod_host::{ChainContext, ChainHost, ChainTarget, ChainWrite};

/// 写一个记录随机/时间产物的包：模块顶层捕获 init 流；处理器把每次
/// 调用的抽值与时钟写入 state（经 result 透传，便于逐调用断言）。
/// sheller 是 wrap 处理器：next() 内层（native 中心与 after 处理器）
/// 执行前后各抽一次，用于钉死 wrap 链跨层流域隔离。
fn write_roller(root: &Path, name: &str) -> PathBuf {
    write_pkg(
        root,
        name,
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
const initDraw = Math.random();
const initNow = Date.now();
export default function register(api) {
  api.handlers.after('builtin:link.tick', { id: 'tally' }, function tally(ctx) {
    ctx.state = {
      draw: Math.random(),
      now: Date.now(),
      newDate: new Date().getTime(),
      temporalGone: typeof Temporal === 'undefined',
      initDraw, initNow,
    };
  });
  api.handlers.after('builtin:link.tick', { id: 'mate' }, function mate(ctx) {
    ctx.state = { draw: Math.random() };
  });
  api.handlers.wrap('builtin:link.tick', { id: 'sheller' }, function sheller(ctx, next) {
    const before = Math.random();
    next();
    ctx.state = { before, after: Math.random() };
  });
}
"#,
        )],
    )
}

fn register_roller(seed: u64, root: &Path) -> ModRealm {
    let packages = discover_packages(root).expect("可装载");
    let options = RegisterOptions::default();
    let mut realm = ModRealm::with_world_seed(seed, &options).expect("realm 可创建");
    realm.register(&packages, options).expect("可注册");
    realm
}

/// 另一个包的处理器也抽随机：跨命名空间互不挤占序列。
fn write_noisy(root: &Path) -> PathBuf {
    write_pkg(
        root,
        "noisy",
        &[(
            "server.js",
            r#"
import { createApi } from '@kkscreeps/mod';
export default function register(api) {
  api.handlers.after('builtin:link.tick', { id: 'rattle' }, function rattle(ctx) {
    ctx.state = { draw: Math.random(), second: Math.random() };
  });
}
"#,
        )],
    )
}

/// 记录逐绑定写入的整槽宿主：read 恒给 tick/room/目标 test 的空状态，
/// write 按（绑定, state）留档供协议断言。
struct RecordingHost(Rc<RefCell<Vec<(String, Value)>>>);
impl kk_mod_host::ObjectsHost for RecordingHost {}
impl kk_mod_host::ResourcesHost for RecordingHost {}
impl kk_mod_host::MovementHost for RecordingHost {}
impl kk_mod_host::CombatHost for RecordingHost {}
impl kk_mod_host::BuildHost for RecordingHost {}
impl ChainHost for RecordingHost {
    fn read(&mut self, _: &str) -> Result<ChainContext, String> {
        Ok(ChainContext {
            args: Value::Null,
            room: "W1N1".into(),
            state: Value::Null,
            target: ChainTarget {
                fields: Value::Null,
                id: "test".into(),
            },
            tick: 9,
        })
    }
    fn write(&mut self, binding: &str, context: ChainWrite) -> Result<(), String> {
        self.0.borrow_mut().push((binding.into(), context.state));
        Ok(())
    }
    fn native(&mut self) -> Result<(), String> {
        Ok(())
    }
}

fn invoke_tally(realm: &mut ModRealm, tick: u64, room: &str) -> Value {
    realm
        .invoke(
            "roller:handler.tally",
            json!({"tick": tick, "room": room, "state": null}),
        )
        .expect("处理器可调用")["state"]
        .clone()
}

/// 同种子同输入逐位相等；异种子（结算抽值与 init 抽值）相异。
#[test]
fn realm_random_is_deterministic_and_seed_sensitive() {
    let root = temp_root("determinism-seed");
    write_roller(&root, "roller");
    let a1 = invoke_tally(&mut register_roller(42, &root), 7, "E1N1");
    let a2 = invoke_tally(&mut register_roller(42, &root), 7, "E1N1");
    let b = invoke_tally(&mut register_roller(43, &root), 7, "E1N1");
    assert_eq!(a1, a2, "同种子同输入必须逐位相等");
    assert_ne!(a1["draw"], b["draw"], "种子必须进入结算流");
    assert_ne!(a1["initDraw"], b["initDraw"], "种子必须进入 init 流");
}

/// 结算抽值钉住协议推导：域=`绑定键#房间#目标ID`（直呼路径目标为空），
/// 序号为本域本轮第 0 抽，tick 来自 ctx。
#[test]
fn settlement_draw_pins_protocol_derivation() {
    let root = temp_root("determinism-pin");
    write_roller(&root, "roller");
    let mut realm = register_roller(123456789, &root);
    let state = invoke_tally(&mut realm, 3, "E1N1");
    let domain = "roller:handler.tally#E1N1#";
    assert_eq!(
        state["draw"].as_f64().unwrap(),
        kk_core::rng::gameplay_unit(kk_core::rng::gameplay_draw(123456789, domain, 3, 0))
    );
    // Date 映射：结算期 = ctx.tick×3000；装载期恒 0。
    assert_eq!(state["now"], json!(3 * 3000));
    assert_eq!(state["newDate"], json!(3 * 3000));
    assert_eq!(state["initNow"], json!(0));
    assert_eq!(
        state["temporalGone"],
        json!(true),
        "玩法 realm 不暴露 Temporal"
    );
}

/// 同轮内域序号推进；结算轮重置后重抽同值（重试/同 tick 重开语义）。
#[test]
fn round_reset_makes_same_tick_redraw_identical() {
    let root = temp_root("determinism-reset");
    write_roller(&root, "roller");
    let mut realm = register_roller(42, &root);
    let first = invoke_tally(&mut realm, 5, "E1N1");
    let second = invoke_tally(&mut realm, 5, "E1N1");
    assert_ne!(first["draw"], second["draw"], "同轮同域两次抽取序号推进");
    realm.begin_settlement().expect("轮开始");
    realm.end_settlement();
    let retried = invoke_tally(&mut realm, 5, "E1N1");
    assert_eq!(first["draw"], retried["draw"], "轮重置后同输入重抽同值");
}

/// 域独立：另一绑定的抽取不挤占本域序号；房间进入域串，跨房各自成流。
#[test]
fn domains_are_independent_across_bindings_and_rooms() {
    let root = temp_root("determinism-domains");
    write_roller(&root, "roller");
    let mut with_mate = register_roller(42, &root);
    invoke_tally(&mut with_mate, 5, "E1N1");
    // 同轮先让 mate 抽一次——不改变 tally 的下一抽。
    with_mate
        .invoke(
            "roller:handler.mate",
            json!({"tick": 5, "room": "E1N1", "state": null}),
        )
        .expect("mate 可调用");
    let after_mate = invoke_tally(&mut with_mate, 5, "E1N1");

    let mut solo = register_roller(42, &root);
    invoke_tally(&mut solo, 5, "E1N1");
    let solo_again = invoke_tally(&mut solo, 5, "E1N1");
    assert_eq!(after_mate["draw"], solo_again["draw"]);

    // 同一绑定换房间：流不同（房名进域），但在两个 realm 间逐位可复现。
    let other_room = invoke_tally(&mut register_roller(42, &root), 5, "E2N2");
    assert_ne!(
        other_room["draw"],
        invoke_tally(&mut register_roller(42, &root), 5, "E1N1")["draw"]
    );
    assert_eq!(
        other_room["draw"],
        invoke_tally(&mut register_roller(42, &root), 5, "E2N2")["draw"]
    );
}

/// init 流不含 tick：两次安装（不同 realm）同值；且与任何结算流首抽不同源
/// （域不同）。装载期 Date 恒 0 已在协议钉死用例覆盖。
#[test]
fn init_stream_is_install_scoped_and_tickless() {
    let root = temp_root("determinism-init");
    write_roller(&root, "roller");
    let first = invoke_tally(&mut register_roller(42, &root), 1, "E1N1");
    let second = invoke_tally(&mut register_roller(42, &root), 999, "E1N1");
    assert_eq!(
        first["initDraw"], second["initDraw"],
        "init 流与装载时刻的世界 tick 无关"
    );
    assert_eq!(
        first["initDraw"].as_f64().unwrap(),
        kk_core::rng::gameplay_unit(kk_core::rng::gameplay_draw(42, "roller#init", 0, 0))
    );
}

/// wrap 链跨层流域隔离：sheller 在 next() 前后各抽一次，内层（native
/// 中心与 after 处理器）的抽取不挤占外层序号——退栈后的第二抽是本域
/// index 1；after 处理器（tally）同样是自己的域 index 0。目标 ID 进入
/// 域串（test）。
#[test]
fn wrap_chain_layers_keep_independent_streams() {
    let root = temp_root("determinism-wrap");
    write_roller(&root, "roller");
    let mut realm = register_roller(42, &root);
    let writes = Rc::new(RefCell::new(Vec::new()));
    realm
        .invoke_chain("builtin:link.tick", Box::new(RecordingHost(writes.clone())))
        .expect("整槽可执行");
    let find = |binding: &str| {
        writes
            .borrow()
            .iter()
            .rev()
            .find(|(b, _)| b == binding)
            .map(|(_, s)| s.clone())
            .unwrap_or(Value::Null)
    };
    let expect = |domain: &str, index: u64| {
        kk_core::rng::gameplay_unit(kk_core::rng::gameplay_draw(42, domain, 9, index))
    };
    let sheller = find("roller:handler.sheller");
    assert_eq!(
        sheller["before"].as_f64().unwrap(),
        expect("roller:handler.sheller#W1N1#test", 0)
    );
    assert_eq!(
        sheller["after"].as_f64().unwrap(),
        expect("roller:handler.sheller#W1N1#test", 1),
        "内层（native/after）抽取不得挤占外层 wrap 的域序号"
    );
    assert_eq!(
        find("roller:handler.tally")["draw"].as_f64().unwrap(),
        expect("roller:handler.tally#W1N1#test", 0)
    );
}

/// 跨命名空间独立：另一包 noisy 先抽两次，不改变 roller 域的序列。
#[test]
fn cross_package_draws_do_not_share_streams() {
    let root = temp_root("determinism-cross-pkg");
    write_roller(&root, "roller");
    write_noisy(&root);
    let mut both = register_roller(42, &root);
    both.invoke(
        "noisy:handler.rattle",
        json!({"tick": 5, "room": "E1N1", "state": null}),
    )
    .expect("noisy 处理器可调用");
    let after_noisy = invoke_tally(&mut both, 5, "E1N1");

    let solo_root = temp_root("determinism-solo-pkg");
    write_roller(&solo_root, "roller");
    let mut solo = register_roller(42, &solo_root);
    let solo_first = invoke_tally(&mut solo, 5, "E1N1");
    // 同轮第二抽序号推进（健全性：对照值不是恒定的重复值）。
    let solo_second = invoke_tally(&mut solo, 5, "E1N1");
    assert_ne!(solo_first["draw"], solo_second["draw"]);

    assert_eq!(
        after_noisy["draw"], solo_first["draw"],
        "他包抽值不得挤占本包域序号（both 的首抽须与 solo 的首抽同值）"
    );
}

/// 密封后的诊断探针（宿主面）里：原生 Date 构造器不可经静态或实例
/// 原型链取回；parse/UTC 纯函数保留；无作用域 Math.random 按注册窗口
/// 报错，不静默给值。
#[test]
fn native_date_unreachable_and_out_of_window_random_rejected() {
    let root = temp_root("determinism-probe");
    write_roller(&root, "roller");
    let mut realm = register_roller(42, &root);
    let probe = realm
        .evaluate_probe(
            r#"
(() => {
  const staticParentIsFunctionProto =
    Object.getPrototypeOf(Date) === Object.getPrototypeOf(function () {});
  const protoConstructorIsDate =
    Object.getPrototypeOf(Date.prototype).constructor === Date;
  let randomOutside;
  try {
    Math.random();
    randomOutside = 'gave';
  } catch (e) {
    randomOutside = /注册/.test(String(e.message)) ? 'rejected' : 'other';
  }
  return {
    staticParentIsFunctionProto,
    protoConstructorIsDate,
    parseInherited: Date.parse('1970-01-01T00:00:00Z') === 0,
    utcInherited: Date.UTC(1970, 0, 1) === 0,
    instanceOfDate: new Date(0) instanceof Date,
    // toJSON→toISOString 是 UTC 文本，确定性；Date() 全文才含本地时区。
    stringifyISO: JSON.stringify({ d: new Date(0) }),
    dateNowOutsideScope: Date.now(),
    randomOutside,
  };
})()
"#,
        )
        .expect("探针可执行");
    assert_eq!(
        probe["staticParentIsFunctionProto"],
        json!(true),
        "静态原型不得指向原生 Date（其 now() 是墙钟）"
    );
    assert_eq!(
        probe["protoConstructorIsDate"],
        json!(true),
        "实例原型上的 constructor 须遮蔽回确定性包装"
    );
    assert_eq!(probe["parseInherited"], json!(true));
    assert_eq!(probe["utcInherited"], json!(true));
    assert_eq!(probe["instanceOfDate"], json!(true));
    assert_eq!(
        probe["stringifyISO"],
        json!(r#"{"d":"1970-01-01T00:00:00.000Z"}"#)
    );
    assert_eq!(probe["dateNowOutsideScope"], json!(0), "无作用域时钟恒 0");
    assert_eq!(
        probe["randomOutside"],
        json!("rejected"),
        "注册窗口外随机必须显式报错"
    );
}
