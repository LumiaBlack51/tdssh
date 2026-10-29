# 验证记录

日期：2026-10-20（Asia/Shanghai）。

## 已验证

- Linux Rust 1.96 格式、Clippy（所有目标、warnings denied）和 30 项自动化测试。
- 真实本地 QUIC：4 MiB 有序双向数据、TCP/QUIC 半关闭、设备拒绝、活动连接撤销、
  票据 ID 替换拒绝、SSH 服务不可达回报，以及并发身份初始化。
- UDP 依赖补丁：首次绑定与重建都调用保护策略；保护失败无法生成无保护套接字。
- 两个独立 CLI 进程通过真实 UDP 传输 4,194,461 字节，SHA256
  `8af3aa21f5746bb876cba20335ab4b1d1abbd8c3304096a4b515f7c7ae733650`。
- 隔离的真实 OpenSSH：远程命令、ProxyCommand、SCP/SFTP 双向传输 2,055,941 字节，
  SHA256 `397df674b07d60243ef9cf3154979ffa88bb8b1b7e84d111d2f83df289a2cf24`，
  原生 ssh 子进程的远程退出码 37。
- 当前 Ubuntu 宿主保持 FlClash TUN 启用，物理出站模式通过真实公共 iroh 中继传输
  同样的 4,194,461 字节；正常系统路由模式也通过同样的数据测试。
- 公共中继上的真实 OpenSSH/SCP/ProxyCommand 强制直连测试通过；strace 记录到
  312 次成功的 `SO_BINDTODEVICE` 调用，目标为物理网卡 `wlo1`。原始 trace 仅用于
  本地检查，随临时缓存清理，不上传可能包含临时认证字节的网络跟踪。
- Release 优化版在 HTTP_PROXY、HTTPS_PROXY、ALL_PROXY 均指向不可用本机代理时，
  通过强制直连完成公共中继上的真实 SSH、SCP/SFTP、ProxyCommand 和退出码检查。
- Ubuntu pwsh 7.6.7 执行 CLI，带空格的身份目录创建与读取通过；所有 pwsh 脚本
  通过 PowerShell AST 语法检查。强制物理网卡模式的真实本地 QUIC UDP 双向校验也通过。
- Bash 安装脚本语法与带空格、百分号、美元符路径的 systemd unit 生成。

## 边界

- 本地双进程和公共中继测试均在同一台宿主上完成，**不是两台独立 NAT 的实测**。
  NAT 协调和打洞由同版本 iroh 提供；无法打洞时保留加密中继退路。
- 没有修改用户的 Clash YAML；提供的合并片段依照 Mihomo 官方文档编写。
- 没有在用户机器上启用持久启动项，也没有改动已运行的 OpenSSH 服务。
- Linux 与 Windows 的原生 CI 结果见仓库 Native validation 工作流。目标机器的
  真实 Windows 重启、S4U 域策略、物理网卡与 VPN 组合仍需现场验证。
- 物理网卡绑定约束本进程套接字，不能保证绕过上级设备或任意内核包重定向。
- 依赖公共 DNS / 中继服务可用性；断线后能创建新会话，不恢复已经断开的 SSH shell。

## 断线退出回归

初始实现使用 Tokio 自动 runtime 退出。新测试保持 ProxyCommand 的 stdin 管道打开，
在服务端撤销设备授权：旧实现已经输出撤销错误，但等待 8 秒后仍未退出。原因是
Tokio stdin 的不可取消阻塞读取阻止了默认 runtime 销毁。

改为在网络端点关闭后使用 `shutdown_background` 结束 runtime。相同测试在修复后
及时以非零状态退出，不需要调用方先关闭 stdin。该回归随双平台 CLI smoke 运行。

## 2026-10-29：路径重试队列内存回归

详见 [故障记录](incidents.md)。固定依赖 iroh 1.3.0 的本地补丁对候选路径重试
去重并限制为 64 项。共享生产入队函数的 3 项回归在原始入队逻辑下失败，补丁后通过。
Linux 离线锁定构建、格式、Clippy 和全部 36 项测试通过（含 3 项按需退出回归）；真实本地双进程 QUIC、
隔离 OpenSSH shell/SCP/SFTP/ProxyCommand 及退出码验证通过。其他依赖版本保持不变。

队列回归模拟多连接同时遇到路径额度耗尽；未重现用户原网络上的额度耗尽，
也未取得原 OOM 进程分配堆栈。尚未验证本次补丁的跨 NAT 长时间运行和 Windows 原生行为。
已构建并部署 Linux 优化客户端，重启后的现有 Windows SSH 目标连通性验证通过。
本机按用户选择开启最后会话结束后 60 秒退出；部署状态和验证边界见故障记录。

## GitHub 原生检查

[最终原生验证](https://github.com/LumiaBlack51/tdssh/actions/runs/39423203064)，
实现提交 `79d02264b5fdd61ce38ba3bf69437a5bb74f7641`：

- Linux：30 项 Rust 测试、Clippy、真实 OpenSSH shell/SCP/SFTP/ProxyCommand、
  保持 stdin 打开的撤销退出回归、systemd unit 检查与原生安装包生成通过。
- Windows：26 项 Rust 测试、Clippy、真实双进程 QUIC 数据传输、同一 stdin 撤销回归、
  pwsh 计划任务参数/开机配置生成、私钥受保护 DACL 检查与原生安装包生成通过。
- 两平台 CI 安装包的外层归档和内层可执行文件 SHA256 均已重新校验。

本地交付保留源码、必要的第三方源文件与许可证、原生可执行文件和安装包。
已删除约 4.2 GiB 的 Cargo 构建输出；上游下载、临时 pwsh、原始 syscall 跟踪、
临时身份/SSH 密钥和数据均由本次任务清理，不触碰其他项目或个人 SSH 配置。
