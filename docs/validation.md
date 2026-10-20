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
