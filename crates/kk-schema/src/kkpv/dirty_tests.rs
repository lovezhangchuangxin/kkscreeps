//! 行级脏位的协议级测试（编解码与块头校验）。
use super::tests::{dec_err, enc, input, obj, put_u32};
use super::*;

fn enc_dirty(
    objects: &[KkpvObject],
    tick: u64,
    prev: Option<&PrevEncodedBlock>,
) -> PrevEncodedBlock {
    let owned = KkpvBlockInput {
        private: true,
        tick,
        generation: 3,
        plan_hash: [7; 32],
        audience_hash: [9; 32],
        room_name: "W1N1".into(),
        terrain: None,
        objects: objects.to_vec(),
    };
    let rows = owned
        .objects
        .iter()
        .map(|object| KkpvRow {
            id: object.id,
            type_key: std::borrow::Cow::Borrowed(object.type_key.as_str()),
            fields: object
                .fields
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_val()))
                .collect(),
        })
        .collect();
    let block = KkpvBlockRef {
        private: true,
        tick,
        generation: 3,
        plan_hash: [7; 32],
        audience_hash: [9; 32],
        room_name: "W1N1",
        terrain: None,
        rows,
    };
    encode_with_dirty(&block, &KkpvLimits::default(), prev).expect("编码成功")
}

#[test]
fn decode_accepts_and_rejects_dirty_variants() {
    let objects = vec![obj(1, "t:a", &[("k", KkpvValue::Int(5))])];
    let first = enc_dirty(&objects, 7, None);
    // 首拍与纯编码逐字节一致（保留区零、无脏位段）。
    let mut owned = input(objects.clone());
    owned.private = true;
    assert_eq!(first.bytes(), &enc(&owned)[..]);
    let second = enc_dirty(&objects, 8, Some(&first));
    let data = second.bytes();
    KkpvView::decode(data, &KkpvLimits::default()).expect("脏位块可建视图");
    assert_eq!(read_u64_at(data, OFF_DIRTY_BASE_TICK), 7);

    // 置位脏段缺失：FLAG_DIRTY 与段不一致。
    let mut flag_only = data.to_vec();
    flag_only[OFF_FLAGS..OFF_FLAGS + 2].copy_from_slice(&1u16.to_le_bytes());
    assert!(matches!(dec_err(&flag_only), KkpvError::ReservedNonZero));

    // 有段无旗（清 FLAG_DIRTY 但保留区未清）：保留区非零先于一致性
    // 检查拒绝。
    let mut seg_only = data.to_vec();
    seg_only[OFF_FLAGS..OFF_FLAGS + 2].copy_from_slice(&1u16.to_le_bytes());
    assert!(matches!(dec_err(&seg_only), KkpvError::ReservedNonZero));

    // 基准拍号晚于本块拍号。
    let future = enc_dirty(&objects, 6, Some(&first));
    assert!(matches!(
        KkpvView::decode(future.bytes(), &KkpvLimits::default()),
        Err(KkpvError::Malformed(msg)) if msg.contains("基准拍号")
    ));

    // 脏位段长度与桶行数不符：段尾追加一字节并同步声长，段界合法
    // 但与桶行数走查不符。
    let mut short = data.to_vec();
    short.push(0);
    let declared = short.len() as u32;
    put_u32(&mut short, OFF_BYTE_LEN, declared);
    let seg_len = read_u32_at(&short, OFF_DIRTY + 4);
    put_u32(&mut short, OFF_DIRTY + 4, seg_len + 1);
    assert!(matches!(
        dec_err(&short),
        KkpvError::Malformed(msg) if msg.contains("脏位段长度")
    ));

    // 脏位图越行尾位置位。
    let mut tail = data.to_vec();
    let seg_off = read_u32_at(&tail, OFF_DIRTY) as usize;
    tail[seg_off] |= 0xFE;
    assert_eq!(dec_err(&tail), KkpvError::BitmapTailBits);
}
