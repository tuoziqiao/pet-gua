//! 节日提醒编辑及提醒。
use crate::{
    app::{App, Cmd, CALENDAR_PAGE},
    lunar,
    paint::{self, Card, H},
    theme::{self, W},
};
use chrono::{Local, TimeZone};
use eframe::egui::{self, Color32};

impl App {
    pub(crate) fn refresh_lunar_month(&mut self) {
        let month = self.calendar.view.month;
        if self.calendar.lunar_month == Some(month) {
            return;
        }
        let dates = crate::calendar::month_grid(month);
        let info: Vec<_> = dates.iter().map(|date| lunar::day_info(*date)).collect();
        self.calendar.birthday_events = dates
            .iter()
            .zip(&info)
            .flat_map(|(date, info)| {
                self.calendar
                    .birthdays
                    .iter()
                    .filter_map(move |birthday| birthday.occurrence(*date, info.as_ref()?))
            })
            .collect();
        self.calendar.lunar_days = info;
        self.calendar.lunar_month = Some(month);
    }
    pub(crate) fn open_birthday_editor(&mut self, uid: Option<&str>) {
        let draft = uid
            .and_then(|uid| self.calendar.birthdays.iter().find(|b| b.uid == uid))
            .map(lunar::Draft::edit)
            .unwrap_or_else(|| lunar::Draft::new(self.calendar.view.day));
        self.show_page(CALENDAR_PAGE);
        self.calendar.draft = None;
        self.calendar.birthday_draft = Some(draft);
    }
    fn save_birthday_editor(&mut self) {
        let Some(mut draft) = self.calendar.birthday_draft.take() else {
            return;
        };
        let result = (|| {
            if !self.calendar.birthday_error.is_empty() {
                return Err(self.calendar.birthday_error.clone());
            }
            let old = draft
                .uid
                .as_ref()
                .and_then(|uid| self.calendar.birthdays.iter().find(|b| &b.uid == uid));
            let birthday = draft.entry(old)?;
            let mut items = self.calendar.birthdays.clone();
            let uid = birthday.uid.clone();
            if let Some(old) = items.iter_mut().find(|b| b.uid == birthday.uid) {
                *old = birthday;
            } else {
                items.push(birthday);
            }
            lunar::save(&items)?;
            self.calendar.birthdays = items;
            self.clear_birthday_alerts(&uid);
            self.calendar.lunar_month = None;
            self.calendar.birthday_checked_at = 0;
            Ok::<(), String>(())
        })();
        if let Err(error) = result {
            draft.error = error;
            self.calendar.birthday_draft = Some(draft);
        }
    }
    fn delete_birthday_editor(&mut self) {
        let Some(draft) = self.calendar.birthday_draft.as_mut() else {
            return;
        };
        let Some(uid) = draft.uid.clone() else {
            return;
        };
        if !self.calendar.birthday_error.is_empty() {
            draft.error = self.calendar.birthday_error.clone();
            return;
        }
        let items: Vec<_> = self
            .calendar
            .birthdays
            .iter()
            .filter(|b| b.uid != uid)
            .cloned()
            .collect();
        if let Err(error) = lunar::save(&items) {
            draft.error = error;
            return;
        }
        self.calendar.birthdays = items;
        self.clear_birthday_alerts(&uid);
        self.calendar.lunar_month = None;
        self.calendar.birthday_checked_at = 0;
        self.calendar.birthday_draft = None;
        self.calendar.view.focus = None;
    }
    fn clear_birthday_alerts(&mut self, uid: &str) {
        self.alerts.retain(|alert| {
            !alert
                .meeting
                .as_ref()
                .is_some_and(|(event, _)| event.uid == uid)
        });
        if self.notice.as_ref().is_some_and(|alert| {
            alert
                .meeting
                .as_ref()
                .is_some_and(|(event, _)| event.uid == uid)
        }) {
            self.notice = None;
        }
    }
    pub(crate) fn draw_birthday_editor(&mut self, c: &Card) {
        let Some(d) = self.calendar.birthday_draft.as_mut() else {
            return;
        };
        let accent = Color32::from_rgb(82, 162, 255);
        c.centered_text(
            43.0,
            20.0,
            if d.uid.is_some() {
                "编辑节日提醒"
            } else {
                "新建节日提醒"
            },
            13.0,
            W::Semibold,
            theme::WHITE,
            H::Left,
        );
        for (label, y) in [
            ("名称", 49.0),
            ("输入", 80.0),
            ("日期", 113.0),
            ("提醒", 149.0),
        ] {
            c.centered_text(43.0, y, label, 10.0, W::Regular, theme::GRAY, H::Left);
        }
        c.fill(86.0, 38.0, 284.0, 23.0, 6.0, theme::CARD);
        for (i, (mode, label)) in [(false, "农历"), (true, "公历转农历")]
            .into_iter()
            .enumerate()
        {
            let x = 86.0 + i as f32 * 105.0;
            let (hit, b) = paint::capsule(
                c,
                x,
                69.0,
                101.0,
                21.0,
                &format!("birthday-mode-{i}"),
                if d.solar_mode == mode {
                    Color32::from_rgb(28, 65, 100)
                } else {
                    theme::BTN
                },
                theme::BTN_HOVER,
            );
            b.centered_text(
                x + 50.5,
                79.5,
                label,
                9.0,
                W::Regular,
                theme::WHITE,
                H::Center,
            );
            if hit.clicked() {
                d.solar_mode = mode;
                d.error.clear();
            }
        }
        if d.solar_mode {
            c.fill(86.0, 102.0, 145.0, 23.0, 6.0, theme::CARD);
            c.centered_text(
                241.0,
                113.0,
                "年-月-日",
                9.0,
                W::Regular,
                theme::GRAY,
                H::Left,
            );
        } else {
            c.fill(86.0, 102.0, 48.0, 23.0, 6.0, theme::CARD);
            c.centered_text(140.0, 113.0, "月", 10.0, W::Regular, theme::GRAY, H::Left);
            c.fill(158.0, 102.0, 48.0, 23.0, 6.0, theme::CARD);
            c.centered_text(212.0, 113.0, "日", 10.0, W::Regular, theme::GRAY, H::Left);
            let (hit, b) = paint::capsule(
                c,
                238.0,
                102.0,
                80.0,
                23.0,
                "birthday-leap",
                if d.leap {
                    Color32::from_rgb(28, 65, 100)
                } else {
                    theme::BTN
                },
                theme::BTN_HOVER,
            );
            if d.leap {
                paint::check(&b, 267.5, 113.5, 7.0, 1.0, 1.2, theme::WHITE);
            }
            b.centered_text(
                if d.leap { 282.5 } else { 278.0 },
                113.5,
                if d.leap { "闰月" } else { "普通月" },
                9.0,
                W::Regular,
                theme::WHITE,
                H::Center,
            );
            if hit.clicked() {
                d.leap = !d.leap;
            }
        }
        for (i, (value, label)) in [(-1, "不提醒"), (0, "当天 09:00"), (1, "前一天 09:00")]
            .into_iter()
            .enumerate()
        {
            let x = 86.0 + i as f32 * 95.0;
            let (hit, b) = paint::capsule(
                c,
                x,
                138.0,
                91.0,
                21.0,
                &format!("birthday-reminder-{i}"),
                if d.reminder == value {
                    Color32::from_rgb(28, 65, 100)
                } else {
                    theme::BTN
                },
                theme::BTN_HOVER,
            );
            b.centered_text(
                x + 45.5,
                148.5,
                label,
                8.3,
                W::Regular,
                theme::WHITE,
                H::Center,
            );
            if hit.clicked() {
                d.reminder = value;
            }
        }
        let msg = if d.error.is_empty() {
            "每年按农历重复 · 闰月仅对应闰年"
        } else {
            &d.error
        };
        c.centered_text(
            86.0,
            177.0,
            &c.fit(msg, 9.0, W::Regular, 284.0),
            9.0,
            W::Regular,
            if d.error.is_empty() {
                theme::GRAY
            } else {
                theme::RED
            },
            H::Left,
        );
        let mut delete = false;
        if d.uid.is_some() {
            let (hit, b) = paint::capsule(
                c,
                43.0,
                194.0,
                82.0,
                22.0,
                "birthday-delete",
                theme::BTN,
                theme::BTN_HOVER,
            );
            b.centered_text(
                84.0,
                205.0,
                if d.confirm_delete {
                    "确认删除"
                } else {
                    "删除提醒"
                },
                9.5,
                W::Regular,
                theme::RED,
                H::Center,
            );
            if hit.clicked() {
                if d.confirm_delete {
                    delete = true;
                } else {
                    d.confirm_delete = true;
                }
            }
        }
        let (cancel, b) = paint::capsule(
            c,
            224.0,
            194.0,
            64.0,
            22.0,
            "birthday-cancel",
            theme::BTN,
            theme::BTN_HOVER,
        );
        b.centered_text(
            256.0,
            205.0,
            "取消",
            10.0,
            W::Regular,
            theme::WHITE,
            H::Center,
        );
        let (save, b) = paint::capsule(
            c,
            298.0,
            194.0,
            72.0,
            22.0,
            "birthday-save",
            accent,
            Color32::from_rgb(110, 180, 255),
        );
        b.centered_text(
            334.0,
            205.0,
            "保存",
            10.0,
            W::Semibold,
            theme::WHITE,
            H::Center,
        );
        if delete {
            self.delete_birthday_editor();
        } else if cancel.clicked() {
            self.calendar.birthday_draft = None;
        } else if save.clicked() {
            self.save_birthday_editor();
        }
    }
    pub(crate) fn birthday_editor_inputs(
        &mut self,
        ui: &mut egui::Ui,
        origin: egui::Pos2,
        scale: f32,
        cmds: &mut Vec<Cmd>,
    ) {
        let Some(d) = self.calendar.birthday_draft.as_mut() else {
            return;
        };
        let mut input = |text: &mut String, id: &str, x: f32, y: f32, w: f32| {
            ui.put(
                egui::Rect::from_min_size(
                    origin + egui::vec2(x, y) * scale,
                    egui::vec2(w, 19.0) * scale,
                ),
                egui::TextEdit::singleline(text)
                    .id(egui::Id::new(id))
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .text_color(theme::WHITE)
                    .font(theme::font(11.0 * scale, W::Regular)),
            )
        };
        let name = input(&mut d.name, "birthday-name", 92.0, 41.0, 272.0);
        if d.solar_mode {
            input(&mut d.solar, "birthday-solar", 92.0, 105.0, 133.0);
        } else {
            input(&mut d.month, "birthday-month", 92.0, 105.0, 36.0);
            input(&mut d.day, "birthday-day", 164.0, 105.0, 36.0);
        }
        let first = !d.focused;
        if first {
            name.request_focus();
            d.focused = true;
            cmds.push(Cmd::Focus);
        }
        if !first && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            self.save_birthday_editor();
        }
    }
    pub(crate) fn check_birthday_reminders(&mut self, now_ts: i64) {
        if !self.settings.notifications
            || self.calendar.birthdays.is_empty()
            || !self.calendar.birthday_error.is_empty()
            || (now_ts >= self.calendar.birthday_checked_at
                && now_ts - self.calendar.birthday_checked_at < 15)
        {
            return;
        }
        self.calendar.birthday_checked_at = now_ts;
        let Some(now) =
            chrono::DateTime::from_timestamp(now_ts, 0).map(|v| v.with_timezone(&Local))
        else {
            return;
        };
        let today = now.date_naive();
        let mut items = self.calendar.birthdays.clone();
        let mut alerts = Vec::new();
        for day in [Some(today), today.succ_opt()].into_iter().flatten() {
            let Some(info) = lunar::day_info(day) else {
                continue;
            };
            for b in &mut items {
                if b.reminder < 0
                    || b.last_notified == Some(day)
                    || (day != today && b.reminder != 1)
                {
                    continue;
                }
                let due_day = if b.reminder == 1 {
                    day.pred_opt().unwrap_or(day)
                } else {
                    day
                };
                let Some(due) = due_day
                    .and_hms_opt(9, 0, 0)
                    .and_then(|v| Local.from_local_datetime(&v).earliest())
                else {
                    continue;
                };
                if now_ts >= due.timestamp() {
                    if let Some(event) = b.occurrence(day, &info) {
                        b.last_notified = Some(day);
                        alerts.push(event.event);
                    }
                }
            }
        }
        if alerts.is_empty() {
            return;
        }
        if let Err(error) = lunar::save(&items) {
            self.calendar.birthday_error = error;
            return;
        }
        self.calendar.birthdays = items;
        for event in alerts {
            self.birthday_alert(event);
        }
    }
}
