# qiqi → gua 重命名实施计划

## 需求回顾

用户明确三项变更：
1. Cargo 内部程序目标名 `qiqi` → `gua`（主程序 binary、lib、default-run）
2. 默认数据目录 `%APPDATA%\qiqi` → `%APPDATA%\gua`；老目录存在时运行时自动迁移（复制全部内容→删除旧目录）
3. 环境变量 `QIQI_DATA_DIR` → `GUA_DATA_DIR`

**不在本次范围内**（用户未要求）：品牌显示名/包名（branding.json）、build.rs 的编译时 env var 前缀 `QIQI_BRAND_*`、录制/测试专用环境变量（`QIQI_FAST`、`QIQI_DEBUG`、`QIQI_DUMP*`、`QIQI_QUOTA_DEMO`、`QIQI_MAIL_DEMO`、`QIQI_CAL_DEMO`、`QIQI_WECOM_*`、`QIQI_OUTLOOK_CLIENT_ID`）、图标/资源文件名（`qiqi.ico`、`qiqi-logo.png`）、测试临时目录名（`qiqi-outlook-test-*`）、子录制器目录名（`src/bin/qiqi-record/`）。

## 影响清单

### A. Cargo 目标名（改后 `cargo build` 产物自动变成 gua.exe / gua.dll）

| 文件 | 位置 | 改动 |
|------|------|------|
| `Cargo.toml` | L2 `name = "qiqi"` | → `"gua"` |
| `Cargo.toml` | L8 `default-run = "qiqi"` | → `"gua"` |
| `Cargo.toml` | L11 `[lib] name = "qiqi"` | → `"gua"` |
| `Cargo.toml` | L15 `[[bin]] name = "qiqi"` | → `"gua"` |
| `Cargo.toml` | L20 `[[bin]] name = "qiqi-record"` | **保留**（用户未要求改，录制器独立命名更清晰） |

**连锁效应**：
- Cargo.lock 自动更新，不需要手工改。
- `README.md` L68-72 的 `--bin qiqi` / `qiqi.exe` / `%APPDATA%\qiqi` / `QIQI_DATA_DIR` 命令示例要更新。
- `CONTRIBUTING.md` L10 / L17 / L18 / L34 的命令示例和环境变量名要更新。
- `build.rs` L78 `qiqi.res` → `gua.res`（输出名随包名走）；L69 / L86 `qiqi.ico` 是输入图标文件名，**不改**（branding.json 没改）。

### B. 数据目录与迁移逻辑（核心改动）

**`src/model.rs`**：
- 更新注释块中的路径说明。
- `"QIQI_DATA_DIR"` → `"GUA_DATA_DIR"`；`.join("qiqi")` → `.join("gua")`。
- 新增**迁移函数** `migrate_legacy_data_dir_if_needed()`：检查 `%APPDATA%\qiqi` 是否存在、`%APPDATA%\gua` 是否不存在，若两者都满足则把旧目录下所有文件递归复制到新目录，成功后删除旧目录。
- 该函数在 `config_dir()` 首次返回新路径前触发一次（幂等）。

**迁移实现要点**：
- 用 `std::fs::read_dir` + `std::fs::copy` 逐文件递归复制，子目录结构保持。
- 复制失败（权限/文件被占用）时**只写日志不崩溃**，程序继续用新目录空跑，下次启动再重试。
- 复制全部成功后 `fs::remove_dir_all(old)` 删除旧目录；删除失败仅警告（用户或其他进程可能正占用旧文件）。
- 在 `glass-diagnostics.txt` 同级写一条迁移日志（如果迁移发生），方便诊断。
- 新增单元测试覆盖：无旧目录时不触发、旧有新无时全量复制、复制一半失败时旧目录保留。

### C. 环境变量 QIQI_DATA_DIR → GUA_DATA_DIR

| 文件 | 位置 | 改动 |
|------|------|------|
| `src/model.rs` | `config_dir()` 内 | `"QIQI_DATA_DIR"` → `"GUA_DATA_DIR"` |
| `src/bin/qiqi-record/main.rs` | L114 | `set_var("QIQI_DATA_DIR", ...)` → `"GUA_DATA_DIR"` |
| `src/mail/outlook.rs` | L446 / L447 / L458 (3处) | `"QIQI_DATA_DIR"` → `"GUA_DATA_DIR"` |
| `README.md` | L102 | 文档里的 `QIQI_DATA_DIR` → `GUA_DATA_DIR` |
| `CONTRIBUTING.md` | L17 | 文档里的 `$env:QIQI_DATA_DIR` → `$env:GUA_DATA_DIR` |

### D. 托盘层硬编码（tray.rs）

这些跟 Cargo 目标名走，必须同步：

| 文件 | 位置 | 原文 | 改动 |
|------|------|------|------|
| `src/tray.rs` | L50 | `RUN_VALUE = w!("qiqi")` | → `"gua"`（注册表启动项值名） |
| `src/tray.rs` | L51 | `MUTEX_NAME = w!("Local\\qiqi.SingleInstance")` | → `"Local\\gua.SingleInstance"`（单实例互斥体） |
| `src/tray.rs` | L354 | `.name("qiqi-tray")` | → `"gua-tray"`（线程名） |
| `src/tray.rs` | L361 | `class = w!("qiqiTray")` | → `"guaTray"`（托盘窗口类名） |
| `src/tray.rs` | L604 | `FindWindowW(w!("qiqiTray"),` | → `"guaTray"`（open_settings 查找类名） |

L112 / L589 的 `include_bytes!("../assets/qiqi.ico")` 不改——图标文件本身没改名。

### E. 安装/卸载脚本（跟 Cargo 目标名走）

| 文件 | 位置 | 改动 |
|------|------|------|
| `installer/install.ps1` | L8 | fallback `target\release\qiqi.exe` → `target\release\gua.exe` |
| `installer/install.ps1` | L16 | `Uninstall\qiqi` → `Uninstall\gua` |
| `installer/install.ps1` | L18 | `Programs\qiqi` → `Programs\gua` |
| `installer/install.ps1` | L21 | 默认值 `'qiqi.exe'` → `'gua.exe'` |
| `installer/setup.nsi` | APP_ID / APP_PACKAGE 默认值 | `qiqi` → `gua` |
| `installer/update.ps1` | 缓存目录和临时 exe 名 | `qiqi` → `gua` |

install.ps1 L7 / L20 的 `$brand.PackageName` 动态值**不改**——branding.json 的 `guagua` 独立于 Cargo 目标名。

## 实施步骤（依赖顺序）

1. **Cargo.toml** 一次性改完 4 处目标名（package、default-run、lib、主 bin），保留 qiqi-record。
2. **build.rs** 改输出 res 文件名 `qiqi.res` → `gua.res`。
3. **src/tray.rs**：5 处硬编码（互斥体名、启动项名、窗口类名、线程名、FindWindow 查找）。
4. **src/model.rs**：
   - 重命名环境变量 + 默认目录；
   - 实现 `migrate_legacy_data_dir_if_needed()` 并在 `config_dir()` 首次调用时触发；
   - 注释块同步更新；
   - 补充迁移逻辑的单元测试。
5. **src/bin/qiqi-record/main.rs + src/mail/outlook.rs**：环境变量字符串替换。
6. **installer/*.ps1 + setup.nsi**：脚本硬编码替换。
7. **README.md + CONTRIBUTING.md**：命令示例和文档中的路径/环境变量名更新。
8. **验证**（见下）。

## 依赖与注意

- **迁移逻辑必须同步执行**，放在 `config_dir()` 内部或首次 `fn init_migration()` 调用——程序启动第一帧就要读 settings.txt，不能延迟。
- **互斥体名变化**意味着新旧版本实例**不会互斥**：用户如果先装旧版再启动新版，会同时跑两个进程。这是 breaking change，但改名不可避免。
- **脚本里的动态值不改**：install.ps1 的 `$brand.PackageName`（来自 branding.json 的 "guagua"）和 `$brand.DisplayName` 保持独立。
- **QIQI_FAST / QIQI_DEBUG / QIQI_DUMP / QIQI_QUOTA_DEMO / QIQI_MAIL_DEMO / QIQI_CAL_DEMO / QIQI_WECOM_* / QIQI_OUTLOOK_CLIENT_ID** 全部**保留原前缀**——这些是录制器、测试专用变量，用户未提，改了只会增加脚本维护成本。
- **Cargo.lock / dist/ 打包产物**：不手工改，`cargo build` 自动刷新。

## 验证

1. `cargo build` → 产物为 `target\debug\gua.exe`（不是 qiqi.exe）。
2. `cargo test` → 所有已有测试 + 新增迁移逻辑测试通过。
3. 手动迁移测试：
   - 前提：先停掉所有 gua/qiqi 进程；手动创建 `%APPDATA%\qiqi\settings.txt` 写入 `pet=0` 和一个子目录；
   - 运行新编译的 gua.exe；
   - 检查 `%APPDATA%\gua\settings.txt` 内容匹配旧版、`%APPDATA%\qiqi` 目录已删除、迁移日志存在。
4. 环境变量测试：`$env:GUA_DATA_DIR = "D:\tmp\gua-test" ; cargo run` 能覆盖默认目录；`QIQI_DATA_DIR` 不再生效。
5. 安装脚本 smoke test：install.ps1 能正确找到 `target\release\gua.exe`，且 branding.json 的 package_name "guagua" 路径不受影响。

## 风险与处理

| 风险 | 处理 |
|------|------|
| 迁移时文件被占用（旧进程没退出），复制失败 | 复制阶段用 `std::fs::copy`，单文件失败就跳过并记日志，不删旧目录；下次启动继续重试 |
| 复制完但删旧目录失败 | 记日志，每次启动都再试一次 remove_dir_all，直到成功为止（幂等：gua 目录已存在且内容对得上就直接过） |
| 新旧 Cargo 名不同，`cargo run --bin qiqi` 会报找不到 | README / CONTRIBUTING 同步更新命令，编译错误就是最直接的反馈 |
| 注册表启动项旧值还指向 qiqi.exe | 新版 install.ps1 安装时会用 `reg delete` 清掉旧值再写新值；手动升级的用户可以改一下启动项路径 |
| 用户有自定义脚本/环境配置依赖 `QIQI_DATA_DIR` | 明确 breaking change，README 同步改；如果后续发现兼容性问题再加 fallback 读旧变量 |
