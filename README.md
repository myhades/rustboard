# Rustboard

原生、无界面、点对点局域网剪贴板同步。Rust Core + Rust CLI + 平台后端。

当前为可运行的 Windows 开发版本：Unicode 文本、Explorer 文件复制捕获、OLE 多文件懒粘贴、TLS 1.3 双向证书指纹认证、用户级 Named Pipe 控制。已在本机完成真实 Explorer 粘贴和独立哈希验证；**尚未完成跨机器 Windows/macOS 验收或长期稳定性验收**。macOS 原生组件由 Mac 端独立集成，Linux 原生后端尚未实现。

## 构建与启动

Windows 需要 Rust stable、MSVC C++ Build Tools 和 Windows SDK。

```powershell
cargo build --release --workspace --locked
.\target\release\rustboardd.exe
.\target\release\rustboard.exe doctor
```

Agent 默认不监听网络。要启用网络，在启动时显式指定本机监听 IP 和端口：

```powershell
.\target\release\rustboardd.exe --listen 192.168.1.10:45990
```

先停止已有 Agent 再更改监听地址。程序不会修改防火墙，不提供公网服务。CLI 不会创建第二个同步引擎。

## 配对与控制

在两端分别运行 `rustboard doctor`，通过可信渠道核对对端的完整 SHA-256 指纹，然后在**两端**配对：

```powershell
rustboard pair 192.168.1.11:45990 --fingerprint <对端64位十六进制指纹>
rustboard status
rustboard peers
rustboard pause
rustboard resume
rustboard mode text
rustboard mode all
rustboard unpair <device_id或fingerprint>
rustboard version
rustboard stop
```

无自动信任、无明文回退。`pair <address>` 未提供核对后的指纹时会拒绝修改信任。文本和元数据通常在约一秒的轮询周期内传播；文件主体只在目标程序读取时传输。

配置位于 `%LOCALAPPDATA%\Rustboard\default`，创建时限制为当前用户访问。`RUSTBOARD_PROFILE` 可隔离测试实例（字母、数字、连字符，最多32字符）；日常使用无需设置。测试 profile 也共享同一个 Windows 系统剪贴板，不能冒充两台设备测试。

可选用户登录自动启动：`scripts/windows/autostart.ps1 -Action Install -Agent <rustboardd.exe绝对路径> -Listen <本机IP:端口>`。移除使用 `-Action Remove`。开发过程未替用户启用自动启动。

## 文件行为与限制

- Explorer Copy 的 `CF_HDROP` 支持单文件和多文件；Cut/Move、目录、重解析点被拒绝。
- Windows 在 Offer 有效期内持有禁止写入/删除共享的源文件句柄，保证读取一致性；因此源文件可能暂时无法修改。覆盖剪贴板、暂停、过期、退出会释放源句柄。
- 默认有效期10分钟，最多256文件、每次读取64 KiB。无全文件预下载、无常驻历史、无完整文件哈希预扫描。
- OLE `FileGroupDescriptorW + FileContents/IStream` 已通过 Explorer 验证；其他应用兼容性未验证。
- 目录层级和未知大小在协议中可表达，但 Windows 发布端当前仅接受平面文件名和已知大小。
- 文本上限256 KiB。剪贴板只保留当前 Offer；覆盖/过期可能使进行中的旧文件粘贴失败。
- 当前不转发第三台设备的 Offer，仅直接配对设备之间同步。

## 验证与交接

```powershell
.\scripts\windows\check.ps1
python scripts/check-vectors.py
```

真实平台结果见 [TEST_RESULTS.md](TEST_RESULTS.md)，续开发状态见 [CONTINUE.md](CONTINUE.md)。协议见 [PROTOCOL.md](PROTOCOL.md)，威胁模型和已知限制见 [SECURITY.md](SECURITY.md)。Mac 开发从 [docs/MACOS_HANDOFF.md](docs/MACOS_HANDOFF.md) 和 [docs/MACOS_CODEX_PROMPT.md](docs/MACOS_CODEX_PROMPT.md) 开始。

## 目录约定

根目录只放共享构建配置和项目文档。`crates/rustboard-core` 不含 Win32/COM 类型；Windows 代码在 `crates/rustboard-platform/src/windows`；各平台脚本在 `scripts/<os>`，后续原生组件在 `native/<os>`、打包在 `packaging/<os>`。不要在根目录散落操作系统专属工程。
