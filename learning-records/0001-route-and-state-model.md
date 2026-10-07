# HTTP 路由与共享状态模型

PFBTD 的请求链是“客户端命令 → HTTP 方法/路径/JSON → Rocket 适配 → `Service::handle` → JSON 响应”。业务状态集中在 `Service` 的 `Mutex<BTreeMap<...>>` 中；Token 检查、文本操作、退出和注销在锁保护下保持原子性，密码哈希则在释放锁后执行，异步服务端再用 `spawn_blocking` 把阻塞计算移出运行时线程。

本阶段的关键决策是：多行输入使用单独一行 `.` 结束，`\.` 表示正文中的句点；客户端纯文本转换放在 `lib.rs`，终端 I/O 留在 `main.rs`，以便测试纯函数。
