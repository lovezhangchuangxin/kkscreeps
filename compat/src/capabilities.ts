// 适配器能力声明（M0 基线）。
// 全部如实为 false：哑世界没有任何游戏规则能力；随着引擎里程碑推进逐项翻真。
// 字段清单与语义对齐 screeps-ok src/adapter.ts 的 AdapterCapabilities（27 项，
// pin 34a630e 实测；此前误多列 structures/construction 两键，上游不存在）。

export interface AdapterCapabilities {
  chemistry: boolean;
  powerCreeps: boolean;
  powerCreepAccountApi: boolean;
  powerEffects: boolean;
  powerSpawn: boolean;
  factory: boolean;
  terminal: boolean;
  marketBasics: boolean;
  market: boolean;
  terminalSend: boolean;
  observer: boolean;
  nuke: boolean;
  deposit: boolean;
  powerBank: boolean;
  terrain: boolean;
  roomStatus: boolean;
  portals: boolean;
  invaderCore: boolean;
  strongholdDeploy: boolean;
  invaderRaidSpawner: boolean;
  multiShard: boolean;
  interShardMemory: boolean;
  cpuShardLimits: boolean;
  liveWorldSize: boolean;
  actionLogCapture: boolean;
  randomInjection: boolean;
  deprecationNotices: boolean;
}

export const capabilities: AdapterCapabilities = {
  chemistry: false,
  powerCreeps: false,
  powerCreepAccountApi: false,
  powerEffects: false,
  powerSpawn: false,
  factory: false,
  terminal: false,
  marketBasics: false,
  market: false,
  terminalSend: false,
  observer: false,
  nuke: false,
  deposit: false,
  powerBank: false,
  // M1 步 5：setTerrain 严格实装（元素级校验 + 四角墙 + 首 tick 守卫，测试锁定）
  terrain: true,
  roomStatus: false,
  portals: false,
  invaderCore: false,
  strongholdDeploy: false,
  invaderRaidSpawner: false,
  multiShard: false,
  interShardMemory: false,
  cpuShardLimits: false,
  liveWorldSize: false,
  actionLogCapture: false,
  randomInjection: false,
  deprecationNotices: false,
};
