//! 安装候选独占全部派生域；返回错误时不触碰已安装引擎。
use crate::settle::{Engine, contrib::ContributionDispatch};
use kk_mod_api::buildings::BuildingCatalog;
use kk_mod_api::builtin::{CombatRanges, LinkRules, MoveGate, StoreRules, TerrainPassability};
use kk_mod_api::extension::ExtensionCatalog;
use kk_mod_api::resources::ResourceCatalog;
use kk_mod_api::{ContributionPlan, Key};
use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::Arc;

/// 只有所有装配步骤成功后才能构造；发布不再进行可失败的校验。
pub(super) struct Installation {
    plan: Arc<ContributionPlan>,
    links: LinkRules,
    stores: StoreRules,
    combat: CombatRanges,
    move_gate: MoveGate,
    terrain_passability: TerrainPassability,
    resources: Rc<ResourceCatalog>,
    buildings: Rc<BuildingCatalog>,
    contrib: Rc<ContributionDispatch>,
    extensions: Rc<ExtensionCatalog>,
}

impl Installation {
    /// 保留原检查顺序：规则/目录 → 执行表 → 扩展目录 → 安装绑定。
    /// 执行表装配只读取当前候选计划与已提供函数，不访问引擎状态。
    pub(super) fn prepare(
        plan: Arc<ContributionPlan>,
        mut installed: BTreeSet<Key>,
        assemble: impl FnOnce(&ContributionPlan, &BTreeSet<Key>) -> Result<ContributionDispatch, String>,
    ) -> Result<Self, String> {
        let links = LinkRules::from_plan(plan.rules())?;
        let stores = StoreRules::from_plan(plan.rules())?;
        let combat = CombatRanges::from_plan(plan.rules())?;
        let move_gate = MoveGate::from_plan(plan.rules())?;
        let terrain_passability = TerrainPassability::from_plan(plan.rules())?;
        let resources = Rc::new(ResourceCatalog::from_plan(&plan));
        let buildings = Rc::new(
            BuildingCatalog::from_plan(&plan)
                .map_err(|error| format!("建筑目录装配失败：{error}"))?,
        );
        let contrib = Rc::new(assemble(&plan, &installed)?);
        let extensions = Rc::new(ExtensionCatalog::from_plan(&plan)?);
        installed.extend(extensions.field_bindings().cloned());
        // Mod 类型字段绑定由 payload 存储与写入门实现（创建/处理器回写/
        // 发布前校验）；建筑字段绑定由完工默认值与生命周期写面实现。
        for ty in plan.projection().types() {
            if ty.descriptor.key.namespace() != "builtin" {
                installed.extend(
                    ty.fields
                        .iter()
                        .map(|field| field.descriptor.binding.clone()),
                );
            }
        }
        for building in plan.buildings() {
            installed.extend(building.fields.iter().map(|field| field.binding.clone()));
        }
        Engine::check_installed_bindings(&plan, &installed)?;
        Ok(Self {
            plan,
            links,
            stores,
            combat,
            move_gate,
            terrain_passability,
            resources,
            buildings,
            contrib,
            extensions,
        })
    }

    /// 同步安装的唯一发布点；不改变世界、玩家、市场或 Mod 状态。
    pub(super) fn publish(self, engine: &mut Engine) {
        engine.links = self.links;
        engine.stores = self.stores;
        engine.combat = self.combat;
        engine.move_gate = self.move_gate;
        engine.terrain_passability = self.terrain_passability;
        engine.resources = self.resources;
        engine.buildings = self.buildings;
        engine.contrib = self.contrib;
        engine.extensions = self.extensions;
        engine.plan = self.plan;
    }
}
