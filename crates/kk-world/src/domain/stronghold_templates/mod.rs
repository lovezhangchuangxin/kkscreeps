//! 官方 bunker 布防模板的静态数据；类型与查表入口在
//! [`super::stronghold`](super::stronghold)，数值搬运核对测试同在其内。
//! 按模板逐文件组织以守源码规模门禁。
use super::stronghold::StrongholdTemplate;

mod bunker1;
mod bunker2;
mod bunker3;
mod bunker4;
mod bunker5;

pub const TEMPLATES: &[StrongholdTemplate] = &[
    bunker1::TEMPLATE,
    bunker2::TEMPLATE,
    bunker3::TEMPLATE,
    bunker4::TEMPLATE,
    bunker5::TEMPLATE,
];
