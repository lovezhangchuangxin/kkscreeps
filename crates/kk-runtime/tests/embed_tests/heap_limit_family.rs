//! 玩家 V8 堆上限的宿主配置面：整卡默认落点、宿主覆盖、显式注入优先、
//! 恢复路径按工作者身份回注与取值边界。工作者堆上限进检查点身份域，
//! 以上断言均经检查点载荷观察（workers.*.heap_limit_bytes）。

use super::*;

pub(super) fn heap_limit_family_impl() -> anyhow::Result<()> {
    // 未设置任何宿主值：工作者携带整卡默认 64MiB。
    {
        let mut e = EmbedEngine::new();
        e.create_shard(&json!({"players": ["p1"], "rooms": [{"name": "W1N1"}]}))?;
        let payload = e.checkpoint(serde_json::Value::Null)?;
        assert_eq!(
            payload["workers"]["p1"]["heap_limit_bytes"].as_u64(),
            Some(kk_runner::DEFAULT_HEAP_LIMIT as u64),
            "未配置玩家须落整卡默认堆上限"
        );
    }
    // 宿主值只补默认配置的玩家；显式注入的玩家保持注入值。
    {
        let mut e = EmbedEngine::new();
        e.set_player_heap_limit(32 * 1024 * 1024)?;
        let explicit = kk_runner::SandboxConfig {
            heap_limit_bytes: 24 * 1024 * 1024,
            ..Default::default()
        };
        e.set_sandbox_config_for_test("p1", explicit);
        e.create_shard(&json!({"players": ["p1", "p2"], "rooms": [{"name": "W1N1"}]}))?;
        let payload = e.checkpoint(serde_json::Value::Null)?;
        assert_eq!(
            payload["workers"]["p1"]["heap_limit_bytes"].as_u64(),
            Some(24 * 1024 * 1024),
            "显式注入配置优先于宿主默认"
        );
        assert_eq!(
            payload["workers"]["p2"]["heap_limit_bytes"].as_u64(),
            Some(32 * 1024 * 1024),
            "默认配置玩家采用宿主覆盖值"
        );
    }
    // 恢复按工作者生效配置回注：新引擎的宿主值/默认值不改写恢复中的
    // 工作者（堆上限是检查点身份域，跨恢复须保持一致）。
    {
        let mut e = EmbedEngine::new();
        e.set_player_heap_limit(24 * 1024 * 1024)?;
        e.create_shard(&json!({"players": ["p1"], "rooms": [{"name": "W1N1"}]}))?;
        let saved = e.checkpoint(serde_json::Value::Null)?;
        let mut restored = EmbedEngine::new();
        restored.set_player_heap_limit(48 * 1024 * 1024)?;
        restored.restore_checkpoint(saved, None)?;
        let payload = restored.checkpoint(serde_json::Value::Null)?;
        assert_eq!(
            payload["workers"]["p1"]["heap_limit_bytes"].as_u64(),
            Some(24 * 1024 * 1024),
            "恢复的工作者保持检查点身份的堆上限"
        );
    }
    // 取值边界与检查点身份域一致（16MiB..=4GiB）。
    {
        let mut e = EmbedEngine::new();
        assert!(e.set_player_heap_limit(16 * 1024 * 1024 - 1).is_err());
        assert!(e.set_player_heap_limit(4 * 1024 * 1024 * 1024 + 1).is_err());
        assert!(e.set_player_heap_limit(16 * 1024 * 1024).is_ok());
        assert!(e.set_player_heap_limit(4 * 1024 * 1024 * 1024).is_ok());
    }
    Ok(())
}
