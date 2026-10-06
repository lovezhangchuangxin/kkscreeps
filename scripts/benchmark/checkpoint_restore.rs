//! 核验固定旧产物生成的完整载荷；直接使用 runtime 恢复，不绕过 sim 的 CLI 身份门。
use std::collections::BTreeMap;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let packages = kk_mod_host::discover_packages(&Path::new(&args[1]).canonicalize()?)?;
    let options = kk_mod_host::RegisterOptions::default();
    let mut reports = Vec::new();
    for path in &args[2..] {
        let saved = kk_runtime::checkpoint::read_checkpoint(Path::new(path))?;
        let mut engine = kk_runtime::EmbedEngine::new();
        let metadata = engine.restore_checkpoint(saved.clone(), Some((&packages, &options)))?;
        assert_eq!(engine.checkpoint(metadata)?, saved, "旧载荷恢复前后必须逐值相等");
        let before = engine.get_game_time();
        let results = engine.run_players(&BTreeMap::from([
            ("p1".into(), "require('main').loop()".into()),
            ("p2".into(), "require('main').loop()".into()),
        ]))?;
        assert!(results.values().all(|result| result.expr["ok"] == true));
        assert_eq!(engine.get_game_time(), before + 1);
        reports.push(serde_json::json!({"fixture":path,"kkpv":saved["kkpv"],"restored_exactly":true,
            "format_version":saved["checkpoint_format_version"],"plan_hash":saved["engine"]["plan_hash"],
            "tick_before":before,"tick_after":engine.get_game_time(),"players":results.len()}));
        // 按 realm 栈顺序销毁，再开始下一份检查点。
        drop(engine);
    }
    println!("{}", serde_json::to_string_pretty(&reports)?);
    Ok(())
}
