# 天气页面（实况 + 四日预报）实现计划

## Repository Research

### 当前状态
- `pet-gua-win` 的天气模块已实现基础功能：Open-Meteo API 拉取当前温度 + 天气代码 → 常驻药丸栏右侧显示迷你天气图标 + 当前温度
- 数据结构：`Reading { temp, kind, day, code, at }` + `State { place, reading, ... }`；fetch 函数仅请求 `current=temperature_2m,weather_code,is_day`
- 现有 8 个页面槽位（`visible_pages: [bool; 8]`，`page_order: [usize; 8]`）：音乐(0)、专注(1)、饮水(2)、置物架(3)、待办(4)、邮箱(5)、AI额度(6)、日程(7/CALENDAR_PAGE)
- 页面名常量：`PAGE_NAMES: [&str; 8]`
- UI 风格统一：逻辑坐标 419×140，Card + theme/paint 模块绘制，egui 文本渲染

### 原版参考（xiaotang-daozai PR #105）
原版把天气 UI 全部放在 `weather.rs` 中，包含：
- 实况大卡片：当前温度（大字）、天气图标（矢量绘制）、天气描述、城市名
- 四日预报：水平排列 4 列，每列含日期、天气图标、最高/最低温
- 一套完整的矢量绘图函数：sun/moon/cloud/rain/snow/bolt/fog 等
- 额外实时数据：湿度、风速、气压等（可选，本计划先做核心）

### Open-Meteo API 能力
当前 URL：`current=temperature_2m,weather_code,is_day`
扩展后：加 `daily=weather_code,temperature_2m_max,temperature_2m_min` + `forecast_days=5`（1天实况+4天预报）

## Files and Modules

| 文件 | 改动 |
|---|---|
| `src/weather.rs` | 扩展 fetch 请求 daily 数据；新增 `DailyForecast` 结构；扩 State 存储预报；新增 `page_ui` 页面渲染函数；新增一套矢量天气图标绘制函数（sun/moon/cloud/rain/snow/bolt/fog） |
| `src/model.rs` | visible_pages/page_order 从 8 扩到 9；配置加载时兼容旧 8 位格式 |
| `src/app.rs` | PAGE_NAMES 从 8 扩到 9，加 "天气"；render_page switch 加 `8 => self.page_weather(c, env, cmds)`；新增 `page_weather` 方法委托给 `weather::page_ui` |
| `src/pages.rs` | 可选：如果希望集中管理页面函数，放在这里；本计划跟原版一致放在 weather.rs |
| `src/lib.rs` | 无需改动（weather 模块已引入） |

## Implementation Steps

### Step 1: 扩展 Open-Meteo API 请求
在 `fetch()` 函数中，将 URL 参数从：
```
current=temperature_2m,weather_code,is_day
```
扩展为：
```
current=temperature_2m,weather_code,is_day,relative_humidity_2m,wind_speed_10m
daily=weather_code,temperature_2m_max,temperature_2m_min
forecast_days=5
```

### Step 2: 新增数据结构
```rust
/// 单日预报
pub struct DailyForecast {
    pub date: String,       // "11/10" 格式
    pub weekday: String,    // "周一"、"明天"、"周三"...
    pub kind: Kind,
    pub temp_max: i32,
    pub temp_min: i32,
}

/// State 中的字段扩展
forecast: Vec<DailyForecast>,  // 4天预报（不含今天）
humidity: Option<i32>,
wind_speed: Option<f64>,
```

### Step 3: 更新 State::save/load 持久化
weather.json 加 forecast/humidity/wind_speed 字段；向后兼容缺失字段。

### Step 4: 更新 fetch 解析逻辑
解析 `daily` 数组，生成 forecast Vec。

### Step 5: 扩展配置容量（model.rs）
```rust
// 原来
pub visible_pages: [bool; 8],
pub page_order: [usize; 8],
// 改为
pub visible_pages: [bool; 9],
pub page_order: [usize; 9],
```
配置加载时：旧配置只有 8 位时补第 9 位为 true；旧 page_order 补 8 到末尾。

### Step 6: 新增天气页面 UI（weather.rs 中 page_ui 函数）
参考原版绘制布局：
```
┌─────────────────────────────────────┐
│ [城市名]          [设置入口]          │
│                                      │
│  ┌───────────实况大卡片────────────┐  │
│  │  [大图标]   23°                │  │
│  │              多云              │  │
│  │              湿度65% · 风速3km/h│  │
│  └───────────────────────────────┘  │
│                                      │
│  ┌──四日预报──────────────────────┐  │
│  │ 周一   周二   周三   周四       │  │
│  │ ☀️    ⛅    ☁️    🌧️          │  │
│  │ 25°   22°   18°   15°         │  │
│  │ 18°   16°   12°   10°         │  │
│  └───────────────────────────────┘  │
└─────────────────────────────────────┘
```

### Step 7: 新增矢量天气图标绘制函数
原版在 weather.rs 中提供了一套独立的大图标绘制（不同于常驻药丸的 draw 函数）：
- `sun(c, cx, cy, size)` — 带 8 道光芒
- `moon(c, cx, cy, size)` — 弯月多边形
- `cloud(c, cx, cy, size)` — 云轮廓
- `rain(c, cx, cy, size)` — 云 + 3 道斜线
- `snow(c, cx, cy, size)` — 云 + 雪花十字
- `thunder(c, cx, cy, size)` — 云 + 闪电
- `fog(c, cx, cy, size)` — 云 + 两条横线

### Step 8: 接入页面路由
```rust
// app.rs render_page 加
8 => self.page_weather(c, env, cmds),

// 新增方法
pub(crate) fn page_weather(&mut self, c: &Card, env: &Env, cmds: &mut Vec<Cmd>) {
    self.weather.page_ui(c, env, cmds, self.settings.show_weather);
}
```

### Step 9: 更新 PAGE_NAMES
```rust
pub(crate) const PAGE_NAMES: [&str; 9] = ["音乐", "专注", "饮水", "置物架", "待办", "邮箱", "AI 额度", "日程", "天气"];
```

## Dependencies and Considerations

- **依赖不变**：现有 serde_json、url、ureq 已满足需求；不需要新增 crate
- **forecast_days=5**：Open-Meteo 的 daily 数组会返回今天+未来4天，取 index 1..=4 即为四日预报
- **配置向后兼容**：visible_pages/page_order 从 8→9，旧配置文件只有 8 字符时要安全扩展
- **面板高度 140**：天气页面在标准 140px 内布局足够，不需要像日历那样扩容到 216px
- **刷新频率**：daily 预报与 current 共用同一 REFRESH（3600s），同线程一次请求搞定

## Validation

1. `cargo check` 编译通过
2. 运行 `gua.exe`，确认：
   - 常驻栏迷你天气仍正常显示
   - 页面切换多出「天气」一页
   - 天气页实况大卡片温度、图标、描述正确
   - 四日预报日期、图标、高低温正确
   - 点击天气页设置入口能跳到天气设置
   - 天气关闭时页面有友好提示
3. 用旧配置文件（8 位 visible_pages）启动，确认不崩溃且天气页默认可见
4. 页面管理中可以拖拽天气页位置、隐藏/显示

## Risks

- **旧配置兼容性**：用户 settings.txt 中 visible_pages/page_order 只有 8 项 → 处理：加载时检测长度，补 8 为 visible=true、page_order 末尾追加 8
- **Open-Meteo daily 字段缺失**：API 变更或请求被限流 → 已有重试机制；UI 上 forecast 为空时显示「预报暂无」
- **面板 140px 内布局紧张**：需精确排版；如果不够，实况卡片可以简化（去掉湿度风速）
