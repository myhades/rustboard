# Windows 原生验收复现

所有测试会修改当前用户剪贴板。先关闭同 profile 的 Agent。测试生成内容，不读取用户私人文件。构建：`cargo build --release --workspace --examples` 和 `cargo build --release --workspace`。

## Explorer 原生多文件

在仓库根目录的 PowerShell 中：

```powershell
$env:RUSTBOARD_PROFILE='manual-test'
New-Item -ItemType Directory -Force test-output/manual-paste | Out-Null
Start-Process .\target\release\rustboardd.exe -WindowStyle Hidden `
  -ArgumentList '--ole-demo','--metrics','test-output/manual-metrics.json' `
  -RedirectStandardError test-output/manual-agent.log
```

打开本仓库的 `test-output/manual-paste` 文件夹。在粘贴前读取 `test-output/manual-metrics.json`，三个source的bytes_read/read_calls必须都是0。然后在Explorer文件区域执行普通Ctrl+V。预期创建4KiB、64MiB和1GiB三个文件。执行：

```powershell
python scripts/verify-pattern.py test-output/manual-paste
```

三个结果必须PASS。再打开另一空目录，**不重新复制**，重复粘贴并验证。metrics应累计第二轮读取量。Agent不拦截Ctrl+V；按键只是Explorer真实OLE消费端测试的触发器。

日志保存在上述manual-agent.log；它仅包含元数据计数。无源文件临时下载、无OleFlushClipboard。结束：`target/release/rustboard.exe stop`。

## TLS全链路（同机隔离Peer）

使用同一profile启动Agent加 `--listen 127.0.0.1:45991`，运行CLI doctor获取其fingerprint。启动测试程序：

```powershell
target/release/examples/loopback_peer.exe <Agent指纹> 45991 45992 test-output/manual-peer
```

该测试程序只绑定loopback，不访问Windows剪贴板；会写入peer-pin.txt。在另一终端、相同RUSTBOARD_PROFILE下执行：

```powershell
target/release/rustboard.exe pair 127.0.0.1:45992 --fingerprint <peer-pin.txt内容>
Set-Content test-output/manual-peer/command.txt 'text'
```

Windows剪贴板应变为固定Unicode测试文本。Agent metrics的suppressed应增加，text_events不能因自己的发布而增加。手动复制另一个测试文本，manual-peer/received.json应收到同样文本（仅在测试目录内保留synthetic数据）。

先导航到一个新的空Explorer目录，再把command.txt写为`files`。reads.json三个bytes必须为0。执行Ctrl+V、验证三个文件哈希，再检查reads.json恰好覆盖对应大小。UI自动化工具可能使用剪贴板实现文本输入，因此不要在发布后用工具输入文件夹路径。

传输中执行CLI pause：读取在有界在途块后停止，Explorer显示读取错误。这是预期取消，不是PASS文件传输。不要继续使用残缺文件。重新发布新Offer后重试。关闭Peer可写command.txt为`stop`，然后CLI unpair测试peer并stop。

这是本机mTLS网络测试，不能标为跨设备/跨系统互通。

## COM生命周期探针

运行 `stream_probe <ready-file> <continue-file>`，它取得真实跨进程IStream并写ready-file。覆盖剪贴板后创建continue-file：旧流仍应读出16个正确pattern字节。另一次运行中，在ready后退出Agent再创建continue-file：Read应明确失败。实测结果在docs/test-results/{overwrite-result,exit-result}.json。

## 诊断

`rustboard doctor`和`status`检查Agent、身份、协议、模式；第二Agent必须退出失败。`platforms/windows/scripts/check-ipc.py`验证原始v1 Named Pipe JSON（测试Agent当前必须拥有复制自pattern文件的Offer）。管道名称见PROTOCOL.md。自动启动脚本默认未执行；签名打包、第二用户ACL拒绝和长期浸泡见验收表待办。

实现依据包括Microsoft的[Shell Clipboard Formats](https://learn.microsoft.com/en-us/windows/win32/shell/clipboard)、[Shell Data Object](https://learn.microsoft.com/en-us/windows/win32/shell/dataobject)和[Named Pipe Security and Access Rights](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)。这些API文档不代替本项目的实测证据。
