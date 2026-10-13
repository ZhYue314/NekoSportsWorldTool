//! 账号页：多账号管理 + 批量链（登录 → 自查 → 跑步 → 登出）。
//!
//! 存储：`accounts.json`（索引 + 凭据）+ `accounts/<id>/{identity,session}.json`。
//! 每账号一份独立设备身份（首次生成后终身复用）；批处理严格串行 + 随机间隔，
//! 避免设备/会话错配与同 IP 时间聚集。

use super::{mobile, theme, App, BATCH};
use crate::api::model::{self, AccountEntry, Config};
use crate::platform::InputKind;
use eframe::egui;
use std::collections::HashMap;
use std::sync::mpsc::Sender;

/// 账号间随机间隔下限 / 上限（秒）。串行执行，稀释同 IP 登录与提交的时间密度。
const GAP_MIN_SECS: u64 = 60;
const GAP_MAX_SECS: u64 = 180;

#[derive(Default)]
pub struct AccountsPage {
    pub entries: Vec<AccountEntry>,
    /// 账号 id → 最近一次操作状态。
    pub status: HashMap<String, String>,
    /// 账号 id → 设备/位置明细（避免每帧读盘）。
    pub info: HashMap<String, String>,
    pub adding: bool,
    pub new_user: String,
    pub new_pass: String,
    /// 新账号的设备与位置（创建时写入槽位 identity；UUID 留空则自动生成，
    /// IDFA / IMEI 留空则不上报）。
    pub new_platform: String,
    pub new_device_name: String,
    pub new_os_version: String,
    pub new_city: String,
    pub new_lat: String,
    pub new_lon: String,
    pub new_device_id: String,
    pub new_idfa: String,
    pub new_imei: String,
    /// 展开了「详情」的账号。
    pub expanded: std::collections::HashSet<String>,
    pub batch_running: bool,
    pub batch_done: usize,
    pub batch_total: usize,
}

impl AccountsPage {
    pub fn new() -> Self {
        let mut page = Self {
            entries: model::load_accounts(),
            ..Default::default()
        };
        page.refresh_info();
        page
    }

    /// 读取各账号槽位的设备身份，生成明细文本（避免每帧读盘）。
    fn refresh_info(&mut self) {
        let mut info = HashMap::new();
        for e in &self.entries {
            let text = match model::load_identity_of(&e.id) {
                Ok(id) => {
                    let sess = model::load_session_of(&e.id);
                    let login = if sess.is_logged_in() {
                        format!("已登录 uid={} {}", sess.uid, sess.name)
                    } else {
                        "未登录".to_string()
                    };
                    format!(
                        "槽位：{slot}\n账号：{user}（{login}）\nUUID：{device_id}\nIDFA：{idfa}\nIMEI：{imei}\nMAC：{mac}　安装时间：{install}\n平台：{platform}　机型：{name}　系统：{os}\n位置：{city}（{lat:.6},{lon:.6}）",
                        slot = e.id,
                        user = e.username,
                        device_id = id.device_id,
                        idfa = if id.idfa.is_empty() { "（空）".into() } else { id.idfa.clone() },
                        imei = if id.imei.is_empty() { "（空）".into() } else { id.imei.clone() },
                        mac = id.mac_address,
                        install = crate::cli::fmt_hms(id.app_install_time),
                        platform = if id.platform == "android" { "Android" } else { "iOS" },
                        name = id.device_name,
                        os = id.os_version,
                        city = id.city,
                        lat = id.anchor_lat,
                        lon = id.anchor_lon,
                    )
                }
                Err(err) => format!("× 读取设备失败：{err}"),
            };
            info.insert(e.id.clone(), text);
        }
        self.info = info;
    }

    /// 用给定设备身份填充新建表单（打开添加面板 / 点「填充当前设备」时）。
    fn fill_from(&mut self, identity: &model::HeaderIdentity) {
        self.new_platform = identity.platform.clone();
        self.new_device_name = identity.device_name.clone();
        self.new_os_version = identity.os_version.clone();
        self.new_city = identity.city.clone();
        self.new_lat = format!("{:.6}", identity.anchor_lat);
        self.new_lon = format!("{:.6}", identity.anchor_lon);
        self.new_device_id = identity.device_id.clone();
        self.new_idfa = identity.idfa.clone();
        self.new_imei = identity.imei.clone();
    }

    /// 只填位置为代码默认值（都匀）。
    fn fill_default_place(&mut self) {
        let d = model::HeaderIdentity::default();
        self.new_city = d.city;
        self.new_lat = format!("{:.6}", d.anchor_lat);
        self.new_lon = format!("{:.6}", d.anchor_lon);
    }
}

/// 登录成功后把昵称写回账号索引（展示名）。
pub fn note_login(name: &str, username: &str) {
    let mut list = model::load_accounts();
    let Some(acc) = list.iter_mut().find(|a| a.username == username) else {
        return;
    };
    acc.label = name.to_string();
    let _ = model::save_accounts(&list);
}

impl App {
    pub fn draw_accounts(&mut self, ui: &mut egui::Ui) {
        let mut page = std::mem::take(&mut self.accounts_page);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| self.draw_accounts_inner(ui, &mut page));
        self.accounts_page = page;
    }

    fn draw_accounts_inner(&mut self, ui: &mut egui::Ui, page: &mut AccountsPage) {
        ui.add_space(6.0);
        ui.colored_label(
            theme::text_dim(),
            "每个账号使用独立的设备身份与会话；批量任务串行执行，账号之间随机间隔 60-180 秒。",
        );
        ui.add_space(8.0);

        // ── 批量 ───────────────────────────────────────────────
        let checked = page.entries.iter().filter(|e| e.enabled).count();
        let mut start = false;
        mobile::row(ui, |ui| {
            let running = page.batch_running;
            let label = if running {
                format!("批量进行中 {}/{}", page.batch_done, page.batch_total)
            } else {
                format!(
                    "一键：登录 → 自查 → 跑步 → 登出（{checked}/{}）",
                    page.entries.len()
                )
            };
            let btn = theme::primary_btn(&label);
            if ui.add_enabled(!running && checked > 0, btn).clicked() {
                start = true;
            }
            if running {
                ui.colored_label(theme::warn(), "批量运行中，单账号操作已锁定");
            } else if checked == 0 && !page.entries.is_empty() {
                ui.colored_label(theme::warn(), "未勾选任何账号");
            }
        });
        if start {
            self.start_batch(page);
        }

        // ── 添加 ───────────────────────────────────────────────
        ui.add_space(6.0);
        let mut toggle_add = false;
        let mut fill_now = false;
        let mut fill_default_place = false;
        mobile::row(ui, |ui| {
            if ui.button(if page.adding { "收起添加" } else { "添加账号" }).clicked() {
                toggle_add = true;
            }
            ui.colored_label(theme::text_dim(), format!("已保存 {} 个账号", page.entries.len()));
        });
        if toggle_add {
            page.adding = !page.adding;
            if page.adding {
                page.fill_from(&self.identity);
            }
        }
        if page.adding {
            egui::Grid::new("account_add")
                .num_columns(2)
                .spacing([10.0, 3.0])
                .show(ui, |ui| {
                    ui.label("账号：");
                    mobile::text_edit(ui, "acc_user", &mut page.new_user, InputKind::Text, 240.0);
                    ui.end_row();
                    ui.label("密码：");
                    mobile::text_edit(
                        ui,
                        "acc_pass",
                        &mut page.new_pass,
                        InputKind::Password,
                        240.0,
                    );
                    ui.end_row();
                });

            ui.add_space(4.0);
            mobile::row(ui, |ui| {
                ui.label(egui::RichText::new("设备与位置").strong());
                if ui.button("填充当前设备").clicked() {
                    fill_now = true;
                }
                if ui.button("默认位置").clicked() {
                    fill_default_place = true;
                }
                ui.colored_label(theme::text_dim(), "UUID 留空自动生成；MAC / 安装时间创建时生成");
            });
            if fill_now {
                page.fill_from(&self.identity);
            }
            if fill_default_place {
                page.fill_default_place();
            }
            // 左：平台 / 机型 / 系统版本；右：城市 / 纬度 / 经度
            ui.columns(2, |cols| {
                egui::Grid::new("account_device")
                    .num_columns(2)
                    .spacing([8.0, 3.0])
                    .show(&mut cols[0], |ui| {
                        ui.label("平台：");
                        ui.horizontal(|ui| {
                            ui.selectable_value(&mut page.new_platform, "ios".to_string(), "iOS");
                            ui.selectable_value(
                                &mut page.new_platform,
                                "android".to_string(),
                                "Android",
                            );
                        });
                        ui.end_row();
                        ui.label("机型：");
                        mobile::text_edit(
                            ui,
                            "acc_model",
                            &mut page.new_device_name,
                            InputKind::Text,
                            150.0,
                        );
                        ui.end_row();
                        ui.label("系统版本：");
                        mobile::text_edit(
                            ui,
                            "acc_os",
                            &mut page.new_os_version,
                            InputKind::Text,
                            150.0,
                        );
                        ui.end_row();
                    });
                egui::Grid::new("account_place")
                    .num_columns(2)
                    .spacing([8.0, 3.0])
                    .show(&mut cols[1], |ui| {
                        ui.label("城市：");
                        mobile::text_edit(
                            ui,
                            "acc_city",
                            &mut page.new_city,
                            InputKind::Text,
                            150.0,
                        );
                        ui.end_row();
                        ui.label("纬度：");
                        mobile::text_edit(
                            ui,
                            "acc_lat",
                            &mut page.new_lat,
                            InputKind::Decimal,
                            150.0,
                        );
                        ui.end_row();
                        ui.label("经度：");
                        mobile::text_edit(
                            ui,
                            "acc_lon",
                            &mut page.new_lon,
                            InputKind::Decimal,
                            150.0,
                        );
                        ui.end_row();
                    });
            });

            ui.add_space(4.0);
            egui::Grid::new("account_ids")
                .num_columns(2)
                .spacing([10.0, 3.0])
                .show(ui, |ui| {
                    ui.label("UUID：");
                    mobile::text_edit(
                        ui,
                        "acc_uuid",
                        &mut page.new_device_id,
                        InputKind::Text,
                        240.0,
                    );
                    ui.end_row();
                    ui.label("IDFA：");
                    mobile::text_edit(ui, "acc_idfa", &mut page.new_idfa, InputKind::Text, 240.0);
                    ui.end_row();
                    ui.label("IMEI：");
                    mobile::text_edit(ui, "acc_imei", &mut page.new_imei, InputKind::Text, 240.0);
                    ui.end_row();
                });

            let create = !self.batch_busy
                && !page.new_user.trim().is_empty()
                && !page.new_pass.is_empty()
                && !page.new_city.trim().is_empty()
                && !page.new_lat.trim().is_empty()
                && !page.new_lon.trim().is_empty();
            if ui
                .add_enabled(create, theme::primary_btn("创建账号"))
                .clicked()
            {
                self.create_account(page);
            }
        }

        // ── 列表 ───────────────────────────────────────────────
        ui.add_space(10.0);
        ui.separator();
        let mut switch_to: Option<String> = None;
        let mut delete: Option<String> = None;
        let mut enable_change: Option<(String, bool)> = None;
        let mut toggle_detail: Option<(String, bool)> = None;
        for i in 0..page.entries.len() {
            let id = page.entries[i].id.clone();
            let title = page.entries[i].display().to_string();
            let username = page.entries[i].username.clone();
            let status = page.status.get(&id).cloned().unwrap_or_default();
            let detail = page.info.get(&id).cloned().unwrap_or_default();
            let active = self.slot.as_deref() == Some(id.as_str());
            let locked = self.batch_busy || page.batch_running;
            let open = page.expanded.contains(&id);
            mobile::row(ui, |ui| {
                let mut checked = page.entries[i].enabled;
                if ui
                    .checkbox(&mut checked, "")
                    .on_hover_text("勾选后才参与一键批量")
                    .changed()
                {
                    enable_change = Some((id.clone(), checked));
                }
                let mark = if active { "● " } else { "○ " };
                ui.label(egui::RichText::new(format!("{mark}{title}")).strong());
                ui.monospace(&username);
                if !status.is_empty() {
                    let color = if status.starts_with('√') {
                        theme::ok()
                    } else if status.starts_with('×') {
                        theme::err()
                    } else if status.starts_with('⚠') {
                        theme::warn()
                    } else {
                        theme::text_dim()
                    };
                    ui.colored_label(color, &status);
                }
                if ui
                    .button(if open { "收起" } else { "详情" })
                    .clicked()
                {
                    toggle_detail = Some((id.clone(), !open));
                }
                if ui
                    .add_enabled(!locked && !active, egui::Button::new("切换"))
                    .clicked()
                {
                    switch_to = Some(id.clone());
                }
                if ui
                    .add_enabled(!locked, egui::Button::new("删除"))
                    .clicked()
                {
                    delete = Some(id.clone());
                }
            });
            if open && !detail.is_empty() {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.colored_label(theme::text_dim(), &detail);
                });
            }
        }
        if page.entries.is_empty() {
            ui.colored_label(theme::text_dim(), "还没有账号，点上方「添加账号」。");
        }
        if let Some((id, enabled)) = enable_change {
            if let Some(e) = page.entries.iter_mut().find(|e| e.id == id) {
                e.enabled = enabled;
            }
            let _ = model::save_accounts(&page.entries);
        }
        if let Some((id, open)) = toggle_detail {
            if open {
                page.expanded.insert(id);
            } else {
                page.expanded.remove(&id);
            }
        }
        if let Some(id) = switch_to {
            self.switch_account(&id);
        }
        if let Some(id) = delete {
            self.delete_account(page, &id);
        }
    }

    /// 创建账号：独立槽位 + 全新设备指纹（设备/城市/锚点取表单值）。
    fn create_account(&mut self, page: &mut AccountsPage) {
        let username = page.new_user.trim().to_string();
        let password = page.new_pass.clone();
        let city = page.new_city.trim().to_string();
        let device_name = page.new_device_name.trim().to_string();
        let lat: f64 = page.new_lat.trim().parse().unwrap_or(f64::NAN);
        let lon: f64 = page.new_lon.trim().parse().unwrap_or(f64::NAN);
        if username.is_empty() || password.is_empty() {
            self.status = "请填写账号和密码".into();
            return;
        }
        if device_name.is_empty() {
            self.status = "请填写机型".into();
            return;
        }
        if city.is_empty() {
            self.status = "请填写城市".into();
            return;
        }
        let coord = match crate::location::Coordinate::new(lat, lon, 0.0) {
            Ok(c) => c,
            Err(e) => {
                self.status = format!("经纬度无效：{e}");
                return;
            }
        };
        let mut list = model::load_accounts();
        if list.iter().any(|a| a.username == username) {
            self.status = "该账号已存在".into();
            return;
        }
        let id = model::new_account_id();
        let mut identity = model::HeaderIdentity {
            platform: page.new_platform.clone(),
            device_name,
            os_version: page.new_os_version.trim().to_string(),
            city,
            anchor_lat: coord.latitude,
            anchor_lon: coord.longitude,
            ..self.identity.clone()
        };
        if identity.os_version.is_empty() {
            identity.os_version = model::HeaderIdentity::default().os_version;
        }
        // UUID 留空则生成；IDFA / IMEI 按表单（留空不上报）
        identity.device_id = if page.new_device_id.trim().is_empty() {
            String::new()
        } else {
            page.new_device_id.trim().to_uppercase()
        };
        identity.idfa = page.new_idfa.trim().to_string();
        identity.imei = page.new_imei.trim().to_string();
        // 补齐设备指纹：device_id 为空才生成，MAC / 安装时间新生成，创建后终身复用
        identity.freshen();
        if let Err(e) = model::save_identity_scoped(Some(&id), &identity) {
            self.status = format!("× 创建失败：{e}");
            return;
        }
        list.push(AccountEntry {
            id: id.clone(),
            username: username.clone(),
            password,
            remember: true,
            label: String::new(),
            enabled: true,
        });
        if let Err(e) = model::save_accounts(&list) {
            self.status = format!("× 保存账号索引失败：{e}");
            return;
        }
        page.entries = list;
        page.refresh_info();
        page.new_user.clear();
        page.new_pass.clear();
        page.adding = false;
        self.status = format!("√ 已添加账号 {username}（设备 {}）", identity.device_id);
        self.log.push(&format!(
            "√ [accounts] 新增账号 {username}（槽位 {id} / 设备 {} / {} / {}）",
            identity.device_id, identity.device_name, identity.city
        ));
    }

    /// 切换当前账号：把该槽位的设备身份与会话载入主界面。
    fn switch_account(&mut self, id: &str) {
        let identity = match model::load_identity_of(id) {
            Ok(i) => i,
            Err(e) => {
                self.status = format!("× 切换失败：{e}");
                return;
            }
        };
        let session = model::load_session_of(id);
        self.identity = identity.clone();
        self.device_buf = identity;
        self.session = session.is_logged_in().then_some(session);
        self.slot = Some(id.to_string());
        let entry = model::load_accounts().into_iter().find(|a| a.id == id);
        let label = entry
            .as_ref()
            .map(|a| a.display().to_string())
            .unwrap_or_else(|| id.to_string());
        // 顶栏输入框跟着切到该账号，后续登录/登出都作用在它身上
        if let Some(e) = &entry {
            self.username = e.username.clone();
            self.password = e.password.clone();
            self.remember = e.remember;
        }
        self.status = format!("已切换到账号 {label}（设备 {}）", self.identity.device_id);
        if self.session.is_some() {
            self.refresh_data_page();
            self.refresh_records();
            self.refresh_user_page();
        } else {
            self.data_page.semester = None;
            self.data_page.cheat = None;
            self.records_page.rows.clear();
        }
    }

    fn delete_account(&mut self, page: &mut AccountsPage, id: &str) {
        if self.batch_busy || page.batch_running {
            self.status = "批量任务进行中，无法删除".into();
            return;
        }
        let list: Vec<AccountEntry> = page
            .entries
            .iter()
            .filter(|a| a.id != id)
            .cloned()
            .collect();
        if let Err(e) = model::save_accounts(&list) {
            self.status = format!("× 删除失败：{e}");
            return;
        }
        model::remove_slot(id);
        page.entries = list;
        page.status.remove(id);
        page.info.remove(id);
        if self.slot.as_deref() == Some(id) {
            // 当前账号被删：退回全局单账号状态
            self.slot = None;
            self.session = None;
            self.identity = model::load_identity();
            self.device_buf = self.identity.clone();
            self.status = "已删除当前账号，回到默认身份".into();
        } else {
            self.status = "账号已删除".into();
        }
    }

    fn start_batch(&mut self, page: &mut AccountsPage) {
        if page.batch_running || self.batch_busy {
            return;
        }
        // 只处理勾选了的账号
        let entries: Vec<AccountEntry> = page
            .entries
            .iter()
            .filter(|e| e.enabled)
            .cloned()
            .collect();
        if entries.is_empty() {
            self.status = "没有勾选任何账号".into();
            return;
        }
        // 跑步参数取全局配置（距离/配速/海拔/人脸/路线），与单账号跑步页一致
        let cfg = model::load_config();
        page.batch_running = true;
        page.batch_done = 0;
        page.batch_total = entries.len();
        page.status.clear();
        self.batch_busy = true;
        self.status = format!("批量任务启动：{} 个账号", entries.len());
        self.log
            .push(&format!("[batch] 开始处理 {} 个账号（串行 + 随机间隔）", entries.len()));
        self.spawn_job(move |tx| batch_run(tx, entries, cfg));
    }
}

/// 批处理主循环：严格串行，账号之间随机间隔。
fn batch_run(tx: Sender<String>, entries: Vec<AccountEntry>, cfg: Config) {
    let total = entries.len();
    for (i, acc) in entries.iter().enumerate() {
        let tag = acc.display().to_string();
        let mut log = |s: &str| {
            tx.send(format!("[{}] {}", tag, crate::textlog::clean(s))).ok();
        };
        let progress = |status: &str| {
            tx.send(format!(
                "{BATCH}{}",
                serde_json::json!({
                    "id": acc.id,
                    "status": status,
                    "done": i + 1,
                    "total": total,
                })
            ))
            .ok();
        };
        progress("处理中…");
        let outcome = run_account(acc, &cfg, &mut log);
        progress(&outcome);
        if i + 1 < total {
            let secs = GAP_MIN_SECS + rand::random::<u64>() % (GAP_MAX_SECS - GAP_MIN_SECS + 1);
            tx.send(format!(
                "[batch] 间隔 {}s 后处理下一个账号（{}/{}）",
                secs,
                i + 1,
                total
            ))
            .ok();
            std::thread::sleep(std::time::Duration::from_secs(secs));
        }
    }
    tx.send(format!(
        "{BATCH}{}",
        serde_json::json!({ "done": total, "total": total, "finished": true })
    ))
    .ok();
}

/// 单账号链：登录 → 自查 → 跑步 → 登出。返回给人看的状态文本。
fn run_account(acc: &AccountEntry, cfg: &Config, log: &mut dyn FnMut(&str)) -> String {
    let identity = match model::load_identity_of(&acc.id) {
        Ok(i) => i,
        Err(e) => return format!("× {e}"),
    };
    if identity.device_id.is_empty() {
        return "× 设备身份缺失，跳过".into();
    }
    if identity.has_unconfigured_default_location() {
        return "× 未配置城市/锚点，跳过（切换到该账号后在设备页填写）".into();
    }

    let mut client =
        crate::api::client::ApiClient::new(identity, None).with_slot(Some(acc.id.clone()));

    // ① 登录（有密码走完整登录链；否则复用本地会话）
    let session = if !acc.password.is_empty() {
        match crate::api::login::login(&mut client, &acc.username, &acc.password, log) {
            Ok(s) => s,
            Err(e) => return format!("× 登录失败: {e}"),
        }
    } else {
        let s = model::load_session_of(&acc.id);
        if !s.is_logged_in() {
            return "× 无密码且本地无有效会话".into();
        }
        log("[login] 复用本地会话");
        s
    };
    log(&format!(
        "[login] √ uid={} name={} weight={}",
        session.uid, session.name, session.weight
    ));
    note_login(&session.name, &acc.username);
    client.login = Some(session);

    // ② 自查
    let cheat = match crate::api::cheat::query(&mut client, 1, log) {
        Ok(r) if r.is_clean() => "自查正常".to_string(),
        Ok(r) => format!("⚠ 自查被标记：{}", r.self_brief()),
        Err(e) => {
            log(&format!("⚠ 自查失败（继续）: {e}"));
            "自查失败".to_string()
        }
    };

    // ③ 跑步
    let (dist, dur, start_ms) = pick_run(cfg);
    log(&format!(
        "[run] {:.2}km / {}s / 开始 {}",
        dist / 1000.0,
        dur,
        crate::cli::fmt_hms(start_ms)
    ));
    let params = crate::api::flow::RunParams {
        dist,
        dur,
        start_ms,
        face_check: if cfg.face_check { 1 } else { 0 },
        manual_altitude: cfg.manual_altitude,
        manual_altitude_range: cfg.manual_altitude_range,
        seed: rand::random::<u64>(),
        route_mode: crate::track::generate_road::RouteMode::from_str(&cfg.route_mode),
    };
    let run = match crate::api::flow::run_full_flow(&mut client, &params, log) {
        Ok(out) => format!("√ 跑步 rrid={}", out.result.rrid),
        Err(e) => {
            // 跑步失败也执行登出，不留挂着的会话
            crate::api::login::logout(&mut client, log);
            return format!("× 跑步失败（已登出）: {e}；{cheat}");
        }
    };

    // ④ 登出
    crate::api::login::logout(&mut client, log);
    format!("{run}，{cheat}")
}

/// 抽一组跑步参数：距离/配速取配置区间，开始时刻落在今天 7:00 至「现在 - 用时 - 抖动余量」。
fn pick_run(cfg: &Config) -> (f64, i64, i64) {
    let (dlo, dhi) = (cfg.dist_min.min(cfg.dist_max), cfg.dist_min.max(cfg.dist_max));
    let (plo, phi) = (cfg.pace_min.min(cfg.pace_max), cfg.pace_min.max(cfg.pace_max));
    let pace = plo + (phi - plo) * rand::random::<f32>();
    let dist_km = dlo as f64 + (dhi as f64 - dlo as f64) * rand::random::<f64>();
    let dist = dist_km * 1000.0;
    let dur = (dist_km * pace as f64).round() as i64;
    (dist, dur, random_start_today(dur))
}

/// 今天 7:00 至 now-dur-5s 之间均匀抽样；窗口失效时退化为上限值，绝不落在未来。
fn random_start_today(dur_secs: i64) -> i64 {
    use chrono::{Datelike, TimeZone};
    let now = crate::crypto::envelope::now_ms();
    let latest = now - dur_secs.max(0) * 1000 - 5_000;
    let Some(local) = chrono::Local.timestamp_millis_opt(now).single() else {
        return latest;
    };
    let lo = chrono::Local
        .with_ymd_and_hms(local.year(), local.month(), local.day(), 7, 0, 0)
        .single()
        .map(|t| t.timestamp_millis())
        .unwrap_or(latest)
        .min(latest);
    let span = (latest - lo).max(0) as f64;
    lo + (rand::random::<f64>() * span) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 开始时刻必须不晚于 now - 用时（今天窗口失效时也不产生未来时间）。
    #[test]
    fn start_time_never_in_future() {
        for dur in [0, 600, 7_200, 86_400] {
            let start = random_start_today(dur);
            let now = crate::crypto::envelope::now_ms();
            assert!(
                start <= now - dur * 1000,
                "dur={dur} start={start} now={now} 落在未来或越过用时"
            );
        }
    }
}
