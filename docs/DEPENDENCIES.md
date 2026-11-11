# 依赖与第三方许可

呱原创代码采用 MIT。第三方依赖、字体、图标及品牌标识遵循各自许可；MIT 不授予第三方商标权。完整归属及许可文本见 [THIRD_PARTY_NOTICES.txt](../THIRD_PARTY_NOTICES.txt)。

天气数据由 [Open-Meteo](https://open-meteo.com/) 提供。国内城市表来自 [GeoNames](https://www.geonames.org/)，以 [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/) 许可提供；本项目从 [aerial-tv](https://github.com/pengchujin/aerial-tv) 复制该项目根据 GeoNames 整理的城市表，未对表格内容作修改。

## 补丁依赖

`eframe`、`egui_extras` 使用 crates.io 的 0.37.0；项目继续使用 Glow 渲染器。上游已包含 Windows OpenGL 当前视口跟踪，旧版 `vendor/eframe` 保留供参考，但不参与构建。

- `imap`：使用本仓库 `vendor/imap` 中的 2.4.1 补丁，仅关闭 `nom` 默认功能并保留 `std`，移除邮件协议解析不需要的 `lexical-core` 浮点解析依赖。
- 构建需要 Rust 1.95 或更高；首次构建需联网下载尚未缓存的 Cargo 依赖。

## 安全检查

2026 年 11 月 3 日发布前，对 Cargo.lock 的 registry 依赖查询了 OSV 公告库，并运行完整测试。已移除受 [RUSTSEC-2023-0086](https://rustsec.org/advisories/RUSTSEC-2023-0086.html) 影响的 `lexical-core 0.7.6`。

当前仍有两项上游停止维护提示：

| 依赖 | 公告 | 当前处理 |
| --- | --- | --- |
| paste 1.0.15 | [RUSTSEC-2024-0436](https://rustsec.org/advisories/RUSTSEC-2024-0436.html) | 随现有 UI 依赖使用，后续跟随上游升级 |
| ttf-parser 0.25.1 | [RUSTSEC-2026-0192](https://rustsec.org/advisories/RUSTSEC-2026-0192.html) | 随字体栈使用，后续评估维护中的替代版本 |

这两项属于维护状态提示，并非本次检查确认的可利用漏洞。公告库检查不能代替完整安全审计。`imap-proto` 还会产生 Rust 未来兼容性警告，当前编译和测试通过。
