use crate::config::{Config, LAUNCH_AT_LOGIN_ID};
use crate::model::SystemStats;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
use objc2::{msg_send, sel, AnyThread, ClassType, MainThreadMarker};
use objc2_app_kit::{
    NSColor, NSControlStateValueOff, NSControlStateValueOn, NSFont, NSMenu, NSMenuItem,
    NSMutableParagraphStyle, NSStatusBar, NSStatusItem, NSTextAlignment,
};
use objc2_foundation::{NSMutableAttributedString, NSRange, NSString, ns_string};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Once;

pub const QUIT_ID: &str = "quit";
pub const SHOW_CHARTS_ID: &str = "show_charts";
pub const SHOW_TEMP_CHARTS_ID: &str = "show_temp_charts";
pub const TEMP_PREFIX: &str = "temp_";

thread_local! {
    static MENU_ACTIONS: RefCell<HashMap<isize, String>> = RefCell::new(HashMap::new());
    static PENDING_EVENT: RefCell<Option<String>> = RefCell::new(None);
}

pub fn take_pending_event() -> Option<String> {
    PENDING_EVENT.with(|p| p.borrow_mut().take())
}

static REGISTER_HANDLER: Once = Once::new();
static mut HANDLER_INSTANCE: *const AnyObject = std::ptr::null();

unsafe extern "C" fn menu_action_triggered(
    _this: *const AnyObject,
    _sel: Sel,
    sender: *const AnyObject,
) {
    if sender.is_null() {
        return;
    }
    let tag: isize = msg_send![sender, tag];
    MENU_ACTIONS.with(|actions| {
        let actions = actions.borrow();
        if let Some(action_id) = actions.get(&tag) {
            PENDING_EVENT.with(|p| {
                *p.borrow_mut() = Some(action_id.clone());
            });
        }
    });
}

fn ensure_menu_handler() -> *const AnyObject {
    REGISTER_HANDLER.call_once(|| unsafe {
        let superclass = AnyClass::get(c"NSObject").unwrap();
        let mut builder = ClassBuilder::new(c"MenuHandler", superclass).unwrap();
        builder.add_method(
            sel!(menuActionTriggered:),
            menu_action_triggered
                as unsafe extern "C" fn(*const AnyObject, Sel, *const AnyObject),
        );
        let cls = builder.register();
        let instance: *const AnyObject = msg_send![cls, new];
        HANDLER_INSTANCE = instance;
    });
    unsafe { HANDLER_INSTANCE }
}

pub struct TrayManager {
    items: Option<ModuleItems>,
    mtm: MainThreadMarker,
    temp_menu: Option<Retained<NSMenu>>,
    temp_reading_items: Vec<Retained<NSMenuItem>>,
    cpu_menu: Option<Retained<NSMenu>>,
    cpu_reading_items: Vec<Retained<NSMenuItem>>,
    cpu_login_item: Option<Retained<NSMenuItem>>,
    mem_menu: Option<Retained<NSMenu>>,
    mem_reading_items: Vec<Retained<NSMenuItem>>,
    disk_menu: Option<Retained<NSMenu>>,
    disk_reading_items: Vec<Retained<NSMenuItem>>,
    net_menu: Option<Retained<NSMenu>>,
    net_reading_items: Vec<Retained<NSMenuItem>>,
}

struct ModuleItems {
    cpu: Retained<NSStatusItem>,
    mem: Retained<NSStatusItem>,
    disk: Retained<NSStatusItem>,
    net: Retained<NSStatusItem>,
    temp: Retained<NSStatusItem>,
}

impl TrayManager {
    pub fn new() -> Self {
        let mtm = MainThreadMarker::new().expect("must be called on main thread");
        ensure_menu_handler();
        Self {
            items: None,
            mtm,
            temp_menu: None,
            temp_reading_items: Vec::new(),
            cpu_menu: None,
            cpu_reading_items: Vec::new(),
            cpu_login_item: None,
            mem_menu: None,
            mem_reading_items: Vec::new(),
            disk_menu: None,
            disk_reading_items: Vec::new(),
            net_menu: None,
            net_reading_items: Vec::new(),
        }
    }

    fn ensure_items(&mut self) {
        if self.items.is_some() {
            return;
        }
        let status_bar = NSStatusBar::systemStatusBar();
        let temp = status_bar.statusItemWithLength(38.0);
        let net = status_bar.statusItemWithLength(42.0);
        let disk = status_bar.statusItemWithLength(28.0);
        let mem = status_bar.statusItemWithLength(28.0);
        let cpu = status_bar.statusItemWithLength(28.0);
        self.items = Some(ModuleItems { cpu, mem, disk, net, temp });
    }

    fn ensure_temp_menu(&mut self, stats: &SystemStats) {
        if self.temp_menu.is_some() {
            // Update existing reading items
            self.update_temp_readings(stats);
            return;
        }
        let mtm = self.mtm;
        unsafe {
            let menu = NSMenu::new(mtm);
            menu.setAutoenablesItems(false);
            let mut tag: isize = 200;

            MENU_ACTIONS.with(|actions| {
                let mut actions = actions.borrow_mut();
                actions.retain(|k, _| *k < 200 || *k >= 300);

                let charts_item = make_action_item("Show Charts", tag, mtm);
                actions.insert(tag, SHOW_TEMP_CHARTS_ID.to_string());
                tag += 1;
                menu.addItem(&charts_item);

                menu.addItem(&NSMenuItem::separatorItem(mtm));

                let temp_choice_item = NSMenuItem::new(mtm);
                temp_choice_item.setTitle(&NSString::from_str("Display"));
                let temp_sub = NSMenu::new(mtm);
                for comp in &["CPU", "GPU", "SSD"] {
                    let item = make_action_item(comp, tag, mtm);
                    actions.insert(tag, format!("{}{}", TEMP_PREFIX, comp));
                    tag += 1;
                    temp_sub.addItem(&item);
                }
                temp_choice_item.setSubmenu(Some(&temp_sub));
                menu.addItem(&temp_choice_item);

                menu.addItem(&NSMenuItem::separatorItem(mtm));
            });

            // Create reading items for each sensor
            self.temp_reading_items.clear();
            for reading in &stats.temperature.readings {
                let item = make_info_item(&format!(
                    "{}: {:.0}C",
                    reading.label, reading.temp_c
                ), mtm);
                menu.addItem(&item);
                self.temp_reading_items.push(item);
            }
            if stats.temperature.readings.is_empty() {
                let item = make_info_item("No sensors found", mtm);
                menu.addItem(&item);
                self.temp_reading_items.push(item);
            }

            let items = self.items.as_ref().unwrap();
            items.temp.setMenu(Some(&menu));
            self.temp_menu = Some(menu);
        }
    }

    fn update_temp_readings(&mut self, stats: &SystemStats) {
        // Update existing items in-place, add/remove if count changed
        let readings = &stats.temperature.readings;
        for (i, item) in self.temp_reading_items.iter().enumerate() {
            if let Some(reading) = readings.get(i) {
                set_menu_item_white(item, &format!(
                    "{}: {:.0}C",
                    reading.label, reading.temp_c
                ), self.mtm);
            }
        }

        // If reading count changed, rebuild the menu next time
        if readings.len() != self.temp_reading_items.len() {
            self.temp_menu = None;
            self.temp_reading_items.clear();
        }
    }

    fn ensure_cpu_menu(&mut self, stats: &SystemStats, config: &Config) {
        if self.cpu_menu.is_some() {
            self.update_cpu_menu(stats, config);
            return;
        }
        let mtm = self.mtm;
        let menu = build_native_menu(stats, config, mtm, &mut self.cpu_reading_items, &mut self.cpu_login_item);
        let items = self.items.as_ref().unwrap();
        items.cpu.setMenu(Some(&menu));
        self.cpu_menu = Some(menu);
    }

    fn update_cpu_menu(&self, stats: &SystemStats, config: &Config) {
        let mtm = self.mtm;
        let mut idx = 0;

        // CPU
        if let Some(item) = self.cpu_reading_items.get(idx) {
            set_menu_item_white(item, &format!(
                "CPU: {:.1}% ({} cores)",
                stats.cpu.global_usage, stats.cpu.core_count
            ), mtm);
        }
        idx += 1;

        // Memory
        if let Some(item) = self.cpu_reading_items.get(idx) {
            let mem = &stats.memory;
            set_menu_item_white(item, &format!(
                "Memory: {} / {} ({:.0}%)",
                format_bytes(mem.used_bytes),
                format_bytes(mem.total_bytes),
                mem.usage_percent
            ), mtm);
        }
        idx += 1;

        // Disk (just first one)
        if let Some(disk) = stats.disks.first() {
            if let Some(item) = self.cpu_reading_items.get(idx) {
                let name = if disk.name.is_empty() { &disk.mount_point } else { &disk.name };
                set_menu_item_white(item, &format!(
                    "Disk {}: {} / {} ({:.0}%)",
                    name,
                    format_bytes(disk.total_bytes - disk.available_bytes),
                    format_bytes(disk.total_bytes),
                    disk.usage_percent
                ), mtm);
            }
            idx += 1;
        }

        // Network
        if let Some(item) = self.cpu_reading_items.get(idx) {
            set_menu_item_white(item, &format!(
                "Net: D {} /s  U {} /s",
                format_speed(stats.network.received_per_sec),
                format_speed(stats.network.transmitted_per_sec)
            ), mtm);
        }
        idx += 1;

        // Temperature readings
        for reading in &stats.temperature.readings {
            if let Some(item) = self.cpu_reading_items.get(idx) {
                set_menu_item_white(item, &format!(
                    "{}: {:.0}C",
                    reading.label, reading.temp_c
                ), mtm);
            }
            idx += 1;
        }

        // Update Launch at Login checkmark
        if let Some(login_item) = &self.cpu_login_item {
            let state = if config.launch_at_login {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            };
            login_item.setState(state);
        }
    }

    // ── MEM menu (tags 300-399) ──

    fn ensure_mem_menu(&mut self, stats: &SystemStats) {
        if self.mem_menu.is_some() {
            self.update_mem_menu(stats);
            return;
        }
        let mtm = self.mtm;
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        self.mem_reading_items.clear();

        // Used
        let used_item = make_info_item("", mtm);
        menu.addItem(&used_item);
        self.mem_reading_items.push(used_item);

        // Available
        let avail_item = make_info_item("", mtm);
        menu.addItem(&avail_item);
        self.mem_reading_items.push(avail_item);

        // Swap
        let swap_item = make_info_item("", mtm);
        menu.addItem(&swap_item);
        self.mem_reading_items.push(swap_item);

        self.update_mem_menu(stats);

        let items = self.items.as_ref().unwrap();
        items.mem.setMenu(Some(&menu));
        self.mem_menu = Some(menu);
    }

    fn update_mem_menu(&self, stats: &SystemStats) {
        let mtm = self.mtm;
        let mem = &stats.memory;
        if let Some(item) = self.mem_reading_items.get(0) {
            set_menu_item_white(item, &format!(
                "Used: {} / {} ({:.0}%)",
                format_bytes(mem.used_bytes),
                format_bytes(mem.total_bytes),
                mem.usage_percent
            ), mtm);
        }
        if let Some(item) = self.mem_reading_items.get(1) {
            set_menu_item_white(item, &format!(
                "Available: {}",
                format_bytes(mem.available_bytes)
            ), mtm);
        }
        if let Some(item) = self.mem_reading_items.get(2) {
            set_menu_item_white(item, &format!(
                "Swap: {} / {}",
                format_bytes(mem.swap_used_bytes),
                format_bytes(mem.swap_total_bytes)
            ), mtm);
        }
    }

    // ── DISK menu (tags 400-499) ──

    fn ensure_disk_menu(&mut self, stats: &SystemStats) {
        if self.disk_menu.is_some() {
            self.update_disk_menu(stats);
            return;
        }
        let mtm = self.mtm;
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        self.disk_reading_items.clear();

        for _disk in &stats.disks {
            let item = make_info_item("", mtm);
            menu.addItem(&item);
            self.disk_reading_items.push(item);
        }

        self.update_disk_menu(stats);

        let items = self.items.as_ref().unwrap();
        items.disk.setMenu(Some(&menu));
        self.disk_menu = Some(menu);
    }

    fn update_disk_menu(&mut self, stats: &SystemStats) {
        // If disk count changed, rebuild
        if stats.disks.len() != self.disk_reading_items.len() {
            self.disk_menu = None;
            self.disk_reading_items.clear();
            return;
        }
        let mtm = self.mtm;
        for (i, disk) in stats.disks.iter().enumerate() {
            if let Some(item) = self.disk_reading_items.get(i) {
                let name = if disk.name.is_empty() { &disk.mount_point } else { &disk.name };
                set_menu_item_white(item, &format!(
                    "{}: {} / {} ({:.0}%)",
                    name,
                    format_bytes(disk.total_bytes - disk.available_bytes),
                    format_bytes(disk.total_bytes),
                    disk.usage_percent
                ), mtm);
            }
        }
    }

    // ── NET menu (tags 500-599) ──

    fn ensure_net_menu(&mut self, stats: &SystemStats) {
        if self.net_menu.is_some() {
            self.update_net_menu(stats);
            return;
        }
        let mtm = self.mtm;
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        self.net_reading_items.clear();

        // Download speed
        let dl_item = make_info_item("", mtm);
        menu.addItem(&dl_item);
        self.net_reading_items.push(dl_item);

        // Upload speed
        let ul_item = make_info_item("", mtm);
        menu.addItem(&ul_item);
        self.net_reading_items.push(ul_item);

        menu.addItem(&NSMenuItem::separatorItem(mtm));

        // Total received
        let total_dl = make_info_item("", mtm);
        menu.addItem(&total_dl);
        self.net_reading_items.push(total_dl);

        // Total transmitted
        let total_ul = make_info_item("", mtm);
        menu.addItem(&total_ul);
        self.net_reading_items.push(total_ul);

        self.update_net_menu(stats);

        let items = self.items.as_ref().unwrap();
        items.net.setMenu(Some(&menu));
        self.net_menu = Some(menu);
    }

    fn update_net_menu(&self, stats: &SystemStats) {
        let mtm = self.mtm;
        let net = &stats.network;
        if let Some(item) = self.net_reading_items.get(0) {
            set_menu_item_white(item, &format!(
                "Download: {} /s",
                format_speed(net.received_per_sec)
            ), mtm);
        }
        if let Some(item) = self.net_reading_items.get(1) {
            set_menu_item_white(item, &format!(
                "Upload: {} /s",
                format_speed(net.transmitted_per_sec)
            ), mtm);
        }
        if let Some(item) = self.net_reading_items.get(2) {
            set_menu_item_white(item, &format!(
                "Total D: {}",
                format_bytes(net.total_received_bytes)
            ), mtm);
        }
        if let Some(item) = self.net_reading_items.get(3) {
            set_menu_item_white(item, &format!(
                "Total U: {}",
                format_bytes(net.total_transmitted_bytes)
            ), mtm);
        }
    }

    pub fn update(&mut self, stats: &SystemStats, config: &Config) {
        self.ensure_items();
        if self.items.is_none() { return; }
        let mtm = self.mtm;
        let items = self.items.as_ref().unwrap();

        // CPU
        let cpu_pct = format!("{:.0}%", stats.cpu.global_usage);
        set_module_title(&items.cpu, &cpu_pct, "CPU", Some(stats.cpu.global_usage), mtm);

        // Memory
        let mem_pct = format!("{:.0}%", stats.memory.usage_percent);
        set_module_title(&items.mem, &mem_pct, "MEM", Some(stats.memory.usage_percent), mtm);

        // Disk
        let disk_usage = stats.disks.first().map(|d| d.usage_percent).unwrap_or(0.0);
        let disk_pct = stats
            .disks
            .first()
            .map(|d| format!("{:.0}%", d.usage_percent))
            .unwrap_or_else(|| "--%".to_string());
        set_module_title(&items.disk, &disk_pct, "SSD", Some(disk_usage), mtm);

        // Network
        let net_up = format_speed(stats.network.transmitted_per_sec);
        let net_dn = format_speed(stats.network.received_per_sec);
        set_net_title(
            &items.net,
            &net_up,
            &net_dn,
            stats.network.transmitted_per_sec,
            stats.network.received_per_sec,
            mtm,
        );

        // Temperature
        let temp_val = stats
            .temperature
            .find_temp(&config.menubar_temp_component)
            .map(|t| format!("{:.0}C", t))
            .unwrap_or_else(|| "--C".to_string());
        let temp_c = stats
            .temperature
            .find_temp(&config.menubar_temp_component)
            .unwrap_or(0.0);
        set_module_title(&items.temp, &temp_val, "TEMP", Some(temp_c), mtm);

        // Menus — update in-place
        self.ensure_temp_menu(stats);
        self.ensure_cpu_menu(stats, config);
        self.ensure_mem_menu(stats);
        self.ensure_disk_menu(stats);
        self.ensure_net_menu(stats);
    }
}

// ── Title renderer ──

/// Two-line module title: line1 (value) + line2 (label)
/// If color_value is Some, line1 gets colored; otherwise uses label color.
fn set_module_title(
    item: &NSStatusItem,
    line1: &str,
    line2: &str,
    color_value: Option<f32>,
    mtm: MainThreadMarker,
) {
    if let Some(button) = item.button(mtm) {
        unsafe {
            let text = format!("{}\n{}", line1, line2);
            let ns_text = NSString::from_str(&text);
            let attr_str = NSMutableAttributedString::initWithString(
                NSMutableAttributedString::alloc(),
                &ns_text,
            );
            let full_len = ns_text.len();
            let full_range = NSRange::new(0, full_len);

            // Font
            let font: Retained<NSFont> = msg_send![
                NSFont::class(),
                monospacedDigitSystemFontOfSize: 9.0_f64,
                weight: 0.4_f64
            ];
            let font_key = ns_string!("NSFont");
            attr_str.addAttribute_value_range(font_key, &font, full_range);

            // Paragraph style: tight line spacing, centered
            let para_style = NSMutableParagraphStyle::new();
            para_style.setAlignment(NSTextAlignment::Center);
            let _: () = msg_send![&para_style, setLineSpacing: 0.0_f64];
            let _: () = msg_send![&para_style, setMaximumLineHeight: 10.0_f64];
            let _: () = msg_send![&para_style, setMinimumLineHeight: 10.0_f64];
            let para_key = ns_string!("NSParagraphStyle");
            attr_str.addAttribute_value_range(para_key, &para_style, full_range);

            // Baseline offset for vertical centering
            let baseline_key = ns_string!("NSBaselineOffset");
            let offset_val: Retained<objc2_foundation::NSNumber> = msg_send![
                objc2_foundation::NSNumber::class(),
                numberWithDouble: -4.0_f64
            ];
            attr_str.addAttribute_value_range(baseline_key, &offset_val, full_range);

            // Colors: line1 colored (if value provided), line2 always label color
            let color_key = ns_string!("NSColor");
            let line1_len = line1.len();
            if let Some(val) = color_value {
                let value_color = get_color_for_value(val);
                let line1_range = NSRange::new(0, line1_len);
                attr_str.addAttribute_value_range(color_key, &value_color, line1_range);

                let label_color = NSColor::labelColor();
                let line2_range = NSRange::new(line1_len + 1, full_len - line1_len - 1);
                attr_str.addAttribute_value_range(color_key, &label_color, line2_range);
            } else {
                let label_color = NSColor::labelColor();
                attr_str.addAttribute_value_range(color_key, &label_color, full_range);
            }

            let _: () = msg_send![&button, setAttributedTitle: &*attr_str];
        }
    }
}

fn get_color_for_value(value: f32) -> Retained<NSColor> {
    if value >= 80.0 {
        NSColor::systemRedColor()
    } else if value >= 60.0 {
        NSColor::systemPurpleColor()
    } else if value >= 30.0 {
        NSColor::systemYellowColor()
    } else {
        NSColor::systemGreenColor()
    }
}

/// Color for network speed (bytes/sec): green < 1MB, yellow < 10MB, purple < 100MB, red >= 100MB
fn get_color_for_speed(bytes_per_sec: u64) -> Retained<NSColor> {
    const MB: u64 = 1024 * 1024;
    if bytes_per_sec >= 100 * MB {
        NSColor::systemRedColor()
    } else if bytes_per_sec >= 10 * MB {
        NSColor::systemPurpleColor()
    } else if bytes_per_sec >= MB {
        NSColor::systemYellowColor()
    } else {
        NSColor::systemGreenColor()
    }
}

/// Two-line network title with each line colored by its speed
fn set_net_title(
    item: &NSStatusItem,
    line1: &str,
    line2: &str,
    speed1: u64,
    speed2: u64,
    mtm: MainThreadMarker,
) {
    if let Some(button) = item.button(mtm) {
        unsafe {
            let text = format!("{}\n{}", line1, line2);
            let ns_text = NSString::from_str(&text);
            let attr_str = NSMutableAttributedString::initWithString(
                NSMutableAttributedString::alloc(),
                &ns_text,
            );
            let full_len = ns_text.len();
            let full_range = NSRange::new(0, full_len);

            let font: Retained<NSFont> = msg_send![
                NSFont::class(),
                monospacedDigitSystemFontOfSize: 9.0_f64,
                weight: 0.4_f64
            ];
            let font_key = ns_string!("NSFont");
            attr_str.addAttribute_value_range(font_key, &font, full_range);

            let para_style = NSMutableParagraphStyle::new();
            para_style.setAlignment(NSTextAlignment::Center);
            let _: () = msg_send![&para_style, setLineSpacing: 0.0_f64];
            let _: () = msg_send![&para_style, setMaximumLineHeight: 10.0_f64];
            let _: () = msg_send![&para_style, setMinimumLineHeight: 10.0_f64];
            let para_key = ns_string!("NSParagraphStyle");
            attr_str.addAttribute_value_range(para_key, &para_style, full_range);

            let baseline_key = ns_string!("NSBaselineOffset");
            let offset_val: Retained<objc2_foundation::NSNumber> = msg_send![
                objc2_foundation::NSNumber::class(),
                numberWithDouble: -4.0_f64
            ];
            attr_str.addAttribute_value_range(baseline_key, &offset_val, full_range);

            let color_key = ns_string!("NSColor");
            let line1_len = line1.len();
            let color1 = get_color_for_speed(speed1);
            let line1_range = NSRange::new(0, line1_len);
            attr_str.addAttribute_value_range(color_key, &color1, line1_range);

            let color2 = get_color_for_speed(speed2);
            let line2_range = NSRange::new(line1_len + 1, full_len - line1_len - 1);
            attr_str.addAttribute_value_range(color_key, &color2, line2_range);

            let _: () = msg_send![&button, setAttributedTitle: &*attr_str];
        }
    }
}

// ── Menu builders ──

/// CPU/system menu (tags 100-199)
fn build_native_menu(
    stats: &SystemStats,
    config: &Config,
    mtm: MainThreadMarker,
    info_items: &mut Vec<Retained<NSMenuItem>>,
    login_item_out: &mut Option<Retained<NSMenuItem>>,
) -> Retained<NSMenu> {
    unsafe {
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        let mut tag: isize = 100;
        info_items.clear();

        MENU_ACTIONS.with(|actions| {
            let mut actions = actions.borrow_mut();
            actions.retain(|k, _| *k < 100 || *k >= 200);

            // About
            let version = env!("CARGO_PKG_VERSION");
            let about_item = make_info_item(&format!("Mac State Monitor v{}", version), mtm);
            menu.addItem(&about_item);

            menu.addItem(&NSMenuItem::separatorItem(mtm));

            // CPU
            let cpu_item = make_info_item(&format!(
                "CPU: {:.1}% ({} cores)",
                stats.cpu.global_usage, stats.cpu.core_count
            ), mtm);
            menu.addItem(&cpu_item);
            info_items.push(cpu_item);

            // Memory
            let mem = &stats.memory;
            let mem_item = make_info_item(&format!(
                "Memory: {} / {} ({:.0}%)",
                format_bytes(mem.used_bytes),
                format_bytes(mem.total_bytes),
                mem.usage_percent
            ), mtm);
            menu.addItem(&mem_item);
            info_items.push(mem_item);

            // Disk (first only)
            if let Some(disk) = stats.disks.first() {
                let name = if disk.name.is_empty() {
                    &disk.mount_point
                } else {
                    &disk.name
                };
                let disk_item = make_info_item(&format!(
                    "Disk {}: {} / {} ({:.0}%)",
                    name,
                    format_bytes(disk.total_bytes - disk.available_bytes),
                    format_bytes(disk.total_bytes),
                    disk.usage_percent
                ), mtm);
                menu.addItem(&disk_item);
                info_items.push(disk_item);
            }

            // Network
            let net_item = make_info_item(&format!(
                "Net: D {} /s  U {} /s",
                format_speed(stats.network.received_per_sec),
                format_speed(stats.network.transmitted_per_sec)
            ), mtm);
            menu.addItem(&net_item);
            info_items.push(net_item);

            menu.addItem(&NSMenuItem::separatorItem(mtm));

            // Temperature
            for reading in &stats.temperature.readings {
                let temp_item = make_info_item(&format!(
                    "{}: {:.0}C",
                    reading.label, reading.temp_c
                ), mtm);
                menu.addItem(&temp_item);
                info_items.push(temp_item);
            }

            menu.addItem(&NSMenuItem::separatorItem(mtm));

            // Show Charts
            let charts_item = make_action_item("Show Charts", tag, mtm);
            actions.insert(tag, SHOW_CHARTS_ID.to_string());
            tag += 1;
            menu.addItem(&charts_item);

            // Refresh interval
            let interval_sub_item = NSMenuItem::new(mtm);
            interval_sub_item.setTitle(&NSString::from_str("Refresh Interval"));
            let interval_sub = NSMenu::new(mtm);
            for (secs, label) in [(1, "1s"), (2, "2s"), (5, "5s"), (10, "10s")] {
                let item = make_action_item(label, tag, mtm);
                actions.insert(tag, format!("interval_{}", secs));
                tag += 1;
                interval_sub.addItem(&item);
            }
            interval_sub_item.setSubmenu(Some(&interval_sub));
            menu.addItem(&interval_sub_item);

            // Launch at Login
            let login_item = make_action_item("Launch at Login", tag, mtm);
            let state = if config.launch_at_login {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            };
            login_item.setState(state);
            actions.insert(tag, LAUNCH_AT_LOGIN_ID.to_string());
            tag += 1;
            menu.addItem(&login_item);
            *login_item_out = Some(login_item);

            menu.addItem(&NSMenuItem::separatorItem(mtm));

            // Quit
            let quit_item = make_action_item("Quit", tag, mtm);
            actions.insert(tag, QUIT_ID.to_string());
            menu.addItem(&quit_item);
        });

        menu
    }
}

// ── Menu helpers ──

unsafe fn make_action_item(
    title: &str,
    tag: isize,
    mtm: MainThreadMarker,
) -> Retained<NSMenuItem> {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str(title));
    item.setEnabled(true);
    item.setTag(tag);
    item.setAction(Some(sel!(menuActionTriggered:)));
    let handler = ensure_menu_handler();
    let _: () = msg_send![&item, setTarget: handler];
    item
}

/// Info menu item with label color text (non-interactive but not grayed out)
fn make_info_item(title: &str, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    unsafe {
        let item = NSMenuItem::new(mtm);
        item.setEnabled(true);
        item.setTag(-1);
        item.setAction(Some(sel!(menuActionTriggered:)));
        let handler = ensure_menu_handler();
        let _: () = msg_send![&item, setTarget: handler];
        set_menu_item_white(&item, title, mtm);
        item
    }
}

/// Set menu item title with system label color attributed string
fn set_menu_item_white(item: &NSMenuItem, title: &str, _mtm: MainThreadMarker) {
    unsafe {
        let ns_text = NSString::from_str(title);
        let attr_str = NSMutableAttributedString::initWithString(
            NSMutableAttributedString::alloc(),
            &ns_text,
        );
        let range = NSRange::new(0, ns_text.len());
        let color_key = ns_string!("NSColor");
        let color = NSColor::labelColor();
        attr_str.addAttribute_value_range(color_key, &color, range);
        let font_key = ns_string!("NSFont");
        let font = NSFont::menuFontOfSize(13.0);
        attr_str.addAttribute_value_range(font_key, &font, range);
        let _: () = msg_send![item, setAttributedTitle: &*attr_str];
    }
}

// ── Formatting ──

fn format_speed(bytes_per_sec: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes_per_sec >= GB {
        format!("{:.1}G", bytes_per_sec as f64 / GB as f64)
    } else if bytes_per_sec >= MB {
        format!("{:.1}M", bytes_per_sec as f64 / MB as f64)
    } else if bytes_per_sec >= KB {
        format!("{:.0}K", bytes_per_sec as f64 / KB as f64)
    } else {
        format!("{}B", bytes_per_sec)
    }
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.0} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
