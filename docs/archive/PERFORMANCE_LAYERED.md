# 专项分层测量

2026-10-17，Windows 192.168.100.6 与 Mac 192.168.100.4。同一实体网络、无隧道。原始证据在 `docs/test-results/layered-perf/`。

## 诊断基线与修复

| 层级/路径 | 测量 | 解释 |
| --- | --- | --- |
| iperf Windows→Mac 单流，12 s | 235.15 Mbps | 当前单流样本 |
| iperf Mac→Windows 单流，12 s | 201.16 Mbps | 当前反向单流样本 |
| iperf Windows→Mac 四流，12 s | 408.31 Mbps | 400 Mbps 聚合能力复现 |
| 原 Core，Mac→Windows，64 MiB | 12.21 s，5.24 MiB/s | 不含平台/磁盘，已经很慢；64 KiB 请求 p50 11.81 ms、p95 14.72 ms |
| 原 Core + typed JSON 编码，64 MiB | read 11.84 s，encode 0.443 s | IPC 编码 239,135,526 字节，约 3.56 倍膨胀；该探针额外 Value 校验时间不可视为产品开销 |
| 二进制+窗口 Core，Mac→Windows，1 GiB | 23.28 s，43.99 MiB/s，369 Mbps | Windows CPU 采样 1.56 s，RSS 16.47 MiB；包含模式校验，不含磁盘 |
| Windows IStream+本机 TLS+落盘，1 GiB | 3.38 s，302.90 MiB/s | CPU 3.17 s，RSS 23.91 MiB；两 TLS 端位于一个进程，非 Explorer、非实体网络 |
| Mac 本机 TLS→JSON Unix IPC→Swift hydrate，1 GiB | 9.65 s，106.08 MiB/s | JSON 解码 3.22 s，成为剩余平台开销 |
| 相同 Mac 路径改二进制 IPC，1 GiB | 2.14 s，478.94 MiB/s | 完成写入/同步后独立核验 SHA-256；Swift RSS 10.20 MiB、Core 16.45 MiB |

网络条件明显波动：单流 iperf 与后来的 Core 样本并非同步测量，不能将 369/201 当作“超过链路效率”。
64 MiB 与 1 GiB 启动成本不同，也不能将上述比例直接宣称为严格 A/B 加速倍数。
macOS `/usr/bin/time` 的 Swift CPU 包含随后哈希，不能全部归为传输 CPU；Windows 探针采样包含模式验证。

## 修改归属

- 共享 Rust Core：有界顺序读取窗口、源/接收端逐块授权检查、统一 typed IPC 结果、控制帧前缀与正文合并写入。
- 网络/P2P：TLS ALPN 显式协商二进制 Data，去除整数数组膨胀；4 MiB 窗口摊薄逐 64 KiB RTT。没有增加连接数量，没有改变发现/配对策略。
- 平台：macOS Swift 按能力选择二进制 IPC；Unix socket 和 Windows Named Pipe 共享同一编码/授权实现。Windows OLE 本身未改读取语义。

契约与内存边界见 [TRANSPORT_V2.md](TRANSPORT_V2.md)。4 MiB 是活动顺序读取窗口，不是整个文件缓存；发布仍为零主体读取。

## 原生验收

三文件共 1,140,854,784 字节（4 KiB、64 MiB、1 GiB），以下原生结果独立 SHA-256 全部一致。

| 实体路径 | 用时/吞吐 | 测量边界 |
| --- | --- | --- |
| Mac→Windows Explorer，旧 Agent/v1 | 217.532 s | 同轮兼容基线 |
| Mac→Windows Explorer，v2 | 26.716 s，341.62 Mbps | 粘贴前标记到目标全部可独占打开；CPU 2.234 s、RSS 29.24 MiB；相对同轮基线约 8.14 倍 |
| Windows→Mac Finder，v2 首轮 | 39.625 s，230.33 Mbps | 首次 fetch 到全部目标无打开句柄；1 GiB hydrate 34.122 s |
| Windows→Mac Finder，最终复测 | 39.374 s，231.79 Mbps | 同上；1 GiB hydrate 35.165 s；发布后三文件均为零块 dataless |

Finder 用时不包含用户按键到回调开始的间隔；哈希在记录目标可用时间后执行。首轮 Mac 活动区间：Provider CPU 5.43 s、RSS 28.50 MiB；Agent CPU 4.06 s、RSS 43.86 MiB。最终复测资源见 finder-confirm-resources.json。不能用 Swift 探针的约 10 MiB 代替原生两进程的资源。
Windows CPU 是 Agent 的累计 CPU 秒，不包含 Explorer 和系统服务；2.234/26.716 约为一个逻辑核的 8.4%，不是整机百分比。
400 Mbps 理想传输三文件需约 22.817 s；Mac→Windows 达参考值约 85.4%，反向未达到目标。

### 未保留的实验与剩余缺口

- Windows SO_SNDBUF 1 MiB：Core 1 GiB 28.77 s，对照 28.19 s；再加 Mac SO_RCVBUF 1 MiB 为 30.32 s。未观察到收益，代码和依赖已撤回，未改全局配置。
- 两条 TLS 各分读同一个 4 MiB 窗口：31.357 s，峰值 RSS 13.97 MiB；无收益，已撤回。此结果不等于持续流水化多流无效。
- 1 MiB 批量 IPC：Mac loopback 写入 2.421 s，对照 64 KiB 的 2.138 s，已撤回。
- 当前追加 iperf：单流 258.07 Mbps、四流 502.87 Mbps。聚合能力与单流不能混用，不同时间样本不是严格 A/B。
- 独立 Windows 合成源在 45453/45451 均连接超时；换回原生 Agent 的 45451 后 nc 成功。此对照未完成，不据此判断源文件开销，未更改安全设置。

剩余工作：测量连续流水化传输、网络请求间空闲和本地 IPC 等待占比；当前窗口仍为请求/应答。四流 iperf 显示仍有聚合带宽，但简单双连接分窗没有解决问题。两方向稳定接近 400 Mbps、本地拷贝开销可忽略、10G 低 CPU/内存均未验收。
首次 Windows 原生测试意外使用旧 Agent（`cargo build --workspace --examples` 只更新 examples），明确归入 v1 兼容测试；已改用独立目录执行 `cargo build --release --workspace` 并核对二进制。

10G 目标未验收：现有链路不足以证明该目标，平台落盘诊断也仍低于 10G 所需的约 1192 MiB/s。


## 验证与清理

Windows：fmt、Clippy 全 targets -D warnings、11 Core + 1 原始向量 + 3 Windows 测试、Release workspace 构建、Python 原始向量通过。
Mac：11 Core + 1 原始向量 + 2 平台测试、Swift 向量/解码/metadata/hydrate 成功与故障清理通过；首次 test.sh 最后被探针格式检查拦住，已同步格式化文件，后续 fmt 和 Clippy 全 targets 通过。原始失败日志保留，见 validation.json。

Mac 原生二进制为用户签名版本；额外实验及其 read_chunk 提取/分配调整均已撤回，保留本轮原生验证的单连接、4 MiB 窗口、64 KiB 二进制 IPC 路径。

测试 Agent、诊断服务、采样均已停止，双端测试配对撤销，测试 File Provider domain 删除，原 Mac Provider 注册恢复。Mac 主工作区保持干净。合成测试输出保留在 Git 忽略的 test-output；未修改防火墙或登录项，未调用开发者私钥，未推送。iPhone 接力可由用户恢复。
