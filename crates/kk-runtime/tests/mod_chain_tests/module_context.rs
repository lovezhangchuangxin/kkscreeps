//! 表达式入口的控制台作用域、真实入口缓存与模块资产恢复。
use super::*;
use kk_runner::ModuleTable;

fn fixture(binary: bool) -> anyhow::Result<EmbedEngine> {
    let mut e = EmbedEngine::new();
    e.set_kkpv_channel(binary);
    e.create_shard(&json!({"players":["p1","p2"],"rooms":[{"name":"W1N1","owner":"p1","rcl":6}]}))?;
    Ok(e)
}

#[test]
fn console_module_aliases_are_local_and_host_closures_are_unreachable() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        let code = "var localProbe=7; exports.a=11; module.exports.b=13; globalThis.savedConsoleModule=module; [exports===module.exports,exports.b,module.exports.a,module.user,typeof globalThis.module,typeof globalThis.exports,arguments.length,this===globalThis]";
        assert_eq!(
            e.run_player("p1", code)?.expr["value"],
            json!([true, 13, 11, "p1", "undefined", "undefined", 3, true])
        );
        let next=e.run_player("p1","[typeof localProbe,typeof module.exports.a,module!==globalThis.savedConsoleModule,typeof ops,typeof gw,typeof safeStringify,typeof evaluateExpression,arguments.callee.caller===null]")?;
        assert_eq!(
            next.expr["value"],
            json!([
                "undefined",
                "undefined",
                true,
                "undefined",
                "undefined",
                "undefined",
                "undefined",
                true
            ])
        );
        assert_eq!(
            e.run_player(
                "p2",
                "[typeof globalThis.savedConsoleModule,module.user,exports===module.exports]"
            )?
            .expr["value"],
            json!(["undefined", "p2", true])
        );
        // 语法/运行时/序列化错误继续由宿主分类，表达式不得逃出宿主出口。
        for (code, kind) in [
            ("var a=;", "syntax"),
            ("throw Error('probe')", "runtime"),
            ("(()=>{const x={};x.x=x;return x})()", "serialization"),
        ] {
            let result = e.run_player("p1", code)?;
            assert_eq!(result.expr["ok"], false);
            assert_eq!(result.expr["kind"], kind);
        }
    }
    Ok(())
}

#[test]
fn default_main_is_real_cached_module_and_explicit_replacement_is_not_patched() -> anyhow::Result<()>
{
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        assert_eq!(e.run_player("p1","globalThis.savedMain=require('main');[typeof savedMain.loop,savedMain===require.cache.main,typeof Memory.runs]")?.expr["value"],json!(["function",true,"undefined"]));
        assert_eq!(e.run_player("p1","[require('main')===globalThis.savedMain,exports===require('main'),module.exports===exports]")?.expr["value"],json!([true,false,true]));
        assert_eq!(e.run_player("p1","const before=require.cache.main;const deleted=delete require.cache.main;globalThis.deletedMain=before;[deleted,require.cache.main===undefined]")?.expr["value"],json!([true,true]));
        assert_eq!(
            e.run_player(
                "p1",
                "[require('main')!==globalThis.deletedMain,typeof require('main').loop]"
            )?
            .expr["value"],
            json!([true, "function"])
        );
        let mut modules = ModuleTable::new();
        modules.insert_js("main","globalThis.loads=(globalThis.loads||0)+1; module.exports={label:'uploaded',loop(){Memory.loops=(Memory.loops||0)+1}};");
        e.set_player_modules("p1", modules)?;
        assert_eq!(e.run_player("p1","const m=require('main');[m.label,m!==globalThis.savedMain,globalThis.loads,typeof Memory.loops]")?.expr["value"],json!(["uploaded",true,1,"undefined"]));
        // 控制台请求不调用上传的 loop；显式无 main 的模块表保持 Unknown module。
        e.set_player_modules("p1", ModuleTable::new())?;
        let r = e.run_player("p1", "require('main')")?;
        assert_eq!(r.expr["ok"], false);
        assert!(
            r.expr["error"]
                .as_str()
                .unwrap()
                .contains("Unknown module 'main'")
        );
    }
    Ok(())
}

#[test]
fn console_retry_runs_once_and_cold_restore_reloads_saved_main_assets() -> anyhow::Result<()> {
    for binary in [false, true] {
        let mut e = fixture(binary)?;
        e.fail_next_writeback_for_test(kk_engine::WritebackStep::RawMemory);
        let code = "globalThis.executions=(globalThis.executions||0)+1; Memory.runs=(Memory.runs||0)+1; globalThis.mainRef=require('main');[executions,module.exports===exports]";
        assert!(e.run_player("p1", code).is_err());
        let retried = e.run_player("p1", "throw Error('不得重新求值')")?;
        assert_eq!(retried.expr["value"], json!([1, true]));
        let checkpoint = e.checkpoint(Value::Null)?;
        assert_eq!(
            checkpoint["workers"]["p1"]["modules"]["main"]["js"],
            "module.exports.loop = function () {};"
        );
        let mut restored = EmbedEngine::new();
        restored.set_kkpv_channel(binary);
        restored.restore_checkpoint(checkpoint, None)?;
        assert_eq!(restored.run_player("p1","[Memory.runs,typeof globalThis.executions,typeof require('main').loop,module.exports===exports]")?.expr["value"],json!([1,"undefined","function",true]));
        let mut modules = ModuleTable::new();
        modules.insert_js("main", "module.exports={label:'cold',loop(){}};");
        e.set_player_modules("p1", modules)?;
        restored.restore_checkpoint(e.checkpoint(Value::Null)?, None)?;
        assert_eq!(
            restored.run_player("p1", "require('main').label")?.expr["value"],
            "cold"
        );
    }
    Ok(())
}
