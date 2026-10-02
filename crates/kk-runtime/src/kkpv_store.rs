//! KKPV 块的固定句柄库：按世界代际发布不可变载荷，句柄持有期间旧块
//! 不被回收、内容不变。
//!
//! 载荷有两种：单块字节（`Arc<[u8]>`，B.4 语义——面向未来的 per-viewer
//! overlay）与共享束（`SharedKkpvBundle`——公开地形通道的发布形态，
//! 全部玩家共享同一份 V8 可挂载内存）。两种载荷都在发布前于锁外完成
//! 完整结构校验（`KkpvView::decode` / `validate_bundle`）并核对头部
//! tick/generation 与发布代际一致；校验限额由发布调用方给定并随块钉死，
//! 读取复用同一份。代际前进性在锁内复核，防并发越序提交。获取只命中
//! 当前代际——工作者回执被拒收不使旧代际块重新可获取；未发布代际同样
//! 返回 None。工作者实际结束或沙箱销毁、引用释放后，旧代际在下次发布
//! 或报告时回收；仍被句柄持有的代际在保留报告中列出（代际、字节数与
//! 库外存活句柄数——句柄克隆不经库锁，读数是近似快照；V8 侧
//! ArrayBuffer 对共享束的引用只延长真实存活，不进入读数）。
//!
//! 锁采用中毒即 panic 策略：临界区只做结构提交与集合维护，若这些
//! 操作 panic，进程状态已不可信，fail-fast 优于带着不一致索引继续。

use std::sync::{Arc, Mutex};

use kk_core::RunEpoch;
use kk_runner::SharedKkpvBundle;
use kk_schema::kkpv::{KkpvLimits, KkpvView};

/// 已发布载荷：单块字节或跨玩家共享的束内存。
enum Payload {
    Block(Arc<[u8]>),
    Shared(SharedKkpvBundle),
}

impl Payload {
    fn byte_len(&self) -> usize {
        match self {
            Self::Block(bytes) => bytes.len(),
            Self::Shared(bundle) => bundle.byte_len(),
        }
    }
}

struct BlockEntry {
    epoch: RunEpoch,
    payload: Payload,
    limits: KkpvLimits,
}

/// 不可变载荷的引用计数句柄。句柄存活期间字节保持可读且内容不变；
/// 新版本发布不修改旧载荷。句柄不依赖库存活。
#[derive(Clone)]
pub struct KkpvBlockHandle {
    entry: Arc<BlockEntry>,
}

impl KkpvBlockHandle {
    pub fn epoch(&self) -> RunEpoch {
        self.entry.epoch
    }

    pub fn byte_len(&self) -> usize {
        self.entry.payload.byte_len()
    }

    /// 共享束形态的载荷句柄；单块载荷返回 None。
    pub fn shared_bundle(&self) -> Option<&SharedKkpvBundle> {
        match &self.entry.payload {
            Payload::Shared(bundle) => Some(bundle),
            Payload::Block(_) => None,
        }
    }

    /// 对单块载荷建立只读视图。块字节存于不可变 `Arc<[u8]>`、解码限额
    /// 随发布钉死、解码是纯函数，且发布时已通过同一校验，因此成功
    /// 路径上不期望 Err；保留 Result 是为与字段读取的错误处理一致，
    /// 兼容解码器防御性变更。
    ///
    /// # Errors
    /// 防御性重校验失败或载荷是共享束形态时返回错误。
    pub fn view(&self) -> anyhow::Result<KkpvView<'_>> {
        let Payload::Block(block) = &self.entry.payload else {
            anyhow::bail!("共享束载荷不提供单块视图");
        };
        KkpvView::decode(block, &self.entry.limits)
            .map_err(|e| anyhow::anyhow!("块重校验失败：{e}"))
    }
}

/// 保留报告中的一条记录：块代际、字节数与库外存活句柄数。
/// 句柄数 = 强引用数减库内唯一一份；并发克隆下是近似快照。
pub struct RetainedBlock {
    pub epoch: RunEpoch,
    pub bytes: usize,
    pub handles: usize,
}

struct StoreInner {
    current: Option<Arc<BlockEntry>>,
    retained: Vec<Arc<BlockEntry>>,
}

/// 按世界代际发布与获取 KKPV 块的库。
pub struct KkpvBlockStore {
    inner: Mutex<StoreInner>,
}

/// 强计数为 1 时库外不存在句柄：任何克隆都要求库外已有一个 Arc，
/// 因此该读数在锁内无竞态。
fn has_handles(entry: &Arc<BlockEntry>) -> bool {
    Arc::strong_count(entry) > 1
}

impl KkpvBlockStore {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(StoreInner {
                current: None,
                retained: Vec::new(),
            }),
        }
    }

    /// 发布当前代际的单块并返回其句柄。代际须严格前进（tick 更大，
    /// 或 tick 相同且 generation 更大）；块头携带的 tick/generation
    /// 必须与发布代际一致。结构校验在锁外完成，代际前进性在锁内复核。
    ///
    /// # Errors
    /// 代际不前进、块头身份不符或块未通过结构校验时返回错误，
    /// 库状态不变。
    pub fn publish(
        &self,
        epoch: RunEpoch,
        block: Arc<[u8]>,
        limits: &KkpvLimits,
    ) -> anyhow::Result<KkpvBlockHandle> {
        let view =
            KkpvView::decode(&block, limits).map_err(|e| anyhow::anyhow!("块不合法：{e}"))?;
        if view.tick() != epoch.tick || view.generation() != epoch.generation {
            anyhow::bail!(
                "块头身份 ({}, {}) 与发布代际 ({}, {}) 不符",
                view.tick(),
                view.generation(),
                epoch.tick,
                epoch.generation
            );
        }
        self.commit(epoch, Payload::Block(block), limits)
    }

    /// 发布当前代际的共享束（公开通道形态）并返回句柄。束字节在锁外经
    /// `validate_bundle` 全量校验（逐块结构 + 束内版本一致），头部
    /// tick/generation 须与发布代际一致；空束无版本可校验，拒绝。
    /// 校验通过后字节一次性移入 backing store——此后挂载不再复制。
    ///
    /// # Errors
    /// 代际不前进、束不合法或束内版本与发布代际不符时返回错误，
    /// 库状态不变。
    pub fn publish_shared(
        &self,
        epoch: RunEpoch,
        bytes: Vec<u8>,
        limits: &KkpvLimits,
    ) -> anyhow::Result<KkpvBlockHandle> {
        let (_, tick, generation) = kk_schema::kkpv::validate_bundle(&bytes, limits)
            .map_err(|e| anyhow::anyhow!("共享束不合法：{e}"))?;
        if (tick, generation) != (epoch.tick, epoch.generation) {
            anyhow::bail!(
                "共享束版本 ({tick}, {generation}) 与发布代际 ({}, {}) 不符",
                epoch.tick,
                epoch.generation
            );
        }
        self.commit(
            epoch,
            Payload::Shared(SharedKkpvBundle::from_vec(bytes)),
            limits,
        )
    }

    fn commit(
        &self,
        epoch: RunEpoch,
        payload: Payload,
        limits: &KkpvLimits,
    ) -> anyhow::Result<KkpvBlockHandle> {
        let entry = Arc::new(BlockEntry {
            epoch,
            payload,
            limits: limits.clone(),
        });
        let mut inner = self.inner.lock().expect("块库锁未中毒");
        if let Some(current) = &inner.current {
            let c = current.epoch;
            if epoch.tick < c.tick || (epoch.tick == c.tick && epoch.generation <= c.generation) {
                anyhow::bail!(
                    "块代际不前进：当前 ({}, {})，请求 ({}, {})",
                    c.tick,
                    c.generation,
                    epoch.tick,
                    epoch.generation
                );
            }
        }
        if let Some(old) = inner.current.take() {
            inner.retained.push(old);
        }
        inner.current = Some(Arc::clone(&entry));
        inner.retained.retain(has_handles);
        Ok(KkpvBlockHandle { entry })
    }

    /// 获取指定代际的当前块句柄；仅当前代际命中。
    pub fn acquire(&self, epoch: RunEpoch) -> Option<KkpvBlockHandle> {
        let inner = self.inner.lock().expect("块库锁未中毒");
        let current = inner.current.as_ref()?;
        (current.epoch == epoch).then(|| KkpvBlockHandle {
            entry: Arc::clone(current),
        })
    }

    /// 回收无句柄的旧代际后，列出仍被保留的载荷（含当前代际）。
    pub fn retention_report(&self) -> Vec<RetainedBlock> {
        let mut inner = self.inner.lock().expect("块库锁未中毒");
        inner.retained.retain(has_handles);
        let mut report: Vec<RetainedBlock> = inner
            .retained
            .iter()
            .map(|e| RetainedBlock {
                epoch: e.epoch,
                bytes: e.payload.byte_len(),
                handles: Arc::strong_count(e) - 1,
            })
            .collect();
        if let Some(current) = &inner.current {
            report.push(RetainedBlock {
                epoch: current.epoch,
                bytes: current.payload.byte_len(),
                handles: Arc::strong_count(current) - 1,
            });
        }
        report
    }
}

impl Default for KkpvBlockStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kk_schema::kkpv::{KkpvBlockInput, KkpvObject, KkpvValue};

    fn epoch(tick: u64, generation: u64) -> RunEpoch {
        RunEpoch { tick, generation }
    }

    fn block_bytes(tick: u64, marker: i64) -> Arc<[u8]> {
        let input = KkpvBlockInput {
            private: true,
            tick,
            generation: 1,
            plan_hash: [7; 32],
            audience_hash: [9; 32],
            room_name: "W1N1".into(),
            terrain: None,
            objects: vec![KkpvObject {
                id: 1,
                type_key: "t:a".into(),
                fields: vec![("marker".to_string(), KkpvValue::Int(marker))],
            }],
        };
        kk_schema::kkpv::encode(&input, &KkpvLimits::default())
            .expect("测试块编码成功")
            .into()
    }

    #[test]
    fn publish_validates_and_returns_handle() {
        let store = KkpvBlockStore::new();
        let bytes = block_bytes(1, 5);
        let handle = store
            .publish(epoch(1, 1), bytes, &KkpvLimits::default())
            .expect("合法块可发布");
        assert_eq!(handle.epoch(), epoch(1, 1));
        assert!(handle.byte_len() > kk_schema::kkpv::HEADER_LEN);
        let view = handle.view().expect("视图");
        assert_eq!(view.tick(), 1);
        assert_eq!(view.field(0, "marker"), Ok(Some(KkpvValue::Int(5))));

        let garbage: Arc<[u8]> = vec![0u8; 200].into();
        assert!(
            store
                .publish(epoch(2, 1), garbage, &KkpvLimits::default())
                .is_err(),
            "畸形块不得入库"
        );
        let mut truncated = block_bytes(2, 6).to_vec();
        truncated.pop();
        let truncated: Arc<[u8]> = truncated.into();
        assert!(
            store
                .publish(epoch(2, 1), truncated, &KkpvLimits::default())
                .is_err(),
            "截断块不得入库"
        );
        // 块头身份必须与发布代际一致：块内 tick=2 不能以代际 (3,1) 发布。
        assert!(
            store
                .publish(epoch(3, 1), block_bytes(2, 7), &KkpvLimits::default())
                .is_err(),
            "块头 tick 与发布代际不符不得入库"
        );
        assert!(
            store
                .publish(epoch(1, 1), block_bytes(1, 7), &KkpvLimits::default())
                .is_err(),
            "同代际重复发布拒绝"
        );
        assert!(
            store
                .publish(epoch(0, 9), block_bytes(0, 8), &KkpvLimits::default())
                .is_err(),
            "代际回退拒绝"
        );
        let report = store.retention_report();
        assert_eq!(report.len(), 1, "失败的发布不改变库状态");
    }

    #[test]
    fn acquire_hits_only_current_epoch() {
        let store = KkpvBlockStore::new();
        store
            .publish(epoch(5, 1), block_bytes(5, 1), &KkpvLimits::default())
            .expect("发布");
        assert!(store.acquire(epoch(5, 1)).is_some());
        store
            .publish(epoch(6, 1), block_bytes(6, 2), &KkpvLimits::default())
            .expect("发布");
        assert!(store.acquire(epoch(5, 1)).is_none(), "旧代际请求返回 None");
        assert!(store.acquire(epoch(6, 1)).is_some());
        assert!(store.acquire(epoch(7, 1)).is_none(), "未发布代际返回 None");
    }

    /// 慢工作者持有旧句柄时旧块保留且内容不变；释放后随下次发布回收。
    #[test]
    fn slow_worker_handle_retains_old_block() {
        let store = KkpvBlockStore::new();
        store
            .publish(epoch(1, 1), block_bytes(1, 11), &KkpvLimits::default())
            .expect("发布");
        let old_handle = store.acquire(epoch(1, 1)).expect("当前代际句柄");
        store
            .publish(epoch(2, 1), block_bytes(2, 22), &KkpvLimits::default())
            .expect("发布");
        let report = store.retention_report();
        assert_eq!(report.len(), 2, "旧代际因句柄仍被保留");
        assert_eq!(report[0].epoch, epoch(1, 1));
        assert_eq!(report[0].handles, 1, "慢工作者的一枚句柄");
        assert_eq!(report[1].epoch, epoch(2, 1));
        // 旧句柄读取的是旧代际内容；新版本未修改旧块。
        let old_view = old_handle.view().expect("旧块仍可解码");
        assert_eq!(old_view.tick(), 1);
        assert_eq!(old_view.field(0, "marker"), Ok(Some(KkpvValue::Int(11))));
        assert!(store.acquire(epoch(1, 1)).is_none(), "旧代际不可再获取");

        drop(old_handle);
        store
            .publish(epoch(3, 1), block_bytes(3, 33), &KkpvLimits::default())
            .expect("发布");
        let report = store.retention_report();
        assert_eq!(
            report.iter().map(|r| r.epoch).collect::<Vec<_>>(),
            vec![epoch(3, 1)],
            "句柄释放后旧代际被回收；无句柄的过渡代际由当前块转为旧块列表时一并回收"
        );
        assert_eq!(report[0].handles, 0);
    }

    /// 句柄不依赖库存活：库销毁后仍可读取不可变块。
    #[test]
    fn handle_outlives_store() {
        let store = KkpvBlockStore::new();
        let handle = store
            .publish(epoch(1, 1), block_bytes(1, 42), &KkpvLimits::default())
            .expect("发布");
        drop(store);
        let view = handle.view().expect("库销毁后块仍可读");
        assert_eq!(view.field(0, "marker"), Ok(Some(KkpvValue::Int(42))));
    }

    fn bundle_bytes(tick: u64, generation: u64) -> Vec<u8> {
        let block = |room: &str| KkpvBlockInput {
            private: false,
            tick,
            generation,
            plan_hash: [7; 32],
            audience_hash: [9; 32],
            room_name: room.into(),
            terrain: Some(vec![3u8; 64]),
            objects: Vec::new(),
        };
        kk_schema::kkpv::encode_bundle(&[block("W1N1"), block("E2N2")], &KkpvLimits::default())
            .expect("测试束编码成功")
    }

    /// 共享束发布：校验、代际一致、句柄共享与保留报告。
    #[test]
    fn publish_shared_validates_and_retains() {
        let store = KkpvBlockStore::new();
        let handle = store
            .publish_shared(epoch(4, 2), bundle_bytes(4, 2), &KkpvLimits::default())
            .expect("合法共享束可发布");
        let bundle = handle.shared_bundle().expect("共享束载荷");
        assert!(bundle.byte_len() > 160, "束字节随句柄可取");
        assert!(handle.view().is_err(), "共享束不提供单块视图");
        // 束版本与发布代际不符拒绝；截断束拒绝；单块字节按束框架拒绝。
        assert!(
            store
                .publish_shared(epoch(5, 2), bundle_bytes(9, 2), &KkpvLimits::default())
                .is_err()
        );
        let mut truncated = bundle_bytes(5, 2);
        truncated.pop();
        assert!(
            store
                .publish_shared(epoch(5, 2), truncated, &KkpvLimits::default())
                .is_err()
        );
        let single = block_bytes(5, 5).to_vec();
        assert!(
            store
                .publish_shared(epoch(5, 2), single, &KkpvLimits::default())
                .is_err(),
            "单块字节不是束框架"
        );
        let report = store.retention_report();
        assert_eq!(report.len(), 1, "失败发布不改库状态");

        // 代际前进：旧句柄保留旧束；保留报告列出两代。
        let second = store
            .publish_shared(epoch(5, 2), bundle_bytes(5, 2), &KkpvLimits::default())
            .expect("代际前进可发布");
        let report = store.retention_report();
        assert_eq!(report.len(), 2, "旧代际因句柄仍保留");
        assert_eq!(report[0].epoch, epoch(4, 2));
        assert_eq!(
            handle.shared_bundle().unwrap().byte_len(),
            bundle_bytes(4, 2).len()
        );
        assert_ne!(handle.shared_bundle().unwrap().byte_len(), 0);
        let _ = second;
        // 同代际重复发布拒绝（与单块口径一致）。
        assert!(
            store
                .publish_shared(epoch(5, 2), bundle_bytes(5, 2), &KkpvLimits::default())
                .is_err()
        );
    }
}
