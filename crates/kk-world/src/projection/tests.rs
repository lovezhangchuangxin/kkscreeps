//! 检查地形载荷、对象投影、玩家视野及 FIND 过滤。

use super::*;
use crate::place::{PlaceCtx, normalize};
use kk_core::Terrain;
use kk_schema::room::ControllerObj;

mod terrain_and_projection_forms;
mod viewer_semantics_and_families;

/// 解码测试中的地形载荷，用来核对实际地形字节；不能单独证明 JS 解码正确。
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
