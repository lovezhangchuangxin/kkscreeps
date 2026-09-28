//! M1 房间快照 schema：对象集与编解码封套。
//!
//! 字段集对照官方引擎对象的持久化面（`engine/src/processor` 各 intent 处理器
//! 读写的字段）；`room` 字段 M1 单房间省略，跨房间（M2+）经版本升级补入。
//! 枚举值的 str 映射（body part / structure type / 资源名）在 kk-world 定义，
//! schema 只存原始字节。

use crate::{Cursor, Schema, SchemaError};

/// 当前 schema 版本。**首个持久化格式发布（M2 存储）起**，字段布局变更必须
/// +1 并在 [`UPGRADE_CHAIN`] 补升级函数；发布前的 v1 内演化无损（尚无任何
/// 落盘字节在流通，灰区表已登记此约定）。
pub const SCHEMA_VERSION: u16 = 1;
/// 封套魔数（LE 落盘字节序为 `50 53 4B 4B` = "PSKK"）。
pub const SNAPSHOT_MAGIC: u32 = 0x4b4b_5350;

crate::kk_schema! {
    /// 房间头部（全固定字段）。
    pub RoomHeader {
        (pub time: u64)
        (pub seed: u64)
        (pub next_obj_id: u64)
        (pub terrain: [u8; 2500])
    }
}

crate::kk_schema! {
    /// 携带资源（M1 起 energy；B2 增 ghodium——官方 store 键为资源常量
    /// `RESOURCE_GHODIUM = "G"`，generateSafeMode 的消耗面；其余资源种类经
    /// 版本升级扩展）。
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
    /// 身体部件（官方形态 {type, hits, boost?}；kind 的 str 映射见 kk-world）。
    pub BodyPart {
        (pub kind: u8)
        (pub hits: u32)
        (pub boost: opt str)
    }
}

crate::kk_schema! {
    /// 孵化任务（官方 spawn.spawning 形态）。
    pub SpawningJob {
        (pub name: str)
        (pub need_time: u32)
        (pub spawn_time: u64)
        (pub directions: vec u8)
    }
}

crate::kk_schema! {
    /// 爬爬虫。
    pub CreepObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub fatigue: u32)
        (pub age_time: u64)
        (pub hits: u32)
        (pub spawning: bool)
        (pub action_log: u16)
        // ↑ 官方 actionLog 是 13 键 {x,y} 对象（processor.js:51-65）；M1 压缩为
        // 位图：bit0=harvest、bit1=build、bit2=upgradeController。坐标与战斗
        // 动作（attack/heal/...）M2 以正式形态替换（schema 版本升级）。
        // B2 起 u8 → u16：8 位已满，reserveController 的官方独立键
        // `reserveController`（reserveController.js:47）需要第 9 位
        (pub name: str)
        (pub owner: str)
        (pub body: vec BodyPart)
        (pub store: Store)
    }
}

crate::kk_schema! {
    /// 能量源（重生钟 next_regeneration_time：None = 未挂钟）。
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
    /// controller 预定（官方 DB 形态 `{user, endTime}`：reserveController.js:36-39
    /// 创建、:49 递增；controllers/tick.js:10-12 到期或房被占时清除）。
    /// `user` 官方存内部 _id，我方身份模型按玩家名（与 owner 字段同基）。
    pub Reservation {
        (pub user: str)
        (pub end_time: u64)
    }
}

crate::kk_schema! {
    /// controller 签名（官方 DB 形态 `{user, text, time, datetime}`：
    /// signController.js:23-28）。第四字段 `datetime` 为墙钟 Date.now()——
    /// 确定性引擎不落盘（投影层按玩家可见形状合成，轨 A 接线；灰区登记）。
    pub Sign {
        (pub user: str)
        (pub text: str)
        (pub time: u64)
    }
}

crate::kk_schema! {
    /// 控制器（safe_mode 0 = 关闭；downgrade_time None = 未挂钟）。
    /// B2 补齐官方持久化面：reservation（预定）、sign（签名）、
    /// upgrade_blocked（attackController 的升级封锁钟，绝对 tick：
    /// attackController.js:48 / controllers/tick.js:20-23）、
    /// safe_mode_cooldown（safeMode 冷却钟，绝对 tick：controllers/tick.js:29）。
    /// A3b 补 owner 三态哨兵（官方降级归零 `user = null`，controllers/tick.js:56）：
    /// owner_cleared=true 表示曾拥有但已清（投影 owner: null → 玩家侧
    /// my === false）；false 且 owner None = 从未拥有（投影无键 → my undefined）。
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
    /// 孵化器（hits 上限 SPAWN_HITS；off = RCL 超限停用，_calc_spawns 维护）。
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
    }
}

crate::kk_schema! {
    /// 扩展（hits 上限 EXTENSION_HITS；off = RCL 超限停用，_calc_spawns 维护）。
    pub ExtensionObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub hits: u32)
        (pub off: bool)
        (pub owner: str)
        (pub store: Store)
    }
}

crate::kk_schema! {
    /// 建造中的墙（hits 从 1 起步，上限 WALL_HITS_MAX=300M）。
    pub ConstructedWallObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub hits: u32)
    }
}

crate::kk_schema! {
    /// 道路（decay 到时移除）。
    pub RoadObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub hits: u32)
        (pub next_decay_time: u64)
    }
}

crate::kk_schema! {
    /// 仓库（hits 上限 STORAGE_HITS）。
    pub StorageObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub hits: u32)
        (pub owner: str)
        (pub store: Store)
    }
}

crate::kk_schema! {
    /// 工地（progress_total 为创建时定格的总工程量：放置路径=裸 CONSTRUCTION_COST
    /// （vanilla getProgressTotal:1871-1874），引擎 intent 建工地=成本×地形系数
    /// （create-construction-site.js:38-48，仅 road 有系数）——存字段而非派生，
    /// 避免 setTerrain 后完成线漂移；structure_type 存官方字符串——M1 结算外
    /// 类型照常可摆放）。
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
    /// 掉落资源（resource_type 0=energy，其余 M2）。官方无衰减钟字段：
    /// 每 tick `amount -= ceil(amount / ENERGY_DECAY)`（energy/tick.js），≤0 移除。
    pub EnergyObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub resource_type: u8)
        (pub amount: u32)
    }
}

crate::kk_schema! {
    /// M1 结算范围外对象的透传桶：完整记录以 JSON 存放（含 type 与
    /// structureType 等玩家可见字段），结算忽略、投影还原。
    pub GenericObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub data: str)
    }
}

kk_schema! {
    /// 旗帜。官方 flag 是独立集合（processor.js:22 bulkFlags，不参与 roomObjects
    /// 循环）；M1 折进 RoomObject 变体是故意简化，投影层注意官方形状差异。
    pub FlagObj {
        (pub id: u64)
        (pub x: u8)
        (pub y: u8)
        (pub name: str)
        (pub color: u8)
        (pub secondary_color: u8)
    }
}

/// 房间内对象（variant 枚举：tag 前缀 + 载荷）。tag 值见 [`RoomObject::tag`]，
/// 一旦发布即冻结。
#[derive(Clone, Debug, PartialEq)]
pub enum RoomObject {
    Creep(CreepObj),
    Source(SourceObj),
    Controller(ControllerObj),
    Spawn(SpawnObj),
    Extension(ExtensionObj),
    ConstructedWall(ConstructedWallObj),
    ConstructionSite(ConstructionSiteObj),
    Energy(EnergyObj),
    Flag(FlagObj),
    Road(RoadObj),
    Storage(StorageObj),
    /// M1 结算范围外的对象（tower/rampart/lab/tombstone/portal/…）：完整记录
    /// 以 JSON 透传存放——保持与 M0 哑世界相同的放置接受面，结算忽略之。
    Generic(GenericObj),
}

impl RoomObject {
    /// 对象 id（所有变体的首字段）。
    pub fn id(&self) -> u64 {
        match self {
            Self::Creep(o) => o.id,
            Self::Source(o) => o.id,
            Self::Controller(o) => o.id,
            Self::Spawn(o) => o.id,
            Self::Extension(o) => o.id,
            Self::ConstructedWall(o) => o.id,
            Self::ConstructionSite(o) => o.id,
            Self::Energy(o) => o.id,
            Self::Flag(o) => o.id,
            Self::Road(o) => o.id,
            Self::Storage(o) => o.id,
            Self::Generic(o) => o.id,
        }
    }

    /// 坐标（所有变体的公共字段）。
    pub fn pos(&self) -> (u8, u8) {
        match self {
            Self::Creep(o) => (o.x, o.y),
            Self::Source(o) => (o.x, o.y),
            Self::Controller(o) => (o.x, o.y),
            Self::Spawn(o) => (o.x, o.y),
            Self::Extension(o) => (o.x, o.y),
            Self::ConstructedWall(o) => (o.x, o.y),
            Self::ConstructionSite(o) => (o.x, o.y),
            Self::Energy(o) => (o.x, o.y),
            Self::Flag(o) => (o.x, o.y),
            Self::Road(o) => (o.x, o.y),
            Self::Storage(o) => (o.x, o.y),
            Self::Generic(o) => (o.x, o.y),
        }
    }

    pub fn set_pos(&mut self, x: u8, y: u8) {
        match self {
            Self::Creep(o) => {
                o.x = x;
                o.y = y;
            }
            Self::Source(o) => {
                o.x = x;
                o.y = y;
            }
            Self::Controller(o) => {
                o.x = x;
                o.y = y;
            }
            Self::Spawn(o) => {
                o.x = x;
                o.y = y;
            }
            Self::Extension(o) => {
                o.x = x;
                o.y = y;
            }
            Self::ConstructedWall(o) => {
                o.x = x;
                o.y = y;
            }
            Self::ConstructionSite(o) => {
                o.x = x;
                o.y = y;
            }
            Self::Energy(o) => {
                o.x = x;
                o.y = y;
            }
            Self::Flag(o) => {
                o.x = x;
                o.y = y;
            }
            Self::Road(o) => {
                o.x = x;
                o.y = y;
            }
            Self::Storage(o) => {
                o.x = x;
                o.y = y;
            }
            Self::Generic(o) => {
                o.x = x;
                o.y = y;
            }
        }
    }
}

impl Schema for RoomObject {
    fn encode_into(&self, out: &mut Vec<u8>) {
        out.push(self.tag());
        match self {
            Self::Creep(o) => o.encode_into(out),
            Self::Source(o) => o.encode_into(out),
            Self::Controller(o) => o.encode_into(out),
            Self::Spawn(o) => o.encode_into(out),
            Self::Extension(o) => o.encode_into(out),
            Self::ConstructedWall(o) => o.encode_into(out),
            Self::ConstructionSite(o) => o.encode_into(out),
            Self::Energy(o) => o.encode_into(out),
            Self::Flag(o) => o.encode_into(out),
            Self::Road(o) => o.encode_into(out),
            Self::Storage(o) => o.encode_into(out),
            Self::Generic(o) => o.encode_into(out),
        }
    }

    fn decode_from(cur: &mut Cursor<'_>) -> Result<Self, SchemaError> {
        match cur.read_u8()? {
            tags::CREEP => Ok(Self::Creep(CreepObj::decode_from(cur)?)),
            tags::SOURCE => Ok(Self::Source(SourceObj::decode_from(cur)?)),
            tags::CONTROLLER => Ok(Self::Controller(ControllerObj::decode_from(cur)?)),
            tags::SPAWN => Ok(Self::Spawn(SpawnObj::decode_from(cur)?)),
            tags::EXTENSION => Ok(Self::Extension(ExtensionObj::decode_from(cur)?)),
            tags::CONSTRUCTED_WALL => {
                Ok(Self::ConstructedWall(ConstructedWallObj::decode_from(cur)?))
            }
            tags::CONSTRUCTION_SITE => Ok(Self::ConstructionSite(
                ConstructionSiteObj::decode_from(cur)?,
            )),
            tags::ENERGY => Ok(Self::Energy(EnergyObj::decode_from(cur)?)),
            tags::FLAG => Ok(Self::Flag(FlagObj::decode_from(cur)?)),
            tags::ROAD => Ok(Self::Road(RoadObj::decode_from(cur)?)),
            tags::STORAGE => Ok(Self::Storage(StorageObj::decode_from(cur)?)),
            tags::GENERIC => Ok(Self::Generic(GenericObj::decode_from(cur)?)),
            b => Err(SchemaError::InvalidVariantTag(b)),
        }
    }
}

/// variant tag 值（编码首字节）。一旦发布即冻结；新增变体追加尾部、
/// 改动已有值必须走 schema 版本升级。
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
}

impl RoomObject {
    fn tag(&self) -> u8 {
        match self {
            Self::Creep(_) => tags::CREEP,
            Self::Source(_) => tags::SOURCE,
            Self::Controller(_) => tags::CONTROLLER,
            Self::Spawn(_) => tags::SPAWN,
            Self::Extension(_) => tags::EXTENSION,
            Self::ConstructedWall(_) => tags::CONSTRUCTED_WALL,
            Self::ConstructionSite(_) => tags::CONSTRUCTION_SITE,
            Self::Energy(_) => tags::ENERGY,
            Self::Flag(_) => tags::FLAG,
            Self::Road(_) => tags::ROAD,
            Self::Storage(_) => tags::STORAGE,
            Self::Generic(_) => tags::GENERIC,
        }
    }
}

crate::kk_schema! {
    /// 房间快照：头部 + 对象表（对象按 id 升序存放——BTreeMap 迭代序）。
    pub RoomSnapshot {
        (pub header: RoomHeader)
        (pub objects: vec RoomObject)
    }
}

/// 编码整个快照（封套：magic + version + 载荷）。
pub fn encode_snapshot(s: &RoomSnapshot) -> Vec<u8> {
    let mut out = Vec::with_capacity(2600);
    out.extend_from_slice(&SNAPSHOT_MAGIC.to_le_bytes());
    out.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
    s.encode_into(&mut out);
    out
}

/// 版本升级函数链（方案 D2）：`UPGRADE_CHAIN[n]` 把 v(n+1) 字节升级为 v(n+2)。
/// 当前 v1 起步无历史版本，链为空；SCHEMA_VERSION +1 时在此追加转换函数，
/// 旧版本字节即自动"读入升级再解码"。
type UpgradeFn = fn(Vec<u8>) -> Result<Vec<u8>, SchemaError>;
const UPGRADE_CHAIN: [UpgradeFn; (SCHEMA_VERSION - 1) as usize] = [];

/// 把 `from` 版本的封套字节（含头）沿升级链推进到当前版本。
/// 链空时对任何旧版本只能拒绝——机制位置由测试锁定（v0 伪造字节走本路径）。
fn upgrade_snapshot(from: u16, mut bytes: Vec<u8>) -> Result<Vec<u8>, SchemaError> {
    if from == 0 || from > SCHEMA_VERSION {
        return Err(SchemaError::UnsupportedVersion(from));
    }
    for step in UPGRADE_CHAIN.iter().skip((from - 1) as usize) {
        bytes = step(bytes)?;
    }
    Ok(bytes)
}

/// 解码整个快照。旧版本字节先过升级链再到当前版本。
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
                RoomObject::Generic(GenericObj {
                    id: 7,
                    x: 9,
                    y: 9,
                    data: r#"{"type":"tombstone","store":{"energy":30}}"#.into(),
                }),
            ],
        }
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
        // 版本 0 走升级链入口（机制位置锁定：链空 ⇒ 拒绝；SCHEMA_VERSION +1 时
        // 此用例改写为"伪造 v0 字节 → 过链 → 成功解码"）
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
