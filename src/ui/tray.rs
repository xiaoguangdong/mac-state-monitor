use crate::config::Config;
use crate::model::SystemStats;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
use objc2::{msg_send, sel, AnyThread, ClassType, MainThreadMarker};
use objc2_app_kit::{
    NSColor, NSFont, NSMenu, NSMenuItem, NSMutableParagraphStyle, NSStatusBar, NSStatusItem,
    NSTextAlignment,
};
use objc2_foundation::{NSMutableAttributedString, NSRange, NSString, ns_string};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Once;

pub const QUIT_ID: &str = "quit";
pub const SHOW_CHARTS_ID: &str = "show_charts";
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
        Self { items: None, mtm }
    }

    fn ensure_items(&mut self) {
        if self.items.is_some() {
            return;
        }
        let status_bar = NSStatusBar::systemStatusBar();
        let temp = status_bar.statusItemWithLength(42.0);
        let net = status_bar.statusItemWithLength(58.0);
        let disk = status_bar.statusItemWithLength(32.0);
        let mem = status_bar.statusItemWithLength(32.0);
        let cpu = status_bar.statusItemWithLength(32.0);
        self.items = Some(ModuleItems { cpu, mem, disk, net, temp });
    }

    pub fn update(&mut self, stats: &SystemStats, config: &Config) {
        self.ensure_items();
        let items = match &self.items {
            Some(i) => i,
            None => return,
        };
        let mtm = self.mtm;

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

        // Network — two-line with U/D labels
        let net_up = format!("U {}", format_speed(stats.network.transmitted_per_sec));
        let net_dn = format!("D {}", format_speed(stats.network.received_per_sec));
        set_module_title(&items.net, &net_up, &net_dn, None, mtm);

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

        // CPU menu (tags 100-199)
        let menu = build_native_menu(stats, mtm);
        items.cpu.setMenu(Some(&menu));

        // Temp menu (tags 200-299, fixed range, reset each time)
        let temp_menu = build_temp_menu(stats, mtm);
        items.temp.setMenu(Some(&temp_menu));
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

                let label_color = NSColor::secondaryLabelColor();
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

// ── Menu builders ──

/// Temperature menu (tags 200-299)
fn build_temp_menu(stats: &SystemStats, mtm: MainThreadMarker) -> Retained<NSMenu> {
    unsafe {
        let menu = NSMenu::new(mtm);
        let mut tag: isize = 200;

        MENU_ACTIONS.with(|actions| {
            let mut actions = actions.borrow_mut();
            // Clear all temp tags (200-299)
            actions.retain(|k, _| *k < 200 || *k >= 300);

            // Show Charts
            let charts_item = make_action_item("Show Charts", tag, mtm);
            actions.insert(tag, SHOW_CHARTS_ID.to_string());
            tag += 1;
            menu.addItem(&charts_item);

            menu.addItem(&NSMenuItem::separatorItem(mtm));

            // Temperature component selector
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

            // Current readings (info only)
            for reading in &stats.temperature.readings {
                let label = format!("{}: {:.0}°C", reading.label, reading.temp_c);
                add_info_item(&menu, &label, mtm);
            }
            if stats.temperature.readings.is_empty() {
                add_info_item(&menu, "No sensors found", mtm);
            }
        });

        menu
    }
}

/// CPU/system menu (tags 100-199)
fn build_native_menu(stats: &SystemStats, mtm: MainThreadMarker) -> Retained<NSMenu> {
    unsafe {
        let menu = NSMenu::new(mtm);
        let mut tag: isize = 100;

        MENU_ACTIONS.with(|actions| {
            let mut actions = actions.borrow_mut();
            // Clear CPU menu tags (100-199)
            actions.retain(|k, _| *k < 100 || *k >= 200);

            // CPU
            let cpu_label = format!(
                "CPU: {:.1}% ({} cores)",
                stats.cpu.global_usage, stats.cpu.core_count
            );
            add_info_item(&menu, &cpu_label, mtm);

            // Memory
            let mem = &stats.memory;
            let mem_label = format!(
                "Memory: {} / {} ({:.0}%)",
                format_bytes(mem.used_bytes),
                format_bytes(mem.total_bytes),
                mem.usage_percent
            );
            add_info_item(&menu, &mem_label, mtm);

            // Disk
            for disk in &stats.disks {
                let name = if disk.name.is_empty() {
                    &disk.mount_point
                } else {
                    &disk.name
                };
                let disk_label = format!(
                    "Disk {}: {} / {} ({:.0}%)",
                    name,
                    format_bytes(disk.total_bytes - disk.available_bytes),
                    format_bytes(disk.total_bytes),
                    disk.usage_percent
                );
                add_info_item(&menu, &disk_label, mtm);
            }

            // Network
            let net_label = format!(
                "Net: D {} /s  U {} /s",
                format_speed(stats.network.received_per_sec),
                format_speed(stats.network.transmitted_per_sec)
            );
            add_info_item(&menu, &net_label, mtm);

            menu.addItem(&NSMenuItem::separatorItem(mtm));

            // Temperature
            for reading in &stats.temperature.readings {
                let label = format!("{}: {:.0}°C", reading.label, reading.temp_c);
                add_info_item(&menu, &label, mtm);
            }

            menu.addItem(&NSMenuItem::separatorItem(mtm));

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

unsafe fn add_info_item(menu: &NSMenu, label: &str, mtm: MainThreadMarker) {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str(label));
    item.setEnabled(false);
    menu.addItem(&item);
}

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
