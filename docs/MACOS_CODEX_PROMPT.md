# 交给 Mac Codex 的开发任务

在现有 Rustboard 主仓库中完成 macOS 原生集成。先阅读 README.md、ARCHITECTURE.md、PROTOCOL.md、SECURITY.md、docs/MACOS_HANDOFF.md、CONTINUE.md 和 tests/protocol/vectors.json。另一台 Windows 的主维护者已实现 Core、TLS1.3双向指纹认证、CLI、Windows OLE，并完成真实 Explorer 本地/经TLS的4KiB、64MiB、1GiB多文件粘贴及哈希验证；不要把这些结果当作 macOS 已通过。

你的机器上原有 `Misc/lazy_paste` POC；先检查可访问性和约定，不要假设 Windows 能读它。复用已验证的 NSFileProviderReplicatedExtension、隐藏 domain、dataless metadata、Finder Cmd+V、fetchContents、APFS clone、Rust本地IPC和launchd机制，但以本仓库协议为唯一契约。

实现范围：

1. `crates/rustboard-platform/src/macos/`：AppKit文本监听/发布、origin回环抑制、文件引用捕获、Rust Agent原生接口和安全IPC。
2. `native/macos/`：Swift File Provider extension和必需的最小.app宿主；签名和app-group sandbox权限。
3. `scripts/macos/`、`packaging/macos/`：构建、测试、签名、用户级launchd、Homebrew包装。
4. 共享v1 IPC的get_offer/read/cancel；调用Rust Node.local_ipc，不自建第二套网络。按四元组映射Item ID，fetchContents时才按64KiB块读取并写入受保护临时文件。取消/断线/过期/版本变更必须失败清理，不返回残缺文件。
5. 原生Finder验收和与Windows真实双向互通。先跑共享Rust和Python向量，再加Swift兼容性测试。记录复制前零主体读取、fetchContents回调、哈希、资源、取消、重复粘贴、重启和权限拒绝。

不要改变共享协议来保留POC临时设计；如确有必要，先提出主仓库协议+向量变更。不要把平台文件放到根目录。不要用重型GUI、预下载全文件、键盘监听或模拟粘贴实现懒粘贴。可以用UI自动化触发真实Finder验收。

自主实现、编译、运行、调试、修复、复测，不仅写骨架。缺少签名、系统授权、第二台机器时具体说明阻塞，不伪造通过。每个阶段更新CONTINUE.md，提供PASS/FAIL/BLOCKED/NOT_TESTED证据。不推送远程仓库，除非用户另外明确授权。
