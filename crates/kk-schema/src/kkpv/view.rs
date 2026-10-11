//! 校验单块并提供按需解码的只读视图。
use super::*;

// —— 解码视图 ——————————————————————————————————————————————

struct FieldView<'a> {
    key: &'a str,
    kind: u8,
    missing: &'a [u8],
    null: &'a [u8],
    column: &'a [u8],
}

struct BucketView<'a> {
    type_key: &'a str,
    rows: u32,
    fields: Vec<FieldView<'a>>,
}

struct DirectoryEntry {
    id: u64,
    handle: u32,
    row: u32,
}

/// 已通过结构校验的块视图。字段值在读取时解码；变长值内的字符串
/// 在读取时校验 UTF-8。视图借用块字节，块本身不可变。
pub struct KkpvView<'a> {
    data: &'a [u8],
    tick: u64,
    generation: u64,
    plan_hash: [u8; 32],
    audience_hash: [u8; 32],
    flags: u16,
    room_name: &'a str,
    terrain: Option<&'a [u8]>,
    arena: &'a [u8],
    limits: KkpvLimits,
    objects: Vec<DirectoryEntry>,
    buckets: Vec<BucketView<'a>>,
    /// FLAG_DIRTY 块的逐桶行脏位图切片（桶序）；未携带时为空。
    dirty_bitmaps: Vec<&'a [u8]>,
}

/// 检查段表：各段完整落在块内、不与头部及彼此重叠；零长度段豁免重叠检查。
fn check_segments(
    data_len: usize,
    segments: [(&'static str, usize, usize); 5],
) -> Result<(), KkpvError> {
    // 零长度段不存在内容，offset 必须为零；与头部保留字节同一规范形。
    if segments
        .iter()
        .any(|(_, offset, length)| *length == 0 && *offset != 0)
    {
        return Err(KkpvError::SegmentOutOfBounds {
            segment: segments
                .iter()
                .find(|(_, offset, length)| *length == 0 && *offset != 0)
                .map(|(s, _, _)| *s)
                .unwrap_or("arena"),
            offset: 0,
            length: 0,
        });
    }
    let mut present: Vec<(&'static str, usize, usize)> = segments
        .into_iter()
        .filter(|(_, _, length)| *length > 0)
        .collect();
    for (segment, offset, length) in &present {
        let end = offset.checked_add(*length);
        if !matches!(end, Some(end) if *offset >= HEADER_LEN && end <= data_len) {
            return Err(KkpvError::SegmentOutOfBounds {
                segment,
                offset: *offset,
                length: *length,
            });
        }
    }
    present.sort_by_key(|(_, offset, _)| *offset);
    for (a, b) in present.iter().zip(present.iter().skip(1)) {
        if a.1 + a.2 > b.1 {
            return Err(KkpvError::SegmentOverlap(b.0));
        }
    }
    Ok(())
}

fn decode_nested(
    limits: &KkpvLimits,
    depth: usize,
    bytes: &mut &[u8],
) -> Result<KkpvValue, KkpvError> {
    if depth > limits.max_value_depth.min(NESTING_HARD_CAP) {
        return Err(KkpvError::DepthExceeded);
    }
    let (&tag, rest) = bytes.split_first().ok_or(KkpvError::UnknownTag(0))?;
    *bytes = rest;
    fn take<'b>(bytes: &mut &'b [u8], n: usize) -> Result<&'b [u8], KkpvError> {
        if bytes.len() < n {
            return Err(KkpvError::ArenaBounds {
                offset: 0,
                length: n,
            });
        }
        let (head, tail) = bytes.split_at(n);
        *bytes = tail;
        Ok(head)
    }
    match tag {
        TAG_NULL => Ok(KkpvValue::Null),
        TAG_BOOL => Ok(KkpvValue::Bool(match take(bytes, 1)?[0] {
            0 => false,
            1 => true,
            b => return Err(KkpvError::InvalidBool(b)),
        })),
        TAG_INT => Ok(KkpvValue::Int(i64::from_le_bytes(
            take(bytes, 8)?.try_into().unwrap(),
        ))),
        TAG_UINT => Ok(KkpvValue::Uint(u64::from_le_bytes(
            take(bytes, 8)?.try_into().unwrap(),
        ))),
        TAG_FLOAT => {
            let v = f64::from_le_bytes(take(bytes, 8)?.try_into().unwrap());
            if !v.is_finite() {
                return Err(KkpvError::NonFiniteFloat);
            }
            Ok(KkpvValue::Float(v))
        }
        TAG_REF => Ok(KkpvValue::Ref(u64::from_le_bytes(
            take(bytes, 8)?.try_into().unwrap(),
        ))),
        TAG_STR => {
            let len = u32::from_le_bytes(take(bytes, 4)?.try_into().unwrap()) as usize;
            if len > limits.max_string_bytes {
                return Err(KkpvError::LimitExceeded("max_string_bytes"));
            }
            let raw = take(bytes, len)?;
            let s = std::str::from_utf8(raw).map_err(|_| KkpvError::InvalidUtf8)?;
            Ok(KkpvValue::Str(s.into()))
        }
        TAG_ARRAY => {
            let len = u32::from_le_bytes(take(bytes, 4)?.try_into().unwrap()) as usize;
            if len > limits.max_array_len {
                return Err(KkpvError::LimitExceeded("max_array_len"));
            }
            let mut items = Vec::with_capacity(len.min(bytes.len()));
            for _ in 0..len {
                items.push(decode_nested(limits, depth + 1, bytes)?);
            }
            Ok(KkpvValue::Array(items))
        }
        TAG_OBJECT => {
            let len = u32::from_le_bytes(take(bytes, 4)?.try_into().unwrap()) as usize;
            if len > limits.max_object_entries {
                return Err(KkpvError::LimitExceeded("max_object_entries"));
            }
            // 嵌套记录保持载荷条目顺序（与 JSON 参考的插入序一致，玩家可见
            // 面——如 Store 的键序——不分叉）；只拒绝重复键，取舍不由消费者
            // 偶然决定。有界条目数下的集合去重成本可忽略。
            let mut entries = Vec::with_capacity(len.min(bytes.len()));
            let mut seen = std::collections::BTreeSet::new();
            for _ in 0..len {
                let key_len = u32::from_le_bytes(take(bytes, 4)?.try_into().unwrap()) as usize;
                if key_len > limits.max_string_bytes {
                    return Err(KkpvError::LimitExceeded("max_string_bytes"));
                }
                let key = std::str::from_utf8(take(bytes, key_len)?)
                    .map_err(|_| KkpvError::InvalidUtf8)?;
                if !seen.insert(key) {
                    return Err(KkpvError::DuplicateField {
                        type_key: "嵌套记录".into(),
                        field: key.into(),
                    });
                }
                let value = decode_nested(limits, depth + 1, bytes)?;
                entries.push((key.into(), value));
            }
            Ok(KkpvValue::Object(entries))
        }
        _ => Err(KkpvError::UnknownTag(tag)),
    }
}

impl<'a> KkpvView<'a> {
    /// 解码并建立块视图，执行模块文档列举的全部结构校验。
    ///
    /// # Errors
    /// 结构非法、限额超限或段关系不成立时返回错误，不产生部分视图。
    pub fn decode(data: &'a [u8], limits: &KkpvLimits) -> Result<Self, KkpvError> {
        if data.len() < HEADER_LEN {
            return Err(KkpvError::ByteLen {
                declared: 0,
                actual: data.len(),
            });
        }
        let mut magic = [0u8; 4];
        magic.copy_from_slice(&data[..4]);
        if magic != MAGIC {
            return Err(KkpvError::BadMagic(magic));
        }
        let version = u16::from_le_bytes([data[OFF_VERSION], data[OFF_VERSION + 1]]);
        if version != VERSION {
            return Err(KkpvError::UnsupportedVersion(version));
        }
        let flags = u16::from_le_bytes([data[OFF_FLAGS], data[OFF_FLAGS + 1]]);
        if flags & !(FLAG_PRIVATE | FLAG_DIRTY) != 0 {
            return Err(KkpvError::UnknownFlags(flags));
        }
        let declared_len = read_u32_at(data, OFF_BYTE_LEN);
        if declared_len as usize != data.len() {
            return Err(KkpvError::ByteLen {
                declared: declared_len,
                actual: data.len(),
            });
        }
        let header_len = read_u32_at(data, OFF_HEADER_LEN);
        if header_len as usize != HEADER_LEN {
            return Err(KkpvError::HeaderLen(header_len));
        }
        // 保留区 144..160 承载脏位段对与基准拍号：FLAG_DIRTY 未置位时
        // 整段保留区必须为零（与无脏位产物同规范形）；置位时基准拍号
        // 不得晚于本块拍号。
        if flags & FLAG_DIRTY == 0 {
            if data[OFF_DIRTY..HEADER_LEN].iter().any(|&b| b != 0) {
                return Err(KkpvError::ReservedNonZero);
            }
        } else if read_u64_at(data, OFF_DIRTY_BASE_TICK) > read_u64_at(data, OFF_TICK) {
            return Err(KkpvError::Malformed("脏位基准拍号晚于本块拍号".into()));
        }
        let object_count = read_u32_at(data, OFF_OBJECT_COUNT) as usize;
        let type_count = read_u32_at(data, OFF_TYPE_COUNT) as usize;
        if object_count > limits.max_objects {
            return Err(KkpvError::LimitExceeded("max_objects"));
        }
        if type_count > limits.max_types {
            return Err(KkpvError::LimitExceeded("max_types"));
        }
        let segment = |offset_pos: usize| {
            (
                read_u32_at(data, offset_pos) as usize,
                read_u32_at(data, offset_pos + 4) as usize,
            )
        };
        let (terrain_off, terrain_len) = segment(OFF_TERRAIN);
        let (directory_off, directory_len) = segment(OFF_DIRECTORY);
        let (buckets_off, buckets_len) = segment(OFF_BUCKETS);
        let (arena_off, arena_len) = segment(OFF_ARENA);
        let (dirty_off, dirty_len) = segment(OFF_DIRTY);
        if (flags & FLAG_DIRTY != 0) != (dirty_len > 0) {
            return Err(KkpvError::Malformed("脏位段与 FLAG_DIRTY 不一致".into()));
        }
        check_segments(
            data.len(),
            [
                ("terrain", terrain_off, terrain_len),
                ("directory", directory_off, directory_len),
                ("buckets", buckets_off, buckets_len),
                ("arena", arena_off, arena_len),
                ("dirty", dirty_off, dirty_len),
            ],
        )?;
        if arena_len == 0 {
            return Err(KkpvError::ArenaMissing);
        }
        if data.len() > limits.max_bytes {
            return Err(KkpvError::LimitExceeded("max_bytes"));
        }
        if arena_len > limits.max_arena_bytes {
            return Err(KkpvError::LimitExceeded("max_arena_bytes"));
        }
        if terrain_len > limits.max_terrain_bytes {
            return Err(KkpvError::LimitExceeded("max_terrain_bytes"));
        }
        let arena = &data[arena_off..arena_off + arena_len];
        let arena_str = |desc: (usize, usize)| -> Result<&'a str, KkpvError> {
            let end = desc.0.checked_add(desc.1).ok_or(KkpvError::ArenaBounds {
                offset: desc.0,
                length: desc.1,
            })?;
            if end > arena.len() || desc.1 > limits.max_string_bytes {
                return Err(KkpvError::ArenaBounds {
                    offset: desc.0,
                    length: desc.1,
                });
            }
            std::str::from_utf8(&arena[desc.0..end]).map_err(|_| KkpvError::InvalidUtf8)
        };
        let room_name = arena_str(segment(OFF_ROOM_NAME))?;
        if room_name.is_empty() {
            return Err(KkpvError::RoomNameMissing);
        }

        if directory_len != object_count * DIRECTORY_ENTRY {
            return Err(KkpvError::DirectoryLength {
                expected: object_count * DIRECTORY_ENTRY,
                actual: directory_len,
            });
        }
        let mut objects: Vec<DirectoryEntry> = Vec::with_capacity(object_count);
        for i in 0..object_count {
            let at = directory_off + i * DIRECTORY_ENTRY;
            let id = read_u64_at(data, at);
            let handle = read_u32_at(data, at + 8);
            let row = read_u32_at(data, at + 12);
            if let Some(last) = objects.last()
                && id <= last.id
            {
                return Err(if id == last.id {
                    KkpvError::DuplicateObjectId(id)
                } else {
                    KkpvError::DirectoryNotSorted
                });
            }
            if handle as usize >= type_count {
                return Err(KkpvError::UnknownTypeHandle(handle));
            }
            objects.push(DirectoryEntry { id, handle, row });
        }

        let mut walk = &data[buckets_off..buckets_off + buckets_len];
        let mut buckets: Vec<BucketView> = Vec::with_capacity(type_count);
        for _ in 0..type_count {
            let step = |walk: &mut &[u8], n: usize| -> Result<(), KkpvError> {
                if walk.len() < n {
                    return Err(KkpvError::BucketWalk {
                        consumed: buckets_len - walk.len(),
                        segment: buckets_len,
                    });
                }
                *walk = &walk[n..];
                Ok(())
            };
            if walk.len() < BUCKET_HEADER {
                return Err(KkpvError::BucketWalk {
                    consumed: buckets_len - walk.len(),
                    segment: buckets_len,
                });
            }
            let type_key =
                arena_str((read_u32_at(walk, 0) as usize, read_u32_at(walk, 4) as usize))?;
            let rows = read_u32_at(walk, 8);
            let field_count = read_u32_at(walk, 12) as usize;
            if field_count > limits.max_fields_per_type {
                return Err(KkpvError::LimitExceeded("max_fields_per_type"));
            }
            if rows as usize > limits.max_objects {
                return Err(KkpvError::LimitExceeded("max_objects"));
            }
            // 零行桶无合法产出方：编码端只按实际存在的对象分桶，行数恒
            // ≥1；位图尾位检查以 bitmap_len ≥ 1 为前提，零行会使其下溢。
            if rows == 0 {
                return Err(KkpvError::EmptyBucket {
                    type_key: type_key.into(),
                });
            }
            if let Some(last) = buckets.last()
                && type_key <= last.type_key
            {
                return Err(KkpvError::KeyOrder);
            }
            step(&mut walk, BUCKET_HEADER)?;
            if walk.len() < field_count * FIELD_ENTRY {
                return Err(KkpvError::BucketWalk {
                    consumed: buckets_len - walk.len(),
                    segment: buckets_len,
                });
            }
            let mut kinds = Vec::with_capacity(field_count);
            let mut keys = Vec::with_capacity(field_count);
            let mut last_key: Option<&str> = None;
            for i in 0..field_count {
                let at = i * FIELD_ENTRY;
                let key = arena_str((
                    read_u32_at(walk, at) as usize,
                    read_u32_at(walk, at + 4) as usize,
                ))?;
                if key == "id" {
                    return Err(KkpvError::DuplicateField {
                        type_key: type_key.into(),
                        field: "id".into(),
                    });
                }
                if last_key.is_some_and(|k| key <= k) {
                    return Err(KkpvError::KeyOrder);
                }
                last_key = Some(key);
                keys.push(key);
                let kind = walk[at + 8];
                if kind > KIND_OBJECT {
                    return Err(KkpvError::UnknownKind(kind));
                }
                if walk[at + 9..at + FIELD_ENTRY].iter().any(|&b| b != 0) {
                    return Err(KkpvError::ReservedNonZero);
                }
                kinds.push(kind);
            }
            step(&mut walk, field_count * FIELD_ENTRY)?;
            let bitmap_len = bitmap_bytes(rows);
            let bitmaps_needed = bitmap_len
                .checked_mul(field_count)
                .and_then(|v| v.checked_mul(2));
            let bitmaps_needed = match bitmaps_needed {
                Some(v) if v <= walk.len() => v,
                _ => {
                    return Err(KkpvError::BucketWalk {
                        consumed: buckets_len - walk.len(),
                        segment: buckets_len,
                    });
                }
            };
            let missing_all = &walk[..bitmap_len * field_count];
            let null_all = &walk[bitmap_len * field_count..bitmaps_needed];
            // 位图在全部已声明行上检查：越行尾位必须为零，
            // 缺失与 null 不得同置。
            let valid_bits: u8 = if rows.is_multiple_of(8) {
                0xFF
            } else {
                (1u8 << (rows % 8)) - 1
            };
            for i in 0..field_count {
                let missing = &missing_all[i * bitmap_len..(i + 1) * bitmap_len];
                let null = &null_all[i * bitmap_len..(i + 1) * bitmap_len];
                for (m, n) in missing.iter().zip(null.iter()) {
                    if m & n != 0 {
                        return Err(KkpvError::AmbiguousBitmap);
                    }
                }
                if missing[bitmap_len - 1] & !valid_bits != 0
                    || null[bitmap_len - 1] & !valid_bits != 0
                {
                    return Err(KkpvError::BitmapTailBits);
                }
            }
            step(&mut walk, bitmaps_needed)?;
            let mut total_width = 0usize;
            let mut widths = Vec::with_capacity(field_count);
            for &kind in &kinds {
                let width = column_width(kind).checked_mul(rows as usize);
                let width = match width {
                    Some(w) => w,
                    None => return Err(KkpvError::LimitExceeded("max_objects")),
                };
                widths.push(width);
                // 总宽与单列同口径 checked：回绕会使下方 walk 长度守卫失效。
                total_width = match total_width.checked_add(width) {
                    Some(v) => v,
                    None => return Err(KkpvError::LimitExceeded("max_objects")),
                };
            }
            if walk.len() < total_width {
                return Err(KkpvError::BucketWalk {
                    consumed: buckets_len - walk.len(),
                    segment: buckets_len,
                });
            }
            let mut fields = Vec::with_capacity(field_count);
            let mut column_data = walk;
            for (i, &kind) in kinds.iter().enumerate() {
                let width = widths[i];
                let column = &column_data[..width];
                column_data = &column_data[width..];
                // 定宽列的值域在建立视图时完成检查。
                if kind == KIND_BOOL
                    && let Some(&b) = column.iter().find(|&&b| b > 1)
                {
                    return Err(KkpvError::InvalidBool(b));
                }
                if kind == KIND_FLOAT {
                    for chunk in column.as_chunks::<8>().0 {
                        let v = f64::from_le_bytes(*chunk);
                        if !v.is_finite() {
                            return Err(KkpvError::NonFiniteFloat);
                        }
                    }
                }
                if is_variable(kind) {
                    for chunk in column.as_chunks::<DESCRIPTOR>().0 {
                        let offset = u32::from_le_bytes(chunk[..4].try_into().unwrap()) as usize;
                        let length = u32::from_le_bytes(chunk[4..].try_into().unwrap()) as usize;
                        let end = offset.checked_add(length);
                        if !matches!(end, Some(end) if end <= arena.len()) {
                            return Err(KkpvError::ArenaBounds { offset, length });
                        }
                    }
                }
                fields.push(FieldView {
                    key: keys[i],
                    kind,
                    missing: &missing_all[i * bitmap_len..(i + 1) * bitmap_len],
                    null: &null_all[i * bitmap_len..(i + 1) * bitmap_len],
                    column,
                });
            }
            step(&mut walk, total_width)?;
            buckets.push(BucketView {
                type_key,
                rows,
                fields,
            });
        }
        if !walk.is_empty() {
            return Err(KkpvError::BucketWalk {
                consumed: buckets_len - walk.len(),
                segment: buckets_len,
            });
        }
        // 脏位段内容：长度须精确等于逐桶位图之和（桶秩序拼接），每桶
        // 越行尾位为零——与缺失/null 位图同一规范形。
        let mut dirty_bitmaps = Vec::new();
        if dirty_len > 0 {
            let mut cursor = dirty_off;
            for bucket in &buckets {
                let bm = bitmap_bytes(bucket.rows);
                let slice = &data[cursor..cursor + bm];
                let valid_bits: u8 = if bucket.rows.is_multiple_of(8) {
                    0xFF
                } else {
                    (1u8 << (bucket.rows % 8)) - 1
                };
                if slice[bm - 1] & !valid_bits != 0 {
                    return Err(KkpvError::BitmapTailBits);
                }
                dirty_bitmaps.push(slice);
                cursor += bm;
            }
            if cursor != dirty_off + dirty_len {
                return Err(KkpvError::Malformed("脏位段长度与桶行数不符".into()));
            }
        }
        // 行占用不变式：每桶的每行恰被一个目录项引用。重复引用使
        // 一行数据以多个 ID 显形，未引用行是幻影数据，两者都拒绝。
        let mut occupied: Vec<Vec<bool>> = buckets
            .iter()
            .map(|b| vec![false; b.rows as usize])
            .collect();
        for entry in &objects {
            let bucket = &buckets[entry.handle as usize];
            if entry.row >= bucket.rows {
                return Err(KkpvError::RowOutOfRange {
                    handle: entry.handle,
                    row: entry.row,
                    rows: bucket.rows,
                });
            }
            let slot = &mut occupied[entry.handle as usize][entry.row as usize];
            if *slot {
                return Err(KkpvError::DuplicateRowRef {
                    handle: entry.handle,
                    row: entry.row,
                });
            }
            *slot = true;
        }
        for (handle, rows) in occupied.iter().enumerate() {
            if let Some(row) = rows.iter().position(|free| !free) {
                return Err(KkpvError::UnreferencedRow {
                    handle: handle as u32,
                    row: row as u32,
                });
            }
        }
        let mut meta_plan = [0u8; 32];
        meta_plan.copy_from_slice(&data[OFF_PLAN_HASH..OFF_PLAN_HASH + 32]);
        let mut meta_audience = [0u8; 32];
        meta_audience.copy_from_slice(&data[OFF_AUDIENCE_HASH..OFF_AUDIENCE_HASH + 32]);
        Ok(Self {
            data,
            tick: read_u64_at(data, OFF_TICK),
            generation: read_u64_at(data, OFF_GENERATION),
            plan_hash: meta_plan,
            audience_hash: meta_audience,
            flags,
            room_name,
            terrain: (terrain_len > 0).then(|| &data[terrain_off..terrain_off + terrain_len]),
            arena,
            limits: limits.clone(),
            objects,
            buckets,
            dirty_bitmaps,
        })
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    pub fn plan_hash(&self) -> [u8; 32] {
        self.plan_hash
    }

    pub fn audience_hash(&self) -> [u8; 32] {
        self.audience_hash
    }

    pub fn is_private(&self) -> bool {
        self.flags & FLAG_PRIVATE != 0
    }

    pub fn room_name(&self) -> &str {
        self.room_name
    }

    pub fn terrain(&self) -> Option<&[u8]> {
        self.terrain
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub fn object_id(&self, index: usize) -> u64 {
        self.objects[index].id
    }

    /// 对象的类型稳定键。
    ///
    /// # Panics
    /// index 越界时 panic；调用方先以 [`Self::object_count`] 约束。
    pub fn object_type(&self, index: usize) -> &str {
        self.buckets[self.objects[index].handle as usize].type_key
    }

    /// 按 ID 定位对象；二分依赖目录升序不变式。
    pub fn object_index_by_id(&self, id: u64) -> Option<usize> {
        self.objects.binary_search_by_key(&id, |e| e.id).ok()
    }

    /// 读取对象的字段值。外层 None 表示字段缺失（未声明于该行），
    /// `Some(KkpvValue::Null)` 表示显式 null。
    ///
    /// # Errors
    /// 变长值解码失败（UTF-8、标签、深度、限额）时返回错误。
    ///
    /// # Panics
    /// index 越界时 panic。
    pub fn field(&self, index: usize, key: &str) -> Result<Option<KkpvValue>, KkpvError> {
        let entry = &self.objects[index];
        let bucket = &self.buckets[entry.handle as usize];
        let Ok(position) = bucket.fields.binary_search_by(|f| f.key.cmp(key)) else {
            return Ok(None);
        };
        let field = &bucket.fields[position];
        if bit_set(field.missing, entry.row) {
            return Ok(None);
        }
        if bit_set(field.null, entry.row) {
            return Ok(Some(KkpvValue::Null));
        }
        let width = column_width(field.kind);
        let cell = &field.column[entry.row as usize * width..][..width];
        let value = match field.kind {
            KIND_BOOL => KkpvValue::Bool(cell[0] == 1),
            KIND_INT => KkpvValue::Int(i64::from_le_bytes(cell.try_into().unwrap())),
            KIND_UINT => KkpvValue::Uint(u64::from_le_bytes(cell.try_into().unwrap())),
            KIND_FLOAT => {
                let v = f64::from_le_bytes(cell.try_into().unwrap());
                if !v.is_finite() {
                    return Err(KkpvError::NonFiniteFloat);
                }
                KkpvValue::Float(v)
            }
            KIND_REF => KkpvValue::Ref(u64::from_le_bytes(cell.try_into().unwrap())),
            _ => {
                let offset = u32::from_le_bytes(cell[..4].try_into().unwrap()) as usize;
                let length = u32::from_le_bytes(cell[4..].try_into().unwrap()) as usize;
                let end = offset.checked_add(length);
                let end = match end {
                    Some(end) if end <= self.arena.len() => end,
                    _ => return Err(KkpvError::ArenaBounds { offset, length }),
                };
                let mut payload = &self.arena[offset..end];
                let value = decode_nested(&self.limits, 0, &mut payload)?;
                if !payload.is_empty() {
                    return Err(KkpvError::ArenaBounds { offset, length });
                }
                value
            }
        };
        Ok(Some(value))
    }

    /// 列出对象上已声明且未缺失的字段键；不含由目录承担的 `id`。
    ///
    /// # Panics
    /// index 越界时 panic。
    pub fn field_keys(&self, index: usize) -> Vec<&str> {
        let entry = &self.objects[index];
        let bucket = &self.buckets[entry.handle as usize];
        bucket
            .fields
            .iter()
            .filter(|field| !bit_set(field.missing, entry.row))
            .map(|field| field.key)
            .collect()
    }

    /// 对象的行脏位（编码端逐格比对结论；置位=须按全量更新处理）。
    /// 块未携带脏位段时返回 None。测试与语义锁的主要消费面。
    ///
    /// # Panics
    /// index 越界时 panic；调用方先以 [`Self::object_count`] 约束。
    pub fn row_dirty(&self, index: usize) -> Option<bool> {
        if self.dirty_bitmaps.is_empty() {
            return None;
        }
        let entry = &self.objects[index];
        let bitmap = self.dirty_bitmaps[entry.handle as usize];
        Some(bit_set(bitmap, entry.row))
    }

    /// 原始块字节。
    pub fn as_bytes(&self) -> &'a [u8] {
        self.data
    }
}
