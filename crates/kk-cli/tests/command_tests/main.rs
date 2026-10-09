//! 真实 CLI 命令契约共用一个测试入口，减少测试入口的重复链接和启动。
//! 各模块仍自行持有夹具与子进程；sim 场景保留独立目标供定向验证。
mod embed_rpc_tests;
mod mod_init_tests;
mod mod_inspect_tests;
mod mod_output_guard_tests;
mod mod_test_tests;
mod mod_types_tests;

mod mod_services_tests;
