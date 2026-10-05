//! 校验分片配置，装配世界、玩家和 Mod，并提供放置入口。
use super::*;

/// `controller: false` 与显式控制器字段互斥（screeps-ok 契约，adapter.ts:45-48）。
const CONTROLLER_ONLY_FIELDS: [&str; 6] = [
    "rcl",
    "owner",
    "safeMode",
    "safeModeAvailable",
    "ticksToDowngrade",
    "powerEnabled",
];

struct RoomSetup<'a> {
    name: &'a str,
    terrain: Terrain,
    spec: &'a Value,
}

fn room_setups(spec: &Value) -> anyhow::Result<Vec<RoomSetup<'_>>> {
    let mut setups = Vec::new();
    let mut seen = BTreeSet::new();
    let rooms = spec
        .get("rooms")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("ShardSpec.rooms 必须是数组"))?;
    for room in rooms {
        let name = room
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("RoomSpec.name 必须是非空字符串"))?;
        if !seen.insert(name) {
            anyhow::bail!("重复房间名: {name}");
        }
        // RoomPosition 用偏移 128 后的 8 位坐标编码房间；超出 -128..=127
        // 会溢出或产生别名，须在世界注入沙箱前拒绝。
        let parsed =
            kk_core::RoomName::parse(name).map_err(|e| anyhow::anyhow!("RoomSpec {name}: {e}"))?;
        if !(-128..=127).contains(&parsed.xx) || !(-128..=127).contains(&parsed.yy) {
            anyhow::bail!(
                "RoomSpec {name}: 世界界外房间坐标 ({}, {})，须在 ±127 内",
                parsed.xx,
                parsed.yy
            );
        }
        if room.get("controller").and_then(Value::as_bool) == Some(false) {
            let set: Vec<&str> = CONTROLLER_ONLY_FIELDS
                .iter()
                .copied()
                .filter(|k| room.get(*k).is_some())
                .collect();
            if !set.is_empty() {
                anyhow::bail!(
                    "RoomSpec {name}: controller: false takes no {}",
                    set.join(", ")
                );
            }
        }
        // 通过浮点数读取再检查整数性，使 JSON 3.0 与 JavaScript 的整数 3 等价。
        // 在转成控制器等级前检查范围，避免截断或夹取掩盖非法输入。
        if let Some(value) = room.get("rcl")
            && !value
                .as_f64()
                .is_some_and(|rcl| rcl.fract() == 0.0 && (0.0..=8.0).contains(&rcl))
        {
            anyhow::bail!("RoomSpec {name}: rcl 非法（需 0..=8 整数）: {value}");
        }
        let mut terrain = match room.get("terrain") {
            Some(value) => {
                Terrain::from_codes(&terrain_codes_from_json(value)?).map_err(anyhow::Error::msg)?
            }
            None => Terrain::plains(),
        };
        terrain.normalize();
        setups.push(RoomSetup {
            name,
            terrain,
            spec: room,
        });
    }
    Ok(setups)
}

/// 校验玩家名及 GCL，统一转成累计点数；power 尚未接入账户状态。
/// 默认等级给已有房间留一次扩张空间，来源：screeps-ok PlayerSpec/gclPoints。
fn player_setups(spec: &Value) -> anyhow::Result<Vec<(String, i64)>> {
    let mut handles = Vec::new();
    let mut seen = BTreeSet::new();
    let players = spec
        .get("players")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("ShardSpec.players 必须是数组"))?;
    for entry in players {
        let name = match entry {
            Value::String(name) => name.as_str(),
            Value::Object(fields) => fields.get("name").and_then(Value::as_str).unwrap_or(""),
            _ => "",
        };
        if name.is_empty() {
            anyhow::bail!("PlayerSpec.name 必须是非空字符串");
        }
        if !seen.insert(name) {
            anyhow::bail!("重复玩家 handle: {name}");
        }
        let owned_rooms = spec["rooms"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|room| room["owner"].as_str() == Some(name))
            .count();
        let default_level = (owned_rooms + 1).max(2) as f64;
        let (level, progress) = match entry.get("gcl") {
            None => (default_level, 0.0),
            Some(gcl) => {
                let level = gcl
                    .get("level")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| anyhow::anyhow!("PlayerSpec {name}: gcl.level 必须为正整数"))?;
                let progress = match gcl.get("progress") {
                    None => 0.0,
                    Some(value) => value.as_f64().ok_or_else(|| {
                        anyhow::anyhow!("PlayerSpec {name}: gcl.progress 必须为非负整数")
                    })?,
                };
                (level, progress)
            }
        };
        let threshold = |level: f64| {
            (kk_core::constants::GCL_MULTIPLY as f64
                * (level - 1.0).powf(kk_core::constants::GCL_POW))
            .ceil()
        };
        let points = threshold(level) + progress;
        let reads_back = (points / kk_core::constants::GCL_MULTIPLY as f64)
            .powf(1.0 / kk_core::constants::GCL_POW)
            .floor()
            + 1.0;
        // 玩家 JS 使用 Number，拒绝不能精确表示的点数，避免饱和转换或等级失真。
        if level < 1.0
            || level.fract() != 0.0
            || progress < 0.0
            || progress.fract() != 0.0
            || !points.is_finite()
            || points > 9_007_199_254_740_991.0
            || points >= threshold(level + 1.0)
            || reads_back != level
        {
            anyhow::bail!("PlayerSpec {name}: gcl 等级、进度或累计点数非法");
        }
        handles.push((name.to_string(), points as i64));
    }
    Ok(handles)
}

/// 解析 JSON 地形字节数组，元素须为整数类型且在 0..=255 内。
/// 数组长度和地形位是否合法，另由 [`Terrain::from_codes`] 校验。
pub fn terrain_codes_from_json(v: &Value) -> anyhow::Result<Vec<u8>> {
    let array = v
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("terrain 必须是数值数组"))?;
    array
        .iter()
        .map(|x| {
            let n = x
                .as_i64()
                .filter(|n| *n >= 0 && *n <= u8::MAX as i64)
                .ok_or_else(|| anyhow::anyhow!("地形元素非法: {x}"))?;
            Ok(n as u8)
        })
        .collect()
}

impl EmbedEngine {
    /// 校验房间和玩家输入后，回收旧沙箱并建立新分片。
    /// 输入校验失败时保留旧分片；后续沙箱注册或对象初始化失败不恢复旧分片。
    pub fn create_shard(&mut self, spec: &Value) -> anyhow::Result<()> {
        self.create_shard_inner(spec, None)
    }

    /// `mod_install` 在引擎重置后、玩家沙箱 spawn 前生效——玩家常量与
    /// 结算读同一份已装配规则，不允许两个平面看到不同的 link 规则。
    pub(super) fn create_shard_inner(
        &mut self,
        spec: &Value,
        mod_install: Option<(&BTreeMap<String, ModPackage>, &RegisterOptions)>,
    ) -> anyhow::Result<()> {
        // 输入校验先于回收旧沙箱，失败时保留现有分片；其后的 mod 注册、
        // 玩家面导出与 canary 安装失败发生在回收之后——旧分片已销毁、
        // 引擎已重置为空（零房间零玩家），宿主须重新调用创建接口。
        let rooms = room_setups(spec)?;
        let handles = player_setups(spec)?;

        if let Some(p) = &mut self.players {
            p.shutdown_all();
        }
        self.engine = Engine::default();
        self.mod_realm = None;
        self.ext_intents = false;
        self.player_surface = None;
        self.mod_player_api = None;
        self.pending = None;
        self.execution_failure = None;
        self.fail_after_player_for_test = None;
        self.last_console.clear();
        self.perf = PerfRecorder::default();
        self.execution_order_for_test = None;
        self.last_execution_order = None;
        self.fail_next_settle = false;
        self.fail_next_settle_room = None;
        self.fail_next_settle_stage = None;
        self.fail_next_writeback = None;
        // 新分片从 tick 0 重新计代；旧公开束句柄全部作废（沙箱先于此
        // 回收，不存在跨分片的合法持有者）。
        self.kkpv_public_store = crate::kkpv_store::KkpvBlockStore::new();

        if let Some((packages, options)) = mod_install {
            let mut realm = ModRealm::with_options(options)
                .map_err(|error| anyhow::anyhow!("mod realm：{error}"))?;
            let registered = realm
                .register(packages, options.clone())
                .map_err(|error| anyhow::anyhow!("mod 注册：{error}"))?;
            // 玩家面目录先于引擎装配导出：保留键冲突/类名非法/条件可见性
            // 在此失败，不产出半装好的分片。
            let surface = std::sync::Arc::new(
                kk_mod_api::player_surface::PlayerSurface::from_plan(&registered.plan)
                    .map_err(|error| anyhow::anyhow!("mod 玩家面：{error}"))?,
            );
            let mod_player_api = {
                let modules = registered
                    .player_modules
                    .iter()
                    .map(|module| kk_runner::ModPlayerModule {
                        namespace: module.namespace.clone(),
                        entry: module.entry.clone(),
                        files: module.files.clone(),
                    })
                    .collect::<Vec<_>>();
                let types = surface
                    .types
                    .iter()
                    .map(|ty| {
                        (
                            ty.key.as_str().to_string(),
                            ty.find_const,
                            ty.look_key.clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                let intents = surface
                    .intents
                    .iter()
                    .map(|key| key.as_str().to_string())
                    .collect::<Vec<_>>();
                (!modules.is_empty() || !types.is_empty() || !intents.is_empty()).then(|| {
                    std::sync::Arc::new(kk_runner::ModPlayerApi {
                        modules: std::sync::Arc::new(modules),
                        types,
                        intents,
                    })
                })
            };
            // 玩家面安装先在一次性 canary 沙箱上完整验证（清单注入 → 模块
            // 装载 → install 调用 → finalize）：坏 player.js（导入缺陷、
            // install 抛错、缺绑定）在引擎装配与玩家 spawn 之前失败，
            // 不留下「计划已装 + 部分玩家已 spawn」的中间态。
            let has_mod_intents = registered
                .plan
                .intents()
                .iter()
                .any(|intent| intent.key.namespace() != "builtin");
            if let Some(api) = &mod_player_api {
                // 预检与真实玩家安装须观察同一冻结规则，否则 player.js 在
                // 安装时读取容量常量会被 vanilla 值错误拒绝。
                let mut constant_overrides =
                    kk_mod_api::builtin::StoreRules::from_plan(registered.plan.rules())
                        .map_err(anyhow::Error::msg)?
                        .constant_overrides();
                constant_overrides.insert(
                    "LINK_CAPACITY".into(),
                    serde_json::json!(
                        kk_mod_api::builtin::LinkRules::from_plan(registered.plan.rules())
                            .map_err(anyhow::Error::msg)?
                            .capacity
                    ),
                );
                kk_runner::Sandbox::new(kk_runner::SandboxConfig {
                    extension_intents: has_mod_intents,
                    constant_overrides,
                    mod_api: Some(api.clone()),
                    ..kk_runner::SandboxConfig::default()
                })
                .map_err(|error| anyhow::anyhow!("Mod 玩家面预检失败：{error}"))?;
            }
            let realm = Rc::new(RefCell::new(ModVm::new(
                realm,
                packages.clone(),
                options.clone(),
                &registered,
            )));
            let executor = Rc::new(ModSlotExecutor {
                realm: realm.clone(),
            });
            let replacements = registered
                .contributions
                .iter()
                .flat_map(|contribution| &contribution.handlers)
                .filter(|handler| handler.kind == kk_mod_api::HandlerKind::Replace)
                .map(|handler| handler.binding.clone())
                .collect();
            let plan = std::sync::Arc::new(registered.plan.clone());
            self.engine
                .install_hosted_plan(plan, executor, registered.executables.clone(), replacements)
                .map_err(anyhow::Error::msg)?;
            // 装配成功后才置位：失败路径保持 vanilla 语义与标志一致。
            self.ext_intents = has_mod_intents;
            self.player_surface = Some(surface);
            self.mod_player_api = mod_player_api;
            self.mod_realm = Some(realm);
        }

        let players = self.players.get_or_insert_with(ThreadExecutor::new);
        // 先取走全部配置，避免注册中途失败后残留配置被下一次创建意外使用。
        let configs = std::mem::take(&mut self.sandbox_configs_for_test);
        for (handle, gcl) in handles {
            let mut config = configs.get(&handle).cloned().unwrap_or_default();
            // link 与仓库容量同步到玩家全局常量。计划是常量的
            // 权威来源，测试配置里的同名覆盖以引擎规则为准。
            config.constant_overrides.insert(
                "LINK_CAPACITY".to_string(),
                serde_json::json!(self.engine.links().capacity),
            );
            config
                .constant_overrides
                .extend(self.engine.stores().constant_overrides());
            // 扩展意图原语与玩家面安装清单的装配由引擎计划决定，测试
            // 配置不得改写。
            config.extension_intents = self.ext_intents;
            config.mod_api = self.mod_player_api.clone();
            let cpu = kk_core::cpu::Bucket::new(
                config.cpu_limit,
                config.bucket_cap,
                config.max_per_tick,
                config.initial_bucket,
            );
            players.spawn(&handle, config)?;
            self.engine.players.entry(handle.clone()).or_default().cpu = cpu;
            self.engine.players.entry(handle.clone()).or_default().gcl = gcl;
            // 初始余额为 10M credits，内部按千分之一存储。
            // 来源：screeps-ok adapters/vanilla/index.ts 的 createShard 玩家初始化。
            self.engine.players.entry(handle.clone()).or_default().money = 10_000_000_000;
        }
        for setup in rooms {
            let name = setup.name;
            let room = setup.spec;
            self.engine.world.register_room(name, setup.terrain);
            let has_controller = room
                .get("controller")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            if has_controller {
                // 有主控制器默认等级 1，无主默认 0。倒计时转成绝对 tick 时多加 1，
                // 来源：screeps-ok adapters/vanilla/index.ts 的 createShard 控制器初始化。
                let owner = room.get("owner").and_then(Value::as_str);
                let rcl = room
                    .get("rcl")
                    .and_then(Value::as_f64)
                    .map(|f| f as i64)
                    .unwrap_or(if owner.is_some() { 1 } else { 0 })
                    .clamp(0, 8);
                let time = self.engine.world.time();
                let mut spec = serde_json::json!({
                    "pos": [1, 1],
                    "structureType": "controller",
                    "level": rcl,
                    "isPowerEnabled": room
                        .get("powerEnabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
                if let Some(owner) = owner {
                    spec["owner"] = serde_json::json!(owner);
                    let downgrade_table = kk_core::constants::CONTROLLER_DOWNGRADE;
                    let default_clock = downgrade_table
                        .iter()
                        .find(|(k, _)| *k == rcl.to_string())
                        .map(|(_, v)| *v)
                        .unwrap_or(0);
                    let downgrade_time = match room.get("ticksToDowngrade").and_then(Value::as_i64)
                    {
                        Some(t) => Some(time.saturating_add(t.max(0) as u64).saturating_add(1)),
                        None if rcl > 0 => {
                            Some(time.saturating_add(default_clock as u64).saturating_add(1))
                        }
                        None => None,
                    };
                    if let Some(dt) = downgrade_time {
                        spec["downgradeTime"] = serde_json::json!(dt);
                    }
                    if let Some(sm) = room
                        .get("safeMode")
                        .and_then(Value::as_i64)
                        .filter(|v| *v > 0)
                    {
                        spec["safeMode"] =
                            serde_json::json!(time.saturating_add(sm as u64).saturating_add(1));
                    }
                    if let Some(avail) = room.get("safeModeAvailable").and_then(Value::as_i64) {
                        spec["safeModeAvailable"] = serde_json::json!(avail.max(0));
                    }
                }
                let ctx = PlaceCtx {
                    time,
                    links: self.engine.links(),
                    stores: self.engine.stores(),
                    controller: None,
                    name_seq: self.engine.world.next_obj_id() as usize,
                };
                let controller = normalize("structure", &spec, &ctx).map_err(anyhow::Error::msg)?;
                self.engine
                    .world
                    .insert(name, controller)
                    .map_err(anyhow::Error::msg)?;
            }
        }
        Ok(())
    }

    /// 以 Mod 包装建分片：注册、玩家面导出/预检与计划装配发生在引擎
    /// 重置后、玩家沙箱 spawn 前（常量与结算同源）。可装配面是 link 两槽与 creep 资源两槽
    /// 的 before/wrap/after/replace 处理器、规则，以及 Mod 自有动作入口的对象
    /// 域执行链。JS 贡献由整槽宿主组合，native 只经普通 op 修改候选。
    /// 对象域外的执行形态（房间/玩家/
    /// 世界域、队列合并、非严格转换、显式入口）同样显式拒绝。玩家面目录导出与
    /// 安装预检先于引擎装配；后续失败不恢复旧分片，已装配计划也可能留存。
    /// 这与 create_shard 的失败语义一致——调用方
    /// 须以 teardown/重建收尾，不得复用半装状态）。
    pub fn create_shard_with_mods(
        &mut self,
        spec: &Value,
        packages: &BTreeMap<String, ModPackage>,
        options: RegisterOptions,
    ) -> anyhow::Result<()> {
        self.create_shard_inner(spec, Some((packages, &options)))
    }

    /// 按适配器的对象类别和初始字段摆放对象，返回对象 ID。
    /// 字段归一化由 [kk_world::place::normalize] 完成，显式字段优先。
    pub fn place_object(&mut self, room: &str, kind: &str, spec: &Value) -> anyhow::Result<String> {
        self.no_open_round("placeObject")?;
        // 使用单调递增的对象 ID 生成默认名字；对象数量会随消亡减少，不能用作序号。
        let ctx = PlaceCtx {
            time: self.engine.world.time(),
            links: self.engine.links(),
            stores: self.engine.stores(),
            controller: self.engine.world.room(room).and_then(|r| r.controller()),
            name_seq: self.engine.world.next_obj_id() as usize,
        };
        let object = normalize(kind, spec, &ctx).map_err(anyhow::Error::msg)?;
        let id = self
            .engine
            .world
            .insert(room, object)
            .map_err(anyhow::Error::msg)?;
        Ok(id.as_str())
    }

    /// 管理命令：按已装配计划里的 Mod 类型创建对象并放入房间，返回对象 ID。
    /// `fields` 须提供除带默认值外的全部声明字段（挂载字段同样要求）；
    /// 缺失的有默认值字段落入默认，整体通过 schema 校验后写入世界存储。
    /// 声明位置字段的类型以 payload 位置为准（信封坐标同步）。
    pub fn place_mod_object(
        &mut self,
        room: &str,
        type_key: &str,
        fields: &Value,
    ) -> anyhow::Result<String> {
        self.no_open_round("placeModObject")?;
        let descriptor = self
            .engine
            .mod_object_type(type_key)
            .ok_or_else(|| {
                anyhow::anyhow!("类型 {type_key} 不在已装配计划中（Mod 对象须经 Mod 分片创建）")
            })?
            .clone();
        let payload =
            kk_mod_api::objects::prepare_fields(fields, &descriptor).map_err(anyhow::Error::msg)?;
        let (x, y) =
            kk_mod_api::objects::position(&payload, &descriptor).map_err(anyhow::Error::msg)?;
        let object = kk_schema::room::RoomObject::Mod(kk_schema::room::ModObj {
            id: 0,
            x,
            y,
            type_key: type_key.to_string(),
            data: payload.to_string(),
        });
        let id = self
            .engine
            .world
            .insert(room, object)
            .map_err(anyhow::Error::msg)?;
        Ok(id.as_str())
    }

    /// 直接注入测试用市场订单，不经过玩家 API 的资格检查和创建费用结算。
    /// `price` 以 credits 输入，乘 1000 四舍五入后存储；`amount` 初始为 0，
    /// `remainingAmount` 等于 `totalAmount`。`active` 默认 true，
    /// `created` 默认当前 tick，`createdTimestamp` 默认墙钟毫秒。
    pub fn place_market_order(&mut self, spec: &Value) -> anyhow::Result<String> {
        self.no_open_round("placeMarketOrder")?;
        let owner = spec
            .get("owner")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("MarketOrderSpec.owner 必须是字符串"))?
            .to_string();
        let order_type = spec
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("MarketOrderSpec.type 必须是 'buy'/'sell'"))?
            .to_string();
        if order_type != "buy" && order_type != "sell" {
            anyhow::bail!("MarketOrderSpec.type 非法: {order_type}");
        }
        let resource_type = spec
            .get("resourceType")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("MarketOrderSpec.resourceType 必须是字符串"))?
            .to_string();
        let price = spec
            .get("price")
            .and_then(Value::as_f64)
            .ok_or_else(|| anyhow::anyhow!("MarketOrderSpec.price 必须是数字"))?;
        let total_amount = spec
            .get("totalAmount")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow::anyhow!("MarketOrderSpec.totalAmount 必须是整数"))?;
        let id = self.engine.market.next_order_id();
        self.engine.market.insert_order(kk_engine::MarketOrder {
            id: id.clone(),
            user: owner,
            active: spec.get("active").and_then(Value::as_bool).unwrap_or(true),
            order_type,
            resource_type,
            price: (price * 1000.0).round() as i64,
            amount: 0,
            remaining_amount: total_amount,
            total_amount,
            room_name: spec
                .get("roomName")
                .and_then(Value::as_str)
                .map(str::to_string),
            created: spec
                .get("created")
                .and_then(Value::as_u64)
                .unwrap_or(self.engine.world.time()),
            created_timestamp: spec
                .get("createdTimestamp")
                .and_then(Value::as_u64)
                .unwrap_or(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0),
                ),
        });
        Ok(id)
    }

    /// 设置房间地形；存在失败轮或世界已执行过玩家 tick 时拒绝修改。
    pub fn set_terrain(&mut self, room: &str, terrain: Vec<u8>) -> anyhow::Result<()> {
        self.no_open_round("setTerrain")?;
        let mut t = Terrain::from_codes(&terrain).map_err(anyhow::Error::msg)?;
        t.normalize();
        self.engine
            .world
            .set_terrain(room, t)
            .map_err(anyhow::Error::msg)
    }
}
