//! `mod types` 端到端验收：两独立包组合、选择门不残留、新鲜度门禁、
//! 路径守卫、内建类清单与玩家面运行时同源、生成物经 tsc 核验可用。
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "kk-mod-types-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    /// 运行 mod types；返回（退出码、报告 JSON、声明文本）。路径守卫
    /// 拒绝时按 mod inspect 语义不落报告，报告 JSON 为 Null。
    fn run(&self, root: &Path, out: &Path, args: &[&str]) -> (i32, Value, String) {
        let report = self.0.join("report.json");
        let output = Command::new(env!("CARGO_BIN_EXE_kkscreeps"))
            .args(["mod", "types", "--root"])
            .arg(root)
            .args(args)
            .arg("--out")
            .arg(out)
            .arg("--report")
            .arg(&report)
            .output()
            .unwrap();
        let json = std::fs::read(&report)
            .ok()
            .map(|bytes| serde_json::from_slice(&bytes).unwrap())
            .unwrap_or(Value::Null);
        let text = std::fs::read_to_string(out).unwrap_or_default();
        (output.status.code().unwrap(), json, text)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/fixtures/mod-typed")
}

#[test]
fn two_independent_packages_compose_without_cross_leakage() {
    let f = Fixture::new();
    let out = f.0.join("mod-typed.d.ts");
    let (code, report, text) = f.run(&root(), &out, &[]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["status"], "ok");
    // 两包的对象投影、意图参数、资源联合与建筑绑定都进入同一计划；
    // 键含命名空间前缀，接口名与参数类型名按各包声明互不冲突。
    assert!(text.contains("export interface TypedBeacon {"), "{text}");
    assert!(text.contains("export interface TypedRelay {"), "{text}");
    assert!(text.contains("modType: 'alpha:beacon';"));
    assert!(text.contains("modType: 'beta:relay';"));
    assert!(text.contains("export interface AlphaSaluteArgs {"));
    assert!(text.contains("export interface BetaPingArgs {"));
    assert!(
        text.contains("export type ModResourceName = 'alpha:fuel' | 'beta:spark';"),
        "{text}"
    );
    assert!(text.contains("bind(typeKey: 'alpha:gate', cls: TypedGateConstructor): void;"));
    // 信封键只声明一次：位置字段 x/y 不因同名声明重复出现。
    assert_eq!(text.match_indices("\n  x: number;").count(), 2, "{text}");
    // 归属者字段可选；ServerOnly 字段与审计字段不出现。
    assert!(text.contains("note?: string;"));
    assert!(!text.contains("secret"), "ServerOnly 字段不得进入声明");
    assert!(!text.contains("audit"), "ServerOnly 字段不得进入声明");
    // 可空参数意图在声明中是可选参数（宿主把缺省 args 归一为 null）。
    assert!(text.contains(
        "emit(intentKey: 'beta:ping', target: { id: string }, args?: BetaPingArgs | null): number;"
    ));
    // Bool→boolean 与 ObjectRef→id 字符串映射在端到端产物中断言
    //（ObjectRef 可空字段带 | null 后缀）。
    assert!(text.contains("armed: boolean;"), "{text}");
    assert!(
        text.contains("mentor: string /* alpha:beacon */ | null;"),
        "{text}"
    );
    // 生成确定性：同输入两次运行字节一致。
    let out2 = f.0.join("again.d.ts");
    let (code2, _, text2) = f.run(&root(), &out2, &[]);
    assert_eq!(code2, 0);
    assert_eq!(text, text2);
}

#[test]
fn selection_gates_leave_no_residual_declarations() {
    let f = Fixture::new();
    let selected = f.0.join("alpha.d.ts");
    let (code, _, text) = f.run(&root(), &selected, &["--select", "alpha"]);
    assert_eq!(code, 0);
    assert!(text.contains("modType: 'alpha:beacon';"), "{text}");
    // 未选包不残留：头部的包清单、接口、资源、意图与查询均无 beta。
    assert!(!text.contains("beta"), "选择门外不得残留未选包声明：{text}");
    // 互不依赖的两包：--exclude 与 --select 选出同一集合，产物逐字节一致。
    let excluded = f.0.join("alpha-exclude.d.ts");
    let (code, _, text_excluded) = f.run(&root(), &excluded, &["--exclude", "beta"]);
    assert_eq!(code, 0);
    assert_eq!(text, text_excluded);
}

#[test]
fn freshness_gate_fails_then_recovers() {
    let f = Fixture::new();
    let out = f.0.join("fresh.d.ts");
    let (code, report, first) = f.run(&root(), &out, &[]);
    assert_eq!(code, 0, "{report}");
    let (code, report, _) = f.run(&root(), &out, &["--check"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["status"], "ok");
    // 手改与缺失同样判过期（exit 2、status=stale），区分于注册失败（1）；
    // check 只读对比，手改内容保持原样不被重写。
    std::fs::write(&out, format!("{first}// 手改\n")).unwrap();
    let (code, report, _) = f.run(&root(), &out, &["--check"]);
    assert_eq!(code, 2, "{report}");
    assert_eq!(report["status"], "stale");
    assert!(std::fs::read_to_string(&out).unwrap().contains("// 手改"));
    std::fs::remove_file(&out).unwrap();
    let (code, report, _) = f.run(&root(), &out, &["--check"]);
    assert_eq!(code, 2, "{report}");
    assert_eq!(report["status"], "stale");
    // 重新生成后恢复新鲜，且内容与首次一致（刷新不漂移）。
    let (code, _, second) = f.run(&root(), &out, &[]);
    assert_eq!(code, 0);
    assert_eq!(first, second);
    let (code, report, _) = f.run(&root(), &out, &["--check"]);
    assert_eq!(code, 0, "{report}");
}

#[test]
fn output_guard_rejects_package_interior_and_non_dts_suffix() {
    let f = Fixture::new();
    let inside = root().join("alpha/generated.d.ts");
    let (code, _, _) = f.run(&root(), &inside, &[]);
    assert_eq!(code, 1);
    assert!(!inside.exists(), "守卫拒绝时不得落盘");
    // 父目录不存在的包内输出：拒绝必须发生在创建目录之前——空目录
    // 留在 mods 根一级会让后续 discover_packages 把它当包读而整体失败。
    let poisoned = root().join("cache/generated.d.ts");
    let (code, _, _) = f.run(&root(), &poisoned, &[]);
    assert_eq!(code, 1);
    assert!(
        !poisoned.parent().unwrap().exists(),
        "守卫拒绝不得在包根内创建目录"
    );
    let bad = f.0.join("bad.txt");
    let (code, _, _) = f.run(&root(), &bad, &[]);
    assert_eq!(code, 1);
    assert!(!bad.exists());
}

/// 报告与声明同守包外纪律（报告进包目录同样改变计划指纹），且不得
/// 与声明输出同路径（先写声明再被 JSON 报告覆盖是静默错误工件）。
#[test]
fn report_guard_keeps_reports_outside_root_and_off_the_declaration() {
    let f = Fixture::new();
    let out = f.0.join("types.d.ts");
    let report_inside = root().join("alpha/report.json");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kkscreeps"))
        .args(["mod", "types", "--root"])
        .arg(root())
        .arg("--out")
        .arg(&out)
        .arg("--report")
        .arg(&report_inside)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!report_inside.exists());
    let same = f.0.join("same.d.ts");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_kkscreeps"))
        .args(["mod", "types", "--root"])
        .arg(root())
        .arg("--out")
        .arg(&same)
        .arg("--report")
        .arg(&same)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(!same.exists(), "报告与声明同路径时不得先落声明再等覆盖");
}

/// 生成器与玩家面安装运行时各自维护一份内建类清单；此处断言两者
/// 逐项一致（顺序与内容），漂移在测试期暴露而不是玩家安装期失败。
#[test]
fn builtin_class_list_matches_player_runtime() {
    let f = Fixture::new();
    let out = f.0.join("builtins.d.ts");
    let (code, _, text) = f.run(&root(), &out, &[]);
    assert_eq!(code, 0);
    let union = text
        .lines()
        .find(|line| line.starts_with("export type ModBuiltinClassName = "))
        .expect("声明须含内建类名联合");
    let generated: Vec<&str> = union
        .split(" = ")
        .nth(1)
        .unwrap()
        .trim_end_matches(';')
        .split(" | ")
        .map(|member| member.trim_matches('\''))
        .collect();
    let runtime_source = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../kk-runner/src/prelude/41_mod_player.js"),
    )
    .unwrap();
    let start = runtime_source
        .find("const BUILTIN_CLASS_NAMES = [")
        .expect("玩家面运行时须有内建类清单");
    let end = runtime_source[start..].find("];").expect("清单须闭合") + start;
    let declared: Vec<&str> = runtime_source[start..end]
        .trim_start_matches("const BUILTIN_CLASS_NAMES = [")
        .split(',')
        .map(|member| member.trim().trim_matches('\''))
        .filter(|member| !member.is_empty())
        .collect();
    assert_eq!(
        generated, declared,
        "生成器与 41_mod_player.js 的内建类清单漂移"
    );
}

/// 生成物经 compat 工作区的 tsc 核验：声明自身无错，且消费者按声明
/// 使用（字面量窄化的 emit/queries、枚举联合字段、可选参数）通过类型
/// 检查。pnpm install 是仓库标准前置；tsc 缺席时失败而非静默跳过。
#[test]
fn generated_declarations_pass_tsc_and_support_consumers() {
    let tsc = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/node_modules/.bin/tsc");
    assert!(
        tsc.exists(),
        "compat/node_modules/.bin/tsc 缺席：先运行 pnpm install（仓库标准前置）"
    );
    let f = Fixture::new();
    let out = f.0.join("generated.d.ts");
    let (code, _, _) = f.run(&root(), &out, &[]);
    assert_eq!(code, 0);
    let status = Command::new(&tsc)
        .arg("--noEmit")
        .arg("--strict")
        .arg(&out)
        .status()
        .unwrap();
    assert!(status.success(), "生成声明须通过 tsc --strict");
    let consumer = f.0.join("consumer.ts");
    std::fs::write(
        &consumer,
        "import type { ModInstallApi, AlphaSaluteArgs, TypedBeacon, ModResourceName } \
         from './generated.d.ts';\n\
         declare const api: ModInstallApi;\n\
         const args: AlphaSaluteArgs = { amount: 3, tag: null };\n\
         const ok: number = api.intents.emit('alpha:salute', { id: 'obj1' }, args);\n\
         const ping: number = api.intents.emit('beta:ping', { id: 'obj2' });\n\
         declare const beacon: TypedBeacon;\n\
         const state: 'off' | 'on' = beacon.state;\n\
         const find: number = api.queries.install('alpha:beacon').find;\n\
         const resource: ModResourceName = 'beta:spark';\n\
         class PlayerBeacon extends api.builtins.RoomObject {\n\
         \x20 get level() { return 1; }\n\
         }\n\
         api.objects.bind('alpha:beacon', PlayerBeacon);\n\
         export { ok, ping, state, find, resource };\n",
    )
    .unwrap();
    let output = Command::new(&tsc)
        .arg("--noEmit")
        .arg("--strict")
        .arg(&consumer)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "消费者类型检查失败：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // 负例证明窄化为真：目录外的意图键与错误字段类型必须报编译错，
    // 否则声明只是注释而不是约束。
    let negative = f.0.join("negative.ts");
    std::fs::write(
        &negative,
        "import type { ModInstallApi, TypedBeacon } from './generated.d.ts';\n\
         declare const api: ModInstallApi;\n\
         api.intents.emit('beta:relay', { id: 'obj2' });\n\
         declare const beacon: TypedBeacon;\n\
         const level: 'off' | 'on' = beacon.level;\n\
         export { level };\n",
    )
    .unwrap();
    let output = Command::new(&tsc)
        .arg("--noEmit")
        .arg("--strict")
        .arg(&negative)
        .output()
        .unwrap();
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!output.status.success(), "负例必须编译失败");
    assert!(
        diagnostics.contains("TS2345"),
        "意图键须按字面量拒绝：{diagnostics}"
    );
    assert!(
        diagnostics.contains("TS2322"),
        "字段类型须按声明拒绝：{diagnostics}"
    );
}

#[test]
fn recursive_arrays_and_all_generated_symbols_compile_without_shadowing() {
    let f = Fixture::new();
    let packages = f.0.join("mods");
    let package = packages.join("alpha");
    std::fs::create_dir_all(&package).unwrap();
    std::fs::write(
        package.join("mod.json"),
        r#"{"id":"alpha","version":"1.0.0","apiVersion":"^1.0.0","entry":"server.js"}"#,
    )
    .unwrap();
    std::fs::write(package.join("server.js"), r#"
export default function register(api) {
  const types = ['Foo', 'FooConstructor', 'ModInstallApi', 'Record', 'AlphaPingArgs', 'string', 'FooGaugeConstructorFields'];
  let first;
  for (let i = 0; i < types.length; i++) {
    const object = api.objects.define('alpha:object' + i, {
      version: 1, playerClass: types[i], visibility: 'room-visible',
      positionFields: ['x', 'y'], fields: {
        x: { type: 'u32', min: 0, max: 49 }, y: { type: 'u32', min: 0, max: 49 },
        modes: { type: 'array', maxLen: 4, element: { type: 'enum', values: ['off', 'on'] } },
        matrix: { type: 'array', maxLen: 4, element: { type: 'array', maxLen: 4, nullable: true, element: { type: 'u32' } } },
        map: { type: 'map', maxLen: 4, value: { type: 'string', maxBytes: 4 } },
      }, tick() {},
    });
    if (i === 0) first = object;
  }
  api.intents.define('alpha:ping', {
    scope: 'object', target: first, args: { type: 'bool' }, process() {},
  });
  api.buildings.define('alpha:gauge', { playerClass: 'FooGauge', cost: {energy: 5}, hitsMax: 100, pathing: 'plain', fields: { heat: {type: 'u32', default: 7} } });
  api.buildings.define('alpha:gate', { playerClass: 'Foo', cost: { energy: 5 }, hitsMax: 100, pathing: 'plain' });
}
"#).unwrap();
    let generated = f.0.join("generated.d.ts");
    let (code, report, text) = f.run(&packages, &generated, &[]);
    assert_eq!(code, 0, "{report}");
    assert!(text.contains("export interface FooConstructor {"));
    assert!(text.contains("cls: FooConstructor2"));
    assert!(text.contains("args: AlphaPingArgs2"));
    let consumer = f.0.join("consumer.ts");
    std::fs::write(
        &consumer,
        r#"
import type { Foo, ModInstallApi, FooGaugeConstructorFields2 } from './generated.d.ts';
const gauge: FooGaugeConstructorFields2 = {'alpha:heat': 7};
// @ts-expect-error 字段接口不能与合法对象类名合并
const invalidGauge: FooGaugeConstructorFields2 = {'alpha:heat': 'bad'};
declare const foo: Foo;
declare const api: ModInstallApi;
const modes: ReadonlyArray<'on' | 'off'> = foo.modes;
const matrix: ReadonlyArray<ReadonlyArray<number> | null> = foo.matrix;
api.intents.emit('alpha:ping', { id: 'x' }, true);
// @ts-expect-error 枚举数组不能混入目录外的成员
const invalid: Foo['modes'] = ['other'];
// @ts-expect-error 嵌套数组不能混入字符串
const badMatrix: Foo['matrix'] = [['wrong']];
// @ts-expect-error 投影数组只读
foo.modes.push('on');
// @ts-expect-error 参数类型碰撞后仍须约束意图
api.intents.emit('alpha:ping', { id: 'x' }, 'wrong');
"#,
    )
    .unwrap();
    let tsc = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compat/node_modules/.bin/tsc");
    let output = Command::new(tsc)
        .args(["--noEmit", "--strict"])
        .arg(consumer)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}\n{text}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
