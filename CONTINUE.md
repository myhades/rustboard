# Rustboard 续开发记录 — 2026-10-18

## 网络结论更正（2026-10-18）

用户指出 Mac 可直达 Windows 192.168.100.6，重新从 Mac 实测 192.168.100.6:45451 TCP 成功（约46ms）。两端 status 正常、pending_restart=false，配置互指当前地址，无SSH转发。之前端口连接超时只代表当时探测失败，“缺少路由/必须反向转发”属于过度推断，撤回。下文关于当前反向不可达的结论已过时。Handoff尚未真机验收；当前通用剪贴板路径可由用户试验文本，不声称已支持完整桥接。

## 当前收尾结果（覆盖以下阶段记录）

两端 transfer-lifetime 修复版已安装，Mac 用户签名通过，身份/配对保持。Windows 13 core+1 vector+9 platform、Clippy 通过；Mac 同版空闲 TLS 回归和构建通过（此前完整 Rust/Swift 套件通过）。真机 Mac→Windows 进程外 COM：两个 4 KiB，首读256字节后等待7秒、文本覆盖两端剪贴板，再读完原批次，包括覆盖后才打开第二个流，全部字节校验通过。Windows→Mac 用户 Finder Cmd+V：两份4KiB目标内容与哈希通过。文本替换后 Mac presented-batches 自动归零，清理通过。原始证据 docs/test-results/transfer-lifetime/。

仍未覆盖 Finder 复制中途替换、并发取消、Handoff、性能或10G。未在热点传输大测试文件。Handoff仅调研，未加功能或私有API；见 docs/TRANSFER_LIFETIME.md。

临时 SSH 反向转发已停止。Mac 恢复旧监听地址无法绑定，已纠正并成功 reload：Mac listen=10.0.2.2:45452、peer=原192.168.100.6:45451；Windows listen=192.168.100.6:45451、peer=10.0.2.2:45452。两端服务保持启用。Mac主动回连Windows路由不通，需后续网络配置，不能声称当前跨热点双向文件传输可用。旧配置/受保护凭据备份仅在各端忽略的test-output中。不得恢复Mac旧监听地址直到它回到原局域网。

以下最新实测修复与连接章节为本轮过程记录，当前状态以上段为准。

## 最新实测修复（2026-10-18）

两个 4 KiB 真机 COM 探针首读成功；Windows 替换为文本并确认 Mac AppKit 收到后，旧流继续失败（0x8003001E）。ready-1 到 continue-1 相隔 23 秒，发现客户端复用的 TLS 连接已超过服务端 5 秒空闲超时。transport ReadConnection 新增 last_request，超过 4 秒在下次网络读取前重连，保留有界预读缓存、重新 TLS 验证，不自动重试失败请求、不新增协议兼容。Windows core 13+vector1、Clippy、release 构建通过并已安装；Mac 同一真实 TLS 空闲断开回归通过，release 构建完成，仅更新候选 bin，等待用户再次本地 sign.sh。尚未安装这次 Mac 修复/真机复测/提交推送。SSH 转发仍 session 24169，先前 probe session 3951 已失败退出。下一轮用 fresh ready-2/continue-2，需重新 Finder Cmd+C；首读后主动等待至少 6 秒再替换文本并继续，验证修复。所有跨机器主体仍限制两个 4 KiB。

## 最新连接与验收约束（2026-10-18）

Mac 在外地，通过 WireGuard 10.0.2.2 可达。用户明确禁止大文件测试，当前为热点链路。SSH 使用原别名加 -o Hostname=10.0.2.2 -o HostKeyAlias=192.168.100.4，已验证原主机身份。不要修改系统防火墙或跑吞吐测试。只传源码差异（首包 101558 字节），CARGO_NET_OFFLINE=true，Mac 使用现有 release 缓存。

transfer-lifetime/repo 已在 Mac 创建，Rust 13 core + 1 vector + 5 platform 全通过；宿主/扩展构建通过，Swift ProtocolVectors/BinaryFraming/ProviderMetadata 全通过。Mac IPC 旧到期断言已修正为活动批次继续可读、显式撤销后失败。未运行 LargeHydration 或跨热点大文件测试。用户已签名，app/Agent/CLI 严格签名验证通过；两端候选已安装，身份与配对指纹保持不变。reload 已实测。临时测试地址：Windows peer=10.0.2.2:45452；Mac listen=10.0.2.2:45452、peer=127.0.0.1:48551，SSH -R 127.0.0.1:48551:192.168.100.6:45451（当前 exec session 24169）。原配置备份在各自 test-output/transfer-lifetime/evidence/config-before.json，含受保护凭据，不提交。旧宿主 revoke 已在安装前执行。已打开 Mac mac-source，等待用户复制两个 4 KiB 文件进行进程外 COM 中途替换测试。

Windows small_batch_probe 已构建、Clippy 通过：只接受 tiny-a.bin/tiny-b.bin 两份 4096 字节 i%251 合成文件，先读 256 字节，写 ready marker，等 continue marker 后读完并校验两文件。总主体 8192 字节，拒绝其他大小/文件名。该探针测试真实进程外 IDataObject/IStream，不能冒充 Explorer GUI。

Mac -> 192.168.100.6:45451 TCP 探测超时；后续可用临时 SSH 反向转发连接 Windows 原监听地址（仍保留 Rustboard TLS 验证），记录这是转发路径，不是直连或性能验收。签名前后两边原身份/配对必须核对；地址若临时改动需保存原配置以便恢复。未提交推送，Handoff 桥接仍待本轮真机通过后研究。


## 进行中：复制生命周期分离（2026-10-18，未部署/未提交）

当前工作区已实现有界文件批次保留、活动读取续期、按批次查询 IPC、Mac 内部批次目录/延迟驱逐，以及 clock.json 旧整数格式分支清理。reload 也在共享 CLI 中；之前仅 Windows CLI 已安装。详见 docs/TRANSFER_LIFETIME.md。保留现有身份和配对；不要把候选实现标成真机通过。

Windows 13 core + 1 vector + 9 platform 测试、fmt/Clippy 通过，包括 COM 经真实 loopback TLS 在新文件/文本替换后读完两份旧文件。Mac SSH 192.168.100.4:22 连接超时，已请求用户恢复唤醒/网络。尚未构建 Swift、请求本轮签名、更新两端安装或完成 Explorer/Finder 真机回归，因此未提交推送。Mac 恢复后创建独立 transfer-lifetime 构建目录；复用既有 Rust release 缓存，不能覆盖已签名 current-only 目录。用户本地签名后再安装。

这轮真机成功后再研究 Handoff 桥接；用户提出 peers 中同 Apple ID 且启用通用剪贴板的 Mac 最多一台。该限制是部署约束，不是自动识别 Apple ID 或防回环机制。


## 当前实现清理（当前状态，覆盖以下历史记录）

已删除旧 TLS ALPN、JSON 文件数据回退、本地 IPC read/capability 分支、旧配置与 identity/peers 文件迁移、旧 retired 缓存转换，以及旧 CLI 内部入口。仅接受 rustboard/2、read_binary 和当前 schema-2 config。protocol:1 只是当前控制消息结构标记，不表示支持另一套传输。

两端当前身份和配对保持原样；Windows 使用 DPAPI，Mac 使用 Keychain。新安装的显式空 identity 才初始化设备身份，凭据读取失败不会生成替代身份。Mac 当前用户签名构建目录为 test-output/current-only/repo，已安装到日常路径；禁止代理调用个人开发者私钥。

两端 Rust、Swift 帧/协议与故障测试、Windows fmt/Clippy、隔离 CLI 配对回归及安装后双向 Unicode 文本通过。文件原生回归与提交状态以本节后续记录和 docs/PROTOTYPE_RESULTS.md 为准。两边日常服务及登录启动继续启用。

以下章节是按开发阶段保留的历史记录，旧迁移、旧协议、暂停命令、测试进程清理和不推送说明均不代表当前实现或当前用户授权。


## CLI 收敛与交互配对（历史）

两端安装版已更新。公开命令：status/start/stop/restart/config/pair/unpair，标准 --help/--version；status --json 用于脚本。start 立即启动并启用登录启动；stop 立即停止并禁用登录启动，Mac 不再被 KeepAlive 拉起。pause/resume/mode/peers/doctor 不再作为公开命令；mode 在 config 中设置，旧 paused 字段启动时移除。

pair 无参数开启 120 秒临时 TLS 配对入口（同步端口 + 1）；带 IP[:port] 主动连接，省略端口默认 45452。双方核对证书指纹并分别确认后才保存；重启生效。终端失联 10 秒后自动撤销窗口。临时入口不处理剪贴板/文件；原同步 TLS pin 验证未放宽。unpair 必须按地址定位，离线也可修改配置。

Windows 和 Mac 已实测停止/禁用自启、启动/启用、重启。已撤销两端旧配对后通过新流程双机配对，独立核对指纹、双方确认并重启，原身份及配对恢复，双向原生文本通过。Windows 双进程测试覆盖拒绝不保存、双确认、重启前不应用、地址撤销、终端关闭过期。Mac 用户签名目录 test-output/cli-0.1.0/repo；主工作区未改。Mac 构建空间不足时仅删除本轮失败 debug 缓存，并复用 credentials-0.1.0/repo/target 的编译缓存；旧 target/macos 已签名产物保留。

状态不冒充持续连接健康探测；整机重启/登录未测试。两边保持服务和登录启动启用。验收见 docs/test-results/cli/ 和 docs/PROTOTYPE_RESULTS.md。未自动推送。


## 单配置与系统私钥存储（历史）

Windows 正式安装已迁移为 schema-2 config.json，包含 peers 和 DPAPI 加密私钥；原 identity.json / peers.json 已移除，指纹与配对保留。所有配置修改（含 CLI pair/unpair/pause/resume/mode）均需手动重启，返回 restart_required，status/peers 显示运行状态。当前正常同步。

Mac 已由用户本地签名并安装 credentials-0.1.0/repo 版本；schema-2 config 引用登录 Keychain，旧 identity.json / peers.json 已移除，指纹和配对保留。launchd 两次重启读取私钥成功；pause/resume 验证仅重启生效。双向 Unicode TLS 同步及 Windows PNG → Mac File Provider 读取/哈希通过。此前 unsigned SSH 探针 -25308 仅属历史失败，不影响本次签名 Agent 验收。不要访问个人开发者私钥、不要覆盖 Mac 主工作区。详见 docs/CREDENTIAL_STORAGE.md 和 PROTOTYPE_RESULTS.md。


## 0.1.0 prototype 部署（最新，覆盖以下历史运行状态）

已按用户选择安装两边当前用户版本并配置登录启动，保留正式 default 配对。Windows 在 LocalAppData/Rustboard/bin；Mac 在 ~/Applications/Rustboard.app 和 ~/Library/Application Support/Rustboard/bin，由 launchd 运行。不要按历史测试清理步骤停止/撤销这套日常安装。

图片文件和原生图片剪贴板统一作为普通文件传输；不增加原生图片协议、目录递归或编解码依赖。使用/限制见 docs/PROTOTYPE_0_1_0.md；当前安装验收见 docs/PROTOTYPE_RESULTS.md。后台双向文本、PNG/BMP/TIFF、真实 Finder/Explorer 粘贴以及服务重启配置持久化已测；整机重启未测。旧性能数据保留，不作为本轮的新测速。

Mac 构建及签名在 test-output/prototype-0.1.0/repo；用户自行签名，禁止代理调用个人私钥。Mac 主工作区未覆盖。本轮仅本地准备和提交，不自动推送/公开发布。当前默认配置为 mode=all、paused=false，固定局域网监听地址；DHCP 地址改变需更新配置和配对地址。


## 专项性能最新状态（优先于以下历史记录）

2026-10-17：二进制 TLS v2 + 有界 4 MiB 顺序窗口 + 协商二进制本地 IPC 已完成双机原生测试。Mac→Windows Explorer 三文件 26.716 s（旧 Agent 217.532 s）；Windows→Mac Finder 两轮 39.625/39.374 s，所有哈希一致。原生证明与探针分开记录于 docs/PERFORMANCE_LAYERED.md，协议在 docs/TRANSPORT_V2.md。400 Mbps 双向目标尚未达到，10G 未验收。

无收益的 socket 缓冲、双连接分窗、1 MiB IPC 批量实验已撤回。Mac 运行版本仍是用户本地签名的 layered-perf/repo/target/macos；未调用个人私钥。Mac 主工作区未覆盖。本次无推送。双端测试配对已撤销，Agent/诊断进程已停止，测试 domain 已删除，原 Mac Provider 注册已恢复；用户可恢复 iPhone 接力。


## macOS 最新交接状态（优先于下方历史阶段）

功能证据已保存，但整体未验收：**性能 FAIL**（1GiB loopback首次fetch383.093秒，约2.67MiB/s），**Windows实体互通 BLOCKED**。M12的哈希PASS不代表速度达标。用户要求将当前实现本地提交，交由主维护者定位共享TLS/IPC及macOS解码瓶颈；本轮没有实施或验证性能修复。后续完整报告见 `docs/MACOS_PROGRESS_REPORT.md`。禁止自行推送远程。

## 当前状态（原 Windows 阶段历史记录）

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

## macOS M0 — 2026-10-17（本机工作，覆盖旧 M6 的平台阻塞）

- PASS：克隆 `myhades/rustboard` 到 `/Users/myhades/Documents/GitHub/rustboard`；未推送。
- PASS：阅读 ARCHITECTURE、PROTOCOL、SECURITY、MACOS_HANDOFF、CONTINUE、共享 vectors；本机 POC 可访问，阅读其 README、最新交接及原生源码。
- FAIL（交接输入缺项）：当前提交没有 README.md，不假定已读。
- PASS：Python共享向量，证据 `docs/test-results/macos/python-vectors.txt`。Rust基线运行中，证据 `core-initial.txt`。
- POC 是独立本机证据，Windows无访问保证。POC无app-group且编译期capability不满足正式安全契约；不迁移其token或网络协议。
- 检测到 Xcode 27、两个 Apple Development 身份；有身份不等于签名/沙盒已通过。采用实际签名团队前缀 app group，Unix socket 双向 UID + audit-token 代码签名校验；无有效签名时拒绝原生运行。
- NOT_TESTED：Rustboard Finder、签名运行、launchd及跨机器Windows互通。Windows对端尚未提供。

## macOS M1/M2 — 原生实现与独立测试

- PASS：Rust workspace测试、APFS clone稳定源/清理测试、共享 Node.local_ipc offset/重复读/错误版本/范围/过期测试。`docs/test-results/macos/rust-tests.txt`。
- PASS：Swift独立读取原共享向量（没有改协议/向量），Unicode、UInt64、frame bounds、未知字段/非法byte拒绝；hydrate成功、取消、断连、错误offset/request ID/EOF、空进度、版本错误、重启撤回均验证清理。`swift-vectors.txt`。这些是注入传输的故障测试，不是Finder结果。
- PASS：真实 AppKit Unicode发布、changeCount回环抑制、外部文本和文件URL捕获。`appkit.txt`；测试仅使用合成内容。
- PASS：Release Rust CLI/Agent、Swift minimal host和Replicated Extension编译，`build-release.txt`。fmt/clippy见`check.txt`。
- BLOCKED：两个已有Apple Development身份签名均`errSecInternalComponent`，`signing.txt`、`signing-second-identity.txt`。不能凭find-identity结果声称能使用私钥。已告知用户检查钥匙串；未解锁/改私钥ACL、未使用ad-hoc绕过生产认证。
- 已实现：TeamID app group；Unix0600/父目录0700/单实例锁；双向getpeereid+LOCAL_PEERTOKEN+Security requirement；无有效签名拒绝启动。AppKit在主线程；复制仅APFS clone，无全量读回退。Swift使用唯一v1协议和完整四元组，不读取远程路径。
- 已实现：隐藏domain、downloadLazily、只读items、按需64KiB写0600独占临时文件，失败清理；旧发布元数据私有索引、过期/替换/重启驱逐请求。OS缓存驱逐实际效果尚未验证。
- NOT_TESTED/BLOCKED：签名后的socket成功/跨用户拒绝、app-group sandbox、真实Finder、资源峰值、后台hydration、重复粘贴/驱逐、真实取消/重启；需要先解除签名阻塞。Windows双向仍需真实对端，已请求IP和指纹。

## macOS M3 — 运行验收、修复与当前阻塞（后续记录优先）

- PASS：一次图形会话签名成功，`signing-gui.txt`；真实签名CLI→Agent双向认证、普通同UID Python客户端被拒绝、0700/0600，`ipc-security.txt`。这些运行结果来自保留的首次签名版本。
- PASS：首次签名版本访问实际TeamID app group、注册hidden domain；本机TLS peer的4KiB Offer已到达，发布前reads=0，`source-reads-before-paste.json`。不是Windows互通。
- FAIL（实测）：sandbox host验证Agent失败，未进入fetchContents，未完成文件发布。修复为sandbox客户端使用audit token+dynamic code验证无Info.plist的Agent，仍保留Apple anchor/team/bundle requirement；新源码编译通过，运行复测等待签名。
- PASS：真实AppKit测试；Swift 4KiB/64MiB/1GiB hydrate循环+独立SHA256匹配共享向量。修复Foundation autorelease累积后RSS从约1.15GB降到8.5MiB。`large-hydration.txt`、`large-resources.txt`；仅库级注入传输，不是Finder或网络资源验收。
- PASS（首次签名版本）：用户级临时launchd服务实际CLI可用、SIGKILL后PID57597→57609、身份不变；job已bootout。`launchd.txt`。未安装持久登录项，也未声称重启后File Provider fetch通过。
- 已完成用户级安装/卸载/签名/launchd生成、Homebrew本地签名包生成与校验配方，正式运行/安装升级仍待验证。
- BLOCKED（最新构建）：用户在Terminal执行sign.sh同样errSecInternalComponent，无提示。只读SecKeychainGetStatus证实login钥匙串unlocked=false，系统日志NO_USER_INTERACTION。已请求用户在系统钥匙串界面解锁，不索取密码、不改私钥ACL。当前修复构建在target/macos；首次签名构建在target/macos-signed-before-fix，仅供诊断，不当作修复版。
- UI工具因安全限制拒绝访问Terminal；未使用其他工具绕过该拒绝。此前提交的图形签名请求在拒绝前已执行成功。
- BLOCKED：用户确认Windows对端尚未运行；未提供IP/指纹，真实双向互通不能验收。
- 所有测试Agent/peer与临时launchd job已停止，配对测试信任已移除。私有身份、构建/日志保留；原POC不修改。详细矩阵见docs/MACOS_RESULTS.md。

### 解除阻塞后的下一步（不要重复声称已过）

1. 在系统界面解锁login钥匙串，运行`SIGN_IDENTITY=<可用身份> scripts/macos/sign.sh`。
2. 用`scripts/macos/acceptance.py prepare --sizes 4096`重测sandbox IPC和真实Finder；确认getUserVisibleURL只生成dataless metadata且reads=0。
3. 采集fetch_start、真实Cmd+V、多文件三种大小哈希、RSS、取消、断线、过期、重复粘贴/驱逐、Agent重启及权限拒绝；修复直到本机通过。
4. 签名构建上重测launchd+真实fetch、Homebrew安装升级卸载；登录/机器重启需实际会话操作。
5. Windows准备好后核对双方指纹，真实双向互通。未授权远程推送。

## macOS 续测 — 用户解锁后重试

用户已明确表示钥匙串已解锁并要求继续原生验收，允许继续既定签名路线。当前执行会话的只读查询仍为 unlocked=false，sign.sh 返回 errSecInternalComponent（`docs/test-results/macos/signing-current.txt`）。已请求用户在刚才解锁成功的同一个 SSH 会话运行签名脚本；不修改私钥ACL，不传递密码。等待签名产物期间不重建或覆盖二进制。旧“完全不再调用开发者私钥”的约束已由本次用户继续指令更新；签名成功仍需实际验证，不凭解锁回复推定。

## macOS M4 — 当前签名构建开始通过原生链路

- 用户在已解锁的SSH会话成功签名，并启动30分钟前台临时sign-session.sh，以支持本次编译/调试循环；不存密码、不改ACL、不安装签名后台服务。
- PASS：dynamic code修复后sandbox宿主通过IPC认证并取得Offer。此前随机IPC失败进一步定位到Darwin accept继承O_NONBLOCK，显式set_nonblocking(false)修复；连续30次签名CLI请求通过（ipc-repeat.txt）。
- PASS：通过Finder的Enable按钮启用Rustboard测试domain；当前运行扩展路径核对为target/macos，旧诊断构建注册已移除。
- PASS：4KiB真实Finder Cmd+V，fetch_start/fetch_complete日志及SHA256匹配。发布前0 blocks/dataless/源reads=0；粘贴后4096 bytes。证据finder-small-provider.txt、finder-small-hash.txt。
- FAIL后修复中：同一Offer重复Finder粘贴因只读capabilities导致uchg，出现权限错误；与POC已记录现象一致。展示层改为可写能力/POSIX权限，远端内容写回仍明确unsupported（这是导出命名空间，不是编辑同步服务）。当前构建已签名，开始三文件与重复粘贴复测。
- 三文件粘贴前4KiB/64MiB/1GiB均0 blocks、dataless、源读取0（multi-before-stat.txt、multi-before-reads.json）。真实Finder三文件粘贴运行中，尚不记为完成。

## macOS M5 — 原生取消与性能修复

- PASS：真实Finder三文件任务中，4KiB及64MiB fetch_complete；1GiB读到179437568 bytes时点击Finder停止。fetch_failed出现，目的目录恢复为空，源读取停止，扩展无残留打开的主体临时文件（finder-cancel-provider.txt）。整个三文件任务是CANCELLED，不记为哈希通过。
- 旧IPC性能：64MiB fetch约55秒；每块重新认证加20ms accept轮询造成开销。修复为poll唤醒、最多8个带10秒超时的已认证连接、单连接顺序v1请求；每次fetch独占IPCSession。仍逐块调用Node.local_ipc；共享协议与向量不改。
- 修复元数据枚举：sync anchor包含有界Item IDs，后续返回删除和更新，不再每次syncAnchorExpired。旧domain残留将通过新域验收排除；运行结果待验证。
- PASS：当前共享Rust/Python/Swift向量及fmt/clippy，check-current.txt；新增连接实现正在编译签名，尚不记运行通过。

- PASS：优化IPC构建中真实Finder读取1GiB期间pause，源读到66584576 bytes后停止；Finder报-36，目标目录为空，fetch_failed。finder-pause-*.txt/json。
- 进一步确认Node.local_ipc每块重建TLS导致远端50ms accept轮询开销。共享transport内部新增最多2个四元组连接缓存（无主体缓存，EOF/cancel释放）；每次仍检查当前Offer、版本、配对状态，RemoteSource每块检查撤销/过期。协议与向量不变，新增缓存读取+撤销测试通过。

## macOS M6 — 枚举崩溃修复与包装验收

- FAIL定位：先前直接把多个Item ID放入sync anchor超过系统500-byte限制；系统抛NSInternalInconsistencyException并SIGABRT，不是网络错误。anchor-crash.txt保留证据。修复为32-byte SHA256元数据摘要，撤回IDs存0700目录/0600 retired.json，enumerateChanges显式删除。新增256文件回归测试通过（provider-metadata.txt）。
- PASS：当前签名产物临时launchd启动+SIGKILL恢复，PID68802→68822，身份和CLI验证成功，job已bootout（launchd-current.txt）。非完整机器重启，非fetch重启。
- PASS：Homebrew本地临时tap安装、brew test、同版本reinstall、uninstall/untap（homebrew.txt）；未发布tap/包，未改其他tap信任。真正跨版本upgrade仍NOT_TESTED。
- 1GiB传输曾读到71827456 bytes后因上述anchor崩溃失败；不记为PASS。正在用修复构建重新从零验收。

- 撤回ledger改为generation+Item IDs并加flock；只有系统回传已处理的9-byte change anchor时才回收，初始anchor为32-byte摘要。避免长期增长和误把initial anchor当删除确认。
- 检查发现旧崩溃遗留UUID临时文件：显式错误清理有效，但进程被杀没有执行defer。新源码将处理中文件命名rb-pending-*并持有flock，完成时锁内改名rb-ready-*交系统；下一次fetch清除未锁住的pending。新增commit失败、保留活跃锁/删除遗留文件测试通过；当前大文件任务结束后部署实测。
- 本地捕获新增open前dataless检查，避免open本身触发其他Provider的下载；open后仍fstat重检。
- 跨机器联调入口：用户已提供SSH别名 `alarics-macbook-air` 供主维护者后续连接此Mac；这不表示Windows端已启动或互通已通过。

## macOS M7 — 真实三文件与重复粘贴通过

- PASS：Finder真实Cmd+V，4KiB/64MiB/1GiB多文件全部SHA256匹配共享向量（finder-multi-hash.txt、finder-multi-provider.txt）。1GiB fetch 01:41:36→01:47:52；64MiB约24秒；复制前全部dataless/0 blocks、源reads=0（final-before-*）。
- PASS：同一Offer第二次真实Finder粘贴，无权限错误，三份哈希再次匹配（finder-repeat-hash.txt）；reads与首次完成完全相同，是OS缓存命中，不宣称再次网络下载。
- 资源采样：前359秒Agent峰值39272448 bytes，扩展32145408 bytes；采样结束后的快照扩展约32.3MB。不是完整全程精确峰值。源计数最终每文件恰好等于其大小。
- 该轮为generation anchor+TLS缓存版本；pending临时文件重启回收及open前dataless检查的新增修复将随后部署复测。Windows实体双向仍BLOCKED。

## macOS M8 — 故障清理、重启与部署

- PASS：SIGKILL真实Provider中途（17MB pending），系统自动重启，stale_pending_removed count=1，再次完整64MiB fetch并哈希匹配。provider-kill-before.json、provider-restart*.txt；临时目录最终为空。
- PASS：15秒TTL Offer在真实Finder读取期间到期，fetch失败，目标和pending均空，源只读16908288 bytes（finder-expiry*）。
- PASS：真实Finder读取时发布新text Offer，旧读取失败，目标和pending均空（finder-replaced*）。这是Offer替换，不冒充同Offer version原地变化。
- PASS：真实Finder读取时SIGKILL合成TLS对端，错误提示，目标和pending均空（finder-disconnect*）。
- PASS：切换到text后隐藏域空（metadata-after-withdraw.txt），AppKit收到预期Unicode文本（text-from-peer.txt）。
- FAIL→修复：实际install测试发现多个profile使用相同displayName导致默认domain注册NSCocoaError516；非default profile改为Rustboard-<profile>，注册先检查已有域；卸载兼容不存在域；安装等待launchd IPC就绪。首次部分部署已卸载，等待最终签名后复测。

## macOS M9 — 当时阻塞与继续入口（已由 M10/M11 更新）

- PASS：用户级install、同版本重复install、launchd IPC启动、uninstall实测通过，identity稳定，install-lifecycle.txt保留首次516失败和修复后成功。正式登录/整机重启仍未测。
- FAIL定位：launchd后文件→文本切换会在新文本发布后清理旧presented，从而清掉新剪贴板；已将过期/撤回清理移动到incoming发布之前，添加不含内容的完成日志。最终源码build及check-final.txt通过，运行复测尚未完成。
- BLOCKED：用户临时sign-session按设计30分钟退出。最新请求1b92abdc7b674a0584d690dde0ce5b89未执行；最近成功签名是951bda94097d4b77be9781cd269461d6。target/macos当前为最新未签名构建，不可用先前原生结果冒充该修复已经通过。已请求用户在已解锁SSH再启动sign-session.sh。
- launchd真实fetch-after-restart尝试：PID78152→78330、身份保持，但Finder未触发fetch且剪贴板为空；不记PASS，launchd-fetch-restart.json中的fetch仍pending。已bootout临时org.rustboard.acceptance，未留下持久job。
- 待继续：签名后bootstrap target/macos/acceptance-launchd.plist，确认最终剪贴板顺序修复；测试file→text→file，真实Finder4KiB和最终三文件；本机合成文本/4KiB经TLS反向捕获；更新package-final.txt；清理acceptance全部测试配对及peer/domain。Windows真实双向仍因对端未启动BLOCKED。

等待签名期间收尾：合成peer已请求停止；离线acceptance配置仅移除地址127.0.0.1:45442的10个测试pin，其他地址保留；没有运行Agent或持久job。hidden acceptance domain保留但没有可用读取源，待签名后重新prepare。paused-cleanup.json。

## macOS M10 — 签名恢复、launchd沙盒授权修复

- PASS：最新剪贴板清理顺序修复已签名，file→text→file正常，text-order-final.txt。
- 进一步定位launchd URL丢失：CFPasteboard日志显示Agent无法为CloudStorage路径签发sandbox extension；路径字符串重建URL丢失授权。现由最小sandbox宿主持有getUserVisibleURL原始security-scoped URL并发布剪贴板，发布前验证当前Offer和expected changeCount，Rust通过返回序号adopt-owned抑制回环。仍仅共享Node.local_ipc，无新增网络协议。签名24548ca070ef47839e57ee1ae55a67c9成功；AppKit adopt序号测试通过。
- PASS：launchd下真实Finder4KiB fetch及哈希（launchd-fetch-hash.txt、launchd-fetch-provider.txt）。之前失败不是冒充已过，记录保留。
- PASS：真实Finder Cmd+C捕获本地合成4KiB文件，仅元数据先到TLS对端；随后原文件改写，再请求读取仍得到原始pattern SHA256，证明APFS稳定快照（local-source-before-read.json、local-source-tls.txt）。命令行fixture直接写Documents文件URL未获得系统授权，未计作成功；真实Finder提供的文件引用可用。
- PASS：本机AppKit文本经TLS双向合成验证（text-to-peer.txt、text-from-peer.txt）；不是Windows实体互通。最终签名构建的三文件Finder复测运行中。

## macOS M11 — 最终发布完整性回归

- FAIL定位：launchd三文件Offer虽全部dataless，宿主退出后pboard拒绝其中两个sandbox grants，只剩4KiB URL；该次不计三文件通过。之前M7三文件结果保留但不冒充最终发布路径。
- 修复：宿主显式构造每个pasteboard item，保留scope等待异步pboard消费；Agent在宿主退出后独立核验changeCount与全部URL数量，部分发布清空并重试。增加部分URL拒绝的AppKit测试。最终全尺寸复测待运行。

### 下一台 Windows 主维护者继续入口

- SSH 别名：`alarics-macbook-air`；仓库 `/Users/myhades/Documents/GitHub/rustboard`。先读本文件最新阶段、`docs/MACOS_RESULTS.md`、`docs/MACOS.md`；本机改动尚未提交/推送，勿覆盖。
- Windows 尚未启动，必须先启动真实 Windows Agent 并提供监听地址及 `rustboard doctor` 指纹；两边通过可信通道核对后显式pair。合成loopback的pin和127.0.0.1端口不能用于实体互通。
- Mac 构建使用 `scripts/macos/build.sh`、Apple Development签名及最小sandbox宿主；修改后必须重新签名。用户临时sign-session到期后，SSH连接不自动获得私钥可用性，不改ACL、不保存密码。
- 真实双向Unicode文本、Windows Explorer→Mac Finder和Mac Finder→Windows Explorer的4KiB/64MiB/1GiB多文件、两端独立哈希、复制前零读取、取消/断线/重启仍需实体对端证据。不要引用原Windows或本机loopback结果冒充该验收。
- 完整机器登录/重启、第二账户权限拒绝、真实同Offer版本变更、跨版本Homebrew upgrade及Developer ID公证分发仍未验收。

## macOS M12 — 最终本机收尾（本段状态优先）

- PASS：签名请求1b9cb73a817244e2819e925f8a753231；最终AppKit完整数量验证、共享Rust/Python/Swift向量、fmt/clippy通过（check-final.txt、appkit-final.txt）。共享协议及vectors未更改。
- PASS：launchd kickstart后Agent PID84423，真实Finder三文件4KiB/64MiB/1GiB全部SHA256匹配（release-finder-hash.txt、release-provider.txt）。复制前clipboard_file_count=3、全部dataless/blocks=0/源reads=0（release-before-*、release-clipboard.txt）。最终源计数恰好等于各文件大小。
- PASS：最终同Offer重复Finder Cmd+V，三份哈希全部匹配；读取计数不变，是OS缓存命中（release-repeat-*）。
- 资源：599.3秒采样覆盖两次大文件fetch，Agent峰值39878656 bytes、扩展32538624 bytes；1GiB fetch383.093秒，64MiB23.942秒。1秒采样峰值，不宣称连续精确最大值。
- PASS：最终本地签名包生成，package-final.txt；产物SHA256及签名请求release-build.json。先前install/Homebrew lifecycle证据仍有效但记录为先前构建；没有发布包或远程push。
- 清理完成：acceptance测试配对为空、合成peer停止、临时org.rustboard.acceptance已bootout、hidden acceptance domain已移除。无生产launchd安装；测试数据与证据保留。用户前台sign-session可Ctrl-C结束，未修改私钥ACL。
- BLOCKED：Windows真实双向，用户尚未启动对端。NOT_TESTED：整机登录/重启、第二账户拒绝、原生同Offer版本变更、跨版本brew升级、公证分发。已记录SSH别名alarics-macbook-air供主维护者联调。其他详细PASS/FAIL证据见docs/MACOS_RESULTS.md。

## Windows 主仓库接手与性能基线 — 2026-10-17（状态优先）

- 已从 Mac SSH fetch 并快进合入 c1c29d2、10736fa；没有 GitHub push。Mac 工作区与已签名应用未修改。
- PASS：Windows fmt、Clippy、11项 workspace 测试、Release 构建、Python 原始向量。不是新增 Explorer/Finder 或真实双机验收。
- Mac release 单次64MiB测量：Core TLS 56.83MiB/s；Node.local_ipc及JSON序列化42.22MiB/s；独立生产Swift解码器7.97MiB/s。解码开销明显，但完整fetch的2.67MiB/s仍未完全解释。
- 新增可复现分层探针与原始记录，详见 docs/PERFORMANCE_BASELINE.md。没有生产性能修复或协议变更；APFS clone延迟、Unix IPC完整链路及原生交付还需单独测量。
- 整体仍为性能FAIL、实体双向互通未验收。根目录README按用户原有“无需README”要求移除，缺失不是验收失败。

## 性能修复实现与两端诊断 — 2026-10-17

- 已修复共享Rust Data枚举解析的逐字节中间值开销，以及Swift生产IPC的逐字节通用JSON解码开销；不改v1编码/向量、安全认证和按需读取语义。
- Windows Core TLS 64MiB约21.17→47.27MiB/s；真实IDataObject/IStream落盘1GiB19.26秒、53.16MiB/s，零发布读取及独立哈希通过。不是Explorer复测。
- Mac隔离TLS→Unix→Swift→落盘64MiB9.55→1.55秒（两次均使用修复后的Rust，仅切换Swift解码器）；1GiB25.39秒、40.33MiB/s，独立哈希通过。不是签名IPC或File Provider/Finder验收。
- Windows12项、Mac11项Rust测试及各自检查通过；Swift兼容/故障清理/新解码器测试通过；Mac完整修复版已独立构建。
- APFS clone原语独立测量：1GiB约0.149ms/0.088ms，两份clone在源改写后哈希不变。不是File Provider交付延迟。
- 用户选择本地签名。首次通知对应旧目录签名，修复版目录仍待签名；未启动原生测试Agent/domain。详见 docs/PERFORMANCE_FIX.md，后续签名/原生验收记录应追加覆盖此状态。

## 修复版真实Finder验收与后续候选 — 2026-10-17（覆盖前文签名阻塞）

- 用户自行解锁并签署 performance-probe/repo 中的修复版；严格签名校验通过。发现系统起初选中旧扩展，已在读取主体前停止并切换注册，核对实际运行路径后复测。
- PASS：真实File Provider程序读取三文件及哈希。64MiB fetch5.498秒，1GiB88.618秒；这不是Finder操作。
- PASS：重新发布未读取Offer，用户真实Finder Cmd+V，三文件独立哈希通过，最终源读取恰好等于文件大小。64MiB fetch3.593秒、1GiB58.055秒；旧基线23.942/383.093秒。没有把OS缓存命中算首次传输。
- Finder样本内Agent RSS峰值41910272 bytes、Provider29540352 bytes；采样从用户操作之后开始，不宣称完整连续峰值。原始记录 docs/test-results/performance/native-fixed。
- 清理：测试Agent/peer退出、配对撤销、测试domain移除、原扩展注册恢复，45441/45442已无监听。未创建持久登录项。
- 额外优化候选：原生采样定位Agent逐字节Value构造/序列化开销，新增typed IPC响应，输出逐字节等价。当前Windows工作区包含此候选；Windows13项/Mac12项Rust测试和各自检查通过。诊断64MiB1.548→1.259秒。
- 候选独立构建在 performance-probe/typed-repo/target/macos，尚未开发者签名/真实Finder复测。上面的Finder数据对应保留的 performance-probe/repo 已签名版本，不混用。详见 docs/PERFORMANCE_FIX.md。无commit或push。

## 本轮收尾与专项性能目标 — 2026-10-17（最新状态）

- 用户要求先收尾，后续另做专项性能优化。本轮停止追加性能实现，保留已验证修复与 typed IPC 候选，验收边界不变。
- 用户报告 Windows→Mac iperf 约400 Mbps；目标为真实文件交付接近同环境测速上限、本地同盘交付额外耗时可忽略，并以低CPU/低内存扩展到10G级链路。当前尚未证明这些目标。
- 专项范围、分层责任、单位换算、资源指标与验收顺序见 docs/PERFORMANCE_TARGETS.md；最新修复及证据汇总见 docs/PERFORMANCE_FIX.md。
- 不再触发签名/部署/原生测试；保留Mac主工作区、两个隔离构建和测试证据。不推送GitHub。

## 真机双向验收完成 — 2026-10-17（最新状态，覆盖上段暂停安排）

- 用户随后要求先提交，再补齐原生及实体双向缺口。前轮修复已本地提交 `083b9c5`，未push。
- 用户签署 typed-repo 候选及本轮诊断构建；签名校验通过，实际运行路径核对无误。
- 初始脱离SSH会话的Agent主动LAN连接报 `No route to host (65)`；相同签名Agent保持SSH会话运行后读取成功。未改权限或防火墙，后台部署的局域网授权仍须独立验收。
- 两次Finder复制被Mac新Offer撤销：首次用户确认iPhone复制，第二次观察到 `com.apple.is-remote-clipboard`。用户暂关iPhone接力后稳定完成；保留中断证据。
- PASS：真实Windows Explorer→Mac Finder三文件4KiB/64MiB/1GiB全部独立SHA256一致；初始零块。1GiB fetch244.031秒，64MiB15.316秒，4KiB0.087秒。这同时补齐typed IPC候选的签名原生读取。
- PASS：真实Mac Finder→Windows Explorer三文件独立SHA256全部一致；点击前标记至最后大文件可独占打开225.517秒，包含三文件及Explorer开销。
- PASS：双向原生剪贴板Unicode文本，UTF-8逐字节一致。不是文本编辑器UI粘贴。
- 新增Provider阶段/错误码和Agent IPC错误日志；新增现有测试身份/配对的单块TLS诊断example。Windows13项测试/fmt/clippy、Mac12项测试/Swift/fmt/clippy/完整构建通过。
- 清理：两端测试pin清空、Agent退出、45451/45452无监听；Mac测试domain移除，原扩展注册恢复。未安装持久服务，Mac主工作区未改。用户可恢复iPhone接力。
- 证据及边界见 docs/PHYSICAL_INTEROP.md、docs/test-results/interop/。正常三文件双向通过不等于整机重启/完整故障矩阵/近链路吞吐通过；下一轮仍按 docs/PERFORMANCE_TARGETS.md 做专项。无GitHub push。
