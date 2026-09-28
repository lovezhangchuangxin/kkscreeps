// 适配器能力声明（M0 基线起，随里程碑逐项翻真）。
// 声明与实装面严格一致——terrain 已于 M1 步 5（embed 地形通道）翻真，其余
// 游戏规则能力仍为 false；未实装的接口不冒充（套件据此跳过或标记）。
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
