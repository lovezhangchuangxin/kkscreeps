//! 编码单块与块束，并校验束的版本一致性。
//!
//! 编码核心只有一套：[`encode_ref`] 直接消费借用行（字段键与字符串值
//! 借用自权威对象，热路径不构建 owned `KkpvObject` 树）；[`encode`]
//! 把 owned 输入桥接为借用形态后走同一核心，两种入口产出逐字节一致。
use std::borrow::Cow;

use super::*;

/// 将 owned 输入编码为完整的 KKPV 块：桥接为 [`KkpvBlockRef`] 后走
/// [`encode_ref`] 单一核心（布局、校验与错误形态同源）。
///
/// # Errors
/// 值域或限额违规、对象 ID 或字段键重复、同键值编码不一致时返回错误，
/// 不产出部分数据。
pub fn encode(input: &KkpvBlockInput, limits: &KkpvLimits) -> Result<Vec<u8>, KkpvError> {
    let rows = input
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
        .collect();
    let block = KkpvBlockRef {
        private: input.private,
        tick: input.tick,
        generation: input.generation,
        plan_hash: input.plan_hash,
        audience_hash: input.audience_hash,
        room_name: input.room_name.as_str(),
        terrain: input.terrain.as_deref(),
        rows,
    };
    encode_ref(&block, limits)
}

/// 单字段的编码计划：列是全调用共享 `columns_all` 缓冲里的区间
/// （`col_base..col_base+rows`，行主序），变长字段的 arena 单元则是
/// `cells_all` 平铺缓冲的区间（`cells_base..+rows`；usize::MAX 表示
/// 定宽字段没有单元）。
pub(super) struct FieldPlan<'o> {
    pub(super) key: &'o str,
    key_desc: (u32, u32),
    pub(super) kind: u8,
    pub(super) col_base: usize,
    pub(super) rows: u32,
    pub(super) cells_base: usize,
}
pub(super) struct BucketPlan<'o> {
    pub(super) rows: u32,
    pub(super) fields: Vec<FieldPlan<'o>>,
}

/// FNV-1a 字符串哈希：编码热路径的键短且高度重复，符号表只用于
/// 把重复字符串折叠成下标，碰撞只多一次等值比较、不改结果；相比
/// 默认 SipHash（防碰撞哈希 DoS 的通用选择，此处输入是自家权威数据）
/// 短键开销低数倍。
fn fnv_str(s: &str) -> u64 {
    // FNV-1a 乘法按设计溢出回绕（哈希本就取低 64 位）。
    s.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
    })
}

/// 单次 [`encode_ref`] 调用内的符号表：开放寻址线性探测，把类型键与
/// 字段键折叠为 `names` 下标，使分桶/并集/查重/秩映射全部退化为整数
/// 比较。键借用输入行，生命周期与调用同界，不跨块复用。
struct SymTab<'k> {
    mask: usize,
    /// u32::MAX 为空槽，否则是 names 下标。
    slots: Vec<u32>,
    names: Vec<&'k str>,
}

impl<'k> SymTab<'k> {
    fn new() -> Self {
        Self {
            mask: 127,
            slots: vec![u32::MAX; 128],
            names: Vec::new(),
        }
    }

    fn intern(&mut self, key: &'k str) -> u32 {
        let mut at = fnv_str(key) as usize & self.mask;
        loop {
            match self.slots[at] {
                u32::MAX => {
                    let sym = self.names.len() as u32;
                    self.names.push(key);
                    self.slots[at] = sym;
                    // 负载过半即扩容（翻倍重插），探测保持 O(1)。
                    if self.names.len() * 2 > self.slots.len() {
                        self.rehash();
                    }
                    return sym;
                }
                sym if self.names[sym as usize] == key => return sym,
                _ => at = (at + 1) & self.mask,
            }
        }
    }

    fn rehash(&mut self) {
        let new_len = self.slots.len() * 2;
        self.slots = vec![u32::MAX; new_len];
        self.mask = new_len - 1;
        for (sym, &name) in self.names.iter().enumerate() {
            let mut at = fnv_str(name) as usize & self.mask;
            while self.slots[at] != u32::MAX {
                at = (at + 1) & self.mask;
            }
            self.slots[at] = sym as u32;
        }
    }

    fn name(&self, sym: u32) -> &'k str {
        self.names[sym as usize]
    }
}

/// 符号分桶：收集期按首现序，随后按名称排序（对齐 BTreeMap 迭代序）。
struct SymBucket {
    type_sym: u32,
    members: Vec<usize>,
}

/// 将借用行编码为完整的 KKPV 块。对象按 ID 升序、类型桶与字段按键
/// 排序后写入；嵌套记录保持条目顺序（玩家可见的键序不因编码重排），
/// 相同输入得到相同字节。与 [`encode`] 的产物逐字节一致。
///
/// 相比逐字段 `find_map` 重扫（每对象 O(字段数²) 字符串比较），字段
/// 先并入秩映射再行主填充矩阵，每对象 O(字段数·log 字段数)。
///
/// # Errors
/// 值域或限额违规、对象 ID 或字段键重复、同键值编码不一致时返回错误，
/// 不产出部分数据。
pub fn encode_ref(input: &KkpvBlockRef<'_>, limits: &KkpvLimits) -> Result<Vec<u8>, KkpvError> {
    encode_impl(input, limits, None, false).map(|(bytes, _)| bytes)
}

/// [`encode_ref`] 的行级脏位版：与上一拍保留块逐格比对（见 [`super::dirty`]），
/// 逐格等值的行写入块内脏位段供解码端跳过就地更新；同时返回本拍的
/// 结构索引供下一拍保留。`prev` 传索引缺失的块（纯编码产物）等同无
/// 基准——只建索引不产脏段。首次编码（prev 为 None）同样只建索引，
/// 从下一拍起产生脏位。
///
/// # Errors
/// 与 [`encode_ref`] 同源。
pub fn encode_with_dirty(
    input: &KkpvBlockRef<'_>,
    limits: &KkpvLimits,
    prev: Option<&PrevEncodedBlock>,
) -> Result<PrevEncodedBlock, KkpvError> {
    let (bytes, index) = encode_impl(input, limits, prev, true)?;
    let index = index.expect("want_index 时总是构建索引");
    Ok(PrevEncodedBlock::new(bytes, index))
}

fn encode_impl(
    input: &KkpvBlockRef<'_>,
    limits: &KkpvLimits,
    prev: Option<&PrevEncodedBlock>,
    want_index: bool,
) -> Result<(Vec<u8>, Option<dirty::PrevBlockIndex>), KkpvError> {
    if input.room_name.is_empty() {
        return Err(KkpvError::RoomNameMissing);
    }
    if input.room_name.len() > limits.max_string_bytes {
        return Err(KkpvError::LimitExceeded("max_string_bytes"));
    }
    if input.rows.len() > limits.max_objects {
        return Err(KkpvError::LimitExceeded("max_objects"));
    }
    let terrain = input.terrain;
    if terrain.is_some_and(|t| t.len() > limits.max_terrain_bytes) {
        return Err(KkpvError::LimitExceeded("max_terrain_bytes"));
    }

    // 目录按 ID 升序；桶内行序沿用该次序。
    let mut order: Vec<usize> = (0..input.rows.len()).collect();
    order.sort_by_key(|&i| input.rows[i].id);
    for pair in order.iter().zip(order.iter().skip(1)) {
        if input.rows[*pair.0].id == input.rows[*pair.1].id {
            return Err(KkpvError::DuplicateObjectId(input.rows[*pair.0].id));
        }
    }

    // 类型分桶：符号化后按名称排序（对齐 BTreeMap 迭代序）。成熟负载
    // 每拍数百块 × 每块数百对象，逐对象一次 intern 取代 BTreeMap 的
    // 逐对象字符串比较树。
    let mut syms = SymTab::new();
    let id_field_sym = syms.intern("id");
    let mut buckets: Vec<SymBucket> = Vec::new();
    for &index in &order {
        let type_sym = syms.intern(input.rows[index].type_key.as_ref());
        match buckets.iter_mut().find(|b| b.type_sym == type_sym) {
            Some(bucket) => bucket.members.push(index),
            None => buckets.push(SymBucket {
                type_sym,
                members: vec![index],
            }),
        }
    }
    if buckets.len() > limits.max_types {
        return Err(KkpvError::LimitExceeded("max_types"));
    }
    buckets.sort_by(|a, b| syms.name(a.type_sym).cmp(syms.name(b.type_sym)));

    let mut arena = ArenaBuilder { bytes: Vec::new() };
    let room_desc = arena.push_str(input.room_name)?;
    let mut type_descs = Vec::with_capacity(buckets.len());
    let mut plans = Vec::with_capacity(buckets.len());
    // 全调用单一列缓冲（columns_all，秩主序、stride=本桶行数；字段经
    // col_base 区间引用，避免桶间切片失效）与秩/查重表，替代逐桶逐列
    // `vec![None; n]` 的一拍数百次分配与释放。
    let mut columns_all: Vec<Option<&KkpvVal<'_>>> = Vec::new();
    let mut bucket_syms: Vec<u32> = Vec::new();
    // 行内查重（stamp=行号+1，0 为未见）与并集标记（rank_of≥0 即已入集）。
    let mut rank_of: Vec<i64> = Vec::new();
    let mut seen_row: Vec<u32> = Vec::new();
    for bucket in &buckets {
        let members = &bucket.members;
        let type_key = syms.name(bucket.type_sym);
        if members.len() > u32::MAX as usize {
            return Err(KkpvError::LimitExceeded("max_objects"));
        }
        if type_key.len() > limits.max_string_bytes {
            return Err(KkpvError::LimitExceeded("max_string_bytes"));
        }
        type_descs.push(arena.push_str(type_key)?);
        // 字段目录是全桶键并集（按名称排序）；重复只指同一对象内出现
        // 两次同名键。行内查重走符号戳（同拍同 stamp 即重复），错误
        // 检测顺序与逐键 BTreeSet 版一致（行序、键序遍历）。
        bucket_syms.clear();
        for (row_no, &index) in members.iter().enumerate() {
            let stamp = (row_no + 1) as u32;
            for (key, _) in &input.rows[index].fields {
                let sym = syms.intern(key);
                if sym == id_field_sym {
                    return Err(KkpvError::DuplicateField {
                        type_key: type_key.into(),
                        field: "id".into(),
                    });
                }
                if key.len() > limits.max_string_bytes {
                    return Err(KkpvError::LimitExceeded("max_string_bytes"));
                }
                if seen_row.len() <= sym as usize {
                    seen_row.resize(sym as usize + 1, 0);
                    rank_of.resize(sym as usize + 1, -1);
                }
                if seen_row[sym as usize] == stamp {
                    return Err(KkpvError::DuplicateField {
                        type_key: type_key.into(),
                        field: (*key).into(),
                    });
                }
                seen_row[sym as usize] = stamp;
                if rank_of[sym as usize] < 0 {
                    rank_of[sym as usize] = bucket_syms.len() as i64;
                    bucket_syms.push(sym);
                }
            }
        }
        if bucket_syms.len() > limits.max_fields_per_type {
            return Err(KkpvError::LimitExceeded("max_fields_per_type"));
        }
        // 并集按名称排序定秩（= BTreeSet 迭代序），随后行主序填充列。
        bucket_syms.sort_by(|&a, &b| syms.name(a).cmp(syms.name(b)));
        let rows = members.len();
        let col_base = columns_all.len();
        columns_all.resize(col_base + bucket_syms.len() * rows, None);
        for (rank, &sym) in bucket_syms.iter().enumerate() {
            rank_of[sym as usize] = rank as i64;
        }
        for (row_no, &index) in members.iter().enumerate() {
            for (key, value) in &input.rows[index].fields {
                let sym = syms.intern(key);
                let rank = rank_of[sym as usize] as usize;
                columns_all[col_base + rank * rows + row_no] = Some(value);
            }
        }
        let mut fields = Vec::with_capacity(bucket_syms.len());
        for (rank, &sym) in bucket_syms.iter().enumerate() {
            let key = syms.name(sym);
            let key_desc = arena.push_str(key)?;
            fields.push(FieldPlan {
                key,
                key_desc,
                kind: KIND_BOOL,
                col_base: col_base + rank * rows,
                rows: rows as u32,
                cells_base: usize::MAX,
            });
        }
        // 桶结束：整表复位查重戳与秩（表长=符号数，量级几十），供下桶复用。
        for stamp in seen_row.iter_mut() {
            *stamp = 0;
        }
        for rank in rank_of.iter_mut() {
            *rank = -1;
        }
        // 列类型按键序逐列推断，与逐字段构建版的错误报告顺序一致。
        for field in &mut fields {
            let mut kind: Option<u8> = None;
            let column = &columns_all[field.col_base..field.col_base + field.rows as usize];
            for value in column.iter().flatten() {
                if let Some(value_kind) = kind_of_val(value) {
                    match kind {
                        None => kind = Some(value_kind),
                        Some(existing) if existing != value_kind => {
                            return Err(KkpvError::KindMismatch {
                                type_key: (*type_key).into(),
                                field: field.key.to_string(),
                            });
                        }
                        _ => {}
                    }
                }
            }
            // 全 null 字段没有可推断的列类型；bool 宽度最小，位图已表达全部信息。
            field.kind = kind.unwrap_or(KIND_BOOL);
        }
        plans.push(BucketPlan {
            rows: members.len() as u32,
            fields,
        });
    }
    // 行 → 桶秩（目录写入的整数查表，取代 BTreeMap row_counters 的
    // 逐对象字符串查找）；桶内行号按成员序（ID 升序）在写入时累加。
    let mut row_bucket = vec![0u32; input.rows.len()];
    for (rank, bucket) in buckets.iter().enumerate() {
        for &member in &bucket.members {
            row_bucket[member] = rank as u32;
        }
    }

    // 变长载荷在结构段之前进入 arena；单元写入全调用平铺缓冲
    // `cells_all`（字段经 cells_base 区间引用），位图缓冲整调用复用。
    let mut cells_all: Vec<(u32, u32)> = Vec::new();
    // 嵌套查重 scratch：整调用一份，栈式 base/truncate 复用。
    let mut nested_seen: Vec<&str> = Vec::new();
    for plan in &mut plans {
        for field in &mut plan.fields {
            if !is_variable(field.kind) {
                continue;
            }
            field.cells_base = cells_all.len();
            let column = &columns_all[field.col_base..field.col_base + field.rows as usize];
            for value in column {
                match value {
                    None | Some(KkpvVal::Null) => cells_all.push((0u32, 0u32)),
                    Some(value) => {
                        let start = arena.bytes.len();
                        encode_nested_val(value, limits, 0, &mut arena.bytes, &mut nested_seen)?;
                        let end = arena.bytes.len();
                        let fits = |v: usize| {
                            u32::try_from(v)
                                .map_err(|_| KkpvError::LimitExceeded("max_arena_bytes"))
                        };
                        cells_all.push((fits(start)?, fits(end - start)?));
                    }
                }
            }
        }
    }
    if arena.bytes.len() > limits.max_arena_bytes {
        return Err(KkpvError::LimitExceeded("max_arena_bytes"));
    }

    // 行级脏位：与上一拍同源保留块逐格比对（无基准或基准无索引则不
    // 产脏段，解码端按全量更新处理，语义等同全脏）。基准拍号取保留
    // 块拍号，由解码端校验自身衔接；保留不按世代过滤——跨代际保留
    // 的内容比对仍保守正确，基准拍号不衔接时解码端自动回退全量。
    let type_keys: Vec<&str> = buckets.iter().map(|b| syms.name(b.type_sym)).collect();
    let (dirty_maps, dirty_base_tick) = match prev.and_then(PrevEncodedBlock::parts) {
        Some((index, bytes)) => (
            dirty::compute(
                index,
                bytes,
                input,
                &plans,
                &type_keys,
                &columns_all,
                &cells_all,
                &arena.bytes,
                &order,
                &row_bucket,
            ),
            Some(index.tick),
        ),
        _ => (Vec::new(), None),
    };
    let dirty_len: usize = dirty_maps.iter().map(Vec::len).sum();

    let directory_len = order.len() * DIRECTORY_ENTRY;
    let mut bucket_bytes = Vec::new();
    let mut bitmap: Vec<u8> = Vec::new();
    for (i, plan) in plans.iter().enumerate() {
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
            bitmap.clear();
            bitmap.resize(bitmap_len, 0);
            for (row, value) in columns_all[field.col_base..field.col_base + field.rows as usize]
                .iter()
                .enumerate()
            {
                if value.is_none() {
                    bitmap[row / 8] |= 1 << (row % 8);
                }
            }
            bucket_bytes.extend_from_slice(&bitmap);
        }
        for field in &plan.fields {
            bitmap.clear();
            bitmap.resize(bitmap_len, 0);
            for (row, value) in columns_all[field.col_base..field.col_base + field.rows as usize]
                .iter()
                .enumerate()
            {
                if matches!(value, Some(KkpvVal::Null)) {
                    bitmap[row / 8] |= 1 << (row % 8);
                }
            }
            bucket_bytes.extend_from_slice(&bitmap);
        }
        for field in &plan.fields {
            let width = column_width(field.kind);
            let rows = field.rows as usize;
            let column = &columns_all[field.col_base..field.col_base + rows];
            let cells = field.cells_base;
            for (row, value) in column.iter().enumerate() {
                match value {
                    None | Some(KkpvVal::Null) => {
                        bucket_bytes.extend(std::iter::repeat_n(0u8, width));
                    }
                    Some(value) => {
                        if is_variable(field.kind) {
                            let cell = cells_all[cells + row];
                            bucket_bytes.extend_from_slice(&cell.0.to_le_bytes());
                            bucket_bytes.extend_from_slice(&cell.1.to_le_bytes());
                        } else {
                            match value {
                                KkpvVal::Bool(b) => bucket_bytes.push(*b as u8),
                                KkpvVal::Int(v) => {
                                    bucket_bytes.extend_from_slice(&v.to_le_bytes());
                                }
                                KkpvVal::Uint(v) => {
                                    bucket_bytes.extend_from_slice(&v.to_le_bytes());
                                }
                                KkpvVal::Float(v) => {
                                    if !v.is_finite() {
                                        return Err(KkpvError::NonFiniteFloat);
                                    }
                                    bucket_bytes.extend_from_slice(&v.to_le_bytes());
                                }
                                KkpvVal::Ref(v) => {
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
    let total = HEADER_LEN
        + terrain_len
        + directory_len
        + bucket_bytes.len()
        + arena.bytes.len()
        + dirty_len;
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
    let dirty_off = arena_off + arena.bytes.len();

    let mut out = vec![0u8; total];
    out[..4].copy_from_slice(&MAGIC);
    out[OFF_VERSION..OFF_VERSION + 2].copy_from_slice(&VERSION.to_le_bytes());
    let dirty_present = dirty_len > 0;
    let mut flags = u16::from(input.private) * FLAG_PRIVATE;
    if dirty_present {
        flags |= FLAG_DIRTY;
    }
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
    out[OFF_OBJECT_COUNT..OFF_OBJECT_COUNT + 4].copy_from_slice(&fits(order.len())?.to_le_bytes());
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
    // 脏位段（第五段，桶秩序拼接的逐桶行位图）与基准拍号只在
    // FLAG_DIRTY 置位时携带；其余情形保留区保持零，与无脏位产物一致。
    let (dirty_o, dirty_l) = seg_pair(dirty_off, dirty_len)?;
    out[OFF_DIRTY..OFF_DIRTY + 4].copy_from_slice(&dirty_o.to_le_bytes());
    out[OFF_DIRTY + 4..OFF_DIRTY + 8].copy_from_slice(&dirty_l.to_le_bytes());
    if dirty_present && let Some(base) = dirty_base_tick {
        out[OFF_DIRTY_BASE_TICK..OFF_DIRTY_BASE_TICK + 8].copy_from_slice(&base.to_le_bytes());
    }
    // 目录按对象（已按 ID 升序）写入；句柄是桶在类型键序中的秩（行→桶
    // 整数查表），行号是对象在其桶内按 ID 的序（逐桶计数器累加）。
    let mut row_counters = vec![0u32; buckets.len()];
    for (i, &index) in order.iter().enumerate() {
        let object = &input.rows[index];
        let bucket_rank = row_bucket[index] as usize;
        let at = directory_off + i * DIRECTORY_ENTRY;
        out[at..at + 8].copy_from_slice(&object.id.to_le_bytes());
        out[at + 8..at + 12].copy_from_slice(&(bucket_rank as u32).to_le_bytes());
        out[at + 12..at + 16].copy_from_slice(&row_counters[bucket_rank].to_le_bytes());
        row_counters[bucket_rank] += 1;
    }
    out[buckets_off..buckets_off + bucket_bytes.len()].copy_from_slice(&bucket_bytes);
    out[arena_off..arena_off + arena.bytes.len()].copy_from_slice(&arena.bytes);
    let mut dirty_cursor = dirty_off;
    for bitmap in &dirty_maps {
        out[dirty_cursor..dirty_cursor + bitmap.len()].copy_from_slice(bitmap);
        dirty_cursor += bitmap.len();
    }
    let index = want_index.then(|| {
        dirty::build_index(
            input.tick,
            input,
            &plans,
            &type_keys,
            &order,
            &row_bucket,
            buckets_off,
            arena_off,
        )
    });
    Ok((out, index))
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
