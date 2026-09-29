# assets/svg/

官方 Screeps 渲染器美术资产 vendor（117 SVG + 9 程序底纹 PNG），随附
`LICENSE.txt` 原文（ISC，Copyright (c) 2016 Artem Chivchalov <contact@screeps.com>，
允许商用/修改/再分发，保留版权与许可声明即可）。

来源：screeps/renderer 本地克隆 `~/Desktop/projects/sources/screeps/renderer`
（HEAD `a2db4a7`，https://github.com/screeps/renderer ，ISC）目录
`metadata/images/`，文件逐字节拷贝、零修改（含 7 个小写 `viewbox` 的 Inkscape
文件——严格 XML 语义下浏览器与 resvg 都忽略它，栅格化管线按浏览器语义取
width/height 属性为内在尺寸，见 scripts/build-assets.mjs 头注）。

文件集以官方目录 1:1 为准（126 图像；官方 resourceMap.js 仅 119 条映射，
commander/executor/fortify/operate-controller/operate-power/operator/tombstone
等 7 文件不在其中，系 powerCreep 动态拼键或官方遗留——resourceMap 仅作命名
参考，不作取舍依据）。清单与 hash 由 `assets/manifest.json` 登记，
`scripts/check-assets-fresh.mjs` 校验。
