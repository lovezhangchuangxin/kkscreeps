//! 合并同版本的公开块与私有覆盖视图。
use super::*;

// —— 公开/私有合并 ——————————————————————————————————————————

/// 公开块与私有 overlay 的合并视图。对象集合取并集并保持 ID 升序；
/// 地形取公开块。两个块须来自相同 tick/generation/计划与房间。
pub struct MergedView<'v> {
    public: &'v KkpvView<'v>,
    overlay: &'v KkpvView<'v>,
    entries: Vec<(u64, bool, usize)>,
}

/// 合并同一世界版本下的公开块与私有 overlay。
///
/// # Errors
/// 块身份（tick/generation/计划/房间）不一致、双方 flags 不是
/// 一公开一私有，或对象 ID 同时出现在两个块中时返回错误。
pub fn merge<'v>(
    public: &'v KkpvView<'v>,
    overlay: &'v KkpvView<'v>,
) -> Result<MergedView<'v>, KkpvError> {
    if public.is_private() || !overlay.is_private() {
        return Err(KkpvError::MergeFlags {
            base: if public.is_private() { FLAG_PRIVATE } else { 0 },
            overlay: if overlay.is_private() {
                FLAG_PRIVATE
            } else {
                0
            },
        });
    }
    if public.tick() != overlay.tick() {
        return Err(KkpvError::MergeTickMismatch {
            base: public.tick(),
            overlay: overlay.tick(),
        });
    }
    if public.generation() != overlay.generation() {
        return Err(KkpvError::MergeGenerationMismatch {
            base: public.generation(),
            overlay: overlay.generation(),
        });
    }
    if public.plan_hash() != overlay.plan_hash() {
        return Err(KkpvError::MergePlanMismatch);
    }
    if public.room_name() != overlay.room_name() {
        return Err(KkpvError::MergeRoomMismatch {
            base: public.room_name().into(),
            overlay: overlay.room_name().into(),
        });
    }
    let mut entries = Vec::with_capacity(public.object_count() + overlay.object_count());
    let mut i = 0;
    let mut j = 0;
    while i < public.object_count() && j < overlay.object_count() {
        let a = public.object_id(i);
        let b = overlay.object_id(j);
        match a.cmp(&b) {
            std::cmp::Ordering::Less => {
                entries.push((a, false, i));
                i += 1;
            }
            std::cmp::Ordering::Greater => {
                entries.push((b, true, j));
                j += 1;
            }
            std::cmp::Ordering::Equal => return Err(KkpvError::MergeObjectConflict(a)),
        }
    }
    for i in i..public.object_count() {
        entries.push((public.object_id(i), false, i));
    }
    for j in j..overlay.object_count() {
        entries.push((overlay.object_id(j), true, j));
    }
    Ok(MergedView {
        public,
        overlay,
        entries,
    })
}

impl MergedView<'_> {
    pub fn object_count(&self) -> usize {
        self.entries.len()
    }

    pub fn object_id(&self, index: usize) -> u64 {
        self.entries[index].0
    }

    /// 对象来源：false 为公开块，true 为私有 overlay。
    pub fn from_overlay(&self, index: usize) -> bool {
        self.entries[index].1
    }

    pub fn tick(&self) -> u64 {
        self.public.tick()
    }

    pub fn generation(&self) -> u64 {
        self.public.generation()
    }

    pub fn plan_hash(&self) -> [u8; 32] {
        self.public.plan_hash()
    }

    pub fn room_name(&self) -> &str {
        self.public.room_name()
    }

    pub fn terrain(&self) -> Option<&[u8]> {
        self.public.terrain()
    }

    /// 读取对象的字段值，语义与 [`KkpvView::field`] 一致。
    ///
    /// # Errors
    /// 变长值解码失败时返回错误。
    ///
    /// # Panics
    /// index 越界时 panic。
    pub fn field(&self, index: usize, key: &str) -> Result<Option<KkpvValue>, KkpvError> {
        let (_, from_overlay, source) = self.entries[index];
        if from_overlay {
            self.overlay.field(source, key)
        } else {
            self.public.field(source, key)
        }
    }

    /// 列出对象的未缺失字段键。
    ///
    /// # Panics
    /// index 越界时 panic。
    pub fn field_keys(&self, index: usize) -> Vec<&str> {
        let (_, from_overlay, source) = self.entries[index];
        if from_overlay {
            self.overlay.field_keys(source)
        } else {
            self.public.field_keys(source)
        }
    }

    pub fn object_type(&self, index: usize) -> &str {
        let (_, from_overlay, source) = self.entries[index];
        if from_overlay {
            self.overlay.object_type(source)
        } else {
            self.public.object_type(source)
        }
    }
}
