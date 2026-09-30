//! 场景夹具：三档规模的世界快照数据，作为存储基准的负载输入。
//!
//! 复用优先（M2 方案 §9「基准 fixture 锁定」的地基）：
//! - 房间内容经 [`kk_world::place::normalize`] 放置（与 embed 通道同一归一化
//!   入口），再经 [`kk_world::world::RoomState::encode_snapshot`] +
//!   [`kk_schema::room::encode_snapshot`] 编码——**字节形态与未来 kk-storage
//!   落盘的房间快照同源**（magic + version 封套），不是合成随机字节；
//! - 随机性全部来自 [`kk_core::rng::SeededRng`]（结算同款 PCG32），
//!   任意档位可从常量复现到逐字节；
//! - 玩家记录与大 value（Memory blob）按官方数据面形态生成（小 JSON 记录 /
//!   百 KB～MB 级 blob；官方 Memory 上限 2MB，xxscreeps 实测常见存档
//!   ~100KB）。
//!
//! 规模档对齐 performance-acceptance.md 的场景表：t1 ≈ 场景 B（单房热点），
//! t3 ≈ 场景 A/C 的房间数（100 房），t2 为中间档。

use kk_core::rng::SeededRng;
use kk_core::{TERRAIN_MASK_SWAMP, Terrain};
use serde_json::{Value, json};

use crate::engine::TableKind;

/// 世界基准时间（任意固定值——远离 0，让各挂钟字段非零，贴近中期存档）。
pub const BASE_TIME: u64 = 1_234_567;

/// 房间成熟度：决定对象数量与 creep 身体规模。对象数对照官方满配房
/// （60 extension、全墙、路网、50 部件 creep ×N——xxscreeps 公布的满配房
/// 数十 KB）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maturity {
    /// 新落地：controller rcl1 + spawn + 少量 creep（数百字节级快照）。
    Young,
    /// 中期：rcl4、10 extension、storage、部分墙与路（数 KB 级）。
    Mid,
    /// 满配：rcl8、60 extension、3 spawn、180 墙、150 路、20 creep
    /// （其中半数 50 部件满身体——数十 KB 级）。
    Mature,
}

/// 规模档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// 单房热点（1 满配房、4 玩家）。
    T1,
    /// 小多房（12 房：满配/中期/新落地各 4、20 玩家）。
    T2,
    /// 较大多房（100 房：34/33/33、100 玩家——场景 A/C 的房间数）。
    T3,
}

impl Tier {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "t1" => Some(Self::T1),
            "t2" => Some(Self::T2),
            "t3" => Some(Self::T3),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::T1 => "t1-single-room",
            Self::T2 => "t2-small-multi",
            Self::T3 => "t3-large-multi",
        }
    }

    pub fn rooms(self) -> usize {
        match self {
            Self::T1 => 1,
            Self::T2 => 12,
            Self::T3 => 100,
        }
    }

    pub fn players(self) -> usize {
        match self {
            Self::T1 => 4,
            Self::T2 => 20,
            Self::T3 => 100,
        }
    }
}

/// 房间名（索引 → 名）：两位坐标保证字典序 = 数字序，前缀扫描语义干净。
/// `W0N0..W9N9` 共 100 个。
pub fn room_name(index: usize) -> String {
    format!("W{}N{}", index / 10, index % 10)
}

/// 每房间成熟度：满配 → 中期 → 新落地轮转（t1 恰好单满配房）。
fn maturity_of(index: usize) -> Maturity {
    match index % 3 {
        0 => Maturity::Mature,
        1 => Maturity::Mid,
        _ => Maturity::Young,
    }
}

/// 确定性种子（域分隔：同索引不同用途不同序列）。
fn seed_for(domain: &str, index: u64) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325; // FNV-1a
    for b in domain.as_bytes().iter().chain(std::iter::once(&0x2f)) {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h ^ index.wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

/// 建好的世界夹具（typed 世界 + 玩家名单）。
pub struct WorldFixture {
    pub world: kk_world::world::World,
    pub room_names: Vec<String>,
    pub players: Vec<String>,
}

/// 建世界：房间内容经 place::normalize 放置（embed 同一归一化入口）。
pub fn build_world(tier: Tier) -> WorldFixture {
    let mut world = kk_world::world::World::new(0xC3C3);
    world.time = BASE_TIME;
    for i in 0..tier.rooms() {
        let name = room_name(i);
        let mut rng = SeededRng::new(seed_for("room", i as u64));
        let terrain = gen_terrain(&mut rng);
        world.register_room(name.clone(), terrain);
        build_room(
            &mut world,
            &name,
            maturity_of(i),
            i,
            tier.players(),
            &mut rng,
        );
    }
    let players = (0..tier.players()).map(|i| format!("p{i:04}")).collect();
    WorldFixture {
        world,
        room_names: (0..tier.rooms()).map(room_name).collect(),
        players,
    }
}

impl WorldFixture {
    /// 某时刻的房间快照字节（正式封套编码；检查点重写用不同 time 得到
    /// 同长度不同内容的值——写路径上引擎无法省略真实重写）。
    pub fn room_bytes(&self, room: &str, time: u64) -> Vec<u8> {
        let state = &self.world.rooms[room];
        kk_schema::room::encode_snapshot(&state.encode_snapshot(
            time,
            self.world.seed,
            self.world.next_obj_id(),
        ))
    }

    /// 房间名作键。
    pub fn room_keys(&self) -> Vec<Vec<u8>> {
        self.room_names
            .iter()
            .map(|n| n.as_bytes().to_vec())
            .collect()
    }
}

/// 地形：平原打底 + 确定性沼泽散布（约 8%），不出 wall（对象层另有墙）。
fn gen_terrain(rng: &mut SeededRng) -> Terrain {
    let mut terrain = Terrain::plains();
    let codes = terrain.as_mut_slice();
    for cell in codes.iter_mut() {
        if rng.below(100) < 8 {
            *cell |= TERRAIN_MASK_SWAMP;
        }
    }
    terrain.normalize();
    terrain
}

/// 位置分配器：内部 46×46 网格线性走位（2..=47），确定性且分散。
struct PosGen {
    seq: u32,
}

impl PosGen {
    fn new() -> Self {
        Self { seq: 0 }
    }

    fn next(&mut self) -> (u8, u8) {
        let s = self.seq;
        self.seq = self.seq.wrapping_add(1);
        ((2 + (s % 46)) as u8, (2 + ((s / 46) % 46)) as u8)
    }
}

/// creep 身体（部件循环到目标数；满配 50 = 官方上限）。
fn body_spec(parts: &[&str], count: usize) -> Vec<Value> {
    (0..count).map(|i| json!(parts[i % parts.len()])).collect()
}

/// 位置 spec（`pos: [x, y]`——tuple 不能直接进 json!，经数组值转一道）。
fn pos_json(p: (u8, u8)) -> Value {
    json!([p.0, p.1])
}

/// 放一个对象（spec → normalize → insert；ctx 借用与插入按作用域交替，
/// 与 kk-world 既有测试同一形态）。
fn place(world: &mut kk_world::world::World, room: &str, kind: &str, spec: Value) {
    let time = world.time;
    let ctx_place = |w: &kk_world::world::World, spec: &Value| {
        let controller = w.rooms[room].controller().cloned();
        kk_world::place::normalize(
            kind,
            spec,
            &kk_world::place::PlaceCtx {
                time,
                controller: controller.as_ref(),
                name_seq: w.rooms[room].objects.len(),
            },
        )
    };
    let object = ctx_place(world, &spec).expect("夹具 spec 必然合法");
    world
        .insert(room, object)
        .unwrap_or_else(|e| panic!("夹具放置失败 ({room}/{kind}): {e}"));
}

/// 按成熟度铺一个房间。
fn build_room(
    world: &mut kk_world::world::World,
    room: &str,
    maturity: Maturity,
    index: usize,
    player_count: usize,
    rng: &mut SeededRng,
) {
    let (rcl, n_spawn, n_ext, n_road, n_wall, n_creep, n_flag, n_site, n_res, n_generic) =
        match maturity {
            Maturity::Young => (1usize, 1, 0, 4, 0, 2, 1, 1, 2, 0),
            Maturity::Mid => (4, 1, 10, 40, 30, 6, 2, 1, 3, 1),
            Maturity::Mature => (8, 3, 60, 150, 180, 20, 3, 2, 5, 2),
        };
    let owner = format!("p{:04}", index % player_count);
    let mut pos = PosGen::new();

    // controller 先落（后续放置的 PlaceCtx 依赖它——source 容量按归属分支）
    place(
        world,
        room,
        "structure",
        json!({
            "pos": pos_json(pos.next()), "structureType": "controller", "rcl": rcl,
            "owner": owner, "progress": rng.below(8_000_000),
            "downgradeTime": world.time + 20_000 + u64::from(rng.below(30_000)),
        }),
    );
    // source ×2（官方固定两处）
    for _ in 0..2 {
        place(
            world,
            room,
            "structure",
            json!({ "pos": pos_json(pos.next()), "structureType": "source" }),
        );
    }
    // spawn
    for s in 0..n_spawn {
        place(
            world,
            room,
            "spawn",
            json!({
                "pos": pos_json(pos.next()), "owner": owner, "name": format!("Spawn{s}"),
                "store": { "energy": 200 + rng.below(200) },
            }),
        );
    }
    // extension
    for _ in 0..n_ext {
        place(
            world,
            room,
            "structure",
            json!({
                "pos": pos_json(pos.next()), "structureType": "extension", "owner": owner,
                "store": { "energy": rng.below(200) },
            }),
        );
    }
    // storage（中期起）
    if rcl >= 4 {
        place(
            world,
            room,
            "structure",
            json!({
                "pos": pos_json(pos.next()), "structureType": "storage", "owner": owner,
                "store": { "energy": rng.below(300_000) },
            }),
        );
    }
    // 路（0 载荷结构，测大量小对象的编码与键值开销）
    for _ in 0..n_road {
        place(
            world,
            room,
            "structure",
            json!({ "pos": pos_json(pos.next()), "structureType": "road" }),
        );
    }
    // 墙（hits 随机——现实中从 1 到 300M 连续分布）
    for _ in 0..n_wall {
        place(
            world,
            room,
            "structure",
            json!({
                "pos": pos_json(pos.next()), "structureType": "constructedWall",
                "hits": rng.below(50_000_000),
            }),
        );
    }
    // creep（满配房半数 50 部件满身体）
    for c in 0..n_creep {
        let (count, parts) = if maturity == Maturity::Mature && c % 2 == 0 {
            (50usize, &["work", "carry", "move", "heal"][..])
        } else if maturity == Maturity::Mature {
            (12, &["work", "carry", "move"][..])
        } else if maturity == Maturity::Mid {
            (8, &["work", "carry", "move"][..])
        } else {
            (3, &["move", "work", "carry"][..])
        };
        place(
            world,
            room,
            "creep",
            json!({
                "pos": pos_json(pos.next()), "owner": owner,
                "name": format!("c{index}_{c}"),
                "body": body_spec(parts, count),
                "store": { "energy": rng.below(2_000) },
                "ticksToLive": 100 + rng.below(1_400),
            }),
        );
    }
    // flag
    for f in 0..n_flag {
        place(
            world,
            room,
            "flag",
            json!({ "pos": pos_json(pos.next()), "name": format!("flag{f}") }),
        );
    }
    // 工地
    for _ in 0..n_site {
        place(
            world,
            room,
            "constructionSite",
            json!({
                "pos": pos_json(pos.next()), "structureType": "extension", "owner": owner,
                "progress": rng.below(200), "progressTotal": 200,
            }),
        );
    }
    // 掉落能量
    for _ in 0..n_res {
        place(
            world,
            room,
            "resource",
            json!({ "pos": pos_json(pos.next()), "resourceType": "energy", "amount": rng.below(2_000) }),
        );
    }
    // 透传桶（mineral/tombstone——schema 未定型类型的真实存量形态）
    for g in 0..n_generic {
        if g == 0 {
            place(
                world,
                room,
                "mineral",
                json!({
                    "pos": pos_json(pos.next()), "mineralType": "X", "mineralAmount": 45_000,
                    "density": 3,
                }),
            );
        } else {
            place(
                world,
                room,
                "tombstone",
                json!({ "pos": pos_json(pos.next()), "store": { "energy": rng.below(50) } }),
            );
        }
    }
}

/// 一份键值数据集（场景负载的最小单元）。
pub struct Dataset {
    pub name: String,
    pub table: TableKind,
    pub pairs: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Dataset {
    pub fn total_bytes(&self) -> u64 {
        self.pairs.iter().map(|(_, v)| v.len() as u64).sum()
    }
}

/// 房间快照数据集（键 = 房间名，值 = 该时刻的快照字节）。
pub fn rooms_dataset(fix: &WorldFixture, round: u64, tier: Tier) -> Dataset {
    let time = BASE_TIME + round * 128; // 每轮时间推进：值变化、长度不变
    Dataset {
        name: format!("rooms@{}", tier.name()),
        table: TableKind::Rooms,
        pairs: fix
            .room_names
            .iter()
            .map(|r| (r.as_bytes().to_vec(), fix.room_bytes(r, time)))
            .collect(),
    }
}

/// 玩家常驻记录数据集：小 JSON 值（cpu 桶/计数器随轮变化——检查点每轮重写）。
pub fn users_dataset(fix: &WorldFixture, round: u64, tier: Tier) -> Dataset {
    Dataset {
        name: format!("users@{}", tier.name()),
        table: TableKind::Users,
        pairs: fix
            .players
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    format!("u:{p}").into_bytes(),
                    user_record(p, i, round).into_bytes(),
                )
            })
            .collect(),
    }
}

/// 玩家记录（~280B 小值；字段取官方用户文档的常见子集）。
fn user_record(player: &str, index: usize, round: u64) -> String {
    json!({
        "username": player,
        "cpu": {
            "limit": 20, "tickLimit": 100,
            "bucket": 4000 + (round * 7 + index as u64) % 1000,
            "used": ((index * 37 + round as usize * 11) % 1990) as f64 / 100.0,
        },
        "gcl": 123_456_789u64 + index as u64 * 991,
        "lastUsedCpu": { "gameTick": round * 100 + index as u64, "cpu": (index % 50) as f64 / 3.0 },
        "badge": { "path": 5, "color1": "#000000", "color2": "#ffffff", "color3": "#123456", "flip": false },
        "placement": { "shard": "shard0", "slot": index },
    })
    .to_string()
}

/// 大 value 数据集：每玩家一条 Memory blob（每 5 名一条 1MB 至多 2 条，其余
/// 100KB——百 KB 常态 + MB 级尾部的真实分布）。
///
/// `round` 扮演"玩家每 tick 只改 Memory 的一小部分"的**局部变更**：每个 blob
/// 头部 4KB 用轮次相关字节重写。增量修改之外也避免同值重写被引擎判为无脏页
/// 事务而跳过落盘（初轮实测 mdbx 会把同值提交优化成 ~30µs 的 no-op commit，
/// 假绿数字）。
pub fn blobs_dataset(fix: &WorldFixture, tier: Tier, round: u64) -> Dataset {
    let mut pairs = Vec::new();
    let mut big_left = 2usize;
    for (i, player) in fix.players.iter().enumerate() {
        let big = big_left > 0 && i % 5 == 4;
        if big {
            big_left -= 1;
        }
        let len = if big { 1 << 20 } else { 100 << 10 };
        let mut value = random_bytes(&mut SeededRng::new(seed_for("blob", i as u64)), len);
        let mut edit = SeededRng::new(seed_for("blob-edit", (i as u64) << 32 | round));
        let head_len = (4 << 10).min(value.len());
        let head = &mut value[..head_len];
        for chunk in head.chunks_mut(4) {
            let b = edit.next_u32().to_le_bytes();
            chunk.copy_from_slice(&b[..chunk.len()]);
        }
        pairs.push((format!("u:{player}:memory").into_bytes(), value));
    }
    Dataset {
        name: format!("blobs@{}", tier.name()),
        table: TableKind::Blobs,
        pairs,
    }
}

/// 确定性伪随机字节（最坏情形：不可压缩。两引擎都不做压缩，内容只影响
/// 页缓存命中差异，伪随机避免同类页被去重的假象）。
pub fn random_bytes(rng: &mut SeededRng, len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(len);
    while out.len() < len {
        out.extend_from_slice(&rng.next_u32().to_le_bytes());
    }
    out.truncate(len);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use kk_schema::room::decode_snapshot;

    /// 逐字节确定性：同档两次构建完全一致（fixture 锁定的前提）。t3 双构建
    /// 曾在 debug 下约 +16s（insert 全房扫描 O(N²)——World 对象目录 O1
    /// 落地后实测 ~0.3s，≈55×；见 world.rs ObjDir 注释），套件墙钟影响
    /// 已可忽略——"基准数据集逐字节可复现"的完整锁定不再以时长为代价。
    #[test]
    fn deterministic_bytes() {
        for tier in [Tier::T1, Tier::T2, Tier::T3] {
            let a = build_world(tier);
            let b = build_world(tier);
            for room in &a.room_names {
                assert_eq!(a.room_bytes(room, BASE_TIME), b.room_bytes(room, BASE_TIME));
            }
        }
    }

    /// 快照可解码（真实封套而非随机字节）且对象数符合成熟度。
    #[test]
    fn snapshots_decode_and_shapes() {
        let fix = build_world(Tier::T2);
        for (i, room) in fix.room_names.iter().enumerate() {
            let bytes = fix.room_bytes(room, BASE_TIME);
            let snap = decode_snapshot(&bytes).expect("正式封套可解码");
            let expect_min = match maturity_of(i) {
                Maturity::Young => 6,
                Maturity::Mid => 55,
                Maturity::Mature => 420,
            };
            assert!(
                snap.objects.len() >= expect_min,
                "{room} 对象数 {} < {expect_min}",
                snap.objects.len()
            );
            assert_eq!(snap.header.time, BASE_TIME);
        }
    }

    /// 规模档形状与值尺寸量级（宽松界——防档位漂移，不锁具体字节）。
    #[test]
    fn tier_shapes_and_sizes() {
        let t1 = build_world(Tier::T1);
        assert_eq!(t1.room_names.len(), 1);
        let mature = t1.room_bytes(&t1.room_names[0], BASE_TIME);
        assert!(
            (8_000..200_000).contains(&mature.len()),
            "满配房 {} 字节",
            mature.len()
        );

        let t3 = build_world(Tier::T3);
        assert_eq!(t3.room_names.len(), 100);
        assert_eq!(t3.players.len(), 100);
        let total: usize = t3
            .room_names
            .iter()
            .map(|r| t3.room_bytes(r, BASE_TIME).len())
            .sum();
        assert!(
            (500_000..20_000_000).contains(&total),
            "百房总快照 {total} 字节"
        );

        // 每轮重写的值：长度不变、内容变（检查点重写形态）
        let young = t3.room_names[2].clone();
        let a = t3.room_bytes(&young, BASE_TIME);
        let b = t3.room_bytes(&young, BASE_TIME + 128);
        assert_eq!(a.len(), b.len());
        assert_ne!(a, b);
    }

    /// 玩家记录与 blob 的尺寸形态。
    #[test]
    fn user_and_blob_sizes() {
        let fix = build_world(Tier::T2);
        let users = users_dataset(&fix, 0, Tier::T2);
        assert_eq!(users.pairs.len(), 20);
        for (_, v) in &users.pairs {
            assert!((120..600).contains(&v.len()), "记录 {} 字节", v.len());
        }
        // 值随轮变化（检查点重写）
        let users2 = users_dataset(&fix, 3, Tier::T2);
        assert_ne!(users.pairs[0].1, users2.pairs[0].1);

        let blobs = blobs_dataset(&fix, Tier::T2, 0);
        let lens: Vec<usize> = blobs.pairs.iter().map(|(_, v)| v.len()).collect();
        assert_eq!(lens.iter().filter(|l| **l == 1 << 20).count(), 2);
        assert_eq!(lens.iter().filter(|l| **l == 100 << 10).count(), 18);
        assert_eq!(blobs.total_bytes() as usize, 18 * (100 << 10) + (2 << 20));
        // 局部变更：同轮确定性、跨轮头部 4KB 变化而长度不变
        let blobs_again = blobs_dataset(&fix, Tier::T2, 0);
        assert_eq!(blobs.pairs[0].1, blobs_again.pairs[0].1);
        let blobs_r1 = blobs_dataset(&fix, Tier::T2, 1);
        assert_eq!(blobs.pairs[0].1.len(), blobs_r1.pairs[0].1.len());
        assert_ne!(blobs.pairs[0].1[..4096], blobs_r1.pairs[0].1[..4096]);
        assert_eq!(blobs.pairs[0].1[4096..], blobs_r1.pairs[0].1[4096..]);
    }

    /// 键序：房间名字典序 = 数字序（前缀扫描的语义前提）。
    #[test]
    fn room_key_order() {
        let names: Vec<String> = (0..100).map(room_name).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        assert_eq!(room_name(0), "W0N0");
        assert_eq!(room_name(99), "W9N9");
    }
}
