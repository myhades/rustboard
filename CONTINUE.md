# Rustboard 续开发记录 — 2026-10-17

## 当前状态

主仓库根目录是GitHub/Rustboard。共用配置和文档在根目录；平台代码、脚本、探针都在平台子目录。已安装官方Rust stable1.99.1，复用MSVC2022和Windows SDK。未关闭安全设置、修改防火墙、启用登录自动启动或推送远程。

可运行文件：target/release/rustboardd.exe、target/release/rustboard.exe。默认不监听网络，显式--listen IP:port才启用TLS。真实Explorer和隔离loopback Peer测试已完成，测试进程已停止。test-output保留约3.3GiB成功测试内容及少量取消残余，Git忽略；未删除用户数据。

## 里程碑证据

| 阶段 | 当前状态 |
|---|---|
| M0 | PASS：workspace、锁文件、fmt、Clippy -D warnings、11项Rust测试、Release构建；远程CI未运行 |
| M1 | PASS（本机已测范围）：原生事件监听、隐藏窗口、双向TLS Unicode文本、回环抑制、Explorer CF_HDROP多文件清单 |
| M2 | PASS（Explorer）：本地及TLS源4KiB/64MiB/1GiB多文件、零发布时读取、哈希、本地重复粘贴、Seek/Clone、覆盖/退出生命周期、暂停中断 |
| M3 | PASS（已测范围）：Offer、路径/范围校验、ReadAt、过期、确定性冲突、持久时钟、Windows源写删锁、无缓存有界读取 |
| M4 | PASS（同机）：双向TLS1.3指纹认证、未配对拒绝、撤销既有连接的后续读取；跨机器 NOT_TESTED |
| M5 | PASS（基础运行）：单实例当前用户Named Pipe、CLI控制、身份重启持久化、隐藏运行、原始IPC与大帧；长期运行/第二用户ACL/登录启动 NOT_TESTED |
| M6 | PASS（文档和测试交付）：Mac handoff/prompt、JSON向量及Python独立验证；Mac原生集成 BLOCKED（需Mac Codex及其机器） |

详细验收在TEST_RESULTS.md，原始小型证据在docs/test-results。以上不代表生产就绪或跨系统验收通过。

## 关键设计

- ClipboardBackend、FilePresentationBackend、ReadAt/Node分责；Core没有Win32/COM类型。
- TCP + rustls TLS1.3，双端显式核对完整SHA-256证书指纹，无TOFU/明文/未认证配对端点。
- v1网络/本地IPC：4字节BE长度+UTF-8 JSON，Data bytes整数数组；64KiB读取、1MiB帧、256文件、16Peers、8服务会话。
- Lamport计数和watermark持久化；当前Offer/内容/源路径不持久化；默认10分钟过期。
- Windows源句柄拒绝写/删共享，可能阻止用户修改源；此语义必须保留在用户说明中。
- OLE STA保留消息泵，网络/IPC独立线程；同步COM读取有超时，不等待同apartment任务，慢网仍可能延迟STA。
- 不缓存主体，每次粘贴重新读取，只保留最新Offer，不向第三个Peer转发。
- Named Pipe只授权当前用户SID并拒绝远端，first-instance保证唯一实例。同用户恶意进程不在隔离边界内。

## 已修复问题

1. 文本sequence须在CloseClipboard后采样，否则自发布重新发送；真实TLS+metrics复测通过。
2. Windows accept继承nonblocking状态，TLS worker需恢复blocking并等握手完成；认证/拒绝测试通过。
3. Named Pipe前连接关闭与disconnect有短间隙，CLI加入有界重试；独立Python大帧读取通过。
4. COM格式枚举必须区分S_OK/S_FALSE且释放ptd。
5. 暂停、过期、模式切换、unsupported剪贴板覆盖应撤回源；COM对象存活不等于源授权存活。

## 下一步

1. Mac Codex按docs/MACOS_CODEX_PROMPT.md迁移AppKit/File Provider，跑共享向量及Finder验收。
2. 真正两台设备双向配对与传输、断网重连和版本失效；不以loopback替代。
3. 多日浸泡、恶意已配对Peer限速/slowloris/连接耗尽、第二用户和logon SID IPC测试。
4. 完善监听地址/连接错误诊断、持久pause/mode、后台启动失败日志。
5. 评估异步OLE/跨线程流改善慢网STA响应，扩展目标程序兼容性矩阵。
6. 实测登录自动启动，再做Windows签名/发行包装。Linux后续实现X11、compositor能力检测和FUSE。

常规检查：scripts/windows/check.ps1、python scripts/check-vectors.py。原生复现和日志位置见docs/WINDOWS_TESTING.md。禁止预下载主体冒充懒粘贴，禁止两端私自改变共享协议。
