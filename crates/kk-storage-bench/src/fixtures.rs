//! 为存储基准生成固定种子的合成数据。
//!
//! 房间对象经 [`kk_world::place::normalize`] 构造，再用 kk-schema 的二进制
//! 房间快照格式编码。玩家记录是小 JSON 值，大字节块模拟 Memory 的大小与
//! 局部修改；它们不是从真实玩家存档采样得到的负载。
//! 数据集构造在计时前完成，相同档位和轮次可重现相同字节。

use kk_core::rng::SeededRng;
use kk_core::{TERRAIN_MASK_SWAMP, Terrain};
use serde_json::{Value, json};

use crate::engine::TableKind;

/// 构造对象时使用的固定世界时间，让绝对时间字段不从 0 起算。
pub const BASE_TIME: u64 = 1_234_567;

/// 合成房间的三种对象组合，决定对象数量和 creep 身体大小。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Maturity {
    /// RCL 1、一个 spawn、两个 creep，另有道路和资源等对象。
    Young,
    /// RCL 4、10 个 extension、storage，另有墙、道路和六个 creep。
    Mid,
    /// RCL 8、60 个 extension、三个 spawn、180 面墙、150 条道路和 20 个 creep，
    /// 其中一半有 50 个身体部件。
    Mature,
}

/// 规模档。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// 单房热点（1 满配房、4 玩家）。
    T1,
    /// 小多房（12 房：满配/中期/新落地各 4、20 玩家）。
    T2,
    /// 100 房：满配 34 房、中期和新落地各 33 房，100 玩家。
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

/// 生成房间名；夹具使用索引 0..100，得到 `W0N0` 至 `W9N9`。
/// 此范围内两个坐标各占一位，字典序与索引顺序相同；更大索引没有此保证。
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

/// 混合用途字符串和索引，为房间、blob 内容与头部修改分别生成稳定种子。
fn seed_for(domain: &str, index: u64) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325;
    for b in domain.as_bytes().iter().chain(std::iter::once(&0x2f)) {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x1000_0000_01b3);
    }
    h ^ index.wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

/// 合成世界及其房间、玩家名单。
pub struct WorldFixture {
    pub world: kk_world::world::World,
    pub room_names: Vec<String>,
    pub players: Vec<String>,
}

/// 按规模档构造世界，相同档位使用相同种子和对象插入顺序。
pub fn build_world(tier: Tier) -> WorldFixture {
    let mut world = kk_world::world::World::new(0xC3C3);
    world.set_time_for_test(BASE_TIME);
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
    /// 编码指定房间的二进制快照，仅将快照头的时间替换为 `time`。
    /// 不推进世界或结算对象；改变头部时间可产生同长度、不同内容的写入值。
    ///
    /// # Panics
    /// 房间未注册时 panic。
    pub fn room_bytes(&self, room: &str, time: u64) -> Vec<u8> {
        let state = self.world.room(room).expect("夹具房已注册");
        kk_schema::room::encode_snapshot(&state.encode_snapshot(
            time,
            self.world.seed,
            self.world.next_obj_id(),
        ))
    }

    /// 按名单顺序返回房间名的 UTF-8 字节键。
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

/// 在房间内部的 46×46 网格按行分配坐标（2..=47），超过 2116 个位置后循环。
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

fn body_spec(parts: &[&str], count: usize) -> Vec<Value> {
    (0..count).map(|i| json!(parts[i % parts.len()])).collect()
}

fn pos_json(p: (u8, u8)) -> Value {
    json!([p.0, p.1])
}

fn place(world: &mut kk_world::world::World, room: &str, kind: &str, spec: Value) {
    let time = world.time();
    let ctx_place = |w: &kk_world::world::World, spec: &Value| {
        let controller = w.room(room).expect("夹具房已注册").controller().cloned();
        kk_world::place::normalize(
            kind,
            spec,
            &kk_world::place::PlaceCtx {
                time,
                links: kk_world::place::builtin_link_rules(),
                stores: kk_world::place::builtin_store_rules(),
                buildings: Default::default(),
                controller: controller.as_ref(),
                name_seq: w.room(room).expect("夹具房已注册").objects.len(),
            },
        )
    };
    let object = ctx_place(world, &spec).expect("夹具 spec 必然合法");
    world
        .insert(room, object)
        .unwrap_or_else(|e| panic!("夹具放置失败 ({room}/{kind}): {e}"));
}

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

    // 先放控制器，让后续 source 的容量能按控制器归属和等级确定。
    place(
        world,
        room,
        "structure",
        json!({
            "pos": pos_json(pos.next()), "structureType": "controller", "rcl": rcl,
            "owner": owner, "progress": rng.below(8_000_000),
            "downgradeTime": world.time() + 20_000 + u64::from(rng.below(30_000)),
        }),
    );
    // 本夹具固定放两个 source。
    for _ in 0..2 {
        place(
            world,
            room,
            "structure",
            json!({ "pos": pos_json(pos.next()), "structureType": "source" }),
        );
    }
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
    // 增加大量字段较少的对象，改变房间快照的对象数量与字节大小。
    for _ in 0..n_road {
        place(
            world,
            room,
            "structure",
            json!({ "pos": pos_json(pos.next()), "structureType": "road" }),
        );
    }

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
    for f in 0..n_flag {
        place(
            world,
            room,
            "flag",
            json!({ "pos": pos_json(pos.next()), "name": format!("flag{f}") }),
        );
    }
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
    for _ in 0..n_res {
        place(
            world,
            room,
            "resource",
            json!({ "pos": pos_json(pos.next()), "resourceType": "energy", "amount": rng.below(2_000) }),
        );
    }
    // 中期房增加 mineral，满配房另加 tombstone。两者均由 normalize 构造。
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

/// 一个场景写入的键值数据集。
pub struct Dataset {
    pub name: String,
    pub table: TableKind,
    pub pairs: Vec<(Vec<u8>, Vec<u8>)>,
}

impl Dataset {
    /// 值的字节总数，不含键、表信息或数据库页开销。
    pub fn total_bytes(&self) -> u64 {
        self.pairs.iter().map(|(_, v)| v.len() as u64).sum()
    }
}

/// 房间快照数据集，键为房间名；每轮把快照头的时间增加 128 tick，不结算对象。
/// 房间数量由 `fix` 决定，`tier` 只用于报告中的数据集名称。
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

/// 玩家小 JSON 记录；键为 `u:<玩家名>`，计数器随轮次变化，长度不保证固定。
/// 玩家数量由 `fix` 决定，`tier` 只用于报告中的数据集名称。
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

/// 每玩家生成一个字节块：索引 4 和 9 使用 1 MiB，其余使用 100 KiB。
/// 内容是伪随机字节，不是有效的 Memory JSON，也不是实测的玩家数据分布。
///
/// 每轮只改变前 4 KiB，其余内容保持不变，但引擎仍收到完整值。
/// 局部修改用于模拟修改集中在一小部分字节的负载，并避免连续写入相同值。
/// 玩家数量由 `fix` 决定，`tier` 只用于报告中的数据集名称。
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

/// 按 RNG 序列生成指定长度的字节，每个 u32 按小端展开，末尾截到 `len`。
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

    // 三档各构造两次，比较每个房间在固定时间下的快照字节。
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

    // 解码快照并检查时间和对象数下限，不锁定完整对象字段。
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

    // 尺寸使用宽松范围，允许 schema 演化；另检查修改头部时间不改变长度。
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
