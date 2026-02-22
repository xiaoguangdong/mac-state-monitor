# Mac State Monitor

## 项目概述

macOS 菜单栏系统监控工具，使用 Rust 开发。实时显示 CPU、内存、磁盘、网络和温度信息，支持温度历史图表窗口。

### 技术栈

- **语言**: Rust (Edition 2021)
- **系统信息采集**: `sysinfo` crate
- **窗口管理**: `tao` (跨平台窗口库)
- **图表渲染**: `plotters` + `plotters-bitmap`
- **原生 macOS UI**: `objc2`、`objc2-app-kit`、`objc2-foundation` (状态栏菜单)
- **图形缓冲**: `softbuffer`

### 架构

```
src/
├── main.rs           # 应用入口，事件循环
├── app.rs            # App 主结构，协调各模块
├── config.rs         # 配置 (刷新间隔、温度组件选择)
├── model.rs          # 数据模型 (SystemStats, HistoryBuffer)
├── monitor/          # 系统监控模块
│   ├── mod.rs        # SystemMonitor 主结构
│   ├── cpu.rs        # CPU 使用率采集
│   ├── memory.rs     # 内存信息采集
│   ├── disk.rs       # 磁盘信息采集
│   ├── network.rs    # 网络流量采集
│   └── temperature.rs # 温度传感器采集
└── ui/               # 用户界面
    ├── mod.rs
    ├── tray.rs       # macOS 原生状态栏 (NSStatusItem)
    ├── menu.rs       # 下拉菜单
    └── chart_window.rs # 温度历史图表窗口
```

## 构建和运行

### 开发模式
```bash
cargo build
cargo run
```

### 发布构建
```bash
cargo build --release
```

### 打包为 macOS 应用
```bash
# 完整打包流程 (构建 + 创建 .app + 生成 .pkg 安装包)
bash scripts/build_release.sh

# 或分步执行:
bash scripts/create_app_bundle.sh  # 创建 .app
bash scripts/create_pkg.sh         # 创建 .pkg 安装包
```

输出文件位于: `target/release/mac-state-monitor-*.pkg`

## 功能特性

### 状态栏显示
- **CPU**: 总体使用率百分比
- **Memory**: 内存使用率百分比
- **Disk**: 磁盘使用率百分比
- **Network**: 上传/下载速度 (格式: ▲速度 ▼速度)
- **Temperature**: 可选择显示 CPU/GPU/SSD 等组件温度

### 菜单功能
- 显示详细系统信息 (CPU 核心数、内存总量、磁盘空间等)
- 刷新间隔设置 (1s/2s/5s/10s)
- 温度组件选择
- 显示温度图表窗口
- 退出应用

### 图表窗口
- 显示 CPU、GPU、SSD 三组件的温度历史曲线
- 最多保存 60 个数据点
- 窗口大小: 240 x 420 像素

## 配置

默认配置 (`src/config.rs`):
- 刷新间隔: 1 秒
- 菜单栏温度组件: CPU

## 发布优化

`Cargo.toml` 中的 release profile:
```toml
[profile.release]
opt-level = "z"    # 优化体积
lto = true         # 链接时优化
strip = true       # 移除符号信息
```

## 开发注意事项

1. **主线程要求**: `TrayManager` 必须在主线程创建 (使用 `MainThreadMarker`)
2. **ObjC 内存管理**: 使用 `objc2::rc::Retained` 管理 Objective-C 对象
3. **温度传感器**: 依赖 macOS 的 SMC (System Management Controller)，不同 Mac 型号传感器名称可能不同
4. **状态栏样式**: 使用 `NSMutableAttributedString` 实现双行显示和颜色编码 (绿色<30%、黄色<60%、紫色<80%、红色≥80%)

## 应用标识

- Bundle ID: `com.mac-state-monitor.app`
- 最低系统要求: macOS 13.0
- LSUIElement: true (不显示在 Dock)
