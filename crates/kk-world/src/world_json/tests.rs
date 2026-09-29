//! world_json 内嵌测试（M2B 前置批 P4 自 world_json.rs 原位拆出；收口终审
//! P1 清偿再拆两子模块——清偿补入的锁定测试使单文件超 1000 行）：内容与
//! 原 mod 块去空白 token 流一致（评审 2 SHA256 对账）；零行为零内容增删。
//! 段序=源文序连续切块。

use super::*;
use crate::place::{PlaceCtx, normalize};
use kk_core::Terrain;
use kk_schema::room::ControllerObj;

mod terrain_and_projection_forms;
mod viewer_semantics_and_families;

/// 测试孪生 base64 解码器（与 prelude game.js 的 b64ToBytes 同语义；
/// 编码器是产品面，解码器只存在于消费侧——Rust 测试承担 JS 侧的对表）。
fn test_b64_decode(s: &str) -> Vec<u8> {
    fn val(c: u8) -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("非法 base64 字符: {}", c as char),
        }
    }
    let bytes = s.as_bytes();
    assert_eq!(bytes.len() % 4, 0, "b64 长度须为 4 的倍数");
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|&&c| c == b'=').count();
        let n = (u32::from(val(chunk[0])) << 18)
            | (u32::from(val(chunk[1])) << 12)
            | (u32::from(if pad > 1 { 0 } else { val(chunk[2]) }) << 6)
            | u32::from(if pad > 0 { 0 } else { val(chunk[3]) });
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    out
}
