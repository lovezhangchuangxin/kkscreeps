use super::*;

fn obj(id: u64, type_key: &str, fields: &[(&str, KkpvValue)]) -> KkpvObject {
    KkpvObject {
        id,
        type_key: type_key.into(),
        fields: fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    }
}

fn input(objects: Vec<KkpvObject>) -> KkpvBlockInput {
    KkpvBlockInput {
        private: false,
        tick: 7,
        generation: 3,
        plan_hash: [7; 32],
        audience_hash: [9; 32],
        room_name: "W1N1".into(),
        terrain: None,
        objects,
    }
}

fn enc(input: &KkpvBlockInput) -> Vec<u8> {
    encode(input, &KkpvLimits::default()).expect("编码成功")
}

fn dec(data: &[u8]) -> KkpvView<'_> {
    KkpvView::decode(data, &KkpvLimits::default()).expect("解码成功")
}

fn dec_err(data: &[u8]) -> KkpvError {
    match KkpvView::decode(data, &KkpvLimits::default()) {
        Err(e) => e,
        Ok(_) => panic!("期望解码失败"),
    }
}

fn put_u16(data: &mut [u8], at: usize, value: u16) {
    data[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(data: &mut [u8], at: usize, value: u32) {
    data[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

/// 单类型单字段单对象的最小块及其结构偏移；字段类型由调用方指定。
fn minimal_block(kind_value: KkpvValue) -> (Vec<u8>, usize, usize, usize) {
    let bytes = enc(&input(vec![obj(1, "t:a", &[("k", kind_value)])]));
    let buckets_off = read_u32_at(&bytes, OFF_BUCKETS) as usize;
    let bitmap_off = buckets_off + BUCKET_HEADER + FIELD_ENTRY;
    let column_off = bitmap_off + 2;
    (bytes, buckets_off, bitmap_off, column_off)
}

#[test]
fn header_metadata_round_trip() {
    let mut spec = input(vec![]);
    spec.private = true;
    spec.tick = 42;
    spec.generation = 9;
    spec.plan_hash = [3; 32];
    spec.audience_hash = [4; 32];
    spec.terrain = Some(vec![7u8; 64]);
    let bytes = enc(&spec);
    let view = dec(&bytes);
    assert!(view.is_private());
    assert_eq!(view.tick(), 42);
    assert_eq!(view.generation(), 9);
    assert_eq!(view.plan_hash(), [3; 32]);
    assert_eq!(view.audience_hash(), [4; 32]);
    assert_eq!(view.room_name(), "W1N1");
    assert_eq!(view.terrain(), Some(&[7u8; 64][..]));
    assert_eq!(view.object_count(), 0);
}

#[test]
fn bundle_framing_round_trip() {
    let mut first = input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(5))])]);
    first.room_name = "W1N1".into();
    let mut second = input(vec![obj(9, "t:b", &[("s", KkpvValue::Str("x".into()))])]);
    second.room_name = "E2N2".into();
    second.tick = 8;
    let bundle = encode_bundle(&[first, second], &KkpvLimits::default()).expect("编码成功");
    // 框架自解析：u32 计数 + 逐块长度，块字节可各自建立视图。
    let count = u32::from_le_bytes(bundle[0..4].try_into().unwrap()) as usize;
    assert_eq!(count, 2);
    let mut cursor = 4;
    let mut lens = Vec::new();
    for _ in 0..count {
        lens.push(u32::from_le_bytes(bundle[cursor..cursor + 4].try_into().unwrap()) as usize);
        cursor += 4;
    }
    let mut rooms = Vec::new();
    for len in lens {
        let view = dec(&bundle[cursor..cursor + len]);
        rooms.push((
            view.room_name().to_string(),
            view.tick(),
            view.object_count(),
        ));
        cursor += len;
    }
    assert_eq!(cursor, bundle.len());
    assert_eq!(
        rooms,
        vec![("W1N1".to_string(), 7, 1), ("E2N2".to_string(), 8, 1)]
    );
    // 空序列是仅含零计数的合法负载（无可见对象的世界）。
    let empty = encode_bundle(&[], &KkpvLimits::default()).expect("空序列编码成功");
    assert_eq!(empty.len(), 4);
    assert_eq!(&empty[..4], &0u32.to_le_bytes());
}

#[test]
fn validate_bundle_checks_framing_and_versions() {
    let same_tick = || {
        let mut a = input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(5))])]);
        a.room_name = "W1N1".into();
        let mut b = input(vec![obj(9, "t:b", &[("s", KkpvValue::Str("x".into()))])]);
        b.room_name = "E2N2".into();
        encode_bundle(&[a, b], &KkpvLimits::default()).expect("编码成功")
    };
    let (count, tick, generation) =
        validate_bundle(&same_tick(), &KkpvLimits::default()).expect("同版本束通过");
    assert_eq!((count, tick, generation), (2, 7, 3));

    // 传输分帧允许混版本（bundle_framing_round_trip 锁定），发布校验
    // 拒绝——共享束不得跨世界版本拼装。
    let mut mixed_a = input(vec![]);
    mixed_a.room_name = "W1N1".into();
    let mut mixed_b = input(vec![]);
    mixed_b.room_name = "E2N2".into();
    mixed_b.tick = 8;
    let mixed = encode_bundle(&[mixed_a, mixed_b], &KkpvLimits::default()).expect("编码成功");
    assert!(
        validate_bundle(&mixed, &KkpvLimits::default()).is_err(),
        "块间版本不一致须拒绝"
    );

    let good = same_tick();
    assert!(
        validate_bundle(&[], &KkpvLimits::default()).is_err(),
        "空缓冲拒绝"
    );
    assert!(
        validate_bundle(&good[..3], &KkpvLimits::default()).is_err(),
        "截断框架头拒绝"
    );
    let mut truncated = good.clone();
    truncated.pop();
    assert!(
        validate_bundle(&truncated, &KkpvLimits::default()).is_err(),
        "尾块截断拒绝"
    );
    let mut inflated = good.clone();
    inflated[0] = 3;
    assert!(
        validate_bundle(&inflated, &KkpvLimits::default()).is_err(),
        "块数虚高时长度目录越界拒绝"
    );
    let empty = encode_bundle(&[], &KkpvLimits::default()).expect("空束");
    assert!(
        validate_bundle(&empty, &KkpvLimits::default()).is_err(),
        "空束无版本可校验，不得发布为共享块"
    );
}

#[test]
fn all_kinds_round_trip() {
    let fields: &[(&str, KkpvValue)] = &[
        ("b", KkpvValue::Bool(true)),
        ("i", KkpvValue::Int(-12)),
        ("u", KkpvValue::Uint(u64::MAX)),
        ("f", KkpvValue::Float(2.5)),
        ("r", KkpvValue::Ref(0xabc)),
        ("s", KkpvValue::Str("结构值".into())),
        (
            "arr",
            KkpvValue::Array(vec![
                KkpvValue::Null,
                KkpvValue::Int(1),
                KkpvValue::Array(Vec::new()),
            ]),
        ),
        (
            "rec",
            KkpvValue::Object(vec![
                ("zero".into(), KkpvValue::Uint(0)),
                ("嵌套".into(), KkpvValue::Str("v".into())),
            ]),
        ),
    ];
    let bytes = enc(&input(vec![obj(5, "t:a", fields)]));
    let view = dec(&bytes);
    assert_eq!(view.object_count(), 1);
    assert_eq!(view.object_id(0), 5);
    assert_eq!(view.object_type(0), "t:a");
    for (key, value) in fields {
        assert_eq!(view.field(0, key), Ok(Some(value.clone())), "字段 {key}");
    }
    assert_eq!(view.field(0, "不存在"), Ok(None));
    let mut expected: Vec<&str> = fields.iter().map(|(k, _)| *k).collect();
    expected.sort_unstable();
    assert_eq!(view.field_keys(0), expected);
    assert_eq!(view.object_index_by_id(5), Some(0));
    assert_eq!(view.object_index_by_id(6), None);
}

/// 缺失、null、零与空集合在解码后保持互异。
#[test]
fn missing_null_zero_empty_distinct() {
    // 三行分别为：缺失、null、有值；三列分别为 Int/Str/Array。
    let objects: Vec<KkpvObject> = vec![
        obj(1, "t:a", &[]),
        obj(
            2,
            "t:a",
            &[
                ("n", KkpvValue::Null),
                ("s", KkpvValue::Null),
                ("a", KkpvValue::Null),
            ],
        ),
        obj(
            3,
            "t:a",
            &[
                ("n", KkpvValue::Int(0)),
                ("s", KkpvValue::Str(String::new())),
                ("a", KkpvValue::Array(Vec::new())),
            ],
        ),
    ];
    let bytes = enc(&input(objects));
    let view = dec(&bytes);
    // 行 1：全部缺失；行 2：全部 null；行 3：零/空字符串/空数组。
    assert_eq!(view.field_keys(0), Vec::<&str>::new());
    for key in ["n", "s", "a"] {
        assert_eq!(view.field(1, key), Ok(Some(KkpvValue::Null)), "行 2 {key}");
    }
    assert_eq!(view.field(2, "n"), Ok(Some(KkpvValue::Int(0))));
    assert_eq!(view.field(2, "s"), Ok(Some(KkpvValue::Str(String::new()))));
    assert_eq!(view.field(2, "a"), Ok(Some(KkpvValue::Array(Vec::new()))));
    assert_eq!(view.field(0, "n"), Ok(None));
}

/// 对象、字段与记录键的提交顺序不影响编码字节。
#[test]
fn deterministic_bytes_regardless_of_order() {
    let fields_a = vec![
        ("x".to_string(), KkpvValue::Int(1)),
        ("a".to_string(), KkpvValue::Str("v".into())),
    ];
    let fields_b = vec![("a".to_string(), KkpvValue::Str("v".into()))];
    let straight = KkpvBlockInput {
        objects: vec![
            KkpvObject {
                id: 1,
                type_key: "t:a".into(),
                fields: fields_a.clone(),
            },
            KkpvObject {
                id: 2,
                type_key: "t:b".into(),
                fields: Vec::new(),
            },
            KkpvObject {
                id: 3,
                type_key: "t:a".into(),
                fields: fields_b.clone(),
            },
        ],
        ..input(vec![])
    };
    let shuffled = KkpvBlockInput {
        objects: vec![
            KkpvObject {
                id: 3,
                type_key: "t:a".into(),
                fields: fields_b.clone(),
            },
            KkpvObject {
                id: 2,
                type_key: "t:b".into(),
                fields: Vec::new(),
            },
            KkpvObject {
                id: 1,
                type_key: "t:a".into(),
                fields: vec![fields_a[1].clone(), fields_a[0].clone()],
            },
        ],
        ..input(vec![])
    };
    assert_eq!(enc(&straight), enc(&shuffled));
    let mut other = straight.clone();
    other.audience_hash = [1; 32];
    assert_ne!(enc(&straight), enc(&other), "受众摘要参与字节");
}

/// 目录在类型交错时仍按 ID 全局升序，类型与字段归属不串行。
#[test]
fn directory_sorted_across_types() {
    let objects = vec![
        obj(1, "t:b", &[("v", KkpvValue::Int(10))]),
        obj(2, "t:a", &[("w", KkpvValue::Int(20))]),
        obj(3, "t:b", &[("v", KkpvValue::Int(30))]),
        obj(4, "t:a", &[("w", KkpvValue::Int(40))]),
    ];
    let bytes = enc(&input(objects));
    let view = dec(&bytes);
    assert_eq!(
        (0..4).map(|i| view.object_id(i)).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert_eq!(view.object_type(0), "t:b");
    assert_eq!(view.object_type(1), "t:a");
    assert_eq!(view.field(0, "v"), Ok(Some(KkpvValue::Int(10))));
    assert_eq!(view.field(1, "w"), Ok(Some(KkpvValue::Int(20))));
    assert_eq!(view.field(2, "v"), Ok(Some(KkpvValue::Int(30))));
    assert_eq!(view.field(3, "w"), Ok(Some(KkpvValue::Int(40))));
    assert_eq!(view.field(0, "w"), Ok(None), "他桶字段不串行");
}

#[test]
fn terrain_round_trip() {
    let mut spec = input(vec![]);
    spec.terrain = Some(vec![0u8; 2500]);
    let bytes = enc(&spec);
    let view = dec(&bytes);
    assert_eq!(view.terrain().map(|t| t.len()), Some(2500));
}

// —— 编码拒绝 ——

#[test]
fn encode_rejects_invalid_inputs() {
    let limits = KkpvLimits::default();
    let dup_id = input(vec![
        obj(1, "t:a", &[("k", KkpvValue::Int(1))]),
        obj(1, "t:a", &[("k", KkpvValue::Int(2))]),
    ]);
    assert_eq!(
        encode(&dup_id, &limits),
        Err(KkpvError::DuplicateObjectId(1))
    );
    let dup_field = input(vec![obj(
        1,
        "t:a",
        &[("k", KkpvValue::Int(1)), ("k", KkpvValue::Int(2))],
    )]);
    assert!(matches!(
        encode(&dup_field, &limits),
        Err(KkpvError::DuplicateField { .. })
    ));
    let kind_mismatch = input(vec![
        obj(1, "t:a", &[("k", KkpvValue::Int(1))]),
        obj(2, "t:a", &[("k", KkpvValue::Bool(true))]),
    ]);
    assert!(matches!(
        encode(&kind_mismatch, &limits),
        Err(KkpvError::KindMismatch { .. })
    ));
    let id_field = input(vec![obj(1, "t:a", &[("id", KkpvValue::Int(1))])]);
    assert!(matches!(
        encode(&id_field, &limits),
        Err(KkpvError::DuplicateField { field, .. }) if field == "id"
    ));
    let nan = input(vec![obj(1, "t:a", &[("k", KkpvValue::Float(f64::NAN))])]);
    assert_eq!(encode(&nan, &limits), Err(KkpvError::NonFiniteFloat));
    let mut nested = KkpvValue::Int(0);
    for _ in 0..20 {
        nested = KkpvValue::Array(vec![nested]);
    }
    let deep = input(vec![obj(1, "t:a", &[("k", nested)])]);
    assert_eq!(encode(&deep, &limits), Err(KkpvError::DepthExceeded));
    let mut record_dup = input(vec![obj(1, "t:a", &[("k", KkpvValue::Null)])]);
    record_dup.objects[0].fields[0].1 = KkpvValue::Object(vec![
        ("a".into(), KkpvValue::Int(1)),
        ("a".into(), KkpvValue::Int(2)),
    ]);
    assert!(matches!(
        encode(&record_dup, &limits),
        Err(KkpvError::DuplicateField { .. })
    ));
    let mut empty_room = input(vec![]);
    empty_room.room_name = String::new();
    assert_eq!(
        encode(&empty_room, &limits),
        Err(KkpvError::RoomNameMissing)
    );
    let two = input(vec![
        obj(1, "t:a", &[("k", KkpvValue::Int(1))]),
        obj(2, "t:a", &[("k", KkpvValue::Int(2))]),
    ]);
    let mut tight = limits.clone();
    tight.max_objects = 1;
    assert_eq!(
        encode(&two, &tight),
        Err(KkpvError::LimitExceeded("max_objects"))
    );
    let mut long_str = input(vec![obj(1, "t:a", &[("k", KkpvValue::Str(String::new()))])]);
    long_str.objects[0].fields[0].1 = KkpvValue::Str("x".repeat(8193));
    assert_eq!(
        encode(&long_str, &limits),
        Err(KkpvError::LimitExceeded("max_string_bytes"))
    );
    // 键超限同样在编码端拒绝：字段键与类型键超出限额的块无法通过
    // 自身解码，不得产出。
    let mut long_key = input(vec![obj(1, "t:a", &[])]);
    long_key.objects[0]
        .fields
        .push(("k".repeat(8193), KkpvValue::Int(1)));
    assert_eq!(
        encode(&long_key, &limits),
        Err(KkpvError::LimitExceeded("max_string_bytes"))
    );
    let mut long_type = input(vec![obj(1, "t:a", &[])]);
    long_type.objects[0].type_key = "t".repeat(8193);
    assert_eq!(
        encode(&long_type, &limits),
        Err(KkpvError::LimitExceeded("max_string_bytes"))
    );
}

// —— 解码拒绝 ——

#[test]
fn decode_rejects_header_mutation() {
    let base = enc(&input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(5))])]));
    let mut bad_magic = base.clone();
    bad_magic[0] = b'X';
    assert!(matches!(dec_err(&bad_magic), KkpvError::BadMagic(_)));
    let mut v2 = base.clone();
    put_u16(&mut v2, OFF_VERSION, 2);
    assert_eq!(dec_err(&v2), KkpvError::UnsupportedVersion(2));
    let mut flags = base.clone();
    put_u16(&mut flags, OFF_FLAGS, 2);
    assert_eq!(dec_err(&flags), KkpvError::UnknownFlags(2));
    let mut byte_len = base.clone();
    put_u32(&mut byte_len, OFF_BYTE_LEN, base.len() as u32 + 1);
    assert!(matches!(dec_err(&byte_len), KkpvError::ByteLen { .. }));
    let mut header_len = base.clone();
    put_u32(&mut header_len, OFF_HEADER_LEN, 32);
    assert_eq!(dec_err(&header_len), KkpvError::HeaderLen(32));
    let mut reserved = base.clone();
    reserved[OFF_RESERVED] = 1;
    assert_eq!(dec_err(&reserved), KkpvError::ReservedNonZero);
    let short = &base[..base.len() - 1];
    assert!(matches!(dec_err(short), KkpvError::ByteLen { .. }))
}

#[test]
fn decode_rejects_directory_mutation() {
    let base = enc(&input(vec![
        obj(1, "t:a", &[("k", KkpvValue::Int(5))]),
        obj(2, "t:b", &[("k", KkpvValue::Int(6))]),
    ]));
    let directory_off = read_u32_at(&base, OFF_DIRECTORY) as usize;
    let mut swapped = base.clone();
    let (first, second) = (
        base[directory_off..directory_off + 16].to_vec(),
        base[directory_off + 16..directory_off + 32].to_vec(),
    );
    swapped[directory_off..directory_off + 16].copy_from_slice(&second);
    swapped[directory_off + 16..directory_off + 32].copy_from_slice(&first);
    assert_eq!(dec_err(&swapped), KkpvError::DirectoryNotSorted);
    let mut dup = base.clone();
    put_u64(&mut dup, directory_off + 16, 1);
    assert_eq!(dec_err(&dup), KkpvError::DuplicateObjectId(1));
    let mut handle = base.clone();
    put_u32(&mut handle, directory_off + 8, 9);
    assert_eq!(dec_err(&handle), KkpvError::UnknownTypeHandle(9));
    let mut row = base.clone();
    put_u32(&mut row, directory_off + 12, 5);
    assert_eq!(
        dec_err(&row),
        KkpvError::RowOutOfRange {
            handle: 0,
            row: 5,
            rows: 1
        }
    );
    let mut count = base.clone();
    put_u32(&mut count, OFF_OBJECT_COUNT, 3);
    assert!(matches!(dec_err(&count), KkpvError::DirectoryLength { .. }));
    let tight_limits = KkpvLimits {
        max_objects: 1,
        ..KkpvLimits::default()
    };
    assert_eq!(
        KkpvView::decode(&base, &tight_limits).err().unwrap(),
        KkpvError::LimitExceeded("max_objects")
    );
}

fn put_u64(data: &mut [u8], at: usize, value: u64) {
    data[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

#[test]
fn decode_rejects_segment_and_bucket_mutation() {
    // 带地形块：扩大地形长度使其与对象目录重叠。
    let mut with_terrain = input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(5))])]);
    with_terrain.terrain = Some(vec![0u8; 8]);
    let base = enc(&with_terrain);
    let mut overlap = base.clone();
    let terrain_len = read_u32_at(&base, OFF_TERRAIN + 4);
    put_u32(&mut overlap, OFF_TERRAIN + 4, terrain_len + 32);
    assert!(matches!(dec_err(&overlap), KkpvError::SegmentOverlap(_)));
    let mut oob = base.clone();
    put_u32(&mut oob, OFF_ARENA, base.len() as u32 - 2);
    assert!(matches!(
        dec_err(&oob),
        KkpvError::SegmentOutOfBounds { .. }
    ));
    let mut shrunk = base.clone();
    let buckets_len = read_u32_at(&base, OFF_BUCKETS + 4);
    put_u32(&mut shrunk, OFF_BUCKETS + 4, buckets_len - 1);
    assert!(matches!(dec_err(&shrunk), KkpvError::BucketWalk { .. }));

    // 单类型单字段块的结构偏移由布局常量推得。
    let (kind_base, buckets_off, _bitmap_off, _column_off) = minimal_block(KkpvValue::Int(5));
    let mut kind = kind_base.clone();
    kind[buckets_off + BUCKET_HEADER + 8] = 9;
    assert_eq!(dec_err(&kind), KkpvError::UnknownKind(9));
    // 桶头 rows 置零：拒绝必须发生在位图尾位检查之前，否则该处按
    // bitmap_len - 1 取尾字节会在 bitmap_len == 0 时下溢 panic。
    let mut zero_rows = kind_base;
    put_u32(&mut zero_rows, buckets_off + 8, 0);
    assert_eq!(
        dec_err(&zero_rows),
        KkpvError::EmptyBucket {
            type_key: "t:a".into()
        }
    );
    // 零字段桶是合法形态：同类型对象全部空 fields 时 field_count=0，
    // 与零行桶不同，不得被连带拒绝。
    let empty_fields = enc(&input(vec![obj(1, "t:a", &[])]));
    let view = dec(&empty_fields);
    assert_eq!(view.object_count(), 1);
    assert_eq!(view.object_type(0), "t:a");
    assert!(view.field(0, "k").expect("空字段桶查任意键").is_none());
    assert!(view.field_keys(0).is_empty());
    let (null_base, _, null_bitmap, _) = minimal_block(KkpvValue::Null);
    assert_eq!(null_base[null_bitmap], 0, "缺失位图原为零");
    assert_eq!(null_base[null_bitmap + 1], 1, "null 位图原为一");
    let mut both_bits = null_base.clone();
    both_bits[null_bitmap] = 1;
    assert_eq!(dec_err(&both_bits), KkpvError::AmbiguousBitmap);
    let (float_base, _, _, float_column) = minimal_block(KkpvValue::Float(2.5));
    let mut nan = float_base.clone();
    nan[float_column..float_column + 8].copy_from_slice(&f64::NAN.to_le_bytes());
    assert_eq!(dec_err(&nan), KkpvError::NonFiniteFloat);
    let (bool_base, _, bool_bitmap, bool_column) = minimal_block(KkpvValue::Bool(true));
    let mut bad_bool = bool_base.clone();
    bad_bool[bool_column] = 2;
    assert_eq!(dec_err(&bad_bool), KkpvError::InvalidBool(2));
    assert_eq!(bool_base[bool_bitmap], 0, "有值行两个位图都为零");
}

#[test]
fn decode_rejects_key_order_and_arena_mutation() {
    // 两个字段键交换描述符后不再升序。
    let two_fields = enc(&input(vec![obj(
        1,
        "t:a",
        &[("a", KkpvValue::Int(1)), ("b", KkpvValue::Int(2))],
    )]));
    let buckets_off = read_u32_at(&two_fields, OFF_BUCKETS) as usize;
    let first_entry = buckets_off + BUCKET_HEADER;
    let mut swapped = two_fields.clone();
    let desc_a = two_fields[first_entry..first_entry + 8].to_vec();
    let desc_b = two_fields[first_entry + FIELD_ENTRY..first_entry + FIELD_ENTRY + 8].to_vec();
    swapped[first_entry..first_entry + 8].copy_from_slice(&desc_b);
    swapped[first_entry + FIELD_ENTRY..first_entry + FIELD_ENTRY + 8].copy_from_slice(&desc_a);
    assert_eq!(dec_err(&swapped), KkpvError::KeyOrder);

    // 两个同形桶交换类型键描述符后不再升序。
    let two_types = enc(&input(vec![
        obj(1, "t:a", &[("k", KkpvValue::Int(1))]),
        obj(2, "t:b", &[("k", KkpvValue::Int(2))]),
    ]));
    let buckets_off = read_u32_at(&two_types, OFF_BUCKETS) as usize;
    let bucket_len = 16 + FIELD_ENTRY + 2 + 8;
    let mut swapped_types = two_types.clone();
    let desc_a = two_types[buckets_off..buckets_off + 8].to_vec();
    let desc_b = two_types[buckets_off + bucket_len..buckets_off + bucket_len + 8].to_vec();
    swapped_types[buckets_off..buckets_off + 8].copy_from_slice(&desc_b);
    swapped_types[buckets_off + bucket_len..buckets_off + bucket_len + 8].copy_from_slice(&desc_a);
    assert_eq!(dec_err(&swapped_types), KkpvError::KeyOrder);

    // 房名是非 UTF-8 时建立视图即失败。
    let mut room_utf8 = two_types.clone();
    let arena_off = read_u32_at(&two_types, OFF_ARENA) as usize;
    room_utf8[arena_off] = 0xFF;
    assert_eq!(dec_err(&room_utf8), KkpvError::InvalidUtf8);

    // 变长描述符越界在建立视图时被结构检查拦截。
    let (str_base, _, _, column_off) = minimal_block(KkpvValue::Str("hi".into()));
    let mut desc_oob = str_base.clone();
    let arena_len = read_u32_at(&str_base, OFF_ARENA + 4);
    put_u32(&mut desc_oob, column_off, arena_len);
    assert!(matches!(dec_err(&desc_oob), KkpvError::ArenaBounds { .. }));

    // 变长值内的坏字符串/坏标签在读取该字段时暴露。
    let mut lazy_utf8 = str_base.clone();
    let payload_off =
        read_u32_at(&str_base, OFF_ARENA) as usize + read_u32_at(&str_base, column_off) as usize;
    lazy_utf8[payload_off + 5] = 0xFF;
    let view = dec(&lazy_utf8);
    assert_eq!(
        view.field(0, "k").unwrap_err(),
        KkpvError::InvalidUtf8,
        "载荷字符串 UTF-8 在读取时校验"
    );
    let mut bad_tag = str_base.clone();
    bad_tag[payload_off] = 0xFF;
    let view = dec(&bad_tag);
    assert!(matches!(view.field(0, "k"), Err(KkpvError::UnknownTag(_))));
}

// —— 载荷与限额的行为锁定 ——

/// 解码端执行字节/arena/地形三项限额：超限块不得建立视图。
#[test]
fn decode_enforces_byte_arena_terrain_limits() {
    let base = enc(&input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(5))])]));
    let tiny = |what: &str, value: usize| KkpvLimits {
        max_bytes: if what == "max_bytes" {
            value
        } else {
            usize::MAX
        },
        max_arena_bytes: if what == "max_arena_bytes" {
            value
        } else {
            usize::MAX
        },
        max_terrain_bytes: if what == "max_terrain_bytes" {
            value
        } else {
            usize::MAX
        },
        ..KkpvLimits::default()
    };
    assert_eq!(
        KkpvView::decode(&base, &tiny("max_bytes", 100)).err(),
        Some(KkpvError::LimitExceeded("max_bytes"))
    );
    assert_eq!(
        KkpvView::decode(&base, &tiny("max_arena_bytes", 2)).err(),
        Some(KkpvError::LimitExceeded("max_arena_bytes"))
    );
    let mut with_terrain = input(vec![]);
    with_terrain.terrain = Some(vec![0u8; 64]);
    let terrain_block = enc(&with_terrain);
    assert_eq!(
        KkpvView::decode(&terrain_block, &tiny("max_terrain_bytes", 4)).err(),
        Some(KkpvError::LimitExceeded("max_terrain_bytes"))
    );
}

/// 嵌套记录的重复键在读取时拒绝；条目顺序保持载荷原序，不重排。
#[test]
fn nested_duplicate_record_key_rejected_on_read() {
    // 记录含 "a"、"b" 两键；把第二个键名字节改为 'a' 构成重复键。
    let bytes = enc(&input(vec![obj(
        1,
        "t:a",
        &[(
            "k",
            KkpvValue::Object(vec![
                ("a".to_string(), KkpvValue::Int(1)),
                ("b".to_string(), KkpvValue::Int(2)),
            ]),
        )],
    )]));
    // 嵌套载荷：TAG_OBJECT + count=2 + (4,"a",TAG_INT,8B) + (4,"b",...)。
    // 'b' 的绝对位置 = 载荷起点 + 1 + 4 + 14 + 4。
    let arena_off = read_u32_at(&bytes, OFF_ARENA) as usize;
    let buckets_off = read_u32_at(&bytes, OFF_BUCKETS) as usize;
    let column_off = buckets_off + BUCKET_HEADER + FIELD_ENTRY + 2;
    let payload_off = arena_off + read_u32_at(&bytes, column_off) as usize;
    let b_key_at = payload_off + 1 + 4 + 14 + 4;
    let mut dup = bytes.clone();
    assert_eq!(dup[b_key_at], b'b', "定位到第二个键名字节");
    dup[b_key_at] = b'a';
    let view = dec(&dup);
    assert_eq!(
        view.field(0, "k"),
        Err(KkpvError::DuplicateField {
            type_key: "嵌套记录".into(),
            field: "a".into()
        }),
        "重复键须在读取时拒绝"
    );
}

/// 嵌套记录保持条目顺序：非字典序键序原样往返，与 JSON 参考的
/// 插入序一致（preserve_order 构建下的玩家可见键序）。
#[test]
fn nested_record_preserves_entry_order() {
    let entries = vec![
        ("energy".to_string(), KkpvValue::Int(50)),
        ("U".to_string(), KkpvValue::Int(10)),
        ("XUH2O".to_string(), KkpvValue::Int(3)),
    ];
    let bytes = enc(&input(vec![obj(
        1,
        "t:a",
        &[("k", KkpvValue::Object(entries.clone()))],
    )]));
    match dec(&bytes).field(0, "k").expect("读取成功") {
        Some(KkpvValue::Object(decoded)) => {
            let keys: Vec<&str> = decoded.iter().map(|(k, _)| k.as_str()).collect();
            assert_eq!(keys, vec!["energy", "U", "XUH2O"], "条目顺序不得重排");
        }
        other => panic!("期望嵌套记录，得到 {other:?}"),
    }
}

/// 变长描述符吞入额外字节后，载荷非恰好消费完即拒绝。
#[test]
fn variable_payload_trailing_bytes_rejected() {
    // 两个同类型对象、各一个 Str 字段：两个载荷相邻，改长第一个
    // 描述符仍留在 arena 内，尾字节检查因此可单独命中。
    let base = enc(&input(vec![
        obj(1, "t:a", &[("s", KkpvValue::Str("ab".into()))]),
        obj(2, "t:a", &[("s", KkpvValue::Str("cd".into()))]),
    ]));
    let buckets_off = read_u32_at(&base, OFF_BUCKETS) as usize;
    // 列起始于桶头 16 + 字段条目 12 + 位图 1 + 1。
    let first_column = buckets_off + BUCKET_HEADER + FIELD_ENTRY + 2;
    let mut stretched = base.clone();
    let len = read_u32_at(&base, first_column + 4);
    put_u32(&mut stretched, first_column + 4, len + 1);
    let view = dec(&stretched);
    assert!(
        matches!(view.field(0, "s"), Err(KkpvError::ArenaBounds { .. })),
        "多读的尾字节使载荷非恰好消费"
    );
}

/// arena 段缺失与零长度段非零 offset 都拒绝。
#[test]
fn arena_missing_and_zero_segment_offset_rejected() {
    let base = enc(&input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(5))])]));
    let mut no_arena = base.clone();
    let arena_off = read_u32_at(&base, OFF_ARENA) as usize;
    let arena_len = read_u32_at(&base, OFF_ARENA + 4) as usize;
    put_u32(&mut no_arena, OFF_ARENA, 0);
    put_u32(&mut no_arena, OFF_ARENA + 4, 0);
    // 把 arena 内容挪进 terrain 段位置以保持总长不变，段表合法但 arena 空。
    let _ = (arena_off, arena_len);
    assert_eq!(dec_err(&no_arena), KkpvError::ArenaMissing);
    // 本块无地形段（len=0）：offset 非零即拒绝。
    let mut bad_offset = base.clone();
    put_u32(&mut bad_offset, OFF_TERRAIN, 99);
    assert!(matches!(
        dec_err(&bad_offset),
        KkpvError::SegmentOutOfBounds { .. }
    ));
}

/// 字段条目保留字节与位图越行尾位非零都拒绝；目录行重复引用拒绝。
#[test]
fn reserved_entry_bytes_bitmap_tail_and_row_alias_rejected() {
    let (kind_base, buckets_off, _, _) = minimal_block(KkpvValue::Int(5));
    let mut reserved = kind_base.clone();
    reserved[buckets_off + BUCKET_HEADER + 9] = 0xAA;
    assert_eq!(dec_err(&reserved), KkpvError::ReservedNonZero);

    let two = enc(&input(vec![
        obj(1, "t:a", &[("k", KkpvValue::Int(1))]),
        obj(2, "t:a", &[("k", KkpvValue::Int(2))]),
    ]));
    let buckets_off = read_u32_at(&two, OFF_BUCKETS) as usize;
    let bitmap_off = buckets_off + BUCKET_HEADER + FIELD_ENTRY;
    let mut tail = two.clone();
    tail[bitmap_off] = 0b1111_1100;
    assert_eq!(dec_err(&tail), KkpvError::BitmapTailBits);

    let directory_off = read_u32_at(&two, OFF_DIRECTORY) as usize;
    let mut aliased = two.clone();
    put_u32(&mut aliased, directory_off + DIRECTORY_ENTRY + 12, 0);
    assert_eq!(
        dec_err(&aliased),
        KkpvError::DuplicateRowRef { handle: 0, row: 0 },
        "两对象引用同行使一行数据以两个 ID 显形"
    );
}

// —— 合并 ——

#[test]
fn merge_public_and_private() {
    let public = input(vec![
        obj(2, "t:a", &[("k", KkpvValue::Int(2))]),
        obj(4, "t:a", &[("k", KkpvValue::Int(4))]),
    ]);
    let mut overlay = input(vec![
        obj(1, "t:a", &[("k", KkpvValue::Int(1))]),
        obj(3, "t:a", &[("k", KkpvValue::Int(3))]),
    ]);
    overlay.private = true;
    overlay.audience_hash = [5; 32];
    let public_bytes = enc(&public);
    let overlay_bytes = enc(&overlay);
    let public_view = dec(&public_bytes);
    let overlay_view = dec(&overlay_bytes);
    let merged = merge(&public_view, &overlay_view).expect("同版本块可合并");
    assert_eq!(merged.object_count(), 4);
    assert_eq!(
        (0..merged.object_count())
            .map(|i| merged.object_id(i))
            .collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    assert_eq!(
        (0..merged.object_count())
            .map(|i| merged.from_overlay(i))
            .collect::<Vec<_>>(),
        vec![true, false, true, false]
    );
    assert_eq!(
        merged.field(0, "k"),
        Ok(Some(KkpvValue::Int(1))),
        "overlay 字段经合并视图读取"
    );
    assert_eq!(merged.room_name(), "W1N1");
}

#[test]
fn merge_rejects_identity_and_conflicts() {
    let public_bytes = enc(&input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(1))])]));
    let mk_overlay = |mut spec: KkpvBlockInput| {
        spec.private = true;
        enc(&spec)
    };
    let conflict_bytes = mk_overlay(input(vec![obj(1, "t:a", &[("k", KkpvValue::Int(9))])]));
    let pv = dec(&public_bytes);
    let cv = dec(&conflict_bytes);
    assert_eq!(
        merge(&pv, &cv).err().unwrap(),
        KkpvError::MergeObjectConflict(1)
    );
    let mut tick_spec = input(vec![obj(2, "t:a", &[("k", KkpvValue::Int(1))])]);
    tick_spec.tick = 8;
    let tick_bytes = mk_overlay(tick_spec);
    let tv = dec(&tick_bytes);
    assert!(matches!(
        merge(&pv, &tv).err().unwrap(),
        KkpvError::MergeTickMismatch { .. }
    ));
    let mut gen_spec = input(vec![obj(2, "t:a", &[("k", KkpvValue::Int(1))])]);
    gen_spec.generation = 4;
    let gen_bytes = mk_overlay(gen_spec);
    let gv = dec(&gen_bytes);
    assert!(matches!(
        merge(&pv, &gv).err().unwrap(),
        KkpvError::MergeGenerationMismatch { .. }
    ));
    let mut plan_spec = input(vec![obj(2, "t:a", &[("k", KkpvValue::Int(1))])]);
    plan_spec.plan_hash = [1; 32];
    let plan_bytes = mk_overlay(plan_spec);
    let plv = dec(&plan_bytes);
    assert_eq!(
        merge(&pv, &plv).err().unwrap(),
        KkpvError::MergePlanMismatch
    );
    let mut room_spec = input(vec![obj(2, "t:a", &[("k", KkpvValue::Int(1))])]);
    room_spec.room_name = "E2S2".into();
    let room_bytes = mk_overlay(room_spec);
    let rv = dec(&room_bytes);
    assert!(matches!(
        merge(&pv, &rv).err().unwrap(),
        KkpvError::MergeRoomMismatch { .. }
    ));
    let both_public_bytes = enc(&input(vec![obj(2, "t:a", &[("k", KkpvValue::Int(1))])]));
    let both_public = dec(&both_public_bytes);
    assert!(matches!(
        merge(&pv, &both_public).err().unwrap(),
        KkpvError::MergeFlags { .. }
    ));
}

/// owned 入口（`encode`）与借用入口（`encode_ref`）必须逐字节一致：
/// 两条路径共用同一编码核心，本测试在混合值域（定宽/变长/嵌套/缺失/
/// null/全 null 列/多类型桶/乱序输入）上锁定不漂移。
#[test]
fn encode_ref_matches_owned_entry() {
    use std::borrow::Cow;
    let objects = vec![
        obj(
            9,
            "t:b",
            &[
                ("only_b", KkpvValue::Bool(false)),
                ("maybe", KkpvValue::Null),
            ],
        ),
        obj(
            2,
            "t:a",
            &[
                ("i", KkpvValue::Int(-3)),
                ("s", KkpvValue::Str("文本".into())),
                (
                    "rec",
                    KkpvValue::Object(vec![(
                        "成员".into(),
                        KkpvValue::Array(vec![KkpvValue::Ref(7), KkpvValue::Null]),
                    )]),
                ),
            ],
        ),
        obj(
            5,
            "t:a",
            &[("i", KkpvValue::Int(9)), ("f", KkpvValue::Uint(1))],
        ),
        // 桶内唯一行且字段全 null：列类型回退 bool。
        obj(11, "t:c", &[("all_null", KkpvValue::Null)]),
    ];
    let input = input(objects);
    let owned = enc(&input);
    let block = KkpvBlockRef {
        private: input.private,
        tick: input.tick,
        generation: input.generation,
        plan_hash: input.plan_hash,
        audience_hash: input.audience_hash,
        room_name: input.room_name.as_str(),
        terrain: None,
        rows: input
            .objects
            .iter()
            .map(|object| KkpvRow {
                id: object.id,
                type_key: Cow::Borrowed(object.type_key.as_str()),
                fields: object
                    .fields
                    .iter()
                    .map(|(key, value)| (key.as_str(), value.as_val()))
                    .collect(),
            })
            .collect(),
    };
    let direct = encode_ref(&block, &KkpvLimits::default()).expect("借用入口编码成功");
    assert_eq!(owned, direct, "owned 与借用入口必须产出相同字节");
}
