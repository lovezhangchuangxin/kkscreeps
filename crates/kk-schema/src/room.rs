//! 单房间快照的字段定义和完整二进制编解码。
//!
//! 房间名由外层关联，不写入此载荷。字段中的对象 id 为内部 u64；部件码的
//! 名称映射、资源名解释和玩家可见 JSON 由 kk-world 处理。
//! 这里保存原始字段，不验证地形位码、对象坐标、资源名或 id 唯一性。
//! 恢复成可运行房间时，还须经过 kk-world 的 RoomState::decode_snapshot 校验。
//! 上游字段来源核对 Screeps engine a90d26fc9eb943147799e65d0bf9d4d0d79a2722；
//! 下文的 engine/src 路径均相对此仓库。

use crate::{Cursor, Schema, SchemaError};

/// 当前快照版本号，写入封套；开发期只接受当前格式，不迁移旧数据。
/// 字段的增删、顺序、类型或标签变化须提升版本，避免按新布局误读旧载荷。
pub const SCHEMA_VERSION: u16 = 2;
/// 封套的格式标识，小端字节为 `50 53 4B 4B`，即 ASCII "PSKK"。
pub const SNAPSHOT_MAGIC: u32 = 0x4b4b_5350;

crate::kk_schema! {
    /// 房间快照头部，所有字段定长，共 2524 字节。
    /// 时间、种子和下一个对象 id 由编码调用方提供，不从房间对象表推导。
    /// 地形按行存放，每格一个位码，索引为 y * 50 + x。
    pub RoomHeader {
        (pub time: u64)
        (pub seed: u64)
        (pub next_obj_id: u64)
        (pub terrain: [u8; 2500])
    }
}

crate::kk_schema! {
    /// 固定资源槽：energy 与 ghodium，数量以资源单位计。
    /// ghodium 的玩家可见资源名为 "G"；其他资源通过对象的 store_extra 保存。
    /// 编码层允许数量为 0，不检查容量或资源列表是否重复。
    pub Store {
        (pub energy: u32)
        (pub ghodium: u32)
    }
}

#[allow(clippy::derivable_impls)]
impl Default for Store {
    fn default() -> Self {
        Self {
            energy: 0,
            ghodium: 0,
        }
    }
}

crate::kk_schema! {
    /// 身体部件，kind 是 kk-world::types::BodyKind 的部件码。
    /// hits 为该部件的生命值，boost 为可选强化资源名；编码层不校验这些值。
    pub BodyPart {
        (pub kind: u8)
        (pub hits: u32)
        (pub boost: opt str)
    }
}

crate::kk_schema! {
    /// 孵化任务：need_time 是所需 tick 数，spawn_time 是预计完成的绝对 tick。
    /// directions 保存允许出生的方向码及其顺序，由孵化结算解释。
    pub SpawningJob {
        (pub name: str)
        (pub need_time: u32)
        (pub spawn_time: u64)
        (pub directions: vec u8)
    }
}

crate::kk_schema! {
    /// 爬爬虫的状态，包括部件、资源、寿命和最近一次结算的动作记录。
    /// 动作位图记录动作是否发生，不保存上游 actionLog 中的目标坐标。
    /// 具体位定义见 kk-engine/src/settle/creep/mod.rs 的 ACTION_* 常量。
    pub CreepObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub fatigue: u32)
        // 寿命结束的绝对 tick；0 表示尚未初始化，孵化结束后由结算设置。
        (pub age_time: u64)
        (pub hits: u32)
        (pub spawning: bool)
        (pub action_log: u16)
        (pub name: str)
        (pub owner: str)
        (pub body: vec BodyPart)
        (pub store: Store)
        (pub store_extra: vec ResourceStack)
        // 结算按 10 个 UTF-16 码元截取说话文本，截断的代理对转成替换字符。
        // 来源：kk-engine/src/settle/creep/lifecycle.rs 的 intent_say、utf16_truncate。
        // 文本与公开标志随 action_log 在每次房间结算开始时清空。
        (pub say_message: opt str)
        (pub say_public: opt bool)
        // 通知标志跨 tick 保留；None 与 Some(false) 不等，含义见 SpawnObj。
        (pub notify_when_attacked: opt bool)
    }
}

crate::kk_schema! {
    /// 能量源。next_regeneration_time 是再生的绝对 tick，None 表示未设置。
    /// energy 与 energy_capacity 分别为当前能量和可再生到的容量。
    pub SourceObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub energy: u32)
        (pub energy_capacity: u32)
        (pub next_regeneration_time: opt u64)
    }
}

crate::kk_schema! {
    /// 控制器预订。user 在本项目中是玩家名，end_time 是结束的绝对 tick。
    /// 上游对应 user/endTime（engine/src/processor/intents/creeps/reserveController.js）。
    pub Reservation {
        (pub user: str)
        (pub end_time: u64)
    }
}

crate::kk_schema! {
    /// 控制器签名。time 为签名时的游戏 tick，datetime 为可选 Unix 毫秒时间。
    /// datetime 来自结算的 TickInput，保存后读取不重新取墙钟；旧夹具可缺省。
    /// 上游字段来源：engine/src/processor/intents/creeps/signController.js。
    pub Sign {
        (pub user: str)
        (pub text: str)
        (pub time: u64)
        (pub datetime: opt u64)
    }
}

crate::kk_schema! {
    /// 控制器状态。safe_mode、downgrade_time、upgrade_blocked 和
    /// safe_mode_cooldown 都保存绝对 tick，不是剩余时长；safe_mode 为 0 表示关闭。
    /// 可选时间为 None 表示未设置，玩家可见的剩余时间由投影层计算。
    /// owner_cleared 区分无主的两种输出：owner 缺失且该位为 true 时输出 null；
    /// 为 false 时省略 owner 字段。owner 有值时以该值为准。
    pub ControllerObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub level: u8)
        (pub safe_mode: u64)
        (pub safe_mode_available: u32)
        (pub progress: u64)
        (pub downgrade_time: opt u64)
        (pub owner: opt str)
        (pub owner_cleared: bool)
        (pub is_power_enabled: bool)
        (pub reservation: opt Reservation)
        (pub sign: opt Sign)
        (pub upgrade_blocked: opt u64)
        (pub safe_mode_cooldown: opt u64)
    }
}

crate::kk_schema! {
    /// 孵化器的生命值、资源和当前孵化任务。
    /// off 表示控制器等级允许的数量不足时停用，由孵化结算维护。
    /// notify_when_attacked 为 None 表示尚无该字段，与 Some(false) 有区别；
    /// 玩家 API 会据此判断是否需要提交设置指令。新建结构通常写入 Some(true)。
    pub SpawnObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub hits: u32)
        (pub off: bool)
        (pub name: str)
        (pub owner: str)
        (pub store: Store)
        (pub spawning: opt SpawningJob)
        (pub notify_when_attacked: opt bool)
    }
}

crate::kk_schema! {
    /// 扩展的生命值、所有者和资源。off 与通知标志的含义见 SpawnObj。
    /// 资源容量由世界与结算层按控制器等级计算，不保存在 Store 中。
    pub ExtensionObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub hits: u32)
        (pub off: bool)
        (pub owner: str)
        (pub store: Store)
        (pub notify_when_attacked: opt bool)
    }
}

crate::kk_schema! {
    /// 建造完成的墙，保存当前生命值；生命值上限由结算层查表。
    /// 通知标志保留未设置、开启和关闭三种状态，含义见 SpawnObj。
    pub ConstructedWallObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub hits: u32)
        (pub notify_when_attacked: opt bool)
    }
}

crate::kk_schema! {
    /// 道路。next_decay_time 保存下一次衰减的绝对 tick，0 表示未设置。
    /// 到期结算会扣除生命值，降到 0 时才移除道路；未移除时重新设置衰减时间。
    /// 来源：上游 engine/src/processor/intents/roads/tick.js。
    pub RoadObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub hits: u32)
        (pub next_decay_time: u64)
        (pub notify_when_attacked: opt bool)
    }
}

crate::kk_schema! {
    /// 仓库，使用固定资源槽和按名称保存的额外资源列表。
    /// 所有资源共享 store_capacity，结算对象 tick 更新并保存；编码不校验是否超限。
    /// 通知标志含义见 SpawnObj。
    pub StorageObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub hits: u32)
        (pub owner: str)
        (pub store: Store)
        (pub notify_when_attacked: opt bool)
        (pub store_extra: vec ResourceStack)
        (pub store_capacity: i64)
    }
}

crate::kk_schema! {
    /// 工地。progress_total 保存创建时确定的总工程量，后续不从地形重算。
    /// 放置入口使用基础建造成本，建造指令创建道路时会乘地形系数。
    /// structure_type 保存结构类型名称；owner、name 可缺省。
    /// 来源：kk-world/src/place/place_structure.rs 的 normalize_site，以及
    /// kk-engine/src/settle/room.rs 的 create_construction_site。
    pub ConstructionSiteObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub structure_type: str)
        (pub progress: u32)
        (pub progress_total: u32)
        (pub owner: opt str)
        (pub name: opt str)
    }
}

crate::kk_schema! {
    /// 掉落资源。resource 有值时由它确定资源名；否则 resource_type
    /// 按 0=energy、1=power、2=G 解释，其他码在世界投影中也按 energy 处理。
    /// 编码保留两个字段，不替调用方选择资源名。amount 为当前数量，衰减按 tick
    /// 扣除数量，没有独立到期时间；见 kk-engine::settle::economy::energy_tick。
    pub EnergyObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub resource_type: u8)
        (pub amount: u32)
        (pub resource: opt str)
    }
}

crate::kk_schema! {
    /// 一个资源名和数量，供固定 energy/G 槽之外的资源使用。
    /// 编码层允许数量为 0、名称重复或与固定槽重叠；构造方负责保持列表一致。
    /// 投影只输出正量项，重名项会覆盖已有 JSON 键，并不会自动求和。
    pub ResourceStack {
        (pub resource: str)
        (pub amount: u32)
    }
}

crate::kk_schema! {
    /// 墓碑，保存死亡时的对象信息和剩余资源。
    /// death_time、decay_time 是绝对 tick；creep_ticks_to_live 是死亡时的剩余 tick 数。
    /// creep_body 保存部件码，creep_saying 保存死亡时的公开遗言，私有或缺失时为 None。
    /// 来源：上游 engine/src/processor/intents/creeps/_die.js；本项目的资源分槽
    /// 和部件码由世界层转换为玩家可见格式。
    pub TombstoneObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub death_time: u64)
        (pub decay_time: u64)
        (pub creep_id: opt u64)
        (pub creep_name: opt str)
        (pub creep_ticks_to_live: u32)
        (pub creep_body: vec u8)
        (pub creep_saying: opt str)
        (pub owner: opt str)
        (pub store: Store)
        (pub store_extra: vec ResourceStack)
    }
}

crate::kk_schema! {
    /// 废墟，保存被摧毁结构的 id、类型、生命值上限和承接的资源。
    /// destroy_time、decay_time 为绝对 tick；structure_hits_max 缺省时为 None。
    /// 原结构 id 与废墟自身 id 不同，玩家可见字符串由世界层生成。
    /// 来源：上游 engine/src/processor/intents/structures/_destroy.js。
    pub RuinObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub structure_id: u64)
        (pub structure_type: str)
        (pub structure_hits_max: opt u32)
        (pub destroy_time: u64)
        (pub decay_time: u64)
        (pub owner: opt str)
        (pub store: Store)
        (pub store_extra: vec ResourceStack)
    }
}

crate::kk_schema! {
    /// 矿物，保存资源名、存量、密度码和再生时间。
    /// density 通常为 1..=4，next_regeneration_time 是绝对 tick，None 表示未设置。
    /// 编码层不校验密度或矿物名称；再生规则见 kk-engine::settle::economy::mineral_tick。
    pub MineralObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub mineral_type: str)
        (pub mineral_amount: u32)
        (pub density: u8)
        (pub next_regeneration_time: opt u64)
    }
}

crate::kk_schema! {
    /// 提取器。cooldown 是剩余 tick 的计数器，结算时递减，0 表示无冷却。
    /// owner 可缺省；通知标志含义见 SpawnObj。
    /// 与控制器的冷却字段不同，cooldown 不存绝对 tick。
    /// 来源：上游 engine/src/processor/intents/extractors/tick.js。
    pub ExtractorObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub hits: u32)
        (pub owner: opt str)
        (pub cooldown: u64)
        (pub notify_when_attacked: opt bool)
    }
}

crate::kk_schema! {
    /// 以 JSON 字符串保存未定义专用结构体的对象，如 tower、rampart 和 terminal。
    /// 世界层还原 JSON，结算层按 structureType 选择已实现的规则；不是统一跳过结算。
    /// JSON 对象投影时以外层 id/x/y 覆盖对应键。编码层不验证 data 是有效 JSON。
    pub GenericObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub data: str)
    }
}

kk_schema! {
    /// 旗帜在本项目中属于房间对象；上游则使用独立 flags 集合。
    /// owner 有值时按玩家过滤；None 为无主旗，在本项目查看者投影中仍可见。
    /// 上游集合来源：engine/src/processor.js 的 processRoom（flags、bulkFlags）。
    /// 两名玩家可分别拥有同名旗，查找与移除须同时考虑 owner。
    pub FlagObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub name: str)
        (pub color: u8)
        (pub secondary_color: u8)
        (pub owner: opt str)
    }
}

crate::kk_schema! {
    /// Mod 声明类型的对象包络：type_key 是冻结计划里的类型稳定键
    /// （namespace:name），data 是已按类型描述符校验的字段 JSON。
    /// x/y 与 data 中声明的位置字段保持同步（写入侧维护）；编码层
    /// 不验证 type_key 存在于计划，也不解析 data。
    pub ModObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub type_key: str)
        (pub data: str)
    }
}

crate::kk_room_object! {
    /// 房间对象，编码为单字节标签加对应载荷；标签值见 [`tags`]。
    /// 访问器和编解码不检查 id 唯一性或坐标范围。
    pub enum RoomObject {
        Creep(CreepObj) = tags::CREEP,
        Source(SourceObj) = tags::SOURCE,
        Controller(ControllerObj) = tags::CONTROLLER,
        Spawn(SpawnObj) = tags::SPAWN,
        Extension(ExtensionObj) = tags::EXTENSION,
        ConstructedWall(ConstructedWallObj) = tags::CONSTRUCTED_WALL,
        ConstructionSite(ConstructionSiteObj) = tags::CONSTRUCTION_SITE,
        Energy(EnergyObj) = tags::ENERGY,
        Flag(FlagObj) = tags::FLAG,
        Road(RoadObj) = tags::ROAD,
        Storage(StorageObj) = tags::STORAGE,
        /// 墓碑，死亡信息与资源。
        Tombstone(TombstoneObj) = tags::TOMBSTONE,
        /// 废墟，被摧毁结构的信息与资源。
        Ruin(RuinObj) = tags::RUIN,
        /// 矿物，存量与再生时间。
        Mineral(MineralObj) = tags::MINERAL,
        /// 提取器，采矿所需结构与冷却计数器。
        Extractor(ExtractorObj) = tags::EXTRACTOR,
        /// 以 JSON 保存的对象，按结构类型参与已实现的结算规则。
        Generic(GenericObj) = tags::GENERIC,
        /// Mod 声明类型的对象，字段经类型描述符校验后整体存放。
        Mod(ModObj) = tags::MOD,
    }
}

/// 对象编码的首字节标签。已有值不可随枚举顺序重新编号，新增类型使用未占用值。
/// 修改既有标签会改变格式，须同时处理旧版本读取。
pub mod tags {
    pub const CREEP: u8 = 0;
    pub const SOURCE: u8 = 1;
    pub const CONTROLLER: u8 = 2;
    pub const SPAWN: u8 = 3;
    pub const EXTENSION: u8 = 4;
    pub const CONSTRUCTED_WALL: u8 = 5;
    pub const CONSTRUCTION_SITE: u8 = 6;
    pub const ENERGY: u8 = 7;
    pub const FLAG: u8 = 8;
    pub const ROAD: u8 = 9;
    pub const STORAGE: u8 = 10;
    pub const GENERIC: u8 = 11;
    pub const TOMBSTONE: u8 = 12;
    pub const RUIN: u8 = 13;
    pub const MINERAL: u8 = 14;
    pub const EXTRACTOR: u8 = 15;
    pub const MOD: u8 = 16;
}

crate::kk_schema! {
    /// 单房间的头部、对象列表和事件记录。
    /// kk-world 编码时按 id 升序提供对象，本类型本身不排序或拒绝重复 id。
    /// events 是最近一次房间结算的事件 JSON 字符串，结算每 tick 整体替换；
    /// 编码不解析 JSON。房间名由快照的外层键关联。
    pub RoomSnapshot {
        (pub header: RoomHeader)
        (pub objects: vec RoomObject)
        (pub events: vec str)
    }
}

/// 编码完整快照：4 字节小端魔数、2 字节小端版本、RoomSnapshot 载荷。
/// 仅序列化字段，不校验世界状态，也不执行磁盘写入。
pub fn encode_snapshot(s: &RoomSnapshot) -> Vec<u8> {
    let mut out = Vec::with_capacity(2600);
    out.extend_from_slice(&SNAPSHOT_MAGIC.to_le_bytes());
    out.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
    s.encode_into(&mut out);
    out
}

/// 解码完整快照，先检查魔数和版本，再读取全部载荷。
/// 只接受当前版本；旧版本和未来版本均直接拒绝。
/// 返回成功只表示二进制格式有效，不保证对象或地形可用于世界运行。
///
/// # Errors
/// 头部或载荷截断、魔数不符、不支持的版本、非法标志/标签、非 UTF-8 字符串，
/// 或载荷后仍有额外字节时返回错误。
pub fn decode_snapshot(bytes: &[u8]) -> Result<RoomSnapshot, SchemaError> {
    let mut cur = Cursor::new(bytes);
    let magic = u32::decode_from(&mut cur)?;
    if magic != SNAPSHOT_MAGIC {
        return Err(SchemaError::BadMagic(magic));
    }
    let version = u16::decode_from(&mut cur)?;
    if version != SCHEMA_VERSION {
        return Err(SchemaError::UnsupportedVersion(version));
    }
    let snap = RoomSnapshot::decode_from(&mut cur)?;
    if cur.remaining() != 0 {
        return Err(SchemaError::TrailingBytes(cur.remaining()));
    }
    Ok(snap)
}

#[cfg(test)]
mod tests;
