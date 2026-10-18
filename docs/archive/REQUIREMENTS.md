# Rustboard — Windows 主仓库开发任务

你是一名精通 Rust、Windows Win32/COM、异步网络、操作系统原生接口和跨平台系统软件架构的高级工程师。

请在当前工作区建立名为 **Rustboard** 的项目，作为一个真正可长期维护的开源跨平台工具进行开发。

**你是 Windows 端的主力开发者，负责 Rust 核心、网络协议、Windows 平台实现、CLI、工程结构、测试和跨平台集成规范。**

项目的 macOS 原生组件将由另一台 Mac 上的 Codex 独立完成。你必须为其提供明确、稳定、可测试的集成契约。

不要只输出架构文档、代码骨架或 TODO。请自主完成实现、构建、运行、测试、调试、修复和复测。

## 一、产品定义

项目名称：**Rustboard**

Rustboard 是一个 Windows、macOS、Linux 跨平台的本地局域网剪贴板同步工具。

设计目标：

1. Native-first：尽量使用每个平台原生机制。
2. Lightweight：低内存、低 CPU、低磁盘占用。
3. Headless-first：无 GUI 也能完整运行。
4. Peer-to-peer：设备直接通信，不需要第三方服务器。
5. Zero Trust LAN：不能因为处于局域网就信任其他设备。
6. Lazy File Paste：复制文件时只同步文件引用和元数据，真正需要文件内容时才传输。
7. Practical Engineering：不做华而不实的架构，不引入没有必要的抽象和依赖。

禁止使用 Qt、Electron、Tauri、CEF 或其他重型 GUI 框架。

默认不开发 GUI、托盘、剪贴板历史管理器或云同步功能。

允许未来加入极简原生 Utilities 界面，但不是当前开发目标。

## 二、开发环境与权限

当前开发环境为 Windows。

你被明确允许自主安装完成开发所需的免费工具链及依赖，包括：

- Rust stable toolchain / rustup / Cargo。
- MSVC Build Tools 和必要的 Windows SDK。
- Git、CMake、LLVM 或其他确实需要的编译工具。
- 合适的 Cargo 工具、测试及诊断工具。
- 适量且有明确用途的第三方 Rust crates。

优先使用系统已有工具。需要安装时，可以使用 winget、rustup 或官方安装程序。

优先安装精简组件，不必为了编译 Rust 安装完整 Visual Studio IDE。

可以自主下载公开技术文档、依赖和开发工具，无须逐项询问。

但不得关闭 Defender、UAC、防火墙、系统安全策略，不得安装来源不明的软件，不得购买服务，不得推送远程仓库。

如果安装确实需要管理员 UAC 确认、人工处理安装程序或重启 Windows，明确说明操作并暂停；完成后继续执行。

先检查当前工作目录。如果它已经是 Rustboard 仓库，则直接使用；否则在当前工作区创建 `Rustboard/`。不要覆盖其他项目。

## 三、总体架构（必须遵守）

采用：

**Rust Core + Rust CLI + 原生 Platform Backend**

建议两个最终可执行文件：

- `rustboardd`：用户会话级常驻 Agent。
- `rustboard`：CLI，负责配置、配对、控制和诊断。

两者共享 Rust Core。

Windows 和 Linux 平台组件原则上编译进 `rustboardd`，不单独创建 Helper 服务。

macOS 唯一允许的额外原生组件是系统管理的 Swift File Provider Extension，以及必需的最小 `.app` 宿主。

推荐项目结构：

```text
Rustboard/
├── Cargo.toml
├── Cargo.lock
├── crates/
│   ├── rustboard-core/
│   ├── rustboard-app/
│   │   └── src/
│   │       ├── windows/
│   │       ├── macos/
│   │       └── linux/
│   ├── rustboardd/
│   └── rustboard-cli/
├── native/
│   └── macos/
├── tests/
│   ├── protocol/
│   ├── interoperability/
│   └── platform/
├── packaging/
│   ├── windows/
│   ├── macos/
│   └── linux/
├── docs/
├── scripts/
├── README.md
├── ARCHITECTURE.md
├── PROTOCOL.md
├── SECURITY.md
├── TEST_RESULTS.md
└── CONTINUE.md
```

可根据实际需要精简结构。不要为每个结构体创建一个 crate，也不要过度工程化。

架构上至少区分三个职责：

**Clipboard Backend**

监听本地剪贴板变化、读取内容类型和引用、发布远端内容、抑制同步回环。

**File Presentation Backend**

把远端文件引用转化为操作系统认识的文件对象，按需响应系统读取请求。

**Transfer Engine**

负责 Peer 通信、按区间读取、流式传输、取消、版本检查、背压及错误处理。

Rust Core 不应包含 Win32、COM、AppKit、File Provider 或 FUSE 类型。

不要为了形式统一，掩盖平台真实差异。

## 四、统一剪贴板数据模型

设计可版本化的 Clipboard Offer 协议。

至少包含：

- Device ID
- Offer ID
- Event Sequence
- Clipboard Kind
- File ID
- 相对文件名及目录结构
- 文件大小（允许未知）
- 文件类型
- 文件版本标识
- 必要的状态及生命周期信息

首批正式支持：

1. UTF-8 纯文本同步。
2. 单文件元数据同步及懒粘贴。
3. 多文件同步及懒粘贴。

目录复制可以作为后续里程碑，但必须从协议设计阶段考虑路径层级及安全边界。

不要把 Windows 绝对路径直接传给另一端作为文件身份。

文件身份建议采用：

`(source_device, offer_id, file_id, version)`

文件内容统一使用支持 offset 的有界读取接口。Windows IStream 和 Linux FUSE 都可能要求随机访问。

文件大小不得决定常驻内存占用。

复制时禁止为了计算完整哈希而提前读取整个文件。

需要考虑复制后源文件发生变化或消失的情况：没有真实快照保证时，不能静默把变化后的内容当成复制时的版本。

实现剪贴板事件去重、origin 标记、回环抑制、有效期管理与双端同时复制的确定性冲突规则。

## 五、Windows 平台实现（当前最高优先级）

这里必须完成真实 Win32/OLE 验证，而不是仅仅定义接口。

### 5.1 剪贴板监听和文本同步

使用 Windows 原生事件机制，例如 `AddClipboardFormatListener` / `WM_CLIPBOARDUPDATE`。

Agent 使用隐藏消息窗口及消息循环，不显示任何可见 UI 或托盘图标。

支持：

- 检测用户复制的纯文本。
- 提取并转换为统一 Clipboard Offer。
- 通过 Core 接收文本并写入系统剪贴板。
- 防止自己发布的内容重复同步。
- 处理剪贴板被其他进程占用、格式不可用等情况。

### 5.2 文件复制捕获

优先支持 Explorer 复制的 `CF_HDROP` 文件列表，同时检查实际 OLE Clipboard 可用格式，不要假设所有文件来源只提供 CF_HDROP。

提取文件名、数量、大小和必要元数据。

复制阶段不得读取完整文件内容。

第一阶段仅支持 Copy，不实现跨设备 Cut/Move，防止产生数据丢失语义。

### 5.3 原生虚拟文件懒粘贴

使用 Windows OLE Clipboard：

- `IDataObject`
- `CFSTR_FILEDESCRIPTORW`
- `CFSTR_FILECONTENTS`
- `IStream`
- `OleSetClipboard`

正确实现文件描述符、文件编号 `lindex`、`FORMATETC`、`STGMEDIUM`、COM 引用计数和对象生命周期。

让 Explorer 在 Ctrl+V 时真正从 `IStream` 读取文件内容。

支持分块读取、Seek、重复读取、超时、取消以及多文件流。

必须正确处理 STA 线程、COM marshaling、Windows 消息循环和 Tokio 异步任务间的关系。

不能通过阻塞同一个 COM apartment 制造死锁。

不能使用 `OleFlushClipboard` 提前兑现文件内容。

不能监听 Ctrl+V 或模拟键盘事件来伪装实现懒粘贴。

必须验证：

- 普通 Explorer Ctrl+V 是否成功。
- 发布虚拟文件时是否没有读取实际内容。
- Explorer 真正请求数据时才触发 Read。
- 文件哈希是否正确。
- 重复粘贴行为。
- 4 KiB、64 MiB、1 GiB 文件。
- 多文件及错误中断。
- Agent 退出、剪贴板覆盖及流生命周期。

如果某些目标程序不支持 Virtual File Clipboard，如实记录兼容性限制，不要用提前写入完整临时文件冒充通过。

可使用 `windows` crate 和必要的少量原生 FFI。

### 5.4 Windows 常驻方式

`rustboardd` 是用户登录会话中的后台 Agent，而不是 Session 0 Windows Service。

优先实现可控的用户级自动启动方式，无需管理员权限。

日常运行无可见窗口、无托盘图标。

## 六、P2P 网络与安全

设计并实现统一的 Rust 传输层。

首选简单、成熟的方案：

- TCP
- TLS 1.3 / rustls
- 长期设备身份
- 显式设备配对及用户核对指纹
- 经过认证的 Peer 连接
- 有界协议消息
- 流式文件传输

不需要公网中继、STUN/TURN、DHT、区块链或复杂 P2P 框架。

mDNS 可作为后续自动发现能力，但发现不能代表信任。首先支持手动指定 IP。

不允许未经配对的设备访问剪贴板。

所有远程文件读取必须验证授权、Offer ID、File ID 和版本。

禁止任意路径读取、路径穿越及不受控符号链接访问。

实现：

- 文本 Offer
- 文件 Manifest
- File Read Request
- File Data Stream
- Cancel
- Error
- Peer Reconnect
- Offer Expiration

提供确定的协议版本和跨平台序列化格式。

不要把 Rust 内部内存布局、Windows 路径或 COM 结构直接当作网络协议。

设计有限并发、有界缓冲区和背压，避免大文件耗尽内存。

所有传输不得记录用户剪贴板正文和敏感文件内容。

网络不可信是硬约束，不得用开发阶段的明文协议冒充可发行的安全版本。

如需调试协议，可建立严格绑定 localhost、只在测试构建启用的测试模式。

## 七、CLI

实现 `rustboard` 命令行工具，至少提供：

```powershell
rustboard status
rustboard peers
rustboard pair <address>
rustboard unpair <device>
rustboard pause
rustboard resume
rustboard mode text
rustboard mode all
rustboard doctor
rustboard version
```

CLI 通过本地 IPC 操作唯一 Agent 实例，不直接启动第二套剪贴板同步引擎。

Windows 优先使用具备适当访问控制的 Named Pipe，不得建立任意本地用户均可滥用的控制接口。

配置应有合理默认值，支持可诊断的错误信息，不需要 GUI。

## 八、macOS 集成契约（非常重要）

另一台 macOS 已经通过独立 POC 验证：

- `NSFileProviderReplicatedExtension`
- 隐藏 File Provider domain
- dataless 文件元数据
- Finder 原生 Cmd+V
- `fetchContents` 按需触发
- APFS clone
- Rust Agent 本地 IPC 文件读取
- launchd 用户级 Agent 与终止自动重启

原始 macOS POC 位于另一台机器的：

`Misc/lazy_paste`

**不要假设当前 Windows 机器能够访问这份代码，也不要在 Windows 上重新实现 macOS File Provider。**

你的职责是为 Mac Codex 提供明确的集成标准。

至少输出：

`docs/MACOS_HANDOFF.md`

和：

`docs/MACOS_CODEX_PROMPT.md`

内容包括：

1. Rustboard 的目录结构和模块职责。
2. macOS 需要实现的 Platform Backend 接口。
3. 本地 IPC 协议、版本、消息字段及错误码。
4. File Provider Item ID 与 Rustboard File ID 的映射。
5. `fetchContents` 如何向 Rust Agent 读取数据并写入临时文件。
6. 取消、断线、版本失效、缓存及错误处理。
7. App Sandbox 的权限与 IPC 身份验证要求。
8. macOS 构建、打包和测试的接口要求。
9. 双方如何运行协议兼容性测试。
10. Mac Codex 应修改什么、不应修改什么。

提供可执行的跨平台协议测试向量、测试数据和预期哈希。

macOS Codex 应负责 Swift 文件提供者、AppKit 适配、签名和 Homebrew 包装。

Windows Codex 作为 Core 和协议的主要维护者。

如果跨平台集成需要修改协议，先在主仓库修改协议定义及兼容性测试，不允许两端私下形成不兼容版本。

不要为了兼容现有 macOS POC 而永久保留不合理的临时设计；如需变更，应在 handoff 中清楚描述迁移步骤。

## 九、Linux 架构预留

Linux 规划使用：

- X11 Clipboard Selection。
- Wayland Data Control（按 compositor 能力检测）。
- FUSE 虚拟文件系统。
- 标准文件 URI / 文件管理器 MIME 格式。

当前不要求在 Windows 上完成 Linux 原生运行验证。

但 Rust Core 不得依赖 Windows，公共模块必须保持平台无关。

Linux 平台接口可以暂时保留清晰、可编译的条件编译结构，不要堆积无用的空实现。

不能宣称 GNOME Wayland 能无条件后台访问剪贴板。

## 十、执行顺序

按以下里程碑推进，前一个阶段的运行结果将作为下一个阶段的基础。

**M0：环境与工程**

检查并安装工具链，建立 Git 仓库和 Cargo Workspace，完成基础 CI/测试脚本、格式化、Clippy 和单元测试。

**M1：Windows 原生剪贴板**

实现事件监听、文本读写、回环抑制；完成 Windows Explorer 文件复制元数据捕获。

**M2：Windows 原生懒粘贴**

优先用本地测试内容源实现 `IDataObject + IStream`，在 Explorer 中真实 Ctrl+V 并校验文件。证明不提前读取内容。

这是 Windows 平台最重要的 P0。

**M3：Rust Core 与文件传输**

接入正式 Offer、ReadAt、传输状态机、取消、边界检查和缓存策略。

**M4：P2P 与安全**

实现设备配对、TLS、文本同步和远端文件读取。

如果只有一台 Windows 机器，可运行隔离配置的测试 Peer 和本机网络测试，但不能把这种测试标记为真正跨机器互通。

**M5：完整 Windows Agent / CLI**

集成 Windows Clipboard、虚拟文件、P2P 和命令行，验证长期运行、重启、暂停和诊断。

**M6：macOS Handoff**

整理接口规范、测试工具、测试向量和可直接交给 Mac Codex 的集成 Prompt。

以上阶段不要仅创建文件就宣称完成，必须有构建或运行证据。

## 十一、测试与验收

所有能够自动完成的测试都应该自动完成。

对于 Windows Explorer Ctrl+V 等需要真实 GUI 的测试，可以使用合理的自动化工具。如果需要我手动操作，明确说明：

- 运行哪个可执行文件。
- 打开哪个测试目录。
- 需要复制或粘贴什么。
- 预期产生什么结果。
- 日志保存在哪里。

人工操作完成后继续自动验证哈希、回调和时序。

每项验收标记为：

`PASS / FAIL / BLOCKED / NOT_TESTED`

不能把成功编译等同于真实运行成功。

不能把本机模拟 Peer 成功等同于 Windows/macOS 跨机器同步成功。

需要重点测试：

- 复制阶段是否真的零文件主体传输。
- 真实粘贴时是否按需请求内容。
- 文本同步和回环抑制。
- 大文件内存使用是否有界。
- 断线及取消是否清理资源。
- Agent 重启后是否正常恢复。
- 文件被修改或删除后的行为。
- 不可信 Peer 是否被拒绝。
- 多次复制及并发请求。
- 不显示窗口或托盘的正常运行。

记录资源占用与真实吞吐量，不要编造性能指标。

## 十二、开发自主权与限制

你可以自主：

- 查询 API 文档和技术资料。
- 安装开发依赖。
- 选择合适的 Rust crates。
- 调整内部模块结构。
- 编写源码和测试。
- 运行程序、构建及检查。
- 修复编译和运行问题。
- 创建本地 Git commit。

不需要每个开发步骤都询问我。

但必须遵守：

- 不破坏其他项目。
- 不擅自删除用户数据。
- 不关闭系统安全功能。
- 不购买服务。
- 不部署公网服务。
- 不未经同意暴露未经认证的 LAN 接口。
- 不擅自推送 GitHub。
- 不使用重型 GUI 框架。
- 不通过模拟 Ctrl+V 代替原生懒粘贴。
- 不用预下载整个文件伪装按需传输。

遇到真正需要管理员授权、人工 UI、第二台机器或系统重启的步骤时可以暂停，告诉我具体操作。完成后继续。

如果某项技术受 Windows API 限制，应查阅官方资料、设计最小实验并验证。不要无限尝试缺少证据的 workaround。

## 十三、最终交付物

项目最终至少包含：

- 可构建的 Rustboard Cargo Workspace。
- 可运行的 Windows `rustboardd`。
- 可运行的 `rustboard` CLI。
- Windows 原生剪贴板 Backend。
- Windows 虚拟文件懒粘贴 Backend。
- 平台无关 Rust Core。
- 有设备认证的 P2P 协议。
- 自动测试、真实平台验证日志。
- `ARCHITECTURE.md`
- `PROTOCOL.md`
- `SECURITY.md`
- `TEST_RESULTS.md`
- `docs/MACOS_HANDOFF.md`
- `docs/MACOS_CODEX_PROMPT.md`
- `CONTINUE.md`

每个里程碑完成后更新 `CONTINUE.md`，记录完成项、关键设计决策、测试结果、当前阻塞和下一步。

最后必须给出真实验收结果，清楚区分已经完成、需要人工测试以及需要 Mac Codex 配合的部分。

**现在开始。首先检查 Windows 开发环境，安装必要工具，建立 Rustboard 主仓库；然后立即进入 Windows 原生剪贴板及 OLE 虚拟文件懒粘贴的真实实现与验证。不要只完成项目初始化就停止。**