# TG Bot Sync

Windows 桌面端 Telegram 信息中转工具：在手机和电脑之间发送文字、图片和文件。

## 功能

- 快速发送窗口与可配置全局快捷键
- 文字、图片、多个文件发送
- 手机消息自动复制到 Windows 剪贴板（可关闭）
- 手机/电脑消息分栏、搜索与本地历史
- SQLite 消息索引与 Windows DPAPI Bot Token 加密
- HTTP/SOCKS5 代理
- Windows 托盘、开机启动、独立上传队列和上传失败原因

## 开发

环境要求：Windows、Node.js 20+、Rust stable、WebView2。

```bash
npm install
npm run tauri dev
```

## 正式构建

```bash
npm run build
npm run tauri build
```

安装包位于 `src-tauri/target/release/bundle/`。

## 配置

首次启动后在设置页填写 Bot Token、Telegram 数字用户 ID 和代理。Bot Token 只保存在本机应用数据目录，并使用 Windows DPAPI 加密；不要提交 Token、历史数据或 `*.sqlite` 文件。

默认快捷键：

- `CTRL+ALT+S`：发送剪贴板文字
- `CTRL+ALT+V`：打开快速发送窗口

## 安全

请只允许自己的 Telegram 用户 ID。不要在 Issue、提交记录或截图中公开 Bot Token。
