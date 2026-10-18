# 验收结果 — 2026-10-17，Windows x64

环境：Rust1.99.1 stable x86_64-pc-windows-msvc、MSVC2022、当前用户的Windows Explorer。只在一台Windows机器测试；隔离TLS Peer不访问系统剪贴板。未在Mac/Linux运行。

## 构建和自动测试

| 项目 | 结果 | 证据 |
|---|---|---|
| Release构建、fmt、Clippy全部目标且-D warnings | PASS | docs/test-results/build-checks.txt |
| Rust：7 Core + 1共享协议 + 3 Windows | PASS | 同上，11通过、0失败 |
| 独立Python：frame/Unicode/offset/三个大小SHA-256 | PASS | scripts/check-vectors.py已实际执行 |
| 远程CI及Mac/Linux构建 | NOT_TESTED | 已配置CI，未push/未运行远程流水线 |

## 真实原生路径

| 验收项 | 结果 | 证据与范围 |
|---|---|---|
| 隐藏消息窗口、原生剪贴板事件 | PASS | Explorer真实复制、文本事件；idle.json MainWindowHandle=0 |
| Unicode文本：Peer→TLS→Windows | PASS | tls-text-final.json |
| Windows文本→TLS→隔离Peer | PASS | tls-text-reverse.json |
| 自发布文本回环抑制 | PASS | text-loop-suppression.json：suppressed=1、text_events=0 |
| Explorer CF_HDROP多文件捕获 | PASS | explorer-capture.json：3文件、大小正确、无绝对路径 |
| 发布本地OLE文件时读取量0 | PASS | before-paste.json |
| Explorer Ctrl+V，本地4KiB/64MiB/1GiB多文件 | PASS | hash-1.json三个SHA-256匹配 |
| 同一Offer重复粘贴到另一目录 | PASS | hash-2.json，after-paste-2.json读取量翻倍 |
| TLS Offer发布时远端主体读取0 | PASS | release-before-paste.json |
| Explorer Ctrl+V触发TLS读取，三个大小均正确 | PASS | tls-release-hashes.json、release-after-paste.json |
| 随机偏移、Seek、Clone、EOF、错误lindex | PASS | COM单测及独立IPC offset测试 |
| 复制后写入/删除源 | PASS（拒绝语义） | Windows源共享锁单测：写删拒绝、释放后可删除；Core检测可变源版本变化 |
| 暂停进行中的1GiB网络粘贴 | PASS（预期中断） | cancellation.json；Explorer报读取错误；有界在途块后读取量稳定 |
| 覆盖剪贴板后已获取的本地IStream | PASS | overwrite-result.json：跨进程仍读16bytes且模式正确 |
| Agent退出后的旧IStream | PASS（明确失败） | exit-result.json：0bytes、HRESULT800706ba |
| 未配对Peer拒绝、unpair撤销既有流后续读取 | PASS | Core真实loopback mTLS单测 |
| CLI所列命令与stop | PASS（基本流程） | 实际运行；pair需双端核对完整指纹 |
| 第二Agent拒绝启动 | PASS | singleton.json，退出码1 |
| 重启身份保持和CLI控制恢复 | PASS | restart-idle.json |
| Native IPC v1含64KiB数据的大JSON帧 | PASS | ipc-compatibility.json，独立Python客户端 |
| 并发压力、多日运行、慢速恶意客户端 | NOT_TESTED | 有界实现不代替负载实测 |
| 第二用户/不同登录会话访问拒绝 | NOT_TESTED | 当前用户DACL及拒绝远端已实现，未第二账户实测 |
| 登录自动启动、签名安装包 | NOT_TESTED | 用户级脚本已实现，未替用户安装自动启动 |
| Windows/macOS跨机器双向互通 | BLOCKED | 需Mac Codex及两台设备联合测试 |
| Linux原生后端、其他Windows目标程序 | NOT_TESTED | Linux尚未实现，未扩展应用兼容性矩阵 |

## 内容和资源证据

模式为byte[i]=i%251。Python流式独立计算期望值和读取Explorer输出；复制阶段没有全文件哈希预扫描。

| 大小 | SHA-256 |
|---|---|
| 4KiB | d67c656e01756650d77717b0839985a056ec28ffe174601d690fc407a2ceffca |
| 64MiB | 98dc891b284e4d84ac25b0c0a24fdbe39a7f0dbd643ad5e8aa06e02fc6258254 |
| 1GiB | 9cc5601236c455c6af19a76e64d2d95953a93b10eeb8b8b756a57090e1499b3e |

本地Debug OLE测试Agent峰值工作集15,085,568bytes（约14.4MiB）。Release TLS测试峰值21,315,584bytes（约20.3MiB），44.64秒采样区间源读取量增加905,904,128bytes，约19.35MiB/s。这是本机回环、JSON编码、当时负载下的采样区间速率，不是完整端到端平均值或LAN承诺。见release-resources.json、release-samples.json、sampled-throughput.json。

无监听空闲Agent的3秒样本：新增CPU时间0.0秒、工作集12,263,424bytes、私有内存3,862,528bytes。短样本不证明长期占用。见idle.json；restart-idle.json的IdleWorkingSet为null（进程退出后属性不可用），资源值以idle.json为准。

暂停测试有意中断，其目录不能算完整1GiB通过。成功网络输出在test-output/tls-release，本地两轮在test-output/paste-1和paste-2。test-output被Git忽略；docs/test-results仅保存小型证据，不含设备私钥或用户正文。

复现见docs/WINDOWS_TESTING.md，发行前待办见CONTINUE.md。所有PASS均只对应上述具体范围。
