# Windows/macOS 真机验收 — 2026-10-17

本轮使用实体 Windows 和 Mac 的局域网地址，双向 TLS 证书指纹配对，独立
`interop-20261017` 测试配置。不是 loopback、SSH 隧道或共享文件夹传输。
三种文件大小为 4 KiB、64 MiB、1 GiB，内容与共享向量一致；两端各自生成真实磁盘源文件。
Windows 运行提交 `083b9c5` 的 release Agent。Mac 运行同一性能候选，另加本轮阶段诊断，
由用户重新签名；实际扩展路径核对为 `performance-probe/typed-repo/target/macos`。

## 已定位的运行环境问题

起初通过 SSH 使用 nohup 启动 Agent 后结束启动会话，Mac 元数据接收成功，但主动读取
Windows 文件失败，Agent 返回 `No route to host (os error 65)`。相同地址在保持会话的
直接网络探针中可达，三文件首块均通过校验。保持 SSH 会话以前台方式运行相同签名 Agent
后，File Provider 4 KiB 读取通过。这是运行上下文差异的实测，不是全局路由中断。

[Apple 的本地网络权限说明](https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy)
区分 CLI/SSH、后台服务和应用的运行上下文。当前结果不能替代正式 launchd 部署的局域网
权限验收；没有修改系统权限或防火墙，也没有关闭 IPC 身份验证。

原错误日志只写 fetch_failed，掩盖了实际失败点。本轮新增 Provider 阶段、底层错误码及
Agent 请求失败原因；网络回复继续使用通用错误，不对远端泄漏内部错误详情。

## 传输期间剪贴板替换

首次保持会话的 Finder 三文件粘贴完成了 4 KiB 和 64 MiB 的 fetch；1 GiB 开始后约
7 秒被 Mac 新本地 Offer 替换。用户确认期间在 iPhone 复制了文本，通用剪贴板将其带到
Mac。Agent 记录 expired，Provider 中止读取；此轮不计完整传输通过。
后续用全新 Offer 重测，要求测试期间三台设备均不再复制其他内容。

第二次用户没有主动复制，但仍观测到 Mac 剪贴板被标记为
`com.apple.is-remote-clipboard` 的 24 字节文本覆盖。未记录文本内容。
用户暂时关闭 iPhone 接力后，再次用全新 Offer 完成了正向验收。

## Windows Explorer → Mac Finder：通过

源是 Windows Explorer 实际复制的三份磁盘文件；用户在 Mac Finder 空目录实际 Cmd+V。
发布后检查三份 File Provider 文件均 dataless/0 blocks，没有先读取主体。
最终三文件独立 SHA-256 与共享向量全部一致。

| 文件 | File Provider fetch 时长 |
| --- | ---: |
| 1 GiB | 244.031 秒 |
| 64 MiB | 15.316 秒 |
| 4 KiB | 0.087 秒 |

此次文件顺序为 1 GiB、64 MiB、4 KiB。1 GiB 约 4.20 MiB/s，尚未接近用户报告的
400 Mbps。上表不是 Finder UI 总耗时。采样从首次 fetch 前开始，276 个约一秒样本内
Agent RSS 最大 36,782,080 bytes，Provider 最大 29,556,736 bytes。
正式性能比较需要独立、重复、受控的专项测量，本次是功能验收运行。

证据：`windows-to-macos-hashes.json`、`finder-success-provider.txt`、
`mac-build-final.json`（均在 `docs/test-results/interop/`）。
这补齐了 typed IPC 候选在签名 Agent/File Provider 和真实 Finder 下的读取验收。

## Mac Finder → Windows Explorer：通过

用户在 Mac Finder 对独立生成的三份本地文件执行 Cmd+C；Windows 使用 Explorer 的
粘贴按钮实际落盘。三份文件关闭后以独占读取方式计算 SHA-256，全部与共享向量一致。
从点击前时间标记到最终 1 GiB 文件可独占打开为 225.517 秒，包含三文件传输、Explorer
开销和约 0.5 秒的检测间隔，不是单独 1 GiB 网络耗时；后续哈希计算不计入该时长。
传输中 Explorer 曾将大文件预分配到 1 GiB 以上，完成后截断为正确大小，未将预分配
长度误判为完成。

442 个约 0.5 秒样本内，Windows Agent 最大工作集 19,415,040 bytes，CPU 时间增量
16.203 秒；不包含 Explorer CPU，也不能与 Mac 源端 CPU 合并解释。
证据：`macos-to-windows-hashes.json`、`explorer-resource-summary.json`。

## 双向文本与清理：通过

使用包含中文和蟹 emoji 的合成文本，分别经 Windows 原生剪贴板和 Mac pbcopy/pbpaste
接口测试，两方向 UTF-8 字节全部一致（记录为 base64）。这是原生剪贴板文本互通测试，
不是文本编辑器 UI 粘贴测试。

验收后两端测试配对清空，Agent 停止，45451/45452 无监听；Mac 测试 domain 删除，
原扩展注册恢复。没有安装持久登录项或修改 Mac 主工作区。测试文件、隔离构建与原始
采样保留在 ignored `test-output/physical-interop`。用户可恢复 iPhone 接力。

本轮指定的 typed IPC 原生读取、Windows Explorer、实体三文件双向和 Unicode 文本
缺口已补齐；正式后台启动/本地网络授权、整机重启、完整故障矩阵和近链路性能仍是
独立验收项，不由本轮正常双向传输结果推定通过。

## 验收边界

首轮失败、中断和最终成功证据保留在 `docs/test-results/interop/`。
Finder fetch 回调时长与整次粘贴目标可用耗时分列；资源采样不是连续精确峰值。
现有本地文件源没有导出每文件完整读取计数，零预读取以捕获日志和 dataless/零块状态
佐证，不宣称已精确测得网络总字节数。完整哈希在传输完成后独立核验。
