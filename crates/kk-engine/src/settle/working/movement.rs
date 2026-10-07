//! 移动登记、疲劳及各策略候选的引擎适配。
use super::*;

impl Working {
    /// 累加疲劳，沿拉拽链传到链首。
    /// 负贡献先抵消本体已有疲劳，剩余部分才传给拉者；对象上只保存非负值。
    /// 来源：上游 engine/src/processor/intents/creeps/_add-fatigue.js。
    pub fn add_fatigue(&mut self, id: ObjId, mut d: i64) {
        // 第一次累加时以对象现有疲劳为起点。
        let mut acc = match self.tick_fatigue.get(&id) {
            Some(v) => *v,
            None => self.creep(id).map(|c| c.fatigue as i64).unwrap_or(0),
        };
        if acc > 0 && d < 0 {
            let resting = acc.min(-d);
            acc -= resting;
            d += resting;
            self.tick_fatigue.insert(id, acc);
            if let Some(creep) = self.creep_mut(id) {
                // 与链首转记分支同口径：tick_fatigue 内部可为超 2^32 的
                // i64 累加（代价槽 u32::MAX 覆盖 ×多次转移），回写钳到
                // 字段上限，恢复路径不得让惩罚经回绕蒸发。
                creep.fatigue = acc.clamp(0, u32::MAX as i64) as u32;
            }
            if d == 0 {
                return;
            }
        }
        // 链上的拉者不存在时停止向上查找。
        let head = self.movement.head_puller(&self.room, id);
        let mut acc = match self.tick_fatigue.get(&head) {
            Some(v) => *v,
            None => self.creep(head).map(|c| c.fatigue as i64).unwrap_or(0),
        };
        acc += d;
        self.tick_fatigue.insert(head, acc);
        if let Some(creep) = self.creep_mut(head) {
            // u32 疲劳字段的表示边界：代价槽允许 u32::MAX 覆盖，同 tick
            // 多次转移可超过 2^32，`as u32` 会回绕——钳到字段上限。
            creep.fatigue = acc.clamp(0, u32::MAX as i64) as u32;
        }
    }

    /// 移动槽整链开始前的执行者请求登记快照：请求条目与意图时疲劳。
    /// 正常拒绝时按快照逐项恢复，只触碰本执行者的登记。
    pub fn snapshot_move_requests(
        &self,
        id: ObjId,
    ) -> crate::settle::movement::MoveRequestSnapshot {
        let mut snapshot = self.movement.snapshot_of(id);
        snapshot.old_fatigue = self.old_fatigue.get(&id).copied();
        snapshot
    }

    /// 恢复执行者的请求登记：先移除本执行者的现行登记，再按快照的
    /// 原格与格内登记序回插；执行者已不在候选时不回插请求（避免为
    /// 不存在的对象留下登记），仅恢复疲劳记录。
    pub fn restore_move_requests(
        &mut self,
        id: ObjId,
        snapshot: crate::settle::movement::MoveRequestSnapshot,
    ) {
        self.movement.remove_requests(id);
        if self.creep(id).is_some() {
            for record in snapshot.requests {
                self.movement.restore_request(id, record);
            }
            self.movement
                .mark_registration_blocked(id, snapshot.registration_blocked);
        }
        match snapshot.old_fatigue {
            Some(fatigue) => {
                self.old_fatigue.insert(id, fatigue);
            }
            None => {
                self.old_fatigue.remove(&id);
            }
        }
    }

    /// 在指令处理后仲裁移动冲突，使用此时生效的安全模式主人。
    /// priorities 是 JS 竞争策略的终值候选，空表等同原生默认。
    pub fn rank_movement(&mut self, priorities: &BTreeMap<ObjId, i64>) {
        self.movement.rank_winners(&self.room, priorities);
    }

    /// 仲裁阶段二：复核胜者拉拽配对并预填原生准入终值（取消链消费）；
    /// Native 与 Hosted 准入路径都必须先执行。原生门槛按 Plane 的
    /// 移动玩家门规则判定，与玩家面预检同源。
    pub fn prefill_movement_admissions(&mut self) {
        let gate = self.plane.move_gate;
        self.movement
            .prefill_admissions(&self.room, &self.old_fatigue, &gate);
    }

    /// 准入槽输入构造；仅 Hosted 路径调用，原生路径不为其分配。
    pub fn movement_admission_input(&self) -> kk_mod_api::movement::admission::MoveAdmissionInput {
        self.movement.admission_input(&self.room, &self.old_fatigue)
    }

    /// 原生准入终值表的拷贝；准入槽构造候选初值用。
    pub fn movement_admission_finals(&self) -> BTreeMap<ObjId, bool> {
        self.movement.admission_finals().clone()
    }

    /// 仲裁阶段三：准入槽成功终值整体替换原生预填。
    pub fn apply_movement_admissions(&mut self, finals: BTreeMap<ObjId, bool>) {
        self.movement.apply_admissions(finals);
    }

    /// 预检槽输入构造；全部意图登记完成后、竞争输入构造之前调用。
    /// 仅 Hosted 路径调用，原生路径不为其分配。
    pub fn movement_precheck_input(&self) -> kk_mod_api::movement::precheck::MovePrecheckInput {
        self.movement.precheck_input(&self.room, &self.index)
    }

    /// 拉拽登记槽输入；仅 Hosted 路径按需构造。
    pub fn movement_pull_registration_input(
        &self,
    ) -> kk_mod_api::movement::pull_registration::PullRegistrationInput {
        self.movement.pull_registration_input(&self.room)
    }

    /// 拉拽登记槽成功后的否决集应用；Native/拒绝路径不调用。
    pub fn apply_movement_pull_denies(&mut self, denies: &std::collections::BTreeSet<ObjId>) {
        self.movement.apply_pull_denies(denies);
    }

    /// 代价槽输入；仅 Hosted 路径按需构造（取消链之后、执行前）。
    pub fn movement_cost_input(&self) -> kk_mod_api::movement::move_cost::MoveCostInput {
        self.movement.cost_input(&self.room, &self.index)
    }

    /// 代价槽成功后的覆盖应用；Native/拒绝路径不调用。
    pub fn apply_movement_costs(&mut self, costs: std::collections::BTreeMap<ObjId, u32>) {
        self.movement.apply_costs(costs);
    }

    /// 预检槽决策应用；成功与正常拒绝都调用（拒绝丢弃 JS 放行、只
    /// 兑现登记时原生拒绝），被否决请求从登记矩阵移除。
    pub fn apply_movement_precheck_decisions(
        &mut self,
        decisions: crate::settle::movement::precheck::PrecheckDecisions,
    ) {
        self.movement.apply_precheck_decisions(decisions);
    }

    /// 此刻生效的安全模式主人：控制器处于安全模式时取房主，否则
    /// None。障碍槽输入、链预演与正式取消链共用，规则变更只改此处。
    fn active_safe_mode_user(&self) -> Option<String> {
        self.controller().and_then(|ctl| {
            if ctl.safe_mode > self.time {
                ctl.owner.clone()
            } else {
                None
            }
        })
    }

    /// 障碍槽输入构造；使用与 finish_movement 同时点生效的安全模式
    /// 主人。仅 Hosted 路径调用。
    pub fn movement_obstacle_input(&self) -> kk_mod_api::movement::obstacle::MoveObstacleInput {
        let safe_mode_user = self.active_safe_mode_user();
        self.movement
            .obstacle_input(&self.room, &self.index, safe_mode_user.as_deref())
    }

    /// 障碍槽成功后的追加集并入取消链。
    pub fn apply_movement_obstacle_blocks(&mut self, blocks: BTreeSet<ObjId>) {
        self.movement.apply_extra_blocks(blocks);
    }

    /// 准入槽的只读链预演：按给定准入终值在克隆状态上运行与
    /// `finish_movement` 完全相同的取消链，返回仍为胜者的对象集合。
    /// 安全模式主人与正式仲裁同源；只在准入槽时点调用（障碍追加集
    /// 尚为空，预演结果是最终取消结果的上界）。
    pub fn movement_simulate_survivors(
        &self,
        overrides: &BTreeMap<ObjId, bool>,
    ) -> BTreeSet<ObjId> {
        let safe_mode_user = self.active_safe_mode_user();
        self.movement.simulate_survivors(
            &self.room,
            &self.index,
            safe_mode_user.as_deref(),
            overrides,
        )
    }

    /// 仲裁阶段四：按准入终值取消阻塞链，使用此时生效的安全模式主人。
    pub fn finish_movement(&mut self) {
        let safe_mode_user = self.active_safe_mode_user();
        self.movement
            .cancel_blocked(&self.room, &self.index, safe_mode_user.as_deref());
    }
}
