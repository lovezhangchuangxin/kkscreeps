//! 跨拍保留的编码结构索引与行级脏位比对。
//!
//! 服务于解码端"干净行跳过"：编码端把上一拍同源块（同房、同受众
//! 口径）的结构索引连同字节一并保留（[`PrevEncodedBlock`]），本拍
//! 编码时逐格比对当前值与上一拍字节，逐格等值的行在桶脏位图中保持
//! 清零；解码端对干净行只重指解码上下文、跳过全字段就地更新。
//!
//! 比对按值级进行且保守：桶按 type_key 配对、字段按键与列类型配对、
//! 行按目录 ID 双指针对位（行号漂移不影响）；任何不确定都判脏——
//! 判脏只损失跳过机会，语义上等同全量更新。基准拍号由编码端随脏段
//! 写入块头，解码端自行校验新旧衔接（见模块级"行级脏位"说明），
//! 因此保留块过期（换代、跳拍后复用）至多退化为全脏，不产生错误。
//!
//! 索引在编码时顺带构建（桶头/字段项/双位图区/列数据的偏移算术与
//! 写入端、解码端同源），保留侧不需要二次解析块字节。
use std::sync::Arc;

use super::encode::{BucketPlan, FieldPlan};
use super::*;

/// 上一拍块的字段索引：键与列类型，缺失/null 位图和列数据（定宽值
/// 或变长 cells 数组）在块内的绝对偏移。
pub(crate) struct PrevFieldIndex {
    key: String,
    kind: u8,
    missing_at: u32,
    null_at: u32,
    col_at: u32,
}

/// 上一拍块的桶索引：type_key、行数与字段索引（按键升序，与写入序
/// 一致）。
pub(crate) struct PrevBucketIndex {
    type_key: String,
    fields: Vec<PrevFieldIndex>,
}

/// 上一拍块的结构索引：基准拍号＋全局目录（ID 升序）＋桶布局偏移。
/// 偏移是块内绝对偏移，arena 内容经 `arena_off` 从块字节读取。保留
/// 不校验世代：同源保留的字节即解码端上一拍所收字节（共享缓存同一
/// Arc），基准拍号由解码端校验，跨代际保留至多退化为全脏。
pub(crate) struct PrevBlockIndex {
    pub(super) tick: u64,
    arena_off: u32,
    dir: Vec<(u64, u32, u32)>,
    buckets: Vec<PrevBucketIndex>,
}

/// 跨拍保留的完整编码块：字节与结构索引。`index` 为 None 表示该块
/// 走纯编码路径（[`super::encode_ref`]）产出、未建索引，不能作为
/// 脏位基准；调用方只应把建过索引的块传回 [`super::encode_with_dirty`]。
pub struct PrevEncodedBlock {
    bytes: Arc<Vec<u8>>,
    index: Option<PrevBlockIndex>,
}

impl std::fmt::Debug for PrevEncodedBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "PrevEncodedBlock({} bytes, {})",
            self.bytes.len(),
            if self.index.is_some() {
                "indexed"
            } else {
                "plain"
            }
        )
    }
}

impl PrevEncodedBlock {
    /// 块字节。
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// 取回字节所有权；保留的唯一引用直接移交，多引用时退化为拷贝。
    pub fn into_bytes(self) -> Vec<u8> {
        Arc::try_unwrap(self.bytes).unwrap_or_else(|shared| (*shared).clone())
    }

    /// 纯编码路径的包装：无索引，不可作脏位基准。
    pub fn from_plain(bytes: Vec<u8>) -> Self {
        Self {
            bytes: Arc::new(bytes),
            index: None,
        }
    }

    pub(super) fn new(bytes: Vec<u8>, index: PrevBlockIndex) -> Self {
        Self {
            bytes: Arc::new(bytes),
            index: Some(index),
        }
    }

    pub(super) fn parts(&self) -> Option<(&PrevBlockIndex, &[u8])> {
        self.index
            .as_ref()
            .map(|index| (index, self.bytes.as_slice()))
    }
}

/// 构建刚编码块的结构索引。偏移算术与写入端逐段推进一致：桶头 16 +
/// 字段项 12×n + 缺失位图 n×bmLen + null 位图 n×bmLen + 逐字段列
/// 数据（定宽 width×rows，变长 cells 8×rows）。目录取写入序（ID
/// 升序，行号按桶内计数器累加）。
#[allow(clippy::too_many_arguments)]
pub(super) fn build_index(
    tick: u64,
    input: &KkpvBlockRef<'_>,
    plans: &[BucketPlan<'_>],
    type_keys: &[&str],
    order: &[usize],
    row_bucket: &[u32],
    buckets_off: usize,
    arena_off: usize,
) -> PrevBlockIndex {
    let mut dir = Vec::with_capacity(order.len());
    let mut counters = vec![0u32; plans.len()];
    for &index in order {
        let rank = row_bucket[index] as usize;
        let row = counters[rank];
        counters[rank] += 1;
        dir.push((input.rows[index].id, rank as u32, row));
    }
    let mut buckets = Vec::with_capacity(plans.len());
    // 桶布局偏移随桶序累计推进（与写入端逐桶 append 同源），不能每桶
    // 从 buckets_off 重算。
    let mut bucket_cursor = buckets_off;
    for (rank, plan) in plans.iter().enumerate() {
        let bm_len = bitmap_bytes(plan.rows);
        let field_count = plan.fields.len();
        let missing_base = bucket_cursor + BUCKET_HEADER + field_count * FIELD_ENTRY;
        let null_base = missing_base + field_count * bm_len;
        let mut col_cursor = null_base + field_count * bm_len;
        let mut fields = Vec::with_capacity(field_count);
        for field in &plan.fields {
            let col_at = col_cursor;
            col_cursor += column_width(field.kind) * plan.rows as usize;
            fields.push(PrevFieldIndex {
                key: field.key.to_string(),
                kind: field.kind,
                missing_at: (missing_base + fields.len() * bm_len) as u32,
                null_at: (null_base + fields.len() * bm_len) as u32,
                col_at: col_at as u32,
            });
        }
        buckets.push(PrevBucketIndex {
            type_key: type_keys[rank].to_string(),
            fields,
        });
        bucket_cursor = col_cursor;
    }
    PrevBlockIndex {
        tick,
        arena_off: arena_off as u32,
        dir,
        buckets,
    }
}

/// 逐格比对当前编码值与上一拍块字节，返回逐桶行脏位图（脏行置位；
/// 无配对桶的桶全脏）。正确性不依赖两拍相邻：比对以字节内容为准，
/// 基准拍号交给解码端衔接校验。
#[allow(clippy::too_many_arguments)]
pub(super) fn compute<'o>(
    prev: &PrevBlockIndex,
    prev_bytes: &[u8],
    input: &KkpvBlockRef<'o>,
    plans: &[BucketPlan<'o>],
    type_keys: &[&str],
    columns_all: &[Option<&KkpvVal<'o>>],
    cells_all: &[(u32, u32)],
    arena: &[u8],
    order: &[usize],
    row_bucket: &[u32],
) -> Vec<Vec<u8>> {
    let mut bitmaps = plans
        .iter()
        .map(|plan| vec![0u8; bitmap_bytes(plan.rows)])
        .collect::<Vec<_>>();
    // 逐桶配对上一拍桶（type_key 相同）并预配对字段（键与列类型均
    // 同才可比较）；配对只做一次，行循环内全部走整数偏移。
    let mut prev_bucket_of: Vec<Option<usize>> = Vec::with_capacity(plans.len());
    let mut field_pairs: Vec<Vec<Option<usize>>> = Vec::with_capacity(plans.len());
    let mut prev_only_fields: Vec<Vec<usize>> = Vec::with_capacity(plans.len());
    for (rank, plan) in plans.iter().enumerate() {
        let matched = prev
            .buckets
            .iter()
            .position(|bucket| bucket.type_key == type_keys[rank]);
        let (pairs, prev_only) = match matched {
            Some(p) => pair_fields(plan, &prev.buckets[p]),
            None => (Vec::new(), Vec::new()),
        };
        prev_bucket_of.push(matched);
        field_pairs.push(pairs);
        prev_only_fields.push(prev_only);
    }
    // 全局双指针按 ID 对位（两侧目录均 ID 升序）；行号用桶内计数器
    // 与写入端同序累加。
    let mut pi = 0usize;
    let mut counters = vec![0u32; plans.len()];
    for &index in order {
        let id = input.rows[index].id;
        while pi < prev.dir.len() && prev.dir[pi].0 < id {
            pi += 1;
        }
        let rank = row_bucket[index] as usize;
        let row = counters[rank];
        counters[rank] += 1;
        // 判定起点是脏：ID 未对位、桶无配对或对象换桶（上一拍句柄不
        // 指向本桶的配对桶）都保持脏，只有逐格等值才清。
        let mut dirty = true;
        if pi < prev.dir.len() && prev.dir[pi].0 == id {
            let (_, prev_handle, prev_row) = prev.dir[pi];
            pi += 1;
            if let Some(p) = prev_bucket_of[rank]
                && prev_handle as usize == p
            {
                dirty = false;
                let plan = &plans[rank];
                let pairs = &field_pairs[rank];
                let prev_fields = &prev.buckets[p].fields;
                // prev-only 字段（本拍桶并集消失）：上一拍缺失才与"本拍
                // 无该属性"同形，否则判脏（须删除物化残留）。
                for &pf in &prev_only_fields[rank] {
                    let prev_field = &prev_fields[pf];
                    if !bit_set(&prev_bytes[prev_field.missing_at as usize..], prev_row) {
                        dirty = true;
                        break;
                    }
                }
                if dirty {
                    // prev-only 已判脏，跳过逐格比对（保持保守）。
                } else {
                    for (fr, pairing) in pairs.iter().enumerate() {
                        let field = &plan.fields[fr];
                        let value = columns_all[field.col_base + row as usize];
                        match pairing {
                            Some(pf) => {
                                if !cell_clean(
                                    value,
                                    field,
                                    row,
                                    cells_all,
                                    arena,
                                    prev_bytes,
                                    prev.arena_off as usize,
                                    &prev_fields[*pf],
                                    prev_row as usize,
                                ) {
                                    dirty = true;
                                    break;
                                }
                            }
                            // 字段无配对（新增键/列类型变化）：当前缺失则与
                            // 上一拍"无该属性"同形、可算干净，其余判脏。
                            None => {
                                if value.is_some() {
                                    dirty = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        if dirty {
            let bitmap = &mut bitmaps[rank];
            bitmap[row as usize / 8] |= 1 << (row % 8);
        }
    }
    bitmaps
}

/// 字段配对：两侧字段均按键升序，双指针推进；返回 (cur 字段秩 →
/// prev 字段秩的配对，prev 侧未配对字段秩列表)。键同且列类型同才算
/// 配对；prev-only 字段（本拍从桶并集消失）由调用方按"prev 缺失位
/// 置位才干净"补判。
fn pair_fields(
    plan: &BucketPlan<'_>,
    prev_bucket: &PrevBucketIndex,
) -> (Vec<Option<usize>>, Vec<usize>) {
    let mut out = Vec::with_capacity(plan.fields.len());
    let mut prev_only = Vec::new();
    let mut j = 0usize;
    for field in &plan.fields {
        while j < prev_bucket.fields.len() && prev_bucket.fields[j].key.as_str() < field.key {
            prev_only.push(j);
            j += 1;
        }
        let paired = match prev_bucket.fields.get(j) {
            Some(prev_field) if prev_field.key == field.key && prev_field.kind == field.kind => {
                let rank = Some(j);
                j += 1;
                rank
            }
            _ => None,
        };
        out.push(paired);
    }
    while j < prev_bucket.fields.len() {
        prev_only.push(j);
        j += 1;
    }
    (out, prev_only)
}

/// 单格等值判定：当前值（None=缺失）与上一拍块的缺失/null 位、列
/// 字节或变长 arena 内容比对。任何形态不一致返回 false（判脏）。
#[allow(clippy::too_many_arguments)]
fn cell_clean(
    value: Option<&KkpvVal<'_>>,
    field: &FieldPlan<'_>,
    cur_row: u32,
    cells_all: &[(u32, u32)],
    arena: &[u8],
    prev_bytes: &[u8],
    prev_arena_off: usize,
    prev_field: &PrevFieldIndex,
    prev_row: usize,
) -> bool {
    let prev_missing = bit_set(
        &prev_bytes[prev_field.missing_at as usize..],
        prev_row as u32,
    );
    let value = match value {
        None => return prev_missing,
        Some(KkpvVal::Null) => {
            return !prev_missing
                && bit_set(&prev_bytes[prev_field.null_at as usize..], prev_row as u32);
        }
        Some(value) => value,
    };
    if prev_missing || bit_set(&prev_bytes[prev_field.null_at as usize..], prev_row as u32) {
        return false;
    }
    if is_variable(prev_field.kind) {
        // 变长：cells (offset,len) 对在两拍 arena 内的内容比对；cells
        // 对相等不保证内容相等，必须比字节。
        let cell = cells_all[field.cells_base + cur_row as usize];
        let at = prev_field.col_at as usize + prev_row * DESCRIPTOR;
        let prev_start = read_u32_at(prev_bytes, at) as usize;
        let prev_len = read_u32_at(prev_bytes, at + 4) as usize;
        if prev_len != cell.1 as usize {
            return false;
        }
        let cur = arena
            .get(cell.0 as usize..cell.0 as usize + cell.1 as usize)
            .map(<[u8]>::as_ref);
        let prev = prev_bytes
            .get(prev_arena_off + prev_start..prev_arena_off + prev_start + prev_len)
            .map(<[u8]>::as_ref);
        cur == prev
    } else {
        let at = prev_field.col_at as usize + prev_row * column_width(prev_field.kind);
        let prev_cell = prev_bytes
            .get(at..at + column_width(prev_field.kind))
            .map(<[u8]>::as_ref);
        // 值形态与配对列类型由编码端同列类型推断保证一致；兜底臂保守
        // 判脏。
        match value {
            KkpvVal::Bool(b) => prev_cell == Some(&[*b as u8]),
            KkpvVal::Int(v) => prev_cell == Some(&v.to_le_bytes()),
            KkpvVal::Uint(v) => prev_cell == Some(&v.to_le_bytes()),
            KkpvVal::Float(v) => prev_cell == Some(&v.to_le_bytes()),
            KkpvVal::Ref(v) => prev_cell == Some(&v.to_le_bytes()),
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_ref<'a>(input: &'a KkpvBlockInput) -> KkpvBlockRef<'a> {
        let rows = input
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
        KkpvBlockRef {
            private: input.private,
            tick: input.tick,
            generation: input.generation,
            plan_hash: input.plan_hash,
            audience_hash: input.audience_hash,
            room_name: input.room_name.as_str(),
            terrain: input.terrain.as_deref(),
            rows,
        }
    }

    fn obj(id: u64, type_key: &str, fields: Vec<(&str, KkpvValue)>) -> KkpvObject {
        KkpvObject {
            id,
            type_key: type_key.into(),
            fields: fields
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        }
    }

    fn block(objects: Vec<KkpvObject>, tick: u64) -> KkpvBlockInput {
        KkpvBlockInput {
            private: true,
            tick,
            generation: tick * 7,
            plan_hash: [1; 32],
            audience_hash: [2; 32],
            room_name: "W1N1".into(),
            terrain: None,
            objects,
        }
    }

    fn encode_again(input: &KkpvBlockInput, prev: Option<&PrevEncodedBlock>) -> PrevEncodedBlock {
        super::super::encode_with_dirty(&block_ref(input), &KkpvLimits::default(), prev).unwrap()
    }

    fn flags_of(data: &[u8]) -> u16 {
        u16::from_le_bytes([data[OFF_FLAGS], data[OFF_FLAGS + 1]])
    }

    /// 按目录读逐行脏位（id → 是否脏）。走查与解码端同源推进。
    fn dirty_by_id(data: &[u8]) -> std::collections::HashMap<u64, bool> {
        assert_ne!(flags_of(data) & FLAG_DIRTY, 0, "块未携带脏位段");
        let buckets_off = read_u32_at(data, OFF_BUCKETS) as usize;
        let type_count = read_u32_at(data, OFF_TYPE_COUNT) as usize;
        let dirty_off = read_u32_at(data, OFF_DIRTY) as usize;
        let mut walk = buckets_off;
        let mut bm_at = Vec::with_capacity(type_count);
        let mut dirty_cursor = dirty_off;
        for _ in 0..type_count {
            let rows = read_u32_at(data, walk + 8);
            let field_count = read_u32_at(data, walk + 12) as usize;
            let bm = bitmap_bytes(rows);
            bm_at.push(dirty_cursor);
            dirty_cursor += bm;
            let fields_at = walk + BUCKET_HEADER;
            walk += BUCKET_HEADER + field_count * FIELD_ENTRY + field_count * bm * 2;
            let mut columns = 0usize;
            for f in 0..field_count {
                let kind = data[fields_at + f * FIELD_ENTRY + 8];
                columns += column_width(kind) * rows as usize;
            }
            walk += columns;
        }
        assert_eq!(
            dirty_cursor,
            dirty_off + read_u32_at(data, OFF_DIRTY + 4) as usize,
            "脏位段长度与桶行数精确一致"
        );
        let dir_off = read_u32_at(data, OFF_DIRECTORY) as usize;
        let mut out = std::collections::HashMap::new();
        for i in 0..read_u32_at(data, OFF_OBJECT_COUNT) as usize {
            let at = dir_off + i * DIRECTORY_ENTRY;
            let id = read_u64_at(data, at);
            let handle = read_u32_at(data, at + 8) as usize;
            let row = read_u32_at(data, at + 12);
            let base = bm_at[handle];
            out.insert(id, data[base + row as usize / 8] & (1 << (row % 8)) != 0);
        }
        out
    }

    /// 索引偏移交叉验证：build_index 的桶布局偏移必须与按解码端同源
    /// 走查块字节得到的偏移一致（锁写入端/索引端/解码端三方不漂移）。
    #[test]
    fn index_offsets_match_independent_walk() {
        let input = block(
            vec![
                obj(
                    1,
                    "t:a",
                    vec![("k", KkpvValue::Int(5)), ("s", KkpvValue::Str("x".into()))],
                ),
                obj(2, "t:a", vec![("k", KkpvValue::Int(6))]),
                obj(3, "t:b", vec![("f", KkpvValue::Float(1.5))]),
            ],
            1,
        );
        let reference = super::encode_ref(&block_ref(&input), &KkpvLimits::default()).unwrap();
        let encoded = encode_again(&input, None);
        // 首拍无基准：字节与纯编码逐字节一致。
        assert_eq!(encoded.bytes(), &reference[..]);
        let (index, bytes) = encoded.parts().unwrap();
        let arena_off = read_u32_at(bytes, OFF_ARENA) as usize;
        let arena = &bytes[arena_off..];
        let mut walk = read_u32_at(bytes, OFF_BUCKETS) as usize;
        assert_eq!(
            index.buckets.len(),
            read_u32_at(bytes, OFF_TYPE_COUNT) as usize
        );
        for bucket in &index.buckets {
            let rows = read_u32_at(bytes, walk + 8);
            let field_count = read_u32_at(bytes, walk + 12) as usize;
            let type_off = read_u32_at(bytes, walk) as usize;
            let type_len = read_u32_at(bytes, walk + 4) as usize;
            assert_eq!(
                &arena[type_off..type_off + type_len],
                bucket.type_key.as_bytes()
            );
            walk += BUCKET_HEADER;
            assert_eq!(bucket.fields.len(), field_count);
            let bm = bitmap_bytes(rows);
            let missing_base = walk + field_count * FIELD_ENTRY;
            let null_base = missing_base + field_count * bm;
            let mut col = null_base + field_count * bm;
            for (f, field) in bucket.fields.iter().enumerate() {
                let field_at = walk + f * FIELD_ENTRY;
                let key_off = read_u32_at(bytes, field_at) as usize;
                let key_len = read_u32_at(bytes, field_at + 4) as usize;
                let kind = bytes[field_at + 8];
                assert_eq!(&arena[key_off..key_off + key_len], field.key.as_bytes());
                assert_eq!(kind, field.kind);
                assert_eq!(field.missing_at as usize, missing_base + f * bm);
                assert_eq!(field.null_at as usize, null_base + f * bm);
                assert_eq!(field.col_at as usize, col);
                col += column_width(kind) * rows as usize;
            }
            walk = col;
        }
        let dir_off = read_u32_at(bytes, OFF_DIRECTORY) as usize;
        for (i, entry) in index.dir.iter().enumerate() {
            let at = dir_off + i * DIRECTORY_ENTRY;
            assert_eq!(read_u64_at(bytes, at), entry.0);
            assert_eq!(read_u32_at(bytes, at + 8), entry.1);
            assert_eq!(read_u32_at(bytes, at + 12), entry.2);
        }
    }

    #[test]
    fn same_input_second_tick_marks_all_clean() {
        let objects = vec![
            obj(1, "t:a", vec![("k", KkpvValue::Int(5))]),
            obj(2, "t:a", vec![("k", KkpvValue::Int(6))]),
        ];
        let first = encode_again(&block(objects.clone(), 1), None);
        assert_eq!(flags_of(first.bytes()) & FLAG_DIRTY, 0, "首拍无脏位段");
        let second = encode_again(&block(objects, 2), Some(&first));
        assert_ne!(flags_of(second.bytes()) & FLAG_DIRTY, 0, "次拍携带脏位段");
        assert_eq!(read_u64_at(second.bytes(), OFF_DIRTY_BASE_TICK), 1);
        let rows = dirty_by_id(second.bytes());
        assert!(rows.values().all(|dirty| !dirty), "同输入逐行干净");
        KkpvView::decode(second.bytes(), &KkpvLimits::default()).unwrap();
    }

    #[test]
    fn field_change_marks_only_that_row() {
        let first = encode_again(
            &block(
                vec![
                    obj(1, "t:a", vec![("k", KkpvValue::Int(5))]),
                    obj(2, "t:a", vec![("k", KkpvValue::Int(6))]),
                ],
                1,
            ),
            None,
        );
        let second = encode_again(
            &block(
                vec![
                    obj(1, "t:a", vec![("k", KkpvValue::Int(5))]),
                    obj(2, "t:a", vec![("k", KkpvValue::Int(9))]),
                ],
                2,
            ),
            Some(&first),
        );
        let rows = dirty_by_id(second.bytes());
        assert!(!rows[&1], "未变行干净");
        assert!(rows[&2], "变化行脏");
    }

    #[test]
    fn variable_content_change_marks_dirty() {
        let first = encode_again(
            &block(
                vec![obj(1, "t:a", vec![("s", KkpvValue::Str("aa".into()))])],
                1,
            ),
            None,
        );
        let same = encode_again(
            &block(
                vec![obj(1, "t:a", vec![("s", KkpvValue::Str("aa".into()))])],
                2,
            ),
            Some(&first),
        );
        assert!(!dirty_by_id(same.bytes())[&1], "同内容干净");
        let changed = encode_again(
            &block(
                vec![obj(1, "t:a", vec![("s", KkpvValue::Str("bb".into()))])],
                3,
            ),
            Some(&first),
        );
        assert!(dirty_by_id(changed.bytes())[&1], "同长度异内容脏");
    }

    #[test]
    fn row_set_changes_align_by_id() {
        let first = encode_again(
            &block(
                vec![
                    obj(1, "t:a", vec![("k", KkpvValue::Int(5))]),
                    obj(3, "t:a", vec![("k", KkpvValue::Int(7))]),
                ],
                1,
            ),
            None,
        );
        let grown = encode_again(
            &block(
                vec![
                    obj(1, "t:a", vec![("k", KkpvValue::Int(5))]),
                    obj(2, "t:a", vec![("k", KkpvValue::Int(6))]),
                    obj(3, "t:a", vec![("k", KkpvValue::Int(7))]),
                ],
                2,
            ),
            Some(&first),
        );
        let rows = dirty_by_id(grown.bytes());
        assert!(
            !rows[&1] && !rows[&3],
            "存量行按 ID 对位保持干净（行号漂移不影响）"
        );
        assert!(rows[&2], "新增行脏");
    }

    #[test]
    fn missing_null_kind_and_retry_semantics() {
        let first = encode_again(
            &block(
                vec![obj(
                    1,
                    "t:a",
                    vec![("k", KkpvValue::Int(5)), ("m", KkpvValue::Null)],
                )],
                1,
            ),
            None,
        );
        let null_to_missing = encode_again(
            &block(vec![obj(1, "t:a", vec![("k", KkpvValue::Int(5))])], 2),
            Some(&first),
        );
        assert!(dirty_by_id(null_to_missing.bytes())[&1], "null→缺失判脏");
        let kind_flip = encode_again(
            &block(
                vec![obj(
                    1,
                    "t:a",
                    vec![("k", KkpvValue::Float(5.0)), ("m", KkpvValue::Null)],
                )],
                3,
            ),
            Some(&first),
        );
        assert!(dirty_by_id(kind_flip.bytes())[&1], "列类型变化判脏");
        let retry = encode_again(
            &block(
                vec![obj(
                    1,
                    "t:a",
                    vec![("k", KkpvValue::Int(5)), ("m", KkpvValue::Null)],
                )],
                1,
            ),
            Some(&first),
        );
        assert_eq!(read_u64_at(retry.bytes(), OFF_DIRTY_BASE_TICK), 1);
        assert!(
            !dirty_by_id(retry.bytes())[&1],
            "同拍重编码（重试形态）全干净"
        );
    }
}
