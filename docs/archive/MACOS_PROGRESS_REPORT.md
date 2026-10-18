# macOS 实现与性能问题交接报告

日期：2026-10-17。实现提交：`c1c29d2`（基于 `c847079`）。本地 `main`，未推送远程。

## 结论与本次交接范围

**整体未验收：性能 FAIL，Windows 实体双向互通 BLOCKED。**

macOS 原生链路已经可运行，有真实 Finder 粘贴、哈希、安全 IPC、故障清理等功能证据。但最终 1GiB 的本机 TLS loopback 首次 fetch 用时 **383.093 秒，约 2.67MiB/s**；64MiB 用时 **23.942 秒，约 2.67MiB/s**。这个速度不可接受。此前以功能 PASS 收尾不正确，现已更正 `CONTINUE.md` 与 `MACOS_RESULTS.md`。

用户要求停止在此继续调优，先提交现有实现，由主项目维护者统筹后续。用户指出同一 APFS 卷本地交付应使用 clone，避免可感知的整文件复制等待。当前没有独立测量同卷 APFS clone 的端到端延迟，也没有建立任何可宣称“近瞬时”的本地交付性能证据。不要把 383 秒说成 APFS clone 的速度，也不要把哈希通过或第二次缓存命中说成性能达标。

共享 Core/TLS/IPC 与 macOS Swift 解码都在慢链路上；**尚无分层 profile，根因未定**。本次最后只阅读了相关实现，没有实施新的性能修复。

## 仓库、机器及接手方式

- SSH 别名：`alarics-macbook-air`。
- Mac 仓库：`/Users/myhades/Documents/GitHub/rustboard`。
- POC：`/Users/myhades/Documents/Codex/Misc/lazy_paste`，已只读检查；未修改其源码、身份、应用或域。Windows 不能假设直接读取此路径。
- 先阅读 README、ARCHITECTURE、PROTOCOL、SECURITY、docs/MACOS_HANDOFF、CONTINUE、tests/protocol/vectors.json，再阅读本报告与 docs/MACOS.md。
- 已有提交保存在 Mac 本地；没有 GitHub push、PR 或发布包。Windows 维护者如需取回，可在自己的仓库执行下面的只读 fetch，再自行审阅、合并或 cherry-pick。不要覆盖双方未提交工作。

```sh
git fetch alarics-macbook-air:/Users/myhades/Documents/GitHub/rustboard main
git log --oneline HEAD..FETCH_HEAD
git diff HEAD...FETCH_HEAD
```

报告自身在实现提交之后单独提交；读取 Mac 的 `git log -2 --oneline` 可见两个提交。

## 实现清单与修改边界

| 路径 | 已实现内容 |
|---|---|
| crates/app/src/macos/mod.rs | AppKit主线程泵、文本/文件捕获发布、origin序号抑制、宿主调用、撤回与过期处理 |
| crates/app/src/macos/local_files.rs | APFS fclonefileat 稳定快照、dataless与symlink拒绝、受保护快照目录、释放/启动清理；没有整文件字节复制回退 |
| crates/app/src/macos/ipc.rs | 用户级Unix socket、0700目录/0600 socket、单实例锁、UID/audit token/同team与明确bundle ID认证、最多8个客户端、超时与顺序v1请求 |
| platforms/macos/native/Bridge.swift | AppKit桥接，文本与URL元数据，不读文件主体；发布序号与完整URL数量验证 |
| platforms/macos/native/main.swift | 最小sandbox .app宿主，隐藏domain、元数据刷新/撤回、持有原始security-scoped URL发布剪贴板 |
| platforms/macos/native/Provider.swift | NSFileProviderReplicatedExtension、四元组Item ID、懒元数据、fetchContents、取消、枚举与有界anchor |
| platforms/macos/native/Wire.swift / Security.swift | 共享v1 get_offer/read/cancel、严格字段与UInt64解析、双向本地认证、64KiB块读取、受保护临时文件及失败清理 |
| platforms/macos/native/Tests | Swift协议/故障/元数据/AppKit测试、注入式大文件校验和合成剪贴板fixture |
| platforms/macos/scripts / platforms/macos/packaging | 构建、测试、Apple签名、有限时签名会话、安装卸载、launchd、包及Homebrew模板、复测工具 |
| crates/app/examples/macos | 真实TLS合成loopback对端，不是Windows对端 |
| CLI、rustboardd、platform lib/build.rs、Cargo.lock | macOS入口与Swift桥接构建；平台libc依赖 |

**共享变更只有内部连接复用优化及其测试，不是协议变更：** `crates/core/src/transport.rs` 的 Node.local_ipc 按 `(source_device, offer_id, file_id, version)` 保留最多2个TLS连接，无主体缓存。每块仍执行授权、当前Offer、版本等检查；EOF/cancel释放缓存，断线不伪造成功。RemoteSource连接改为共享引用，避免每块重连及Arc循环。主维护者应复核此处并在Windows重新跑回归；没有在Windows构建验证本次共享变更。

`PROTOCOL.md`、`tests/protocol/vectors.json` 没有更改。不得为了适配POC临时设计引入第二套网络或暗改编码；若最终性能优化确需新协议，先提出主仓库协议、兼容策略和共享向量变更。

## 真实功能验收证据

原始紧凑记录在 `docs/test-results/macos/`，完整状态矩阵见 `MACOS_RESULTS.md`。早期 FAIL 记录有意保留，不能把每份历史日志都当作最终构建结果。

| 项目 | 状态与证据 |
|---|---|
| Rust workspace、Python共享向量、Swift兼容与故障测试、fmt/clippy | PASS：check-final.txt、rust-tests.txt、python-vectors.txt、swift-vectors.txt、provider-metadata.txt |
| 最终原生三文件Finder Cmd+V | 功能PASS、性能FAIL：release-finder-hash.txt、release-provider.txt；4KiB/64MiB/1GiB全部匹配共享哈希 |
| 发布前不读主体 | PASS：release-before-reads.json全0；release-before-stat.txt均dataless、blocks=0；release-clipboard.txt引用数3 |
| 同Offer重复Finder粘贴 | PASS：release-repeat-hash.txt；release-repeat-reads.json与release-after-reads.json相同，是OS缓存命中，不是第二次网络下载 |
| Agent launchd重启后的真实fetch | PASS：最终PID84423；release-build.json；不是整机重启 |
| 本机TLS双向Unicode文本 | PASS：text-from-peer.txt、text-to-peer.txt；不是Windows实体双向 |
| Finder Cmd+C本地文件→TLS对端 | PASS 4KiB：local-source-before-read.json、local-source-tls.txt；先元数据，再改写原文件，之后对端读取仍得到原始快照哈希。没有完成反向64MiB/1GiB实体测试 |
| Finder Stop / pause | PASS先前构建：finder-cancel-provider.txt、finder-pause-*；失败、读取停止、目标为空 |
| 到期 / Offer替换 / TLS断线 | PASS先前构建：finder-expiry*、finder-replaced*、finder-disconnect*；目标与pending为空。Offer替换不等于同Offer version原地改变 |
| Provider中途SIGKILL | PASS先前构建：provider-kill-before.json、provider-restart*；遗留pending回收，系统重试64MiB哈希正确 |
| 认证与权限拒绝 | PASS已测范围：ipc-security*；普通同UID Python被拒绝，签名CLI/宿主可用。第二账户攻击测试NOT_TESTED |
| 用户安装/重复安装/卸载 | PASS先前构建：install-lifecycle.txt；修复domain显示名冲突 |
| Homebrew生命周期 | PASS先前构建：homebrew.txt；临时本地tap install/test/reinstall/uninstall/untap；跨版本upgrade NOT_TESTED |
| 最终签名及本地打包 | PASS：release-build.json、package-final.txt；Apple Development，不是Developer ID公证分发 |
| Windows物理设备双向 | BLOCKED：用户尚未启动对端，没有实际地址/指纹；不能套用原Windows维护者的Explorer结果 |

最终源读取量分别恰好为4096、67108864、1073741824字节。资源采样 `release-full-rss.json` 最终持续599.3秒，采样开始于粘贴之后、覆盖两次大文件fetch；Agent峰值39,878,656 bytes，Provider峰值32,538,624 bytes。是每秒采样最大值，不是连续精确峰值。早期报告曾记录采样尚未结束时的461.8秒，现已更正。

## 关键已修故障与仍需审阅的实现

1. Darwin accept继承非阻塞标记导致认证/分帧竞态：恢复阻塞并限定超时；accept用poll唤醒，连接复用。
2. sandbox内验证非bundle Agent失败：用内核audit token及动态代码guest验证，不通过放宽权限解决。
3. Item ID列表直接作为sync anchor超过系统500-byte限制导致SIGABRT：初始anchor32-byte摘要，change anchor9-byte generation；锁保护的退休元数据账本，上限4096，系统确认后回收。
4. 只读展示导致uchg、重复Finder粘贴报权限错误：展示能力允许写；create/modify/delete仍unsupported，不支持远端编辑。
5. SIGKILL跳过defer遗留临时文件：rb-pending文件flock，commit锁内改名rb-ready；后续fetch只清理未锁住pending。旧崩溃遗留文件单独保留证据，未冒充新清理机制PASS。
6. 文件→文本时旧presented清理擦除新文本：先撤回旧展示，再发布新内容。
7. launchd Agent重建路径URL失去sandbox授权：宿主持有getUserVisibleURL原始scope并发布；Agent采用返回的剪贴板序号。
8. 宿主过早释放scope导致pboard异步拒绝三项中的两项：显式pasteboard item，scope保留0.5秒供服务消费；Agent退出后独立检查序号与完整URL数，部分发布清空并重试。**0.5秒是当前实现的等待窗口，可靠性依赖后续完整性检查；值得后续改为更明确的同步机制，不能只删除检查。**

目前支持已知大小的平面文件；目录/未知大小拒绝。APFS快照不支持的卷拒绝，不回退整文件预复制。OS已打开的缓存句柄无法追溯撤销，不能宣称撤回后所有旧句柄立即不可读。

## 性能问题：已知事实、假设与主项目排查顺序

最终测试实际路径：

```text
合成源 PatternSource
  → Rust Core TLS1.3 loopback（127.0.0.1）
  → Rust Node.local_ipc
  → v1 JSON framed Unix IPC
  → Swift IPCSession / JSONDecoder
  → fetchContents中64KiB写入临时文件
  → File Provider交付 → Finder目的文件
```

1GiB的383.093秒是fetch_start到fetch_complete区间，不包含手工操作等待。它不是两个普通本地APFS文件之间单独clone的计时。首次远端内容必须落盘一次，但同卷本地快照/交付应避免额外O(n)数据复制。APFS clone语义与远端传输性能应分别验收。

**待验证假设，不是定位结论：**

- v1 bytes是JSON数字数组；Rust网络帧解码、构造IPC serde_json::Value、再次序列化，以及Swift `[UInt8]` JSONDecoder解码可能有大开销。
- 每个fetch只有一个64KiB请求在途；1GiB需要16384次串行读。383.093秒折算平均每块约23.4ms，但没有证据证明这是固定等待、解码CPU或往返时延。
- TLS两端已设置TCP_NODELAY，Node已有有界连接缓存；不能把“加nodelay/复用连接”当作尚未尝试的新修复。
- IPC身份认证已按连接复用，不应每块重新认证。需用profile确认，而不是凭代码推测。
- 注入式LargeHydration绕过网络和JSON，不能拿它的速度证明真实链路达标。

建议主维护者先建立分层耗时/吞吐/CPU/RSS基线：

1. 同卷APFS clone及本地交付：不同文件大小、独立验证内容与共享extent语义，确认没有暗中整文件复制；计时中不混入全文件hash。
2. 纯Core TLS连续读取：固定64KiB、同机loopback，测吞吐与每块时间，脱离Swift和Finder。
3. 同一数据经Node.local_ipc/Unix socket：分别计序列化、socket收发和Swift解码时间；采样栈或Instruments，测生产release构建。
4. 完整Finder：在同一机器/同一输入上对比，重复记录零预读、fetch开始结束、源字节数、hash、RSS与CPU。
5. 真实Windows↔Mac：再增加实际网络因素，两个方向独立验证，不把loopback当跨机器证据。

优先优化协议内实现，不能通过预下载、全文件内存缓存、跳过授权/版本检查或移除失败清理换速度。如果确需流水线或编码变更，先由主项目确认协议兼容与共享向量。本报告不预先承诺一个未测量的性能阈值；最终应以本地clone近瞬时、远端吞吐合理接近实测链路能力为验收目标。

## 复现与签名

```sh
cd /Users/myhades/Documents/GitHub/rustboard
platforms/macos/scripts/test.sh
platforms/macos/scripts/build.sh
SIGN_IDENTITY='<本机可用Apple身份>' platforms/macos/scripts/sign.sh
source platforms/macos/scripts/env.sh
cargo build -p rustboard-app --example macos_acceptance_peer --release
python3 platforms/macos/scripts/acceptance.py prepare
python3 platforms/macos/scripts/acceptance.py status
# 记录reads.json为零、CloudStorage本测试域dataless/blocks=0。
# 通过真实Finder在空的test-output/macos目录执行Cmd+V。
python3 platforms/macos/scripts/verify-files.py <实际粘贴目标绝对路径>
python3 platforms/macos/scripts/acceptance.py stop
```

prepare建立隔离acceptance profile、两端loopback TLS及合成文件，不是Windows。Finder首次可能需要启用domain。默认Offer TTL600秒；若慢到过期，应记录失败，不要把重试或缓存命中伪装成首次成功。复测launchd用 `platforms/macos/scripts/test-launchd.py` 及相关脚本；KeepAlive下CLI stop不是永久停止，必须bootout。不要同时启动两个同profile Agent。

当前已签名产物在 `target/macos/`（不入Git）；`release-build.json`含哈希。修改/构建会覆盖签名，必须重新签名后才可原生复测。本机team为A3443J88T5，group为A3443J88T5.org.rustboard.shared；不要将开发者私钥或机器绑定team硬编码进可分发产品。

独立执行会话曾无法访问已锁钥匙串；用户在可用SSH会话启动 `platforms/macos/scripts/sign-session.sh` 后完成签名。它仅前台最多30分钟调用sign.sh，不保存密码、不修改ACL。以后SSH别名可连接不代表该会话私钥已解锁；不得以ad-hoc绕过实际app-group/身份认证验收。开发签名成功不代表公证或其他Mac可安装。

## 交接时运行状态与未完成项

测试peer和Agent已停止，临时org.rustboard.acceptance已bootout，acceptance配对列表为空，隐藏测试domain已移除。没有生产launchd安装。测试输出在被Git忽略的test-output中保留，源代码及约512KiB紧凑证据入Git，构建产物/钥匙串/私钥不入Git。未启动新的性能测量任务。

待完成：性能定位与修复；真实Windows双向文本及4KiB/64MiB/1GiB文件；最终修改的Windows回归；整机登录/重启；第二账户权限拒绝；原生同Offer版本变化；Homebrew跨版本升级；Developer ID公证分发。没有针对这些项目伪造PASS。
