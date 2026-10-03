//! 单房间快照的字段定义和完整二进制编解码。
//!
//! 房间名由外层关联，不写入此载荷。字段中的对象 id 为内部 u64；部件码的
//! 名称映射、资源名解释和玩家可见 JSON 由 kk-world 处理。
//! 这里保存原始字段，不验证地形位码、对象坐标、资源名或 id 唯一性。
//! 恢复成可运行房间时，还须经过 kk-world 的 RoomState::decode_snapshot 校验。
//! 上游字段来源核对 Screeps engine a90d26fc9eb943147799e65d0bf9d4d0d79a2722；
//! 下文的 engine/src 路径均相对此仓库。

use crate::{Cursor, Schema, SchemaError};

/// 当前快照版本号，写入封套；版本 0 不属于可升级版本。
/// 字段的增删、顺序、类型或标签变化都会改变二进制格式。需保留旧字节可读性时，
/// 必须增加版本并提供升级步骤；仅向结构体末尾追加字段也不会自动兼容旧载荷。
pub const SCHEMA_VERSION: u16 = 1;
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
    /// 所有资源共享容量，容量由世界与结算层处理；编码不校验是否超限。
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

/// 接收完整封套字节并输出下一版本的完整封套；必须同时更新头部版本号。
type UpgradeFn = fn(Vec<u8>) -> Result<Vec<u8>, SchemaError>;
// 第 n 项把版本 n+1 升为 n+2，数组长度保证每个旧版本都有相邻步骤。
// 当前版本为 1，尚无历史版本转换函数。
const UPGRADE_CHAIN: [UpgradeFn; (SCHEMA_VERSION - 1) as usize] = [];

/// 按版本顺序执行升级函数。版本 0 和未来版本始终拒绝，不尝试转换。
/// 调用方保证 from 与封套头部一致；每一步负责更新载荷和封套版本号。
fn upgrade_snapshot(from: u16, mut bytes: Vec<u8>) -> Result<Vec<u8>, SchemaError> {
    if from == 0 || from > SCHEMA_VERSION {
        return Err(SchemaError::UnsupportedVersion(from));
    }
    for step in UPGRADE_CHAIN.iter().skip((from - 1) as usize) {
        bytes = step(bytes)?;
    }
    Ok(bytes)
}

/// 解码完整快照，先检查魔数和版本，再读取全部载荷。
/// 当前版本直接解码；可支持的旧版本先转换完整封套再解码。
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
        // 旧版本：整封套（含头）过升级链，产出当前版本字节后重解码
        let upgraded = upgrade_snapshot(version, bytes.to_vec())?;
        return decode_snapshot(&upgraded);
    }
    let snap = RoomSnapshot::decode_from(&mut cur)?;
    if cur.remaining() != 0 {
        return Err(SchemaError::TrailingBytes(cur.remaining()));
    }
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SchemaError as Se;

    crate::kk_schema! {
        /// 宏边界用例：零字段。
        EmptyShape {}
    }
    crate::kk_schema! {
        /// 宏边界用例：单原始字段。
        SingleField { (pub v: u8) }
    }
    crate::kk_schema! {
        /// 宏边界用例：变长字段打头（其后原始字段不产出偏移）。
        VarFirst {
            (pub name: str)
            (pub n: u32)
            (pub more: vec u8)
        }
    }

    #[test]
    fn macro_edge_shapes() {
        // 零字段：编码为空、解码回空实例
        let e = EmptyShape {};
        let mut buf = Vec::new();
        e.encode_into(&mut buf);
        assert!(buf.is_empty());
        assert_eq!(
            EmptyShape::decode_from(&mut crate::Cursor::new(&buf)).unwrap(),
            e
        );
        // 单字段：u8 小端 + 偏移 0
        let s = SingleField { v: 77 };
        let mut buf = Vec::new();
        s.encode_into(&mut buf);
        assert_eq!(buf, vec![77]);
        assert_eq!(SingleField::OFFSET_v, 0);
        // 变长打头：roundtrip 成立
        let v = VarFirst {
            name: "abc".into(),
            n: 9,
            more: vec![1, 2, 3],
        };
        let mut buf = Vec::new();
        v.encode_into(&mut buf);
        let back = VarFirst::decode_from(&mut crate::Cursor::new(&buf)).unwrap();
        assert_eq!(back, v);
    }

    fn sample_snapshot() -> RoomSnapshot {
        let mut terrain = [0u8; 2500];
        terrain[7] = 1;
        terrain[8] = 2;
        RoomSnapshot {
            header: RoomHeader {
                time: 1234,
                seed: 42,
                next_obj_id: 7,
                terrain,
            },
            objects: vec![
                RoomObject::Creep(CreepObj {
                    id: 1,
                    x: 10,
                    y: 11,
                    fatigue: 6,
                    age_time: 2734,
                    hits: 300,
                    spawning: false,
                    action_log: 0,
                    name: "w1".into(),
                    owner: "p1".into(),
                    body: vec![
                        BodyPart {
                            kind: 0,
                            hits: 100,
                            boost: None,
                        },
                        BodyPart {
                            kind: 1,
                            hits: 50,
                            boost: Some("XUHO2".into()),
                        },
                    ],
                    store: Store {
                        energy: 37,
                        ghodium: 1000,
                    },
                    // 额外资源列表保存固定 energy/G 槽之外的矿物。
                    store_extra: vec![ResourceStack {
                        resource: "U".into(),
                        amount: 12,
                    }],
                    say_message: None,
                    say_public: None,
                    notify_when_attacked: None,
                }),
                RoomObject::Source(SourceObj {
                    id: 2,
                    x: 30,
                    y: 30,
                    energy: 2500,
                    energy_capacity: 3000,
                    next_regeneration_time: Some(1500),
                }),
                RoomObject::Controller(ControllerObj {
                    id: 3,
                    x: 1,
                    y: 1,
                    level: 3,
                    safe_mode: 0,
                    safe_mode_available: 1,
                    progress: 12_345,
                    downgrade_time: None,
                    owner: Some("p1".into()),
                    owner_cleared: false,
                    is_power_enabled: false,
                    reservation: Some(Reservation {
                        user: "p2".into(),
                        end_time: 4321,
                    }),
                    sign: Some(Sign {
                        user: "p1".into(),
                        text: "room owned by p1".into(),
                        time: 100,
                        datetime: None,
                    }),
                    upgrade_blocked: Some(1100),
                    safe_mode_cooldown: Some(50_100),
                }),
                RoomObject::Spawn(SpawnObj {
                    id: 4,
                    x: 25,
                    y: 25,
                    hits: 5000,
                    off: false,
                    name: "Spawn1".into(),
                    owner: "p1".into(),
                    store: Store {
                        energy: 300,
                        ghodium: 0,
                    },
                    notify_when_attacked: Some(true),
                    spawning: Some(SpawningJob {
                        name: "w2".into(),
                        need_time: 9,
                        spawn_time: 1243,
                        directions: vec![1, 3, 5],
                    }),
                }),
                RoomObject::Energy(EnergyObj {
                    id: 5,
                    x: 12,
                    y: 13,
                    resource_type: 0,
                    amount: 40,
                    resource: None,
                }),
                RoomObject::ConstructionSite(ConstructionSiteObj {
                    id: 6,
                    x: 30,
                    y: 31,
                    structure_type: "tower".into(),
                    progress: 120,
                    progress_total: 5000,
                    owner: Some("p1".into()),
                    name: None,
                }),
                RoomObject::Tombstone(TombstoneObj {
                    id: 8,
                    x: 20,
                    y: 21,
                    death_time: 1233,
                    decay_time: 1233 + 10 * 5,
                    creep_id: Some(1),
                    creep_name: Some("w1".into()),
                    creep_ticks_to_live: 1,
                    creep_body: vec![0, 1],
                    creep_saying: None,
                    owner: Some("p1".into()),
                    store: Store {
                        energy: 37,
                        ghodium: 0,
                    },
                    store_extra: vec![ResourceStack {
                        resource: "XUHO2".into(),
                        amount: 5,
                    }],
                }),
                RoomObject::Ruin(RuinObj {
                    id: 9,
                    x: 22,
                    y: 21,
                    structure_id: 4,
                    structure_type: "spawn".into(),
                    structure_hits_max: Some(5000),
                    destroy_time: 1234,
                    decay_time: 1234 + 500,
                    owner: Some("p1".into()),
                    store: Store {
                        energy: 300,
                        ghodium: 0,
                    },
                    store_extra: Vec::new(),
                }),
                RoomObject::Mineral(MineralObj {
                    id: 10,
                    x: 40,
                    y: 40,
                    mineral_type: "U".into(),
                    mineral_amount: 35_000,
                    density: 2,
                    next_regeneration_time: None,
                }),
                RoomObject::Extractor(ExtractorObj {
                    id: 11,
                    x: 40,
                    y: 40,
                    hits: 500,
                    owner: Some("p1".into()),
                    cooldown: 3,
                    notify_when_attacked: Some(true),
                }),
                RoomObject::Generic(GenericObj {
                    id: 7,
                    x: 9,
                    y: 9,
                    data: r#"{"type":"tombstone","store":{"energy":30}}"#.into(),
                }),
            ],
            events: vec![
                r#"{"event":2,"objectId":"kk0000000000000001","data":{"type":"creep"}}"#.into(),
            ],
        }
    }

    /// 检查墓碑/废墟标签的固定值，以及掉落资源名和额外资源项的编码往返。
    #[test]
    fn corpse_tags_and_named_resources_roundtrip() {
        assert_eq!(tags::TOMBSTONE, 12, "快照中的固定标签值");
        assert_eq!(tags::RUIN, 13, "快照中的固定标签值");
        let named = EnergyObj {
            id: 1,
            x: 2,
            y: 3,
            resource_type: 0,
            amount: 9,
            resource: Some("XUHO2".into()),
        };
        let mut buf = Vec::new();
        named.encode_into(&mut buf);
        assert_eq!(
            EnergyObj::decode_from(&mut crate::Cursor::new(&buf)).unwrap(),
            named,
            "编码保留资源名与数值码，资源名优先级由世界层解释"
        );
        let stack = ResourceStack {
            resource: "LH".into(),
            amount: 4,
        };
        let mut buf = Vec::new();
        stack.encode_into(&mut buf);
        assert_eq!(
            ResourceStack::decode_from(&mut crate::Cursor::new(&buf)).unwrap(),
            stack
        );
    }

    /// 检查矿物/提取器标签的固定值，以及再生时间和无主提取器的编码往返。
    #[test]
    fn mineral_tags_and_optional_fields_roundtrip() {
        assert_eq!(tags::MINERAL, 14, "快照中的固定标签值");
        assert_eq!(tags::EXTRACTOR, 15, "快照中的固定标签值");
        assert_eq!(tags::MOD, 16, "快照中的固定标签值（向前兼容承诺）");
        let mineral = MineralObj {
            id: 1,
            x: 10,
            y: 11,
            mineral_type: "Z".into(),
            mineral_amount: 70_000,
            density: 3,
            next_regeneration_time: Some(50_100),
        };
        let mut buf = Vec::new();
        mineral.encode_into(&mut buf);
        assert_eq!(
            MineralObj::decode_from(&mut crate::Cursor::new(&buf)).unwrap(),
            mineral
        );
        let extractor = ExtractorObj {
            id: 2,
            x: 10,
            y: 11,
            hits: 500,
            owner: None,
            cooldown: 5,
            notify_when_attacked: None,
        };
        let mut buf = Vec::new();
        extractor.encode_into(&mut buf);
        assert_eq!(
            ExtractorObj::decode_from(&mut crate::Cursor::new(&buf)).unwrap(),
            extractor,
            "owner 为 None 的提取器应保留全部字段"
        );
    }

    #[test]
    fn snapshot_roundtrip() {
        let s = sample_snapshot();
        let bytes = encode_snapshot(&s);
        let back = decode_snapshot(&bytes).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn envelope_guards() {
        let bytes = encode_snapshot(&sample_snapshot());
        // 坏魔数
        let mut bad = bytes.clone();
        bad[0] ^= 0xff;
        assert_eq!(
            decode_snapshot(&bad),
            Err(SchemaError::BadMagic(u32::from_le_bytes([
                bad[0], bad[1], bad[2], bad[3]
            ])))
        );
        // 坏版本
        let mut old = bytes.clone();
        old[4] = 9;
        old[5] = 0;
        assert_eq!(
            decode_snapshot(&old),
            Err(SchemaError::UnsupportedVersion(9))
        );
        // 版本 0 没有对应格式，即使增加升级步骤也应拒绝。
        let mut v0 = bytes.clone();
        v0[4] = 0;
        v0[5] = 0;
        assert_eq!(
            decode_snapshot(&v0),
            Err(SchemaError::UnsupportedVersion(0))
        );
        // 截断载荷
        assert_eq!(
            decode_snapshot(&bytes[..bytes.len() - 3]),
            Err(SchemaError::UnexpectedEof)
        );
        // 坏 variant 标签：头部后第一个对象的 tag 位
        let header_size = {
            let mut v = Vec::new();
            sample_snapshot().header.encode_into(&mut v);
            v.len()
        };
        let mut badtag = bytes.clone();
        badtag[4 + 2 + header_size + 4] = 200; // vec 长度前缀 4 字节之后是首对象 tag
        assert_eq!(
            decode_snapshot(&badtag),
            Err(SchemaError::InvalidVariantTag(200))
        );
    }

    #[test]
    fn fixed_offsets_are_compile_time_layout() {
        // RoomHeader 全固定：偏移链 = 0,8,16,24
        assert_eq!(RoomHeader::OFFSET_time, 0);
        assert_eq!(RoomHeader::OFFSET_seed, 8);
        assert_eq!(RoomHeader::OFFSET_next_obj_id, 16);
        assert_eq!(RoomHeader::OFFSET_terrain, 24);
        // CreepObj 原始前缀：id 0 / x 8 / y 9 / fatigue 10 / age_time 14 /
        // hits 22 / spawning 26 / action_log 27；name 起为变长区（无偏移）
        assert_eq!(CreepObj::OFFSET_id, 0);
        assert_eq!(CreepObj::OFFSET_x, 8);
        assert_eq!(CreepObj::OFFSET_y, 9);
        assert_eq!(CreepObj::OFFSET_fatigue, 10);
        assert_eq!(CreepObj::OFFSET_age_time, 14);
        assert_eq!(CreepObj::OFFSET_hits, 22);
        assert_eq!(CreepObj::OFFSET_spawning, 26);
        assert_eq!(CreepObj::OFFSET_action_log, 27);
    }

    #[test]
    fn rejects_trailing_and_bad_tags() {
        let bytes = encode_snapshot(&sample_snapshot());
        // 载荷尾部追加垃圾
        let mut padded = bytes.clone();
        padded.extend_from_slice(&[0xde, 0xad]);
        assert_eq!(decode_snapshot(&padded), Err(SchemaError::TrailingBytes(2)));
        // 坏 option 标志：SpawningJob 存在标志位翻为 2
        let pos = find_spawning_flag(&bytes).expect("样例含 Some(spawning)");
        let mut bad = bytes.clone();
        bad[pos] = 2;
        assert_eq!(decode_snapshot(&bad), Err(Se::InvalidOptionTag(2)));
        // 输入过短
        assert_eq!(decode_snapshot(&[0x50]), Err(Se::UnexpectedEof));
    }

    /// 在编码字节里定位 SpawningJob 的存在标志位（u8 1）。
    fn find_spawning_flag(bytes: &[u8]) -> Option<usize> {
        // 扫描封套后的字节，找 0x01 后跟长度 2 的字符串 "w2"（样例孵化名）
        let mut i = 6;
        while i + 6 < bytes.len() {
            // 标志 01 + 长度前缀 02 00 00 00（LE）+ "w2"
            if bytes[i] == 1
                && bytes[i + 1] == 2
                && bytes[i + 2] == 0
                && bytes[i + 3] == 0
                && bytes[i + 4] == 0
                && bytes[i + 5] == b'w'
                && bytes[i + 6] == b'2'
            {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    #[test]
    fn offsets_match_encoding() {
        // 偏移常量与实际编码位置一致（抽查 RoomHeader.terrain）
        let s = sample_snapshot();
        let bytes = encode_snapshot(&s);
        let body_start = 4 + 2;
        assert_eq!(bytes[body_start + RoomHeader::OFFSET_terrain], 0);
        assert_eq!(bytes[body_start + RoomHeader::OFFSET_terrain + 7], 1);
    }
}
