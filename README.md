# tdssh

基于 **TwoDrive dev 同版本 iroh 1.3.0 / QUIC** 的跨 NAT SSH 工具。双方无需同一个
LAN、VPN、固定公网 IP 或路由器端口转发。先尝试 UDP 打洞，网络不允许时使用端到端
加密的 iroh 中继。Shell、SCP、SFTP、端口转发与 IDE 仍使用系统 OpenSSH。

支持 Ubuntu bash、Ubuntu pwsh、Windows pwsh。每台设备运行独立 `tdssh`，不要求安装
TwoDrive、不使用 OneDrive 账号。当前版本是首个可用实现；验证范围见
[验证记录](docs/validation.md)。

## 安装

需要 Rust 1.96+、原生 OpenSSH 客户端；作为被连接端还需要 SSH 服务。

```bash
cargo build --release --locked
./scripts/install.sh
export PATH="$HOME/.local/bin:$PATH"
```

Windows 在 pwsh 7 中：

```powershell
cargo build --release --locked
./scripts/install.ps1 -AddToPath
```

GitHub Actions 同时生成 Linux x64 和 Windows x64 可执行文件及配套脚本；从仓库的
Actions → Native validation → 成功的运行 → Artifacts 下载，无需本地 Rust。
Ubuntu 的 pwsh 可以直接执行 `tdssh`，安装服务时调用 bash 脚本。

Ubuntu 服务端需要已有的本地 OpenSSH 服务，例如：

```bash
sudo apt install openssh-server
sudo systemctl enable --now ssh
```

Windows 可用系统 OpenSSH Server；在管理员 pwsh 中安装并启动：

```powershell
Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0
Start-Service sshd
Set-Service sshd -StartupType Automatic
```

SSH 服务应允许密钥登录。tdssh 只连接本机回环 SSH 端口；跨网无需开放公网 22 端口。

## 第一次连接

两端分别初始化，记下输出的完整公共设备 ID：

```text
tdssh init
```

通过可信渠道核对完整 ID。服务端只授权客户端的 ID，然后保持运行：

```text
tdssh allow CLIENT_FULL_DEVICE_ID
tdssh serve
```

客户端连接（把 `alice` 换成服务端操作系统用户名）：

```text
tdssh ssh --peer SERVER_FULL_DEVICE_ID --user alice
```

原生 SSH 继续校验主机密钥并验证登录凭据；设备授权不代替 SSH 认证。主机密钥按
`tdssh-SERVER_FULL_DEVICE_ID` 保存，重连和本地随机端口变化不产生新别名。
远程命令与退出码：

```text
tdssh ssh --peer SERVER_FULL_DEVICE_ID --user alice -- uname -a
tdssh ssh --peer SERVER_FULL_DEVICE_ID --user alice --ssh-option=-i --ssh-option=/path/to/key
```

授权管理和撤销（服务端执行；撤销会在约一秒内关闭已有连接）：

```text
tdssh peers
tdssh revoke CLIENT_FULL_DEVICE_ID
```

## 多个会话、SCP、SFTP 和 IDE

推荐在客户端常驻一个本地转发，然后按普通 SSH 使用。一个转发支持多个并发会话，
新会话会重新建立失效的 QUIC 连接；已经断开的 SSH 会话由调用者重连。

```text
tdssh forward --peer SERVER_FULL_DEVICE_ID --listen 127.0.0.1:2222
ssh -o HostKeyAlias=tdssh-SERVER_FULL_DEVICE_ID -p 2222 alice@127.0.0.1
scp -o HostKeyAlias=tdssh-SERVER_FULL_DEVICE_ID -P 2222 file.txt alice@127.0.0.1:~/
sftp -o HostKeyAlias=tdssh-SERVER_FULL_DEVICE_ID -P 2222 alice@127.0.0.1
```

在 `~/.ssh/config`（Windows 为 `$HOME/.ssh/config`）中保存：

```sshconfig
Host remote-via-tdssh
    HostName 127.0.0.1
    Port 2222
    User alice
    HostKeyAlias tdssh-SERVER_FULL_DEVICE_ID
```

之后直接 `ssh remote-via-tdssh`，也可在 VS Code Remote SSH 中使用该主机。

每份身份目录同时只允许一个网络进程，以防同一设备 ID 的多次中继注册互相顶掉。
`allow`、`revoke`、`id` 可在后台运行时使用。已有 `forward` 时不要用同一身份额外
启动 `tdssh ssh` 或 `proxy`；通过该转发运行任意多个原生 `ssh`，或用另外的
`--state-dir` 创建并授权独立身份。两台机器也必须各自初始化，不要复制私钥。

## 强制绕过本机 VPN / 代理

两端可分别启用；只约束启用该选项的这一端：

```text
tdssh interfaces
tdssh --force-direct --interface wlo1 serve
tdssh --force-direct --interface "Wi-Fi" ssh --peer SERVER_FULL_DEVICE_ID --user alice
```

不指定 `--interface` 时自动选择正在工作的物理网卡。Linux 使用 `SO_BINDTODEVICE`，
Windows 使用 `IP_UNICAST_IF` / `IPV6_UNICAST_IF`；覆盖 QUIC UDP（包括自动重建）、
DNS UDP/TCP、中继 TCP 和签名地址发布。内部旁路 CONNECT 代理仅绑定随机本机端口，
使用随机认证口令，透传中继 TLS，不修改系统代理、Clash 或路由配置。

**强制模式不回退**：网卡不可用、绑定权限不足、物理 DNS 或中继无法访问时，连接失败。
这不是强制 UDP 打洞成功；中继也可以通过物理网卡直连，不经本机 VPN。
它约束本程序的套接字出站，不能控制上级路由器、运营商或任意内核级重定向。
存在额外防火墙重定向时应通过抓包验证。不要用关闭证书检查解决网络问题。

若 Linux 报 `physical interface binding failed: Operation not permitted`，可给安装后的
可执行文件 `cap_net_raw`，然后重试；每次替换文件后需要重新设置：

```bash
sudo setcap cap_net_raw=ep "$HOME/.local/bin/tdssh"
```

不启用强制模式时遵从系统路由及 HTTP(S) 代理；配合下节 Clash 规则同样可以直连。

## Clash / Mihomo YAML：不启用强制模式也直连

将 [examples/clash-direct.yaml](examples/clash-direct.yaml) 合并到现有配置，不要整份
覆盖订阅。使用 **规则模式**，把下面规则放到 `MATCH`、规则集和 GEOIP 等规则前面：

```yaml
mode: rule
find-process-mode: always
rules:
  - PROCESS-NAME,tdssh,DIRECT
  - PROCESS-NAME,tdssh.exe,DIRECT
  - DOMAIN-SUFFIX,iroh.link,DIRECT
  # 后面保留原来的 rules
```

进程规则负责动态对端 IP 的 UDP 流量，域名规则负责默认中继和发现服务，二者一起保留。
需要支持进程识别的 Mihomo / Clash Meta 内核及相应权限；不要把通用 `ssh.exe` 全局
设为 DIRECT。启用 fake-IP DNS 时，把 `+.iroh.link` 追加到现有 `dns.fake-ip-filter`
的黑名单；若你使用 `fake-ip-filter-mode: rule`，请按该模式的语法配置。自建中继应
额外追加其域名的 DIRECT 和 fake-IP 排除规则。

全局代理模式会跳过这些规则。`DIRECT` 服从操作系统路由；如果系统还运行另一个
全隧道 VPN，应启用 tdssh 的 `--force-direct`。在 Clash 连接面板确认 tdssh 命中
DIRECT；本项目未自动改动你的 FlClash 配置。

依据：[进程与域名规则](https://wiki.metacubex.one/config/rules/)、
[进程查找设置](https://wiki.metacubex.one/config/general/)、
[DNS fake-IP 过滤](https://wiki.metacubex.one/config/dns/)。

## 开机自启动

Ubuntu：安装好二进制后，在实际运行 tdssh 的用户下执行。以下 `--boot` 会启用
systemd lingering，从而在用户尚未登录时启动。系统若要求授权，先执行
`sudo loginctl enable-linger "$(id -un)"`。

```bash
# 服务端；选项保存在用户 unit 中
./scripts/install-service.sh --boot --force-direct

# 客户端：常驻转发
./scripts/install-service.sh --boot --role forward --peer SERVER_FULL_DEVICE_ID --force-direct

systemctl --user status tdssh
journalctl --user -u tdssh -f
./scripts/install-service.sh --uninstall
```

可用 `--binary`、`--state-dir`、`--interface`、`--target`、`--listen`、`--relay`、
`--ticket-file` 定制；`--dry-run` 只打印 unit。服务重启策略为失败后等待 10 秒。
卸载只移除本项目 unit，保留身份、授权和 SSH 设置。

Windows pwsh：默认随当前用户登录启动；加 `-AtBoot` 在登录前启动。
后者要在**同一个运行用户的管理员 pwsh** 中注册 S4U 计划任务，不保存密码，任务以
该用户的普通权限运行。更换账户会使用不同身份与任务，不能代替提权。

```powershell
./scripts/install-service.ps1 -ForceDirect -AtBoot
# 客户端
./scripts/install-service.ps1 -Role forward -Peer SERVER_FULL_DEVICE_ID -ForceDirect -AtBoot

Get-ScheduledTask -TaskName 'tdssh-*'
./scripts/install-service.ps1 -Uninstall
```

支持 `-Binary`、`-StateDir`、`-Interface`、`-Relay`、`-TicketFile`、`-DryRun`。
计划任务失败后一分钟重启；机器断网后恢复时也可重新建立连接。
任务启动与真实重启后行为需要在目标 Windows 机器上验证，CI 不模拟机器重启。

## 自建中继、手动票据和诊断

默认使用 iroh 的公共中继和签名 DNS 发现，不需要自己部署服务器。
也可使用兼容 iroh 1.3.0 的 HTTPS 中继，并手动交换服务端公共地址票据：

```text
tdssh --relay https://relay.example.com serve --ticket-out server.ticket
tdssh --relay https://relay.example.com ssh --peer SERVER_FULL_DEVICE_ID --ticket-file server.ticket --user alice
```

票据只含公共 ID 与地址，不授予权限；必须与已核对的 `--peer` 匹配。
自建中继不自动注册公共发现，票据可能在服务端重启后需要重新导出。

`--relay-only` 关闭对端 UDP，仅用加密中继，适合定位 UDP 被封的问题。
`--no-relay` 关闭中继，仅用于可达 UDP 诊断，不能保证跨 NAT 可用。两者都可与
`--force-direct` 组合，但互斥；`--no-relay` 也与 `--relay` 互斥。
日志写 stderr，ProxyCommand 的 stdout 只传 SSH 字节。

默认状态目录：Ubuntu `~/.local/share/tdssh`（遵循 XDG），Windows
`$env:LOCALAPPDATA/tdssh`。私钥文件 `identity.json` 应保密。Linux 目录/文件分别为
0700/0600；Windows 为当前用户 SID 和 LocalSystem 的受保护 DACL。

## 开发验证

```bash
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --locked
python3 scripts/smoke.py
python3 scripts/smoke-ssh.py                    # Linux，需本地可运行 sshd
python3 scripts/smoke.py --live-relay --force-direct
```

测试只创建临时身份和数据，不使用个人 SSH 密钥、不修改现有 sshd 或代理配置。
[来源与本地依赖补丁](THIRD_PARTY.md)，[MIT](LICENSE)。
