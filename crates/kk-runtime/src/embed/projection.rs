//! 玩家投影、共享 KKPV 束和市场查询视图。
use super::*;

/// 私有块的查看者摘要：SHA-256(玩家名)。audience_hash 只是宿主定义的
/// 一致性摘要，不是授权凭证（设计见 domain-projection.md §6）。
pub(super) fn kkpv_audience_hash(viewer: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(viewer.as_bytes());
    hasher.finalize().into()
}

/// 公开地形束的受众组摘要：SHA-256("kkpv-public-terrain-v1")。公开通道
/// 对全部查看者使用同一值——束内容与查看者无关，摘要只承载"哪个公开
/// 受众组"的一致性校验与缓存分组，同样不是授权凭证。
const KKPV_PUBLIC_AUDIENCE_HASH: [u8; 32] = [
    0x52, 0x96, 0x13, 0x9a, 0x7f, 0x68, 0xc1, 0x8a, 0x0a, 0xe7, 0x05, 0x50, 0xa1, 0xb4, 0x17, 0x76,
    0x5c, 0x9c, 0x88, 0xa3, 0x29, 0xcc, 0x59, 0xe8, 0xc7, 0xfe, 0x27, 0x9e, 0xc8, 0x4a, 0xdb, 0x7b,
];
impl EmbedEngine {
    /// 获取当前代际的公开地形束共享句柄；未发布则投影、编码并发布。
    /// 同代际的重入（一轮内多次 run_players、失败轮重投影）命中缓存
    /// 不重编码；结算推进 tick 或世界变异推进 generation 后的首次获取
    /// 重编码（块头 tick 与信封 time 交叉校验要求束随代际更新）。
    /// 零注册房世界直接下发空束（库发布要求可校验的块版本，空束没有；
    /// 沙箱侧空公开束解出空地形表，与 JSON 通道的零房行为一致）。
    ///
    /// # Errors
    /// 世界源不提供公开块、编码失败或发布校验失败时返回错误。
    pub(super) fn public_bundle_for_epoch(
        &self,
        source: &dyn WorldSource,
        epoch: RunEpoch,
    ) -> anyhow::Result<kk_runner::SharedKkpvBundle> {
        if let Some(handle) = self.kkpv_public_store.acquire(epoch) {
            if let Some(bundle) = handle.shared_bundle() {
                return Ok(bundle.clone());
            }
            anyhow::bail!("公开束载荷不是共享形态（发布库被误用）");
        }
        let blocks = source
            .kkpv_public_terrain_blocks(self.engine.plan_hash().bytes(), KKPV_PUBLIC_AUDIENCE_HASH)
            .ok_or_else(|| anyhow::anyhow!("世界源未提供 KKPV 公开地形块"))?;
        let bytes =
            kk_schema::kkpv::encode_bundle(&blocks, &kk_schema::kkpv::KkpvLimits::default())
                .map_err(|e| anyhow::anyhow!("KKPV 公开束编码失败：{e}"))?;
        if blocks.is_empty() {
            return Ok(kk_runner::SharedKkpvBundle::from_vec(bytes));
        }
        let handle = self.kkpv_public_store.publish_shared(
            epoch,
            bytes,
            &kk_schema::kkpv::KkpvLimits::default(),
        )?;
        Ok(handle
            .shared_bundle()
            .expect("publish_shared 载荷恒为共享形态")
            .clone())
    }

    /// 当前代际的束分组资格：世界全部房可共享且空块可共享。同代际
    /// 缓存探测结果。
    pub(super) fn kkpv_grouping_eligible_for_epoch(
        &self,
        source: &dyn WorldSource,
        epoch: RunEpoch,
    ) -> bool {
        {
            let cache = self.kkpv_grouped_bundles.lock().expect("分组束缓存锁");
            if cache.epoch == Some(epoch) {
                return cache.eligible;
            }
        }
        let eligible = source.kkpv_grouping_eligible(self.engine.plan_hash().bytes());
        let mut cache = self.kkpv_grouped_bundles.lock().expect("分组束缓存锁");
        // 代际更替必须整体作废：束字节携带块头 tick，跨代际复用会被解码
        // 端"块头 tick 与信封不一致"拒绝（资格真假与否都一样）。
        cache.epoch = Some(epoch);
        cache.eligible = eligible;
        cache.bundles.clear();
        eligible
    }

    /// 查询当前代际某分组键的共享束句柄；未装配过返回 None（调用方逐
    /// 查看者装配后经 [`EmbedEngine::kkpv_publish_grouped_bundle`] 发布）。
    /// 代际不匹配的陈旧条目按懒惰清理（查询/发布时同代际才可见）。
    pub(super) fn kkpv_grouped_bundle_hit(
        &self,
        epoch: RunEpoch,
        key: &str,
    ) -> Option<kk_runner::SharedKkpvBundle> {
        let cache = self.kkpv_grouped_bundles.lock().expect("分组束缓存锁");
        (cache.epoch == Some(epoch))
            .then(|| cache.bundles.get(key).cloned())
            .flatten()
    }

    /// 发布某分组键的共享束（首个装配该组的查看者产物）；同代际已有
    /// 条目时保留既有句柄。
    pub(super) fn kkpv_publish_grouped_bundle(
        &self,
        epoch: RunEpoch,
        key: &str,
        bundle: kk_runner::SharedKkpvBundle,
    ) {
        let mut cache = self.kkpv_grouped_bundles.lock().expect("分组束缓存锁");
        if cache.epoch == Some(epoch) {
            cache.bundles.entry(key.to_string()).or_insert(bundle);
        }
    }

    /// 读取公开束保留报告（固定内存测量面）：仍被句柄持有的代际、
    /// 字节数与库外句柄数。V8 侧 ArrayBuffer 引用只延长真实存活，
    /// 不进入读数——报告是存活下界。
    pub fn kkpv_public_retention_report(&self) -> Vec<crate::kkpv_store::RetainedBlock> {
        self.kkpv_public_store.retention_report()
    }

    /// 切换世界注入通道：true 走 KKPV 二进制块束，false（默认）走 JSON
    /// 参考通道。两条通道装配出的玩家可见面等价，集成测试按此锁定。
    pub fn set_kkpv_channel(&mut self, enabled: bool) {
        self.kkpv_channel = enabled;
    }

    /// 采样指定玩家当前世界的注入体积（诊断/基准用）：JSON 通道是整
    /// 世界串字节，KKPV 通道是信封串 + 私有束字节 + 公开束字节（共享，
    /// 全玩家一份）+ 公开束保留报告。只读世界、复用与派发相同的投影
    /// 入口（含 market 注入），不在任何计时窗口内调用。
    pub fn injection_size_sample(&self, handle: &str) -> anyhow::Result<Value> {
        let source: &dyn WorldSource = self.engine.world();
        let gcl = self
            .engine
            .players()
            .get(handle)
            .map(|p| p.gcl)
            .unwrap_or(0);
        if self.kkpv_channel {
            let audience = kkpv_audience_hash(handle);
            let (mut envelope, bundle) = source
                .kkpv_bundle_bytes_for_viewer(
                    handle,
                    gcl,
                    self.engine.extensions(),
                    self.player_surface.as_deref(),
                    self.engine.plan_hash().bytes(),
                    audience,
                )
                .ok_or_else(|| anyhow::anyhow!("世界源未提供 KKPV 投影"))?
                .map_err(|e| anyhow::anyhow!("KKPV 投影失败：{e}"))?;
            if let Some(obj) = envelope.as_object_mut() {
                obj.insert("market".into(), market_view(&self.engine, handle));
            }
            let blocks = kk_schema::kkpv::bundle_block_slices(&bundle)
                .map_err(|e| anyhow::anyhow!("KKPV 束分帧失败：{e}"))?
                .len();
            // 公开束按当前世界重算字节数（不发布、不动库）；块数与私
            // 有束的房间集一致（全注册房间）。
            let public_blocks = source
                .kkpv_public_terrain_blocks(
                    self.engine.plan_hash().bytes(),
                    KKPV_PUBLIC_AUDIENCE_HASH,
                )
                .ok_or_else(|| anyhow::anyhow!("世界源未提供 KKPV 公开地形块"))?;
            let public_bundle = kk_schema::kkpv::encode_bundle(
                &public_blocks,
                &kk_schema::kkpv::KkpvLimits::default(),
            )
            .map_err(|e| anyhow::anyhow!("KKPV 公开束编码失败：{e}"))?;
            let retention: Vec<Value> = self
                .kkpv_public_retention_report()
                .into_iter()
                .map(|r| {
                    json!({
                        "tick": r.epoch.tick,
                        "generation": r.epoch.generation,
                        "bytes": r.bytes,
                        "handles": r.handles,
                    })
                })
                .collect();
            Ok(json!({
                "channel": "kkpv",
                "envelope_bytes": serde_json::to_string(&envelope)?.len(),
                "bundle_bytes": bundle.len(),
                "public_bundle_bytes": public_bundle.len(),
                "public_shared": true,
                "blocks": blocks,
                "public_retained": retention,
            }))
        } else {
            let mut world = source.world_json_for_viewer_with_extensions(
                handle,
                gcl,
                self.engine.extensions(),
                self.player_surface.as_deref(),
            );
            if let Some(obj) = world.as_object_mut() {
                obj.insert("market".into(), market_view(&self.engine, handle));
            }
            Ok(json!({
                "channel": "json",
                "world_json_bytes": serde_json::to_string(&world)?.len(),
            }))
        }
    }
}

/// 构造玩家 power 账号视图：GPL 源 power、配额计数、墙钟 now（官方
/// spawnCooldownTime 门与 Date.now() 同源）与 powerCreep 注册表。prelude
/// 以 world.power_account 读取（Game.gpl/Game.powerCreeps/类 getter 面）。
pub(super) fn power_account_view(
    engine: &kk_engine::settle::Engine,
    viewer: &str,
    now_ms: u64,
) -> Value {
    let empty = kk_engine::settle::PlayerState::default();
    let p = engine.players().get(viewer).unwrap_or(&empty);
    let power_creeps: serde_json::Map<String, Value> = p
        .power_creeps
        .iter()
        .map(|(id, r)| {
            let powers: serde_json::Map<String, Value> = r
                .powers
                .iter()
                .map(|(pid, level)| (pid.to_string(), json!({ "level": level })))
                .collect();
            (
                kk_world::ObjId(*id).as_str().to_string(),
                json!({
                    // 注册表按 viewer 查询，owner 恒为本人（未生成记录的
                    // my/owner getter 面依赖此键）。
                    "owner": viewer,
                    "name": r.name,
                    "className": r.class_name,
                    // 单 shard 世界：已生成记录属于 shard0，未生成/死亡态无 shard。
                    "shard": r.spawn_cooldown_time.is_none().then_some("shard0"),
                    "level": r.level,
                    "hitsMax": r.hits_max,
                    "storeCapacity": r.store_capacity,
                    "powers": Value::Object(powers),
                    // 三态与 PlayerState 注释同义：null=已生成；数值=冷却
                    //（0 为新建未生成）。
                    "spawnCooldownTime": match r.spawn_cooldown_time {
                        Some(t) => json!(t),
                        None => Value::Null,
                    },
                    "deleteTime": match r.delete_time {
                        Some(t) => json!(t),
                        None => Value::Null,
                    },
                }),
            )
        })
        .collect();
    json!({
        "power": p.power,
        "now": now_ms,
        "powerCreeps": Value::Object(power_creeps),
    })
}

/// 构造玩家市场视图：收支交易各取最近 100 条，金额和价格保留千分之一 credit 单位。
/// 交易中的玩家名保持字符串，由 prelude 转成 `{username}` 并换算价格。
pub(super) fn market_view(engine: &kk_engine::settle::Engine, viewer: &str) -> Value {
    fn tx_list(engine: &kk_engine::settle::Engine, viewer: &str, outgoing: bool) -> Vec<Value> {
        engine
            .market()
            .transactions_for(viewer, outgoing, 100)
            .iter()
            .map(|t| {
                let mut v = json!({
                    "id": t.id.to_string(),
                    "time": t.time,
                    "resourceType": t.resource_type,
                    "amount": t.amount,
                    "from": t.from,
                    "to": t.to,
                });
                if let Some(d) = &t.description {
                    v["description"] = json!(d);
                }
                if let Some(o) = &t.order {
                    v["order"] = o.clone();
                }
                if let Some(s) = &t.sender {
                    v["sender"] = json!(s);
                }
                if let Some(r) = &t.recipient {
                    v["recipient"] = json!(r);
                }
                v
            })
            .collect()
    }
    // 活动订单按资源分组，本人订单还包括非活动订单；价格保持内部单位。
    // 历史行情当前未实现，返回只有 all 空数组的占位对象。
    let mut orders_all = serde_json::Map::new();
    let mut by_rt: BTreeMap<String, serde_json::Map<String, serde_json::Value>> = BTreeMap::new();
    for order in engine.market().orders.values().filter(|o| o.active) {
        let v = order_projection(order);
        by_rt
            .entry(order.resource_type.clone())
            .or_default()
            .insert(order.id.clone(), v.clone());
        orders_all.insert(order.id.clone(), v);
    }
    let mut orders = serde_json::Map::new();
    orders.insert("all".into(), Value::Object(orders_all));
    for (rt, m) in by_rt {
        orders.insert(rt, Value::Object(m));
    }
    let my_orders: Vec<Value> = engine
        .market()
        .orders
        .values()
        .filter(|o| o.user == viewer)
        .map(order_projection)
        .collect();
    // 跨分片资源余额用于市场 deal 的资源检查。
    let mut resources = serde_json::Map::new();
    if let Some(p) = engine.players().get(viewer) {
        for (k, v) in &p.resources {
            resources.insert(k.clone(), json!(v));
        }
    }
    json!({
        "money_milli": engine.players().get(viewer).map(|p| p.money).unwrap_or(0),
        "resources": Value::Object(resources),
        "transactions": {
            "outgoing": tx_list(engine, viewer, true),
            "incoming": tx_list(engine, viewer, false),
        },
        "orders": Value::Object(orders),
        "my_orders": my_orders,
        "history": { "all": [] },
    })
}

/// 序列化订单内部字段，价格保留千分之一 credit 单位，时间保留绝对值。
/// 面向玩家的价格换算和 `user` 字段移除由 prelude 完成。
fn order_projection(o: &kk_engine::MarketOrder) -> Value {
    json!({
        "id": o.id,
        "user": o.user,
        "active": o.active,
        "type": o.order_type,
        "resourceType": o.resource_type,
        "price": o.price,
        "amount": o.amount,
        "remainingAmount": o.remaining_amount,
        "totalAmount": o.total_amount,
        "roomName": o.room_name,
        "created": o.created,
        "createdTimestamp": o.created_timestamp,
    })
}
