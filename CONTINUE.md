# Rustboard 续开发记录 — 2026-10-17

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
