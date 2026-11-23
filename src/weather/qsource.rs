use crate::qweather::{self, Client, Config, Error, Location, Provider, Runtime};
use eframe::egui::{self, Color32};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc::Receiver,
    Arc,
};

const DEFAULT_API_HOST: &str = "n97jpkxq72.re.qweatherapi.com";
const DEFAULT_API_KEY: &str = "3068180b440544329633dfa8c7f44d32";

fn initial_draft(config: &Config) -> (String, String) {
    if config.host.is_empty() && config.credential.is_empty() {
        (DEFAULT_API_HOST.to_owned(), DEFAULT_API_KEY.to_owned())
    } else {
        (config.host.clone(), String::new())
    }
}

fn paste_menu(response: &egui::Response) {
    response.context_menu(|ui| {
        if ui.button("粘贴").clicked() {
            response.request_focus();
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            ui.close();
        }
    });
}

fn provider_option(
    ui: &mut egui::Ui,
    provider: Provider,
    selected: bool,
    width: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 66.0), egui::Sense::click());
    let ctx = ui.ctx();
    let painter = ui.painter();
    let accent = crate::settings_theme::color(
        ctx,
        Color32::from_rgb(111, 174, 255),
        Color32::from_rgb(48, 113, 211),
    );
    let base = crate::settings_theme::color(ctx, Color32::from_rgb(37, 37, 41), Color32::WHITE);
    let fill = if selected {
        crate::paint::mix(base, accent, 0.10)
    } else if response.hovered() {
        crate::settings_theme::color(
            ctx,
            Color32::from_rgb(45, 45, 50),
            Color32::from_rgb(246, 248, 252),
        )
    } else {
        base
    };
    let border = if selected || response.has_focus() {
        accent
    } else {
        crate::settings_theme::color(
            ctx,
            Color32::from_rgb(65, 65, 71),
            Color32::from_rgb(221, 226, 234),
        )
    };
    painter.rect(
        rect,
        9.0,
        fill,
        egui::Stroke::new(if selected { 1.3 } else { 1.0 }, border),
        egui::StrokeKind::Inside,
    );
    let overseas = provider == Provider::OpenMeteo;
    let title = if overseas {
        "Open-Meteo"
    } else {
        "和风天气"
    };
    let ink = if overseas {
        accent
    } else {
        crate::settings_theme::color(
            ctx,
            Color32::from_rgb(111, 209, 177),
            Color32::from_rgb(29, 139, 105),
        )
    };
    let icon = egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 10.0), egui::vec2(24.0, 24.0));
    painter.rect_filled(icon, 6.0, crate::paint::mix(fill, ink, 0.15));
    let center = icon.center();
    let stroke = egui::Stroke::new(1.2, ink);
    if overseas {
        painter.circle_stroke(center, 7.0, stroke);
        painter.line_segment(
            [
                center + egui::vec2(-7.0, 0.0),
                center + egui::vec2(7.0, 0.0),
            ],
            stroke,
        );
        painter.line_segment(
            [
                center + egui::vec2(0.0, -7.0),
                center + egui::vec2(0.0, 7.0),
            ],
            stroke,
        );
        for sign in [-1.0, 1.0] {
            painter.add(egui::Shape::line(
                vec![
                    center + egui::vec2(0.0, -7.0),
                    center + egui::vec2(sign * 3.0, -3.5),
                    center + egui::vec2(sign * 3.0, 3.5),
                    center + egui::vec2(0.0, 7.0),
                ],
                stroke,
            ));
        }
    } else {
        let mut points = vec![center + egui::vec2(0.0, 9.0)];
        for index in 0..=16 {
            let angle =
                std::f32::consts::PI / 6.0 - index as f32 / 16.0 * std::f32::consts::PI * 4.0 / 3.0;
            points.push(center + egui::vec2(angle.cos() * 6.0, angle.sin() * 6.0 - 2.0));
        }
        painter.add(egui::Shape::closed_line(points, stroke));
        painter.circle_filled(center + egui::vec2(0.0, -2.0), 1.8, ink);
    }
    let title_size = if width < 185.0 { 11.5 } else { 13.0 };
    painter.text(
        rect.min + egui::vec2(42.0, 20.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(title_size),
        ui.visuals().text_color(),
    );
    let badge = if overseas {
        "默认"
    } else if width < 210.0 {
        "Key"
    } else {
        "自备 Key"
    };
    let badge_ink = if overseas {
        ui.visuals().weak_text_color()
    } else {
        crate::settings_theme::color(
            ctx,
            Color32::from_rgb(235, 194, 127),
            Color32::from_rgb(143, 93, 29),
        )
    };
    let galley = painter.layout_no_wrap(badge.into(), egui::FontId::proportional(9.0), badge_ink);
    let badge_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.right() - 10.0 - galley.size().x - 10.0,
            rect.top() + 12.0,
        ),
        galley.size() + egui::vec2(10.0, 6.0),
    );
    painter.rect_filled(badge_rect, 4.0, crate::paint::mix(fill, badge_ink, 0.10));
    painter.galley(badge_rect.min + egui::vec2(5.0, 3.0), galley, badge_ink);
    let mut x = rect.left() + 42.0;
    for (label, fee) in [
        (if overseas { "海外" } else { "国内" }, false),
        (if overseas { "免费" } else { "免费额度" }, true),
    ] {
        let ink = if fee {
            crate::settings_theme::color(
                ctx,
                Color32::from_rgb(130, 211, 168),
                Color32::from_rgb(30, 127, 79),
            )
        } else {
            ui.visuals().weak_text_color()
        };
        let galley = painter.layout_no_wrap(label.into(), egui::FontId::proportional(10.0), ink);
        let pill = egui::Rect::from_min_size(
            egui::pos2(x, rect.top() + 40.0),
            galley.size() + egui::vec2(10.0, 6.0),
        );
        painter.rect_filled(
            pill,
            4.0,
            crate::paint::mix(fill, ink, if fee { 0.10 } else { 0.06 }),
        );
        painter.galley(pill.min + egui::vec2(5.0, 3.0), galley, ink);
        x = pill.right() + 5.0;
    }
    response.on_hover_text(if overseas {
        "海外源，默认免费使用，无需 Key。"
    } else {
        "和风天气：国内源·精准。自备 API Host 与 Key；有免费额度，超额可能收费。"
    })
}

pub(super) struct Source {
    pub config: Config,
    pub selected: Provider,
    pub draft_host: String,
    pub draft_key: String,
    pub client: Option<Client>,
    tested: Option<Client>,
    testing: Option<Receiver<Result<Client, Error>>>,
    draft_gate: Option<Arc<AtomicBool>>,
    search: Option<Receiver<Result<qweather::Search, Error>>>,
    pub query: String,
    province: String,
    search_input: (String, String),
    search_changed_at: f64,
    search_sent: bool,
    pub results: Vec<Location>,
    credits: Vec<String>,
    pub picked: Option<Location>,
    city_choice: Option<Location>,
    pub message: String,
    pub runtime: Runtime,
    pub snapshot: Option<super::Snapshot>,
    pub condition: Option<String>,
    pub attributions: Vec<String>,
    pub show_config: bool,
}
impl Source {
    pub fn load() -> Self {
        let config = if cfg!(test) {
            Config::default()
        } else {
            Config::load()
        };
        let client = Client::load(
            &config.host,
            &config.credential,
            Arc::new(AtomicBool::new(true)),
        );
        let mut runtime = Runtime::default();
        runtime.reset(&config.host, config.location.clone());
        let selected = config.provider;
        let (draft_host, draft_key) = initial_draft(&config);
        let mut this = Self {
            draft_host,
            picked: config.location.clone(),
            city_choice: None,
            config,
            selected,
            draft_key,
            client,
            tested: None,
            testing: None,
            draft_gate: None,
            search: None,
            query: String::new(),
            province: String::new(),
            search_input: (String::new(), String::new()),
            search_changed_at: 0.0,
            search_sent: false,
            results: Vec::new(),
            credits: Vec::new(),
            message: String::new(),
            runtime,
            snapshot: None,
            condition: None,
            attributions: Vec::new(),
            show_config: true,
        };
        this.sync_snapshot();
        this
    }
    pub fn is_active(&self) -> bool {
        self.config.provider == Provider::Qweather
    }
    // 指数独立于天气来源启停，但使用同一套凭据和请求额度。
    pub(super) fn indices_client(&self, gate: Arc<AtomicBool>) -> Option<Client> {
        if self.config.host.is_empty() && self.config.credential.is_empty() {
            Client::new(DEFAULT_API_HOST, DEFAULT_API_KEY, gate).ok()
        } else {
            Client::load(&self.config.host, &self.config.credential, gate)
        }
    }
    fn sync_snapshot(&mut self) {
        if let Some(s) = self.runtime.snapshot() {
            self.condition = s.condition.clone();
            self.attributions = s.attributions.clone();
            let position = s.position;
            self.snapshot = position.and_then(|p| {
                let code = i64::from(s.code);
                Some(super::Snapshot {
                    place: super::Place {
                        name: p.label,
                        region: p.region,
                        lat: p.lat,
                        lon: p.lon,
                    },
                    reading: super::Reading {
                        temp: s.temp.round() as i32,
                        kind: super::Kind::from_code(code)?,
                        day: s.day,
                        code,
                        at: s.at.max(0) as u64,
                    },
                    located_at: 0,
                    forecast: s
                        .days
                        .into_iter()
                        .map(|d| super::DailyForecast {
                            date: d.date,
                            code: Some(i64::from(d.code)),
                            temp_max: Some(d.high.round() as i32),
                            temp_min: Some(d.low.round() as i32),
                            rain: Some(d.rain),
                        })
                        .collect(),
                    humidity: s.humidity.map(|n| n as i32),
                    wind_speed: s.wind.map(f64::from),
                    feels: s.feels.map(|n| n.round() as i32),
                    timezone: self.config.location.as_ref().map(|l| l.timezone.clone()),
                    sun: s.sun_times.map(|t| super::SunTimes {
                        timezone: t.timezone,
                        day_start: t.day_start,
                        day_end: t.day_end,
                        sunrise: t.sunrise,
                        sunset: t.sunset,
                    }),
                })
            });
        } else {
            self.snapshot = None;
            self.condition = None;
            self.attributions.clear();
        }
    }
    pub fn tick(&mut self, enabled: bool, network: Option<bool>, ctx: &egui::Context) {
        let active = enabled && self.is_active();
        if let Some(client) = &mut self.client {
            if !active && client.gate.swap(false, Ordering::SeqCst) {
                client.gate = Arc::new(AtomicBool::new(false));
            } else {
                client.gate.store(active, Ordering::SeqCst);
            }
        }
        if !enabled {
            if let Some(g) = self.draft_gate.take() {
                g.store(false, Ordering::SeqCst);
            }
            if let Some(t) = self.tested.take() {
                t.gate.store(false, Ordering::SeqCst);
            }
            self.testing = None;
            self.search = None;
            self.runtime.poll(false, chrono::Utc::now().timestamp());
            return;
        }
        if let Some(result) = self.testing.as_ref().map(|r| r.try_recv()) {
            match result {
                Ok(Ok(client)) => {
                    self.testing = None;
                    self.message = "连接测试通过，请搜索城市并保存启用。".into();
                    self.tested = Some(client)
                }
                Ok(Err(e)) => {
                    self.testing = None;
                    self.message = e.text().into()
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.testing = None;
                    self.message = "连接测试中断，请重试".into()
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(result) = self.search.as_ref().map(|r| r.try_recv()) {
            match result {
                Ok(Ok(found)) => {
                    self.search = None;
                    self.results = found.locations;
                    self.credits = found.credits;
                    self.message = if self.results.is_empty() {
                        "没有找到城市"
                    } else {
                        "请选择城市"
                    }
                    .into()
                }
                Ok(Err(e)) => {
                    self.search = None;
                    self.message = e.text().into()
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.search = None;
                    self.message = "城市搜索中断，请重试".into()
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.testing.is_some() || self.search.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if !self.is_active() {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        if self.runtime.poll(true, now) {
            self.sync_snapshot();
        }
        if network != Some(false) && !cfg!(test) {
            if let Some(client) = &self.client {
                self.runtime.refresh(client, now, false);
            }
        }
        if self.runtime.loading() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs(60));
        }
    }
    pub fn refresh(&mut self, ctx: &egui::Context) {
        if let Some(client) = &self.client {
            self.runtime
                .refresh(client, chrono::Utc::now().timestamp(), true);
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
    pub fn can_refresh(&self) -> bool {
        self.client.is_some() && self.runtime.can_refresh(chrono::Utc::now().timestamp())
    }
    fn use_open(&mut self) {
        let mut config = self.config.clone();
        config.provider = Provider::OpenMeteo;
        if config.save().is_err() {
            self.message = Error::Storage.text().into();
            return;
        }
        if let Some(c) = &self.client {
            c.gate.store(false, Ordering::SeqCst);
        }
        self.config = config;
        self.selected = Provider::OpenMeteo;
        self.snapshot = None;
        self.message = "已切换至 Open-Meteo".into();
    }
    pub fn choose(&mut self, location: Location) {
        let mut config = self.config.clone();
        config.location = Some(location.clone());
        if config.save().is_err() {
            self.message = Error::Storage.text().into();
            return;
        }
        if let Some(c) = &mut self.client {
            c.gate.store(false, Ordering::SeqCst);
            c.gate = Arc::new(AtomicBool::new(true));
        }
        self.config = config;
        self.picked = Some(location);
        self.runtime
            .reset(&self.config.host, self.config.location.clone());
        self.sync_snapshot();
    }
    fn add_city(&mut self) {
        let Some(city) = self.city_choice.clone() else {
            self.message = "请先从搜索结果选择城市".into();
            return;
        };
        if self.config.saved.contains(&city) {
            self.message = "该城市已保存".into();
            return;
        }
        if self.config.saved.len() >= 5 {
            self.message = "最多保存 5 个城市，请先移除一个".into();
            return;
        }
        let mut config = self.config.clone();
        config.saved.push(city.clone());
        if config.save().is_err() {
            self.message = Error::Storage.text().into();
            return;
        }
        self.config = config;
        if !self.is_active() && self.picked.is_none() {
            self.picked = Some(city);
        }
        self.city_choice = None;
        self.query.clear();
        self.results.clear();
        self.credits.clear();
        self.message = "城市已添加".into();
    }
    fn apply(&mut self) {
        let active_client = self
            .client
            .as_ref()
            .filter(|c| self.is_active() && self.draft_key.is_empty() && self.draft_host == c.host);
        let Some(client) = self.tested.as_ref().or(active_client).cloned() else {
            self.message = "请先测试连接".into();
            return;
        };
        let Some(location) = self.picked.clone() else {
            self.message = "请先选择城市".into();
            return;
        };
        let mut config = self.config.clone();
        config.provider = Provider::Qweather;
        config.host = client.host.clone();
        config.location = Some(location.clone());
        if !config.saved.contains(&location) {
            if config.saved.len() >= 5 {
                self.message = "最多保存 5 个城市".into();
                return;
            }
            config.saved.push(location);
        }
        let old = config.credential.clone();
        config.credential = format!(
            "{}-{}",
            client.host,
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        );
        if let Err(e) = client.store(&config.credential) {
            self.message = e.text().into();
            return;
        }
        if let Err(e) = config.save() {
            let _ = Client::delete(&config.credential);
            self.message = e.text().into();
            return;
        }
        if !old.is_empty() {
            let _ = Client::delete(&old);
        }
        if let Some(c) = &self.client {
            c.gate.store(false, Ordering::SeqCst);
        }
        let mut client = client;
        client.gate = Arc::new(AtomicBool::new(true));
        self.client = Some(client);
        self.config = config;
        let (draft_host, draft_key) = initial_draft(&self.config);
        self.draft_host = draft_host;
        self.draft_key = draft_key;
        self.tested = None;
        self.runtime
            .reset(&self.config.host, self.config.location.clone());
        self.sync_snapshot();
        self.message = "已启用和风天气".into();
    }
    fn start_search(&mut self) {
        if self.search.is_some() || self.query.trim().is_empty() {
            return;
        }
        if let Some(c) = self
            .tested
            .as_ref()
            .or_else(|| {
                if self.is_active() {
                    self.client.as_ref()
                } else {
                    None
                }
            })
            .cloned()
        {
            let query = self.query.trim().to_string();
            let province = self.province.clone();
            self.search = Some(qweather::spawn(move || c.search(&query, &province)));
            self.search_sent = true;
            self.message = "正在搜索城市…".into();
        }
    }
    pub fn close_draft(&mut self) {
        if let Some(g) = self.draft_gate.take() {
            g.store(false, Ordering::SeqCst);
        }
        if let Some(t) = self.tested.take() {
            t.gate.store(false, Ordering::SeqCst);
        }
        self.testing = None;
        self.search = None;
        let (draft_host, draft_key) = initial_draft(&self.config);
        self.draft_host = draft_host;
        self.draft_key = draft_key;
        self.query.clear();
        self.province.clear();
        self.search_input = (String::new(), String::new());
        self.search_sent = false;
        self.results.clear();
        self.credits.clear();
        self.selected = self.config.provider;
        self.picked = self.config.location.clone();
        self.city_choice = None;
    }
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        crate::extras::settings_label(ui,"天气来源");
        crate::extras::settings_card(ui, |ui| {
            ui.horizontal(|ui| {
                let width = ((ui.available_width() - ui.spacing().item_spacing.x) * 0.5).max(120.0);
                if provider_option(
                    ui,
                    Provider::OpenMeteo,
                    self.selected == Provider::OpenMeteo,
                    width,
                )
                .clicked()
                {
                    self.selected = Provider::OpenMeteo;
                }
                if provider_option(
                    ui,
                    Provider::Qweather,
                    self.selected == Provider::Qweather,
                    width,
                )
                .clicked()
                {
                    self.selected = Provider::Qweather;
                }
            });
            ui.small(format!(
                "当前生效：{}{}",
                self.config.provider.name(),
                if self.config.provider != self.selected {
                    " · 所选来源尚未应用"
                } else {
                    ""
                }
            ));
            if self.selected == Provider::OpenMeteo {
                if self.is_active() && ui.button("应用 Open-Meteo").clicked() {
                    self.use_open();
                }
                return;
            }
            ui.separator();
            if ui
                .button(if self.show_config {
                    "和风配置 · 收起"
                } else {
                    "和风配置 · 展开"
                })
                .clicked()
            {
                self.show_config = !self.show_config;
            }
            if self.show_config {
                ui.strong("1  填写并测试连接");
                ui.label("API Host");
                let host = ui.add(
                    egui::TextEdit::singleline(&mut self.draft_host)
                        .hint_text("专属域名.qweatherapi.com")
                        .desired_width(f32::INFINITY),
                );
                paste_menu(&host);
                ui.label("API Key");
                let key = ui.add(
                    egui::TextEdit::singleline(&mut self.draft_key)
                        .password(true)
                        .hint_text("填写 API Key；留空保留已保存的 Key")
                        .desired_width(f32::INFINITY),
                );
                paste_menu(&key);
                if host.changed() || key.changed() {
                    if let Some(g) = self.draft_gate.take() {
                        g.store(false, Ordering::SeqCst);
                    }
                    if let Some(t) = self.tested.take() {
                        t.gate.store(false, Ordering::SeqCst);
                    }
                    self.testing = None;
                    self.search = None;
                    self.results.clear();
                }
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(self.testing.is_none(), egui::Button::new("测试连接"))
                        .clicked()
                    {
                        let gate = Arc::new(AtomicBool::new(true));
                        let candidate =
                            if self.draft_key.is_empty() && self.draft_host == self.config.host {
                                Client::load(&self.config.host, &self.config.credential, gate)
                            } else {
                                Client::new(&self.draft_host, &self.draft_key, gate).ok()
                            };
                        if let Some(client) = candidate {
                            self.draft_gate = Some(client.gate.clone());
                            self.testing = Some(qweather::spawn(move || {
                                client.test()?;
                                Ok(client)
                            }));
                            self.message = "正在测试连接…".into();
                        } else {
                            self.message = Error::Invalid.text().into();
                        }
                    }
                    if !self.config.credential.is_empty() && ui.button("清除本机 Key").clicked()
                    {
                        if self.is_active() {
                            self.use_open();
                        }
                        if !self.is_active() {
                            if Client::delete(&self.config.credential).is_ok() {
                                self.config.credential.clear();
                                let _ = self.config.save();
                                self.client = None;
                                self.tested = None;
                                self.message = "已清除 Key".into();
                            } else {
                                self.message = Error::Storage.text().into();
                            }
                        }
                    }
                });
                ui.small("默认 Key 随程序提供；保存后的 Key 使用 Windows 凭据管理器。需要 GeoAPI 与天气预报权限，使用量和费用以和风账号为准。");
                ui.small(format!(
                    "本机本月请求：{} / {}（搜索、连接测试与刷新均计入）",
                    qweather::usage(),
                    qweather::LIMIT
                ));
            }
        });
        if self.selected != Provider::Qweather {
            return;
        }
        crate::extras::settings_label(ui,"和风城市");
        crate::extras::settings_card(ui, |ui| {
            ui.strong("2  选择城市并启用");
            ui.small(format!("已保存城市 {} / 5；和风暂不支持自动定位。",self.config.saved.len()));
            if self.config.saved.is_empty(){ui.small("还没有保存城市，搜索后点「添加城市」，最多 5 个。");}
            ui.separator();
            let mut remove = None;
            let mut selected = None;
            for city in self.config.saved.clone() {
                let temp=self.snapshot.as_ref().filter(|s|self.config.location.as_ref()==Some(&city)&&s.place.lat==city.position.lat&&s.place.lon==city.position.lon).map(|s|s.reading.temp);
                let (clicked,delete)=super::settings_city_row(ui,&city.position.label,&city.position.region,temp,self.picked.as_ref()==Some(&city),true);
                if clicked {selected=Some(city.clone());}
                if delete {remove=Some(city);}
            }
            if let Some(city) = selected {
                self.picked = Some(city.clone());
                if self.is_active() {
                    self.choose(city);
                }
            }
            if let Some(city) = remove {
                let mut config = self.config.clone();
                config.saved.retain(|l| l != &city);
                if config.location.as_ref() == Some(&city) {
                    config.location = config.saved.first().cloned();
                }
                if config.save().is_ok() {
                    self.config = config;
                    self.picked = self.config.location.clone();
                    if self.is_active() {
                        if let Some(c) = &mut self.client {
                            c.gate.store(false, Ordering::SeqCst);
                            c.gate = Arc::new(AtomicBool::new(true));
                        }
                        self.runtime
                            .reset(&self.config.host, self.config.location.clone());
                        self.sync_snapshot();
                    }
                } else {
                    self.message = Error::Storage.text().into();
                }
            }
            let mut search_clicked = false;
            ui.horizontal_wrapped(|ui| {
                egui::ComboBox::from_id_salt("qweather-province")
                    .selected_text(if self.province.is_empty() {
                        "不限省份"
                    } else {
                        &self.province
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.province, String::new(), "不限省份");
                        for province in [
                            "北京",
                            "天津",
                            "河北",
                            "山西",
                            "内蒙古",
                            "辽宁",
                            "吉林",
                            "黑龙江",
                            "上海",
                            "江苏",
                            "浙江",
                            "安徽",
                            "福建",
                            "江西",
                            "山东",
                            "河南",
                            "湖北",
                            "湖南",
                            "广东",
                            "广西",
                            "海南",
                            "重庆",
                            "四川",
                            "贵州",
                            "云南",
                            "西藏",
                            "陕西",
                            "甘肃",
                            "青海",
                            "宁夏",
                            "新疆",
                            "香港",
                            "澳门",
                            "台湾",
                        ] {
                            ui.selectable_value(&mut self.province, province.into(), province);
                        }
                    });
                ui.add(
                    egui::TextEdit::singleline(&mut self.query).hint_text("输入城市、县或区名称"),
                );
                if ui
                    .add_enabled(
                        (self.tested.is_some() || (self.is_active() && self.client.is_some()))
                            && !self.query.trim().is_empty()
                            && self.search.is_none(),
                        egui::Button::new("搜索"),
                    )
                    .clicked()
                {
                    search_clicked = true;
                }
            });
            let input = (self.query.trim().to_string(), self.province.clone());
            let now = ui.input(|i| i.time);
            if input != self.search_input {
                self.search_input = input;
                self.search_changed_at = now;
                self.search_sent = false;
                self.search = None;
                self.results.clear();
                self.credits.clear();
                self.city_choice = None;
            }
            if search_clicked {
                self.start_search();
            }
            let query = self.query.trim();
            let ready = if query.is_ascii() {
                query.chars().count() >= 2
            } else {
                !query.is_empty()
            };
            if ready
                && !self.search_sent
                && self.search.is_none()
                && (self.tested.is_some() || (self.is_active() && self.client.is_some()))
            {
                if now - self.search_changed_at >= 0.45 {
                    if !cfg!(test) {
                        self.start_search();
                    }
                } else {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(60));
                }
            }
            for credit in &self.credits {
                ui.small(credit);
            }
            for city in self.results.clone() {
                if ui
                    .selectable_label(
                        self.city_choice.as_ref() == Some(&city),
                        format!("{} · {}", city.position.label, city.position.region),
                    )
                    .clicked()
                {
                    self.city_choice = Some(city);
                }
            }
            if ui
                .add_enabled(self.city_choice.is_some(), egui::Button::new("添加城市"))
                .clicked()
            {
                self.add_city();
            }
            if ui.button("保存并启用").clicked() {
                self.apply();
            }
            if !self.message.is_empty() {
                ui.add(egui::Label::new(&self.message).wrap());
            }
            if self.is_active() {
                if let Some(error) = self.runtime.error {
                    ui.add(egui::Label::new(error.text()).wrap());
                }
                let (current, daily) = self.runtime.stamps();
                let stamp = |time| {
                    chrono::DateTime::from_timestamp(time, 0)
                        .map(|t| {
                            t.with_timezone(&chrono::Local)
                                .format("%m-%d %H:%M")
                                .to_string()
                        })
                        .unwrap_or_else(|| "未取得".into())
                };
                ui.small(format!("实况 {} · 预报 {}", stamp(current), stamp(daily)));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn applying_source_preserves_open_meteo_and_city_selection_across_reload() {
        let _guard = crate::ui_tests::ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os("GUA_DATA_DIR");
        let dir = std::env::temp_dir().join(format!(
            "gua-qsource-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("GUA_DATA_DIR", &dir);
        let city = Location {
            position: super::super::qmodel::Position {
                lat: 22.5,
                lon: 114.1,
                label: "深圳".into(),
                region: "广东".into(),
            },
            timezone: "Asia/Shanghai".into(),
        };
        let mut source = Source::load();
        source.selected = Provider::Qweather;
        source.tested = Some(
            Client::new(
                "example.re.qweatherapi.com",
                "test-key",
                Arc::new(AtomicBool::new(true)),
            )
            .unwrap(),
        );
        source.picked = Some(city.clone());
        source.apply();
        assert!(source.is_active());
        let config = Config::load();
        assert!(config.provider == Provider::Qweather);
        assert!(config.location.as_ref() == Some(&city));
        assert!(config.saved.contains(&city));
        let second = Location {
            position: super::super::qmodel::Position {
                lat: 39.9,
                lon: 116.4,
                label: "北京".into(),
                region: "北京".into(),
            },
            timezone: "Asia/Shanghai".into(),
        };
        source.city_choice = Some(second.clone());
        source.add_city();
        assert!(
            source.config.location.as_ref() == Some(&city),
            "添加城市不应切换当前城市"
        );
        assert!(source.config.saved.contains(&second));
        source.use_open();
        assert!(!source.is_active());
        let config = Config::load();
        assert!(config.provider == Provider::OpenMeteo);
        assert!(config.saved.contains(&city));
        if let Some(value) = previous {
            std::env::set_var("GUA_DATA_DIR", value)
        } else {
            std::env::remove_var("GUA_DATA_DIR")
        };
        std::fs::remove_file(dir.join("weather-provider.json")).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
