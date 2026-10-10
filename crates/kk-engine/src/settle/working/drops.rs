//! 资源掉落、容器填充及同格合并。
use super::*;

impl Working {
    /// 在指定位置放下 energy：先填充存活容器，再合并同资源掉落堆。
    pub fn create_energy(&mut self, x: u8, y: u8, amount: i64) {
        self.create_energy_typed(x, y, amount, 0);
    }

    /// 按旧资源编码放下资源：1 为 power，2 为 G，其余为 energy。
    pub fn create_energy_typed(&mut self, x: u8, y: u8, amount: i64, res_code: u8) {
        let name = match res_code {
            1 => "power",
            2 => kk_core::constants::RESOURCE_GHODIUM,
            _ => "energy",
        };
        self.create_resource(x, y, amount, name);
    }

    /// 放下任意命名资源；非正量不处理。
    /// 同格存活容器先接收，按全部资源合计计算余量；剩余与同格同名堆合并。
    /// 来源：上游 engine/src/processor/intents/_create-energy.js。
    pub fn create_resource(&mut self, x: u8, y: u8, amount: i64, resource: &str) {
        if amount <= 0 {
            return;
        }
        let amount = self.deposit_into_container(x, y, resource, amount);
        if amount <= 0 {
            return;
        }
        let existing = self.index.at(x as usize, y as usize).iter().find_map(|id| {
            match self.room.objects.get(id) {
                Some(RoomObject::Energy(e)) if energy_resource_name(e) == resource => Some(*id),
                _ => None,
            }
        });
        match existing {
            Some(id) => {
                if let Some(RoomObject::Energy(e)) = self.room.objects.get_mut(&id) {
                    // 数量存储为 u32，超出范围时饱和，避免溢出回绕。
                    e.amount = e
                        .amount
                        .saturating_add(u32::try_from(amount).unwrap_or(u32::MAX));
                }
            }
            None => {
                let (res_code, named) = match resource {
                    kk_core::constants::RESOURCE_GHODIUM => (2u8, None),
                    other => (0u8, Some(other.to_string())),
                };
                // 动作到此结束；分配错误由共享候选在发布前报告。
                let _ = self.insert_new(RoomObject::Energy(kk_schema::room::EnergyObj {
                    id: 0,
                    x,
                    y,
                    resource_type: res_code,
                    amount: u32::try_from(amount).unwrap_or(u32::MAX),
                    resource: named,
                }));
            }
        }
    }

    /// 放入同格第一个存活容器，返回未放入的数量。
    /// 容器使用 Generic.data 的 JSON store，容量按全部资源之和计算。
    /// 自动承接沿用 _die/资源创建的全局容量口径，读取冻结规则。
    pub(in crate::settle) fn deposit_into_container(
        &mut self,
        x: u8,
        y: u8,
        resource: &str,
        amount: i64,
    ) -> i64 {
        let container = self
            .index
            .at(x as usize, y as usize)
            .iter()
            .find(|id| {
                self.room.objects.get(id).is_some_and(|o| {
                    matches!(o, RoomObject::Generic(_))
                        && crate::settle::movement::type_name_of(o) == "container"
                        && damage::object_hits(o) > 0
                })
            })
            .copied();
        let Some(cid) = container else {
            return amount;
        };
        let Some(RoomObject::Generic(g)) = self.room.objects.get_mut(&cid) else {
            return amount;
        };
        let mut data: Value = serde_json::from_str(&g.data).unwrap_or_default();
        // 非整数槽位按 0 计入容量，写入槽位时同样按 0 处理。
        let total_before = data
            .pointer("/store")
            .and_then(Value::as_object)
            .map(|s| s.values().map(|v| v.as_i64().unwrap_or(0)).sum::<i64>())
            .unwrap_or(0);
        let headroom = self.plane.stores.container - total_before;
        let taken = amount.min(headroom);
        if taken <= 0 {
            return amount;
        }
        // 缺少 store 时创建空对象；JSON 或 store 类型错误时，保留全部掉落量。
        let Some(obj) = data.as_object_mut() else {
            return amount;
        };
        let store = obj.entry("store".to_string()).or_insert_with(|| json!({}));
        let Some(store) = store.as_object_mut() else {
            return amount;
        };
        let slot = store.entry(resource.to_string()).or_insert(json!(0));
        let v = slot.as_i64().unwrap_or(0);
        *slot = json!(v.saturating_add(taken));
        g.data = data.to_string().into();
        amount - taken
    }
}
