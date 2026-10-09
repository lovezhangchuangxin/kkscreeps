//! 数据模型与管理入口的单元行为验证。
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

#[test]
fn version_one_without_authoritative_storage_capacity_is_rejected() {
    let mut bytes = encode_snapshot(&RoomSnapshot {
        header: RoomHeader {
            time: 0,
            seed: 0,
            next_obj_id: 1,
            terrain: [0; 2500],
        },
        objects: vec![],
        events: vec![],
    });
    bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
    assert!(matches!(
        decode_snapshot(&bytes),
        Err(SchemaError::UnsupportedVersion(1))
    ));
}
