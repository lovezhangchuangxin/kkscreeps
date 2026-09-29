# assets/fonts/

Roboto woff2（latin，400/700），官方渲染器文字规格所用字体（say 气泡
60px、名字 55px 声明 `'Roboto, sans-serif'`，mineral 符号声明
`'Roboto, serif'` 且 bold——引擎只声明 family，字体由客户端环境提供，故
本包自带）。

来源：npm `@fontsource/roboto@5.3.0`（registry.npmjs.org 官方 tarball），
文件 `files/roboto-latin-{400,700}-normal.woff2` 逐字节拷贝；许可
**OFL-1.1**（SIL Open Font License 1.1，随附 `LICENSE` 原文——注意：
Google 已将 classic Roboto 转为 OFL-1.1 分发，方案 §1.4 写的 Apache-2.0
是旧版时代的观察，以此处随附许可为准）。

清单与 hash 由 `assets/manifest.json` 登记，`scripts/check-assets-fresh.mjs` 校验。
