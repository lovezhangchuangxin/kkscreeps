//! 寻路模块：通过 FFI 直接复用官方 `driver/native/src/pf.cc` 源码，
//! 保证路径与官方逐点一致（含 tie-breaking）——不自行实现算法（ADR-003）。
//! 实装在 M2；官方源码位置见 ~/Desktop/projects/sources/screeps/。
