# macOS 集成契约 v1

本仓库为 Core/网络协议的主仓库。Windows 上未访问或迁移另一台机器的 `Misc/lazy_paste` POC，也未声称验证了 macOS。Mac Codex 应把已经验证的 File Provider 能力迁入本文约定的结构，不要维护第二套不兼容的网络或文件身份协议。

## 1. 目录与修改范围

| 目录 | 责任 |
|---|---|
| crates/rustboard-core | 共享 Offer、ReadAt、Engine、TLS Node、IPC 数据操作；Windows 主维护 |
| crates/rustboard-platform/src/macos | AppKit 事件/发布、Swift bridge、本地 IPC 与权限认证 |
| native/macos | 最小宿主 .app、Swift NSFileProviderReplicatedExtension、entitlements、签名配置 |
| packaging/macos | 包装和 Homebrew formula |
| scripts/macos | 构建、签名、安装、测试工具 |
| tests/protocol | 三端共享协议向量，不能各自更改 |
| docs | 集成约定与实测记录 |

根目录只放共用内容。macOS 可添加目标平台依赖、条件编译入口和必要构建钩子；不要把 AppKit/Swift/Win32 类型放入 Core，不要修改 Windows 后端来适应 POC 的临时路径协议。需要协议变更时先提交共享定义和兼容性测试。

## 2. Rust 平台接口

`rustboard-platform::ClipboardBackend::publish_text(&mut self, &str)` 在 AppKit 合适线程发布文本并抑制自己的回环。

`FilePresentationBackend::publish_files(Vec<PresentedFile>)` 表达原生文件展示职责；Windows 使用 IStream，Mac 应建立 dataless File Provider items 并在剪贴板发布其系统文件 URL。跨进程 Swift 不应接收 Rust trait object 指针，而应通过下述 IPC 和 Offer 元数据完成相同职责。

本地剪贴板事件进入 `Node.engine` 的 `Engine.local(Content, HashMap<FileID, Arc<dyn ReadAt>>)`。本地文件源必须有稳定版本语义：优先快照/APFS clone 或等效保证；仅元数据无法保证版本时应拒绝或明确失效，不可静默读取变更后的字节。不能复制时读完整主体或算全文件哈希。`ReadAt::read_at(offset, &mut buffer)` 每次最多65,536 bytes，支持任意合法偏移和重复读取。

网络接收使用 `Node.incoming` 单槽 latest Offer。原生 adapter 在适当线程消费，应用前再次确认 `Engine.current.offer_id` 仍匹配。同一 Offer 不得重新起源；使用 `(event_sequence, source_device, offer_id)` 的统一冲突规则。`Engine.persist_clock` 必须在配置目录初始化，并在跨进程启动前保护目录权限。

## 3. 本地 IPC

规范见根目录 `PROTOCOL.md` 和 `protocol.rs::{IpcRequest,IpcOperation}`，不要照搬 POC 自定义请求。帧为4字节大端长度 + UTF-8 JSON，最大1 MiB；协议号1。

最低操作：get_offer、read、cancel、control。`Node.local_ipc` 已实现前三个操作，可直接由认证后的 Unix domain socket/XPC adapter 调用。Windows Native Pipe 的独立 Python 验证脚本已经证明原始 JSON 客户端可以获取 Offer、按 offset 读、拒绝错误版本和过大范围。

读取示例：

```json
{"protocol":1,"request":{"type":"read","request_id":7,"offer_id":"00000000-0000-0000-0000-000000000002","file_id":"00000000-0000-0000-0000-000000000003","version":"pattern-v1","offset":249,"length":5}}
```

成功响应为 `{ok:true, protocol:1, response:{type:"data",request_id:7,offset:249,bytes:[249,250,0,1,2],eof:false}}`。示例元数据、偏移和哈希以 `tests/protocol/vectors.json` 为准。JSON `bytes` 是字节整数数组，不是 base64。UInt64 字段不要经过 Double。

失败响应 `ok:false`；当前 Windows adapter 粗粒度使用 `code:"io"`，带非敏感诊断 `error`。共享错误词汇为 unauthorized/expired/not_found/version_changed/invalid_request/cancelled/busy/io/unsupported。消费端必须对未知错误做失败处理，不可当空文件成功。细分错误码可以兼容扩展；改变成功字段或语义需修订测试。

每条连接只允许一个在途请求。Cancel 的含义为停止后续请求并关闭流；已在执行的一个有界读取可能完成。无需实现跨线程强杀同步 syscall。不要把控制 socket 开放到 TCP。

## 4. File Provider Item ID 映射

建议固定字符串：`rb1:<source-device-uuid>:<offer-uuid>:<file-uuid>:<sha256(version-utf8)>`。目录父子关系单独依据安全相对路径映射，不能从绝对路径生成身份。将完整 version 和 Offer 元数据保存在 app-group 私有元数据索引中；Item ID 内的 version digest 只作映射，不替代传输的原始 version。

建议 domain 标识以设备与用户 profile 隔离。文件名只是展示名，绝不能用 Item ID 解码结果直接拼接任意本机路径。当前 Windows 仅发布平面文件；请先实现平面多文件互通，再共同扩展目录验收。size=null 在协议保留，当前正式端到端支持已知大小；未知大小请返回 unsupported。

## 5. fetchContents 工作流

1. 根据 Item ID 从受保护索引获取当前 Offer/FileEntry；检查生命周期、版本、授权。
2. 创建位于 extension 授权临时目录的独占临时文件，使用随机名字，避免路径穿越和符号链接覆盖。
3. 从 offset0 开始，每次请求≤64 KiB。检查 response.request_id、offset、长度和 EOF；以流式方式写入，内存与文件大小无关。
4. 每个 await/写入后检查 Progress cancellation。取消后停止请求、关闭连接，并删除自己创建的临时文件；不能删除用户目标文件。
5. EOF 必须与 manifest size 一致。版本失效、断连、数据错误均使 fetchContents 失败；不能返回残缺文件 URL。
6. 成功后按 File Provider API 要求交付临时文件及版本。应用层可使用 APFS clone 优化，但复制阶段不能提前 materialize 文件主体。

重连仅在相同 tuple/version 仍有效时按确定 offset 继续；如果旧 Offer 过期/撤回，则必须失败。Core 当前只保留最新 Offer，没有历史恢复。后台 hydration 也可能触发 fetchContents，必须记录真实系统回调时序，不能声称所有读取都只来自 Cmd+V。

## 6. 缓存与生命周期

Core 当前不缓存内容。Mac 若需缓存，键必须包含完整四元组，设置磁盘上限、引用/进行中状态与过期清理，权限仅 app group。旧内容不能在版本失效后被误当新版本。不要永久保留 POC 的全文件临时目录。Agent 重启后不恢复剪贴板内容，extension 旧 Item 应不可用或重新枚举当前有效 Offer。

## 7. Sandbox 与身份认证

宿主和 extension 需共同 app-group entitlement，签名身份、bundle IDs、domain 注册由 Mac 端维护。必须根据实际 sandbox 能力选择 XPC 或受保护 Unix socket，不要假设任意 socket 路径可访问。

如果使用 Unix socket：父目录0700、socket0600；核对 peer UID，必要时结合审计令牌/签名身份；防止 socket 符号链接替换和其他用户连接。UID 只能证明同用户，不能证明同应用；涉及 sandbox 边界时应核对 audit token/code-signing requirement 或通过受控 XPC broker。token 如需使用必须经共享 Keychain/app group 安全配置传递，不能写入世界可读目录。密钥和证书不得提交仓库。

Windows pipe 实现不是 macOS 安全性的证据；Mac 必须提供权限拒绝实测。无需把扩展当作一个网络 Peer，TLS和配对仍由 Rust Agent 负责。

## 8. 构建与验收

Cargo stable 构建 Core/CLI；最小 .app 包含系统要求的 File Provider Extension。Swift/AppKit/签名/安装/卸载/launchd 用户 Agent/Homebrew 均在 Mac 专属子目录。不要创建额外长期自管 Helper；允许最小系统管理 extension 和宿主。

先运行 `cargo test -p rustboard-core --locked`、`python3 scripts/check-vectors.py`，再让 Swift测试解码相同 vectors.json，验证 frame、Unicode、u64、offset和错误路径。原始内容模式为 byte[i]=i%251，4 KiB/64 MiB/1 GiB SHA-256 已固定。

真实验收：Finder 原生 Cmd+V、多文件、零复制时主体读取、fetchContents时序、哈希、重复粘贴、取消、Agent重启、断连、版本失效、sandbox拒绝、launchd恢复。最后与 Windows 双向测试文本/文件，在双方明确核对指纹后配对。所有结果标为 PASS/FAIL/BLOCKED/NOT_TESTED；本机两进程不得标为跨机器通过。

## 9. 从 POC 迁移

先复用已证明的隐藏 domain/dataless item/Finder粘贴链路，再把临时源文件路径替换为四元组身份，把 POC IPC替换为v1有界read。移除无认证服务、绝对路径远程读取、复制时全量下载和私有协议分支。最后增加 launchd、签名和包装，而不是先重写 Core。任何系统API限制请用最小复现记录，不要改为预下载来掩盖。
