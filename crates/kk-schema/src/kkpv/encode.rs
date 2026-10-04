//! 编码单块与块束，并校验束的版本一致性。
use super::*;

/// 将输入编码为完整的 KKPV 块。对象按 ID 升序、类型桶与字段按键
/// 排序后写入；嵌套记录保持条目顺序（玩家可见的键序不因编码重排），
/// 相同输入得到相同字节。
///
/// # Errors
/// 值域或限额违规、对象 ID 或字段键重复、同键值编码不一致时返回错误，
/// 不产出部分数据。
pub fn encode(input: &KkpvBlockInput, limits: &KkpvLimits) -> Result<Vec<u8>, KkpvError> {
    if input.room_name.is_empty() {
        return Err(KkpvError::RoomNameMissing);
    }
    if input.room_name.len() > limits.max_string_bytes {
        return Err(KkpvError::LimitExceeded("max_string_bytes"));
    }
    if input.objects.len() > limits.max_objects {
        return Err(KkpvError::LimitExceeded("max_objects"));
    }
    let terrain = input.terrain.as_deref();
    if terrain.is_some_and(|t| t.len() > limits.max_terrain_bytes) {
        return Err(KkpvError::LimitExceeded("max_terrain_bytes"));
    }

    let mut objects = input.objects.clone();
    objects.sort_by_key(|o| o.id);
    for (a, b) in objects.iter().zip(objects.iter().skip(1)) {
        if a.id == b.id {
            return Err(KkpvError::DuplicateObjectId(a.id));
        }
    }

    // 类型分桶；BTreeMap 同时给出类型键的确定顺序与句柄分配。
    let mut buckets: std::collections::BTreeMap<&str, Vec<&KkpvObject>> =
        std::collections::BTreeMap::new();
    for object in &objects {
        buckets
            .entry(object.type_key.as_str())
            .or_default()
            .push(object);
    }
    if buckets.len() > limits.max_types {
        return Err(KkpvError::LimitExceeded("max_types"));
    }

    struct FieldPlan<'o> {
        key_desc: (u32, u32),
        kind: u8,
        column: Vec<Option<&'o KkpvValue>>,
    }
    struct BucketPlan<'o> {
        rows: u32,
        fields: Vec<FieldPlan<'o>>,
    }
    let mut arena = ArenaBuilder { bytes: Vec::new() };
    let room_desc = arena.push_str(&input.room_name)?;
    let mut type_descs = Vec::with_capacity(buckets.len());
    let mut plans = Vec::with_capacity(buckets.len());
    for (type_key, objects) in &buckets {
        if objects.len() > u32::MAX as usize {
            return Err(KkpvError::LimitExceeded("max_objects"));
        }
        if type_key.len() > limits.max_string_bytes {
            return Err(KkpvError::LimitExceeded("max_string_bytes"));
        }
        type_descs.push(arena.push_str(type_key)?);
        // 字段目录是全桶键并集；重复只指同一对象内出现两次同名键。
        let mut field_order: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        for object in objects {
            let mut seen = std::collections::BTreeSet::new();
            for (key, _) in &object.fields {
                if key == "id" {
                    return Err(KkpvError::DuplicateField {
                        type_key: (*type_key).into(),
                        field: "id".into(),
                    });
                }
                if key.len() > limits.max_string_bytes {
                    return Err(KkpvError::LimitExceeded("max_string_bytes"));
                }
                if !seen.insert(key.as_str()) {
                    return Err(KkpvError::DuplicateField {
                        type_key: (*type_key).into(),
                        field: key.clone(),
                    });
                }
                field_order.insert(key.as_str());
            }
        }
        if field_order.len() > limits.max_fields_per_type {
            return Err(KkpvError::LimitExceeded("max_fields_per_type"));
        }
        let mut fields = Vec::with_capacity(field_order.len());
        for key in field_order {
            let key_desc = arena.push_str(key)?;
            let mut kind: Option<u8> = None;
            let mut column = Vec::with_capacity(objects.len());
            for object in objects {
                let value = object
                    .fields
                    .iter()
                    .find_map(|(k, v)| (k == key).then_some(v));
                let Some(value) = value else {
                    column.push(None);
                    continue;
                };
                if let Some(value_kind) = kind_of(value) {
                    match kind {
                        None => kind = Some(value_kind),
                        Some(existing) if existing != value_kind => {
                            return Err(KkpvError::KindMismatch {
                                type_key: (*type_key).into(),
                                field: key.to_string(),
                            });
                        }
                        _ => {}
                    }
                }
                column.push(Some(value));
            }
            // 全 null 字段没有可推断的列类型；bool 宽度最小，位图已表达全部信息。
            let kind = kind.unwrap_or(KIND_BOOL);
            fields.push(FieldPlan {
                key_desc,
                kind,
                column,
            });
        }
        plans.push(BucketPlan {
            rows: objects.len() as u32,
            fields,
        });
    }

    // 变长载荷在结构段之前进入 arena；描述符按桶/字段/行的确定顺序写入。
    let mut variable_cells: Vec<Vec<Vec<(u32, u32)>>> = Vec::with_capacity(plans.len());
    for plan in &plans {
        let mut per_field = Vec::with_capacity(plan.fields.len());
        for field in &plan.fields {
            let mut cells = Vec::with_capacity(field.column.len());
            if is_variable(field.kind) {
                for value in &field.column {
                    match value {
                        None | Some(KkpvValue::Null) => cells.push((0u32, 0u32)),
                        Some(value) => {
                            let mut payload = Vec::new();
                            encode_nested(value, limits, 0, &mut payload)?;
                            cells.push(arena.push_bytes(&payload)?);
                        }
                    }
                }
            }
            per_field.push(cells);
        }
        variable_cells.push(per_field);
    }
    if arena.bytes.len() > limits.max_arena_bytes {
        return Err(KkpvError::LimitExceeded("max_arena_bytes"));
    }

    let directory_len = objects.len() * DIRECTORY_ENTRY;
    let mut bucket_bytes = Vec::new();
    for (i, (plan, per_field)) in plans.iter().zip(&variable_cells).enumerate() {
        let type_desc = type_descs[i];
        bucket_bytes.extend_from_slice(&type_desc.0.to_le_bytes());
        bucket_bytes.extend_from_slice(&type_desc.1.to_le_bytes());
        bucket_bytes.extend_from_slice(&plan.rows.to_le_bytes());
        bucket_bytes.extend_from_slice(&(plan.fields.len() as u32).to_le_bytes());
        for field in &plan.fields {
            bucket_bytes.extend_from_slice(&field.key_desc.0.to_le_bytes());
            bucket_bytes.extend_from_slice(&field.key_desc.1.to_le_bytes());
            bucket_bytes.push(field.kind);
            bucket_bytes.extend_from_slice(&[0; 3]);
        }
        let bitmap_len = bitmap_bytes(plan.rows);
        for field in &plan.fields {
            let mut bitmap = vec![0u8; bitmap_len];
            for (row, value) in field.column.iter().enumerate() {
                if value.is_none() {
                    bitmap[row / 8] |= 1 << (row % 8);
                }
            }
            bucket_bytes.extend_from_slice(&bitmap);
        }
        for field in &plan.fields {
            let mut bitmap = vec![0u8; bitmap_len];
            for (row, value) in field.column.iter().enumerate() {
                if matches!(value, Some(KkpvValue::Null)) {
                    bitmap[row / 8] |= 1 << (row % 8);
                }
            }
            bucket_bytes.extend_from_slice(&bitmap);
        }
        for (field, cells) in plan.fields.iter().zip(per_field) {
            let width = column_width(field.kind);
            for (row, value) in field.column.iter().enumerate() {
                match value {
                    None | Some(KkpvValue::Null) => {
                        bucket_bytes.extend(std::iter::repeat_n(0u8, width));
                    }
                    Some(value) => {
                        if is_variable(field.kind) {
                            let cell = cells[row];
                            bucket_bytes.extend_from_slice(&cell.0.to_le_bytes());
                            bucket_bytes.extend_from_slice(&cell.1.to_le_bytes());
                        } else {
                            match value {
                                KkpvValue::Bool(b) => bucket_bytes.push(*b as u8),
                                KkpvValue::Int(v) => {
                                    bucket_bytes.extend_from_slice(&v.to_le_bytes());
                                }
                                KkpvValue::Uint(v) => {
                                    bucket_bytes.extend_from_slice(&v.to_le_bytes());
                                }
                                KkpvValue::Float(v) => {
                                    if !v.is_finite() {
                                        return Err(KkpvError::NonFiniteFloat);
                                    }
                                    bucket_bytes.extend_from_slice(&v.to_le_bytes());
                                }
                                KkpvValue::Ref(v) => {
                                    bucket_bytes.extend_from_slice(&v.to_le_bytes());
                                }
                                _ => unreachable!("定宽列的值类型已由 kind 校验"),
                            }
                        }
                    }
                }
            }
        }
    }

    let terrain_len = terrain.map_or(0, |t| t.len());
    let total = HEADER_LEN + terrain_len + directory_len + bucket_bytes.len() + arena.bytes.len();
    if total > limits.max_bytes {
        return Err(KkpvError::LimitExceeded("max_bytes"));
    }
    let fits = |value: usize| -> Result<u32, KkpvError> {
        u32::try_from(value).map_err(|_| KkpvError::LimitExceeded("max_bytes"))
    };
    // 零长度段写 (0, 0)：不存在的段不携带 offset，与解码端规范形一致。
    let seg_pair = |offset: usize, length: usize| -> Result<(u32, u32), KkpvError> {
        if length == 0 {
            Ok((0, 0))
        } else {
            Ok((fits(offset)?, fits(length)?))
        }
    };
    let terrain_off = HEADER_LEN;
    let directory_off = terrain_off + terrain_len;
    let buckets_off = directory_off + directory_len;
    let arena_off = buckets_off + bucket_bytes.len();

    let mut out = vec![0u8; total];
    out[..4].copy_from_slice(&MAGIC);
    out[OFF_VERSION..OFF_VERSION + 2].copy_from_slice(&VERSION.to_le_bytes());
    let flags = u16::from(input.private) * FLAG_PRIVATE;
    out[OFF_FLAGS..OFF_FLAGS + 2].copy_from_slice(&flags.to_le_bytes());
    out[OFF_BYTE_LEN..OFF_BYTE_LEN + 4].copy_from_slice(&fits(total)?.to_le_bytes());
    out[OFF_HEADER_LEN..OFF_HEADER_LEN + 4].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
    out[OFF_TICK..OFF_TICK + 8].copy_from_slice(&input.tick.to_le_bytes());
    out[OFF_GENERATION..OFF_GENERATION + 8].copy_from_slice(&input.generation.to_le_bytes());
    out[OFF_PLAN_HASH..OFF_PLAN_HASH + 32].copy_from_slice(&input.plan_hash);
    out[OFF_AUDIENCE_HASH..OFF_AUDIENCE_HASH + 32].copy_from_slice(&input.audience_hash);
    let (room_off, room_len) = room_desc;
    out[OFF_ROOM_NAME..OFF_ROOM_NAME + 4].copy_from_slice(&room_off.to_le_bytes());
    out[OFF_ROOM_NAME + 4..OFF_ROOM_NAME + 8].copy_from_slice(&room_len.to_le_bytes());
    out[OFF_OBJECT_COUNT..OFF_OBJECT_COUNT + 4]
        .copy_from_slice(&fits(objects.len())?.to_le_bytes());
    out[OFF_TYPE_COUNT..OFF_TYPE_COUNT + 4].copy_from_slice(&fits(plans.len())?.to_le_bytes());
    let (terrain_o, terrain_l) = seg_pair(terrain_off, terrain_len)?;
    out[OFF_TERRAIN..OFF_TERRAIN + 4].copy_from_slice(&terrain_o.to_le_bytes());
    out[OFF_TERRAIN + 4..OFF_TERRAIN + 8].copy_from_slice(&terrain_l.to_le_bytes());
    if let Some(terrain) = terrain {
        out[terrain_off..terrain_off + terrain_len].copy_from_slice(terrain);
    }
    let (directory_o, directory_l) = seg_pair(directory_off, directory_len)?;
    out[OFF_DIRECTORY..OFF_DIRECTORY + 4].copy_from_slice(&directory_o.to_le_bytes());
    out[OFF_DIRECTORY + 4..OFF_DIRECTORY + 8].copy_from_slice(&directory_l.to_le_bytes());
    let (buckets_o, buckets_l) = seg_pair(buckets_off, bucket_bytes.len())?;
    out[OFF_BUCKETS..OFF_BUCKETS + 4].copy_from_slice(&buckets_o.to_le_bytes());
    out[OFF_BUCKETS + 4..OFF_BUCKETS + 8].copy_from_slice(&buckets_l.to_le_bytes());
    let (arena_o, arena_l) = seg_pair(arena_off, arena.bytes.len())?;
    out[OFF_ARENA..OFF_ARENA + 4].copy_from_slice(&arena_o.to_le_bytes());
    out[OFF_ARENA + 4..OFF_ARENA + 8].copy_from_slice(&arena_l.to_le_bytes());
    // 目录按对象（已按 ID 升序）写入；句柄是桶在类型键序中的秩，
    // 行号是对象在其桶内按 ID 的序。
    let mut row_counters: std::collections::BTreeMap<&str, (u32, u32)> = buckets
        .keys()
        .enumerate()
        .map(|(rank, key)| (*key, (rank as u32, 0)))
        .collect();
    for (i, object) in objects.iter().enumerate() {
        let counter = row_counters
            .get_mut(object.type_key.as_str())
            .expect("对象必属已建桶");
        let at = directory_off + i * DIRECTORY_ENTRY;
        out[at..at + 8].copy_from_slice(&object.id.to_le_bytes());
        out[at + 8..at + 12].copy_from_slice(&counter.0.to_le_bytes());
        out[at + 12..at + 16].copy_from_slice(&counter.1.to_le_bytes());
        counter.1 += 1;
    }
    out[buckets_off..buckets_off + bucket_bytes.len()].copy_from_slice(&bucket_bytes);
    out[arena_off..arena_off + arena.bytes.len()].copy_from_slice(&arena.bytes);
    Ok(out)
}

/// 把多房间块序列编码为单一传输缓冲：`u32` 块数、逐块 `u32` 字节
/// 长度、随后依次拼接块字节。框架只承载宿主到沙箱的传输分帧，不属于
/// 块布局本身；整数小端与块内约定一致。空序列编码为仅含零计数的 4
/// 字节缓冲（无可见对象的世界仍是合法负载）。
///
/// # Errors
/// 任一块编码失败时返回错误，不产出部分缓冲。
pub fn encode_bundle(inputs: &[KkpvBlockInput], limits: &KkpvLimits) -> Result<Vec<u8>, KkpvError> {
    let blocks = inputs
        .iter()
        .map(|input| encode(input, limits))
        .collect::<Result<Vec<_>, _>>()?;
    let framing = 4 + blocks.len() * 4;
    let total = framing + blocks.iter().map(|b| b.len()).sum::<usize>();
    if total > limits.max_bytes.saturating_mul(16) {
        // 束总量按 16×单块限额兜底（与块数无直接关系：1000 个小块可过、
        // 17 个满块才失败）；多房世界的束不应超过单房块的合理倍数。
        return Err(KkpvError::LimitExceeded("max_bytes"));
    }
    let fits = |v: usize| -> Result<u32, KkpvError> {
        u32::try_from(v).map_err(|_| KkpvError::LimitExceeded("max_bytes"))
    };
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&fits(blocks.len())?.to_le_bytes());
    for block in &blocks {
        out.extend_from_slice(&fits(block.len())?.to_le_bytes());
    }
    for block in blocks {
        out.extend_from_slice(&block);
    }
    Ok(out)
}

/// 校验整束并返回（块数、块 tick、块 generation）：逐块走
/// [`KkpvView::decode`] 的全量结构校验，并要求全部块携带同一
/// (tick, generation)——同一次投影产出的束不得混入不同世界版本。
/// 沙箱侧解码器只做逐块结构校验（全量不变式由"生产者即编码器"
/// 保证）；本函数是共享束发布前的宿主侧校验入口。
///
/// # Errors
/// 框架分帧不符、任一块不合法或块间版本不一致时返回错误。
pub fn validate_bundle(data: &[u8], limits: &KkpvLimits) -> Result<(usize, u64, u64), KkpvError> {
    // 与 encode_bundle 同口径的束总量兜底：逐块限额之外再限总量，
    // 防御未来对外来束开放校验时的放大输入。
    if data.len() > limits.max_bytes.saturating_mul(16) {
        return Err(KkpvError::LimitExceeded("max_bytes"));
    }
    if data.len() < 4 {
        return Err(KkpvError::Malformed("束短于框架头部".into()));
    }
    let count = read_u32_at(data, 0) as usize;
    let lens_end = 4usize
        .checked_add(
            count
                .checked_mul(4)
                .ok_or(KkpvError::Malformed("束长度目录溢出".into()))?,
        )
        .ok_or(KkpvError::Malformed("束长度目录溢出".into()))?;
    if data.len() < lens_end {
        return Err(KkpvError::Malformed("束短于长度目录".into()));
    }
    let mut cur = lens_end;
    let mut version: Option<(u64, u64)> = None;
    for index in 0..count {
        let len = read_u32_at(data, 4 + index * 4) as usize;
        let end = cur
            .checked_add(len)
            .ok_or(KkpvError::Malformed("块长度溢出".into()))?;
        if end > data.len() {
            return Err(KkpvError::Malformed("块越界".into()));
        }
        let view = KkpvView::decode(&data[cur..end], limits)?;
        match version {
            None => version = Some((view.tick(), view.generation())),
            Some((tick, generation)) => {
                if (tick, generation) != (view.tick(), view.generation()) {
                    return Err(KkpvError::Malformed("束内块携带不同的世界版本".into()));
                }
            }
        }
        cur = end;
    }
    if cur != data.len() {
        return Err(KkpvError::Malformed("束长度与块目录不符".into()));
    }
    version.map_or_else(
        || Err(KkpvError::Malformed("空束无版本可校验".into())),
        |(tick, generation)| Ok((count, tick, generation)),
    )
}
