//! Generic 载荷的零拷贝行提取：受限文法扫描器直接从不可变 JSON 原文
//! 产出借用 `KkpvVal`，跳过 `object_json` 为每个对象建而又弃的
//! serde_json Value 树——成熟服数百房的 rampart/link/tower 等都以
//! Generic 存储，共享块构建的解析成本集中在这条路径。
//!
//! 与参考路径（`object_json` + `json_row`）逐位等价是硬约束，由
//! `scanner_row_matches_reference` 等对照测试锁定：
//! - 顶层键集与覆盖规则：`id` 恒不导出（目录承担），`room`/`x`/`y`/
//!   `pos` 以对象表派生值覆盖，powerCreep 强制 `shard:"shard0"`，
//!   `effects` 有条目表则覆盖，目录 ext 存在时原生 `ext` 丢弃；
//! - serde_json::Map 是 IndexMap（线上二进制经 deno_core 传递开启
//!   preserve_order，kk-world 已显式钉住）：迭代按插入序，`insert` 对
//!   已存在键原地换值不挪位——扫描器按首现位置保留、后写值覆盖复刻，
//!   派生键沿用同一插入序列（shard→id→room→x→y→pos→effects）；
//! - 数值分类与 `json_val` 同序：i64 优先、u64 次之、f64 最后；
//! - 嵌套对象同样保持插入序（KKPV 编码保持嵌套条目顺序）。
//!
//! 超出受限文法的载荷（转义字符串、病态数字、非对象顶层、嵌套超过
//! serde 递归上限 128、尾随内容）一律 `None` 回退参考路径，由 serde
//! 复刻全部行为，包括无效 JSON 的空记录分支与非对象顶层的错误文案。

use std::borrow::Cow;

use kk_schema::kkpv::{KkpvRow, KkpvVal};
use kk_schema::room::GenericObj;

use super::objects::builtin_structure_key;
use crate::world::ObjId;

/// serde_json 默认递归上限：第 128 层容器合法、第 129 层报错
/// （serde_json/de.rs 的 `check_recursion!`，remaining_depth 自 128 起减）。
/// 扫描器按同一口径计数，超限回退由 serde 复刻空记录结果。
const RECURSION_LIMIT: usize = 128;

/// 已通过受限文法校验的 Generic 扫描结果：顶层键值（首现位置保留、
/// 后写值覆盖，IndexMap 语义）加上派生行所需的上下文。`scan` 返回
/// None 的载荷全部由参考路径处理；本结构只承载确定可零拷贝的形态。
pub(super) struct GenericScan<'a> {
    room: &'a str,
    id: ObjId,
    generic: &'a GenericObj,
    time: u64,
    effects: Option<&'a Vec<crate::EffectEntry>>,
    entries: Vec<(&'a str, KkpvVal<'a>)>,
}

impl<'a> GenericScan<'a> {
    /// 扫描载荷顶层对象；非对象顶层、受限文法外或尾随内容返回 None。
    pub(super) fn scan(
        room: &'a str,
        id: ObjId,
        generic: &'a GenericObj,
        time: u64,
        effects: Option<&'a Vec<crate::EffectEntry>>,
    ) -> Option<Self> {
        let mut scanner = Scanner::new(generic.data.as_str());
        scanner.skip_ws();
        if scanner.peek() != Some(b'{') {
            return None;
        }
        let entries = scanner.scan_object(1)?;
        scanner.skip_ws();
        if scanner.pos != scanner.bytes.len() {
            return None;
        }
        Some(Self {
            room,
            id,
            generic,
            time,
            effects,
            entries,
        })
    }

    /// ext 目录查键；与参考路径从 serde 记录读 `structureType` 同源
    /// （该键不被派生插入覆盖，载荷后写值即最终值）。
    pub(super) fn structure_type(&self) -> Option<&'a str> {
        self.str_field("structureType")
    }

    fn str_field(&self, key: &str) -> Option<&'a str> {
        self.entries
            .iter()
            .find(|(k, _)| *k == key)
            .and_then(|(_, v)| match v {
                KkpvVal::Str(s) => Some(*s),
                _ => None,
            })
    }

    /// 产出借用行：复刻 `object_json` 的插入覆盖 + `json_row` 的键过滤。
    /// 字段保持插入序（IndexMap 迭代序；派生键沿用 object_json 的插入
    /// 序列——已存在则原地换值，新键追加到尾部）。
    pub(super) fn into_row(self, ext_replaced: bool) -> KkpvRow<'a> {
        let Self {
            room,
            id,
            generic,
            time,
            effects,
            entries,
        } = self;
        let str_field = |key: &str| {
            entries
                .iter()
                .find(|(k, _)| *k == key)
                .and_then(|(_, v)| match v {
                    KkpvVal::Str(s) => Some(*s),
                    _ => None,
                })
        };
        let type_val = str_field("type");
        // powerCreep 判定读载荷最终 type（object_json 在插入前读取同源）。
        let is_power_creep = type_val == Some("powerCreep");
        let structure_type = str_field("structureType");
        let mod_type = str_field("modType");
        // IndexMap::insert 语义：已存在键原地换值保留位置，新键追加。
        let insert =
            |fields: &mut Vec<(&'a str, KkpvVal<'a>)>, key: &'a str, value: KkpvVal<'a>| {
                match fields.iter_mut().find(|(k, _)| *k == key) {
                    Some(slot) => slot.1 = value,
                    None => fields.push((key, value)),
                }
            };
        let mut fields: Vec<(&'a str, KkpvVal<'a>)> = entries;
        // object_json 的插入序列（id 由 json_row 跳过，省略不影响其余键位）。
        if is_power_creep {
            insert(&mut fields, "shard", KkpvVal::Str("shard0"));
        }
        insert(&mut fields, "room", KkpvVal::Str(room));
        insert(&mut fields, "x", KkpvVal::Int(generic.x as i64));
        insert(&mut fields, "y", KkpvVal::Int(generic.y as i64));
        // pos 读的是已覆盖后的派生 x/y/room（object_json 插入先于 pos 装配）；
        // 嵌套键序 x,y,roomName 是 json! 宏字面序。
        insert(
            &mut fields,
            "pos",
            KkpvVal::Object(vec![
                ("x", KkpvVal::Int(generic.x as i64)),
                ("y", KkpvVal::Int(generic.y as i64)),
                ("roomName", KkpvVal::Str(room)),
            ]),
        );
        if let Some(effect_entries) = effects {
            // effects::project_raw：全部条目不过滤；endTime/ticksRemaining
            // 的 u64 与 json_val 的 as_i64→as_u64 同序分类。
            insert(&mut fields, "effects", effects_val(effect_entries, time));
        }
        // json_row 过滤：id 恒不导出（目录承担），目录 ext 存在时原生 ext 丢弃。
        fields.retain(|(key, _)| *key != "id" && !(ext_replaced && *key == "ext"));
        // 桶键与 objects::type_key_cow 同序同源：结构/出生点按
        // structureType，Mod 记录用类型键本身，其余 builtin:{type}。
        let kind = type_val.unwrap_or("generic");
        let type_key = match kind {
            "mod" => Cow::Borrowed(mod_type.unwrap_or("builtin:mod")),
            "structure" | "spawn" => match structure_type {
                Some(st) => builtin_structure_key(st),
                None => Cow::Owned(format!("builtin:{kind}")),
            },
            "creep" => Cow::Borrowed("builtin:creep"),
            "constructionSite" => Cow::Borrowed("builtin:constructionSite"),
            "resource" => Cow::Borrowed("builtin:resource"),
            "flag" => Cow::Borrowed("builtin:flag"),
            "tombstone" => Cow::Borrowed("builtin:tombstone"),
            "ruin" => Cow::Borrowed("builtin:ruin"),
            other => Cow::Owned(format!("builtin:{other}")),
        };
        KkpvRow {
            id: id.0,
            type_key,
            fields,
        }
    }
}

/// effects 条目表 → KKPV 嵌套数组（effects.rs::project_raw 的借用版）：
/// effect 缺省不写（官方 SHIELD 分支），键序为 project_raw 的插入序
/// （effect→power→level→endTime→ticksRemaining，与 object_row 类型化臂
/// 一致），ticksRemaining 饱和减法与 endTime 的 Uint 分类同参考。
fn effects_val(entries: &[crate::EffectEntry], time: u64) -> KkpvVal<'static> {
    let u64_val = |v: u64| {
        if v <= i64::MAX as u64 {
            KkpvVal::Int(v as i64)
        } else {
            KkpvVal::Uint(v)
        }
    };
    KkpvVal::Array(
        entries
            .iter()
            .map(|e| {
                let mut entry: Vec<(&'static str, KkpvVal<'static>)> = Vec::new();
                if let Some(effect) = e.effect {
                    entry.push(("effect", KkpvVal::Int(effect)));
                }
                entry.push(("power", KkpvVal::Int(e.power)));
                entry.push(("level", KkpvVal::Int(e.level)));
                entry.push(("endTime", u64_val(e.end_time)));
                entry.push(("ticksRemaining", u64_val(e.end_time.saturating_sub(time))));
                KkpvVal::Object(entry)
            })
            .collect(),
    )
}

/// 受限文法光标：JSON 的 ws/对象/数组/简单字符串/标准数字/字面量。
/// 所有偏离返回 None 交参考路径，不做错误细分。
struct Scanner<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            bytes: text.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.bytes.get(self.pos) {
            self.pos += 1;
        }
    }

    fn expect_literal(&mut self, literal: &[u8]) -> Option<()> {
        if self.bytes.get(self.pos..self.pos + literal.len())? == literal {
            self.pos += literal.len();
            Some(())
        } else {
            None
        }
    }

    /// 受限文法字符串：内容无反斜杠与原始控制字符，直接借用原文切片。
    /// 切界只落在 ASCII 引号/反斜杠上，必为 UTF-8 字符边界；转义与
    /// 控制字符（serde 判无效）返回 None。
    fn scan_string(&mut self) -> Option<&'a str> {
        self.pos += 1; // 开引号
        let start = self.pos;
        loop {
            match self.bytes.get(self.pos)? {
                b'"' => {
                    let end = self.pos;
                    self.pos += 1;
                    return std::str::from_utf8(&self.bytes[start..end]).ok();
                }
                b'\\' => return None,
                b if *b < 0x20 => return None,
                _ => self.pos += 1,
            }
        }
    }

    /// 受限文法数值：标准 JSON 数字（禁前导零）。整数按 i64→u64 分类
    /// （与 json_val 同序）；小数/指数走正确舍入的 f64（workspace 启用
    /// serde_json float_roundtrip，与 str::parse 同为正确舍入）。病态
    /// 形态（`-0` 整数形态、i64/u64 溢出、非有限如 1e999）返回 None，
    /// serde 会给出 f64 归一/降档/Null 等形态，由参考路径复刻。
    fn scan_number(&mut self) -> Option<KkpvVal<'a>> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return None; // 前导零：serde 判为无效数字
                }
            }
            Some(b'1'..=b'9') => {
                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return None, // 孤立负号或非数字
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.pos += 1;
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return None;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            is_float = true;
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return None;
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let token = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
        if is_float {
            let value: f64 = token.parse().ok()?;
            if !value.is_finite() {
                return None;
            }
            Some(KkpvVal::Float(value))
        } else if let Some(digits) = token.strip_prefix('-') {
            if digits == "0" {
                return None; // `-0`：serde 归一为 f64 -0.0，交参考路径
            }
            // 整 token 含符号解析（i64::MIN 只有带符号才可表示）；
            // 负向溢出时 serde 落到 f64，交参考路径。
            token.parse::<i64>().ok().map(KkpvVal::Int)
        } else {
            // u64 溢出时 serde 落到 f64，交参考路径。
            token.parse::<u64>().ok().map(|v| {
                if v <= i64::MAX as u64 {
                    KkpvVal::Int(v as i64)
                } else {
                    KkpvVal::Uint(v)
                }
            })
        }
    }

    /// 容器计数与 serde 的 check_recursion 同口径：本容器是第 depth 层，
    /// 超过 128 层回退（serde 对第 129 层报错，空记录结果由参考路径复刻）。
    fn scan_value(&mut self, depth: usize) -> Option<KkpvVal<'a>> {
        if depth > RECURSION_LIMIT {
            return None;
        }
        self.skip_ws();
        match self.peek()? {
            b'{' => self.scan_object(depth).map(KkpvVal::Object),
            b'[' => self.scan_array(depth).map(KkpvVal::Array),
            b'"' => self.scan_string().map(KkpvVal::Str),
            b't' => self.expect_literal(b"true").map(|_| KkpvVal::Bool(true)),
            b'f' => self.expect_literal(b"false").map(|_| KkpvVal::Bool(false)),
            b'n' => self.expect_literal(b"null").map(|_| KkpvVal::Null),
            b'-' | b'0'..=b'9' => self.scan_number(),
            _ => None,
        }
    }

    /// 对象成员 → 去重键值表（IndexMap 语义：重复键后写值覆盖、保留
    /// 首现位置；迭代按插入序）。对象成员数小，重复键就地线性查找足够。
    fn scan_object(&mut self, depth: usize) -> Option<Vec<(&'a str, KkpvVal<'a>)>> {
        self.pos += 1; // '{'
        let mut entries: Vec<(&'a str, KkpvVal<'a>)> = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Some(entries);
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return None;
            }
            let key = self.scan_string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return None;
            }
            self.pos += 1;
            let value = self.scan_value(depth + 1)?;
            match entries.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = value,
                None => entries.push((key, value)),
            }
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    break;
                }
                _ => return None,
            }
        }
        Some(entries)
    }

    /// 数组成员（顺序保持，与 serde Value::Array 一致）。
    fn scan_array(&mut self, depth: usize) -> Option<Vec<KkpvVal<'a>>> {
        self.pos += 1; // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Some(items);
        }
        loop {
            items.push(self.scan_value(depth + 1)?);
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    break;
                }
                _ => return None,
            }
        }
        Some(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kkpv::objects::json_row;
    use crate::projection::object_json;
    use kk_core::Terrain;

    fn generic_of(text: &str) -> GenericObj {
        GenericObj {
            id: 0,
            x: 12,
            y: 34,
            data: text.into(),
        }
    }

    /// 参考路径与扫描行就地比对：参考行的字段借用局部 record，不能跨
    /// 函数返回，故比对在本函数内完成（与 encode_room_block 回退分支
    /// 完全同构：object_json → json_row）。
    fn assert_row_matches_reference(
        text: &str,
        time: u64,
        effects: Option<&Vec<crate::EffectEntry>>,
        ext_replaced: bool,
    ) {
        let room = "E2S2";
        let id = ObjId(7);
        let generic = generic_of(text);
        let scan = GenericScan::scan(room, id, &generic, time, effects)
            .unwrap_or_else(|| panic!("受限文法载荷应可扫描：{text}"));
        let scanned = scan.into_row(ext_replaced);
        let object = kk_schema::room::RoomObject::Generic(generic_of(text));
        let record = object_json(room, id, &object, time, &Terrain::plains(), None, effects)
            .expect("Generic 对象必然产出记录");
        let reference =
            json_row(id, &record, ext_replaced).unwrap_or_else(|e| panic!("参考路径不应失败：{e}"));
        assert_eq!(scanned.id, reference.id, "id 不一致：{text}");
        assert_eq!(scanned.type_key, reference.type_key, "桶键不一致：{text}");
        assert_eq!(scanned.fields, reference.fields, "字段不一致：{text}");
    }

    fn entry(effect: Option<i64>, power: i64, level: i64, end_time: u64) -> crate::EffectEntry {
        crate::EffectEntry {
            effect,
            power,
            level,
            end_time,
        }
    }

    /// 受限文法内的成熟负载形态：字段集、键序、数值分类、派生覆盖与
    /// 桶键逐位对齐参考路径；覆盖 effects 有无与 ext 裁切四个组合。
    #[test]
    fn scanner_row_matches_reference() {
        let effects = vec![entry(Some(12), 3, 4, 500), entry(None, 5, 2, 1_000)];
        let payloads = [
            // rampart：载荷 room/x/y 与对象表不同，派生值覆盖。
            r#"{"type":"structure","structureType":"rampart","hits":1000,"hitsMax":3000,"room":"W1N1","x":1,"y":2,"nextDecayTime":123}"#,
            // tower：无 nextDecayTime；owner/user 字符串。
            r#"{"type":"structure","structureType":"tower","hits":2500,"hitsMax":3000,"owner":"player2","room":"E2S2","x":12,"y":34,"energy":600,"energyCapacity":1000}"#,
            // powerCreep：载荷 shard 被强制 shard0。
            r#"{"type":"powerCreep","name":"pc1","className":"operator","level":11,"shard":"shard3","room":"E5N5"}"#,
            // 重复键后写覆盖：hits 取 2、type 取 structure。
            r#"{"hits":1,"hits":2,"type":"x","type":"structure","structureType":"link"}"#,
            // 嵌套对象与数组：内层键字节序排序、数组序保持。
            r#"{"store":{"Z":"z","A":{"b":1,"a":2}},"arr":[1,{"y":1,"x":2},[3,1]],"empty":{},"emptyArr":[]}"#,
            // 数值形态：0、负数、i64/u64 边界、小数、指数、1.0。
            r#"{"a":0,"b":-5,"c":9223372036854775807,"d":18446744073709551615,"e":1.5,"f":1e3,"g":1.0,"h":-9223372036854775808,"i":true,"j":false,"k":null}"#,
            // Unicode 原文（无转义）与空白形态。
            "{\n \"名\" : \"中文值\" ,\t\"type\" : \"nuker\"\r\n}",
            // 空对象：仅派生字段。
            "{}",
            // Mod 形态 type（桶键走 modType）与非字符串 type。
            r#"{"type":"mod","modType":"custom:thing"}"#,
            r#"{"type":5,"hits":1}"#,
            // ext 键：ext_replaced=false 时保留、true 时由调用方追加。
            r#"{"type":"structure","structureType":"lab","ext":{"x":1}}"#,
            // 结构无 structureType：桶键 builtin:structure 兜底。
            r#"{"type":"structure","hits":1}"#,
            // 载荷 effects 键：有条目表时覆盖、无时保留。
            r#"{"type":"structure","structureType":"factory","effects":[{"power":1}],"level":2}"#,
            // 载荷 pos/id/shard（非 powerCreep）：全部被派生/目录语义处置。
            r#"{"type":"resource","pos":{"x":1},"id":"zzz","shard":"shard9"}"#,
        ];
        for payload in payloads {
            for ext_replaced in [false, true] {
                assert_row_matches_reference(payload, 100, None, ext_replaced);
                assert_row_matches_reference(payload, 100, Some(&effects), ext_replaced);
            }
        }
    }

    /// effects 值域：endTime 超 i64 落 Uint、缺 effect 键、饱和剩余时间。
    #[test]
    fn effects_merge_matches_reference() {
        let effects = vec![
            entry(None, 7, 1, 9_223_372_036_854_775_808),
            entry(Some(3), 2, 5, 50),
        ];
        assert_row_matches_reference(
            r#"{"type":"structure","structureType":"storage","hits":10}"#,
            100,
            Some(&effects),
            false,
        );
    }

    /// 参考路径的行键序（本地 record 不能跨函数返回借用行，取证只取键）。
    fn reference_keys(text: &str) -> Vec<String> {
        let object = kk_schema::room::RoomObject::Generic(generic_of(text));
        let record = object_json(
            "E2S2",
            ObjId(7),
            &object,
            100,
            &Terrain::plains(),
            None,
            None,
        )
        .expect("Generic 对象必然产出记录");
        json_row(ObjId(7), &record, false)
            .expect("参考路径不应失败")
            .fields
            .into_iter()
            .map(|(k, _)| k.to_string())
            .collect()
    }

    /// 参考路径的错误文案（json_row 对非对象顶层的上抛）。
    fn reference_err(text: &str) -> String {
        let object = kk_schema::room::RoomObject::Generic(generic_of(text));
        let record = object_json(
            "E2S2",
            ObjId(7),
            &object,
            100,
            &Terrain::plains(),
            None,
            None,
        )
        .expect("Generic 对象必然产出记录");
        json_row(ObjId(7), &record, false).unwrap_err()
    }

    /// 受限文法外的载荷：扫描器必须回退（None），且参考路径给出既定
    /// 行为——无效 JSON 为空记录派生行、非对象顶层的错误文案不变。
    #[test]
    fn anomalies_fall_back_to_reference_semantics() {
        let escapes = [
            r#"{"a\"b":1}"#,                // 转义键
            r#"{"s":"x\ny"}"#,              // 转义值
            "{\"s\":\"\u{0}\"}",            // 原始控制字符（serde 判无效）
            "{\"a\":01}",                   // 前导零
            "{\"a\":-0}",                   // 负零整数形态
            "{\"a\":18446744073709551616}", // u64 溢出（serde 落 f64）
            "{\"a\":-9223372036854775809}", // i64 负向溢出（serde 落 f64）
            "{\"a\":1e999}",                // 非有限（serde 归一为 null）
            "{\"a\":1,}",                   // 尾随逗号
            "{\"a\":1} x",                  // 尾随内容
            "{oops}",                       // 无效 JSON
            "",                             // 空载荷
            "[1,2]",                        // 非对象顶层
            "\"str\"",                      // 字符串顶层
            "5",                            // 数字顶层
            "null",                         // null 顶层
        ];
        for text in escapes {
            let generic = generic_of(text);
            assert!(
                GenericScan::scan("E2S2", ObjId(7), &generic, 100, None).is_none(),
                "受限文法外载荷应回退：{text}"
            );
        }
        // 无效 JSON 的参考行为：空记录 + 派生字段（插入序 room,x,y,pos；
        // id 先插入但被 json_row 跳过）。
        assert_eq!(reference_keys("{oops"), ["room", "x", "y", "pos"]);
        // 非对象顶层的参考行为：json_row 错误文案（回退分支原样上抛）。
        assert_eq!(reference_err("[1,2]"), "对象记录必须是 JSON 对象");
    }

    /// 递归边界与 serde check_recursion 同口径：128 层容器双方放行且
    /// 逐位一致，129 层扫描器回退、serde 报错走空记录分支。
    #[test]
    fn recursion_boundary_matches_serde() {
        for depth in [126, 127, 128] {
            // 直接构造 depth 层嵌套：外层 depth-1 个 {"a": 包住 1。
            let mut text = String::new();
            for _ in 0..depth - 1 {
                text.push_str("{\"a\":");
            }
            text.push('1');
            for _ in 0..depth - 1 {
                text.push('}');
            }
            assert_row_matches_reference(&text, 1, None, false);
        }
        for depth in [129, 130] {
            let mut text = String::new();
            for _ in 0..depth - 1 {
                text.push_str("{\"a\":");
            }
            text.push('1');
            for _ in 0..depth - 1 {
                text.push('}');
            }
            let generic = generic_of(&text);
            assert!(
                GenericScan::scan("E2S2", ObjId(7), &generic, 1, None).is_none(),
                "超 serde 递归上限的载荷应回退（{depth} 层）"
            );
            // serde 同样报错 → 空记录派生行（参考路径兜底复刻；
            // 行键不随 time 变化，取证助手固定 time 即可）。
            assert_eq!(reference_keys(&text), ["room", "x", "y", "pos"]);
        }
    }
}
