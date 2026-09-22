# PicoForge All

[English](README.md) | 中文

面向 **[Pico All](https://github.com/XiaoNetwork-Astral/pico-all)** 安全密钥的桌面管理工具，使用 Rust 和 GPUI 构建

## 功能

- 管理通行密钥、OATH 账户、OTP 槽位、PIV、OpenPGP 和 SmartCard-HSM
- 搜索和筛选已存储的密钥、对象及安全事件
- 读取、验证审计日志，并显示日历时间
- 配置状态灯、本地签名固件及校验更新
- 支持英语、简体中文和可搜索的 IANA 时区列表

这是 [PicoForge](https://github.com/librekeys/picoforge) 的独立分支，问题请反馈至[本仓库](https://github.com/BlueFunny19/picoforge-all/issues)；保留对 RS-Key 和 pico-fido 的支持，具体功能取决于设备固件

## 下载与安装

从 [Releases](https://github.com/BlueFunny19/picoforge-all/releases/latest) 下载 **Windows x64 便携包**，解压后运行 `picoforge.exe`；Linux 和 macOS 用户可参考[源码构建说明](docs/Building.md)

固件功能需要安装 [picotool 2.3.1+](https://github.com/raspberrypi/picotool/releases)，将其加入 PATH，或通过 `PICOTOOL` 环境变量指定可执行文件；PicoForge All 本身不需要 Python

## 使用

- **设备编排**调整设备参数；**软件 → 设置**选择界面语言和时间显示所用的时区
- **固件**用于检查、签名和刷写 UF2；签名时选择现有的 secp256k1 PEM 密钥，后续更新沿用同一密钥
- **审计**用于开启事件记录、读取和验证日志；默认在连接或刷新设备时同步时钟
- **HSM 对象**支持输入文本或导入文件，大小上限为 **1,800 字节**；新对象的 ID 自动分配

指示灯闪烁时，按下并松开设备按键（BOOTSEL）；配套 Pico All 固件的默认确认时限为 60 秒；FIDO 没有默认 PIN，其他应用仅在设备确认仍使用出厂值时自动填入默认 PIN

已开启安全启动的设备必须使用原来的受信任密钥签名，请妥善离线备份；擦除、重置和永久硬件设置会在确认前说明影响

## 构建

需要当前稳定版 Rust 和对应平台的依赖，详见[构建说明](docs/Building.md)

```sh
git clone https://github.com/BlueFunny19/picoforge-all.git
cd picoforge-all
cargo build --release --locked
```

Windows 产物为 `target/release/picoforge.exe`，Linux/macOS 为 `target/release/picoforge`；运行 `cargo test --locked` 可执行离线单元测试，硬件测试默认跳过

## 常见问题

设备识别和固件工具配置见[故障排查](docs/Troubleshooting.md)；反馈问题时请附应用版本、固件版本和相关控制台错误，不要附上 PIN 或私钥

## 许可与致谢

采用 [AGPL-3.0](LICENSE)，基于 Suyog Tandel 及 PicoForge 贡献者维护的 [PicoForge](https://github.com/librekeys/picoforge)；Pico All 适配和分支由 BlueFunny 维护
