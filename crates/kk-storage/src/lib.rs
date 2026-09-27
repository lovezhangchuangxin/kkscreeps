//! 存储层：热数据全内存，约 2 分钟把变化的房间快照写盘（嵌入式单文件键值库）。
//! 引擎不依赖具体存储实现，通过本 crate 定义的接口读写（M2 实装，选型 redb vs libmdbx 届时基准后定）。
//! 设计见 docs/architecture/storage.md。
