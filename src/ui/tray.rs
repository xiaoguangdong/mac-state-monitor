use crate::config::{config_dir, Config, CustomRunnerSet, RunnerIconMode, LAUNCH_AT_LOGIN_ID};
use crate::model::SystemStats;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
use objc2::{msg_send, sel, AnyThread, ClassType, MainThreadMarker};
use objc2_app_kit::{
    NSBundleImageExtension, NSCellImagePosition, NSColor, NSControlStateValueOff,
    NSControlStateValueOn, NSFont, NSImage, NSImageScaling, NSMenu, NSMenuItem,
    NSMutableParagraphStyle, NSSquareStatusItemLength, NSStatusBar, NSStatusItem, NSTextAlignment,
};
use objc2_foundation::{ns_string, NSBundle, NSMutableAttributedString, NSRange, NSSize, NSString};
use rfd::FileDialog;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
use std::time::Instant;

pub const QUIT_ID: &str = "quit";
pub const SHOW_CHARTS_ID: &str = "show_charts";
pub const SHOW_TEMP_CHARTS_ID: &str = "show_temp_charts";
pub const TEMP_PREFIX: &str = "temp_";
pub const RUNNER_DISPLAY_PREFIX: &str = "runner_display_";
pub const RUNNER_IMPORT_ID: &str = "runner_import_custom";
pub const RUNNER_TOGGLE_PREFIX: &str = "runner_toggle_";
pub const RUNNER_CATEGORY_PREFIX: &str = "runner_category_";
pub const RUNNER_ALL_ID: &str = "runner_all";

const EMBEDDED_RUN_CAT_UI_BUNDLE_RELATIVE: &str = "LocalPackage_UserInterface.bundle";
const EMBEDDED_RUN_CAT_UI_ASSETS_RELATIVE: &str =
    "LocalPackage_UserInterface.bundle/Contents/Resources/Assets.car";
const EXPORTED_RUN_CAT_FRAMES_RELATIVE: &str = "runcat-frames";
const EXPORTED_RUN_CAT_FRAMES_WHITE_RELATIVE: &str = "runcat-frames-white";

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
            menu_action_triggered as unsafe extern "C" fn(*const AnyObject, Sel, *const AnyObject),
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
    last_cpu_usage: f32,
    runner: RunnerAnimator,
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
    runner: Retained<NSStatusItem>,
    cpu: Retained<NSStatusItem>,
    mem: Retained<NSStatusItem>,
    disk: Retained<NSStatusItem>,
    net: Retained<NSStatusItem>,
    temp: Retained<NSStatusItem>,
}

#[derive(Clone)]
struct RunnerMenuOption {
    id: String,
    title: String,
}

struct RunnerAnimator {
    run_cat_bundle: Option<Retained<NSBundle>>,
    icon_mode: RunnerIconMode,
    active_frames_precolored_white: bool,
    configured_runner_id: String,
    selected_id: String,
    rotation_ids: Vec<String>,
    rotation_index: usize,
    display_secs: u64,
    frame_ms: u64,
    frame_index: usize,
    frame_accumulator: f64,
    last_step: Instant,
    last_runner_switch: Instant,
    active_frames: Vec<Retained<NSImage>>,
    default_sets: Vec<RunnerMenuOption>,
    custom_sets_snapshot: Vec<CustomRunnerSet>,
}

impl TrayManager {
    pub fn new() -> Self {
        let mtm = MainThreadMarker::new().expect("must be called on main thread");
        ensure_menu_handler();
        Self {
            items: None,
            mtm,
            last_cpu_usage: 0.0,
            runner: RunnerAnimator::new(),
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

    pub fn animate(&mut self, now: Instant) {
        if self.items.is_none() {
            return;
        }
        if let Some(frame) = self.runner.advance(now, self.last_cpu_usage) {
            self.apply_runner_frame(Some(frame.as_ref()));
        }
    }

    pub fn sync_runner_config(&mut self, config: &Config) {
        if self.runner.sync_config(config) {
            self.invalidate_cpu_menu();
        }
        if let Some(frame) = self.runner.current_frame() {
            self.apply_runner_frame(Some(frame.as_ref()));
        }
    }

    pub fn import_custom_runner_frames(&mut self, config: &mut Config) -> bool {
        let mut files = match FileDialog::new()
            .set_title("Select animation frames in order")
            .add_filter(
                "Images",
                &["png", "jpg", "jpeg", "gif", "bmp", "tiff", "webp", "heic"],
            )
            .pick_files()
        {
            Some(files) if files.len() >= 2 => files,
            _ => return false,
        };

        files.sort_by(|a, b| {
            a.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .cmp(&b.file_name().unwrap_or_default().to_string_lossy())
        });

        let set_name = files
            .first()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "Custom Runner".to_string());

        let (set_id, copied) = match copy_custom_frames(&files) {
            Ok(v) if !v.1.is_empty() => v,
            _ => return false,
        };

        let new_set = CustomRunnerSet::new(set_id.clone(), set_name, copied);
        let selected_id = format!("custom:{}", new_set.id);
        config.custom_runner_sets.push(new_set);
        config.runner_id = selected_id;
        self.runner.sync_config(config);
        self.invalidate_cpu_menu();
        true
    }

    pub fn toggle_runner_in_rotation(&mut self, config: &mut Config, runner_id: &str) -> bool {
        if !self.runner.runner_id_exists(runner_id) {
            return false;
        }

        if let Some(idx) = config
            .runner_rotation_ids
            .iter()
            .position(|id| id == runner_id)
        {
            if config.runner_rotation_ids.len() == 1 {
                return false;
            }
            config.runner_rotation_ids.remove(idx);
            if config.runner_id == runner_id {
                config.runner_id = config
                    .runner_rotation_ids
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "runcat:cat".to_string());
            }
        } else {
            config.runner_rotation_ids.push(runner_id.to_string());
        }

        self.runner.sync_config(config);
        self.invalidate_cpu_menu();
        true
    }

    pub fn select_all_runners(&mut self, config: &mut Config) {
        let all_ids: Vec<String> = self.runner.menu_options().iter().map(|o| o.id.clone()).collect();
        // Always select all — no toggle behavior
        config.runner_rotation_ids = all_ids;
        // Keep current runner_id unchanged so no jarring switch
        if !config.runner_rotation_ids.contains(&config.runner_id) {
            config.runner_id = config.runner_rotation_ids.first().cloned().unwrap_or_else(|| "runcat:cat".to_string());
        }
        self.runner.sync_config(config);
        self.invalidate_cpu_menu();
    }

    pub fn select_runner_category(&mut self, config: &mut Config, category: &str) {
        let categories: &[(&str, &[&str])] = &[
            ("Cats", &["runcat:cat", "runcat:cat-b", "runcat:cat-c", "runcat:cat-tail", "runcat:flash-cat", "runcat:golden-cat", "runcat:metal-cluster-cat", "runcat:mock-nyan-cat", "runcat:maneki-neko"]),
            ("Dogs", &["runcat:dog", "runcat:puppy", "runcat:terrier", "runcat:welsh-corgi", "runcat:greyhound"]),
            ("Animals", &["runcat:bird", "runcat:butterfly", "runcat:chameleon", "runcat:cheetah", "runcat:chicken", "runcat:dinosaur", "runcat:dolphin", "runcat:dragon", "runcat:fishman", "runcat:fox", "runcat:frog", "runcat:hamster-wheel", "runcat:hedgehog", "runcat:horse", "runcat:mouse", "runcat:octopus", "runcat:otter", "runcat:owl", "runcat:parrot", "runcat:penguin", "runcat:penguin2", "runcat:pig", "runcat:rabbit", "runcat:reindeer-sleigh", "runcat:sheep", "runcat:squirrel", "runcat:uhooi", "runcat:whale"]),
            ("Food", &["runcat:coffee", "runcat:frypan", "runcat:mochi", "runcat:rotating-sushi", "runcat:rubber-duck", "runcat:sausage", "runcat:sushi", "runcat:tapioca-drink"]),
            ("People", &["runcat:dogeza", "runcat:human", "runcat:party-people", "runcat:push-up", "runcat:sit-up"]),
            ("Machines", &["runcat:cogwheel", "runcat:engine", "runcat:factory", "runcat:reactor", "runcat:rocket", "runcat:steam-locomotive"]),
            ("Nature", &["runcat:bonfire", "runcat:drop", "runcat:earth", "runcat:slime", "runcat:snowman", "runcat:sparkler", "runcat:wind-chime"]),
            ("Fantasy", &["runcat:ghost", "runcat:jack-o-lantern", "runcat:triforce"]),
            ("Abstract", &["runcat:city", "runcat:cradle", "runcat:dots", "runcat:entaku", "runcat:pendulum", "runcat:pulse", "runcat:sine-curve"]),
        ];

        let cat_ids: Vec<String> = if let Some((_, ids)) = categories.iter().find(|(name, _)| *name == category) {
            let all_options = self.runner.menu_options();
            ids.iter()
                .filter(|id| all_options.iter().any(|o| o.id == **id))
                .map(|id| id.to_string())
                .collect()
        } else {
            return;
        };

        let all_in_rotation = cat_ids.iter().all(|id| config.runner_rotation_ids.contains(id));
        if all_in_rotation {
            // Remove category runners (but keep at least one runner total)
            config.runner_rotation_ids.retain(|id| !cat_ids.contains(id));
            if config.runner_rotation_ids.is_empty() {
                config.runner_rotation_ids = vec!["runcat:cat".to_string()];
            }
        } else {
            // Add all category runners
            for id in &cat_ids {
                if !config.runner_rotation_ids.contains(id) {
                    config.runner_rotation_ids.push(id.clone());
                }
            }
        }
        config.runner_id = config.runner_rotation_ids.first().cloned().unwrap_or_else(|| "runcat:cat".to_string());
        self.runner.sync_config(config);
        self.invalidate_cpu_menu();
    }

    pub fn invalidate_cpu_menu(&mut self) {
        self.cpu_menu = None;
        self.cpu_reading_items.clear();
        self.cpu_login_item = None;
    }

    fn apply_runner_frame(&self, frame: Option<&NSImage>) {
        let Some(items) = &self.items else {
            return;
        };
        if let Some(button) = items.runner.button(self.mtm) {
            let white_mode = self.runner.icon_mode == RunnerIconMode::White;
            if let Some(img) = frame {
                let use_template_tint = white_mode && !self.runner.active_frames_precolored_white;
                img.setTemplate(use_template_tint);
            }
            button.setImage(frame);
            button.setImagePosition(NSCellImagePosition::ImageOnly);
            button.setImageScaling(NSImageScaling::ScaleProportionallyDown);
            button.setImageHugsTitle(false);
            button.setTitle(&NSString::from_str(""));
            unsafe {
                let use_template_tint = white_mode && !self.runner.active_frames_precolored_white;
                if use_template_tint {
                    let white = NSColor::whiteColor();
                    let _: () = msg_send![&button, setContentTintColor: Some(&*white)];
                } else {
                    let _: () = msg_send![&button, setContentTintColor: Option::<&NSColor>::None];
                }
            }
        }
    }

    fn ensure_items(&mut self) {
        if self.items.is_some() {
            return;
        }
        let status_bar = NSStatusBar::systemStatusBar();
        let module_width = 42.0;
        let temp = status_bar.statusItemWithLength(module_width);
        let net = status_bar.statusItemWithLength(module_width);
        let disk = status_bar.statusItemWithLength(module_width);
        let mem = status_bar.statusItemWithLength(module_width);
        let cpu = status_bar.statusItemWithLength(module_width);
        let runner = status_bar.statusItemWithLength(NSSquareStatusItemLength);
        self.items = Some(ModuleItems {
            runner,
            cpu,
            mem,
            disk,
            net,
            temp,
        });
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
            let mut tag: isize = 400;

            MENU_ACTIONS.with(|actions| {
                let mut actions = actions.borrow_mut();
                actions.retain(|k, _| *k < 400 || *k >= 500);

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
                let item =
                    make_info_item(&format!("{}: {:.0}C", reading.label, reading.temp_c), mtm);
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
                set_menu_item_white(
                    item,
                    &format!("{}: {:.0}C", reading.label, reading.temp_c),
                    self.mtm,
                );
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
        let runner_options = self.runner.menu_options();
        let runner_preview_images = self.runner.preview_images(&runner_options);
        let menu = build_native_menu(
            stats,
            config,
            mtm,
            &runner_options,
            &runner_preview_images,
            &mut self.cpu_reading_items,
            &mut self.cpu_login_item,
        );
        let items = self.items.as_ref().unwrap();
        items.cpu.setMenu(Some(&menu));
        items.runner.setMenu(Some(&menu));
        self.cpu_menu = Some(menu);
    }

    fn update_cpu_menu(&self, stats: &SystemStats, config: &Config) {
        let mtm = self.mtm;
        let mut idx = 0;
        let cpu_percent = to_total_cpu_percent(stats);

        // CPU
        if let Some(item) = self.cpu_reading_items.get(idx) {
            set_menu_item_white(item, &format!("CPU: {:.1}%", cpu_percent), mtm);
        }
        idx += 1;

        // Memory
        if let Some(item) = self.cpu_reading_items.get(idx) {
            let mem = &stats.memory;
            set_menu_item_white(
                item,
                &format!(
                    "Memory: {} / {} ({:.0}%)",
                    format_bytes(mem.used_bytes),
                    format_bytes(mem.total_bytes),
                    mem.usage_percent
                ),
                mtm,
            );
        }
        idx += 1;

        // Disk (just first one)
        if let Some(disk) = stats.disks.first() {
            if let Some(item) = self.cpu_reading_items.get(idx) {
                let name = if disk.name.is_empty() {
                    &disk.mount_point
                } else {
                    &disk.name
                };
                set_menu_item_white(
                    item,
                    &format!(
                        "Disk {}: {} / {} ({:.0}%)",
                        name,
                        format_bytes(disk.total_bytes - disk.available_bytes),
                        format_bytes(disk.total_bytes),
                        disk.usage_percent
                    ),
                    mtm,
                );
            }
            idx += 1;
        }

        // Network
        if let Some(item) = self.cpu_reading_items.get(idx) {
            set_menu_item_white(
                item,
                &format!(
                    "Net: D {} /s  U {} /s",
                    format_speed(stats.network.received_per_sec),
                    format_speed(stats.network.transmitted_per_sec)
                ),
                mtm,
            );
        }
        idx += 1;

        // Temperature readings
        for reading in &stats.temperature.readings {
            if let Some(item) = self.cpu_reading_items.get(idx) {
                set_menu_item_white(
                    item,
                    &format!("{}: {:.0}C", reading.label, reading.temp_c),
                    mtm,
                );
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
            set_menu_item_white(
                item,
                &format!(
                    "Used: {} / {} ({:.0}%)",
                    format_bytes(mem.used_bytes),
                    format_bytes(mem.total_bytes),
                    mem.usage_percent
                ),
                mtm,
            );
        }
        if let Some(item) = self.mem_reading_items.get(1) {
            set_menu_item_white(
                item,
                &format!("Available: {}", format_bytes(mem.available_bytes)),
                mtm,
            );
        }
        if let Some(item) = self.mem_reading_items.get(2) {
            set_menu_item_white(
                item,
                &format!(
                    "Swap: {} / {}",
                    format_bytes(mem.swap_used_bytes),
                    format_bytes(mem.swap_total_bytes)
                ),
                mtm,
            );
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
                let name = if disk.name.is_empty() {
                    &disk.mount_point
                } else {
                    &disk.name
                };
                set_menu_item_white(
                    item,
                    &format!(
                        "{}: {} / {} ({:.0}%)",
                        name,
                        format_bytes(disk.total_bytes - disk.available_bytes),
                        format_bytes(disk.total_bytes),
                        disk.usage_percent
                    ),
                    mtm,
                );
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
            set_menu_item_white(
                item,
                &format!("Download: {} /s", format_speed(net.received_per_sec)),
                mtm,
            );
        }
        if let Some(item) = self.net_reading_items.get(1) {
            set_menu_item_white(
                item,
                &format!("Upload: {} /s", format_speed(net.transmitted_per_sec)),
                mtm,
            );
        }
        if let Some(item) = self.net_reading_items.get(2) {
            set_menu_item_white(
                item,
                &format!("Total D: {}", format_bytes(net.total_received_bytes)),
                mtm,
            );
        }
        if let Some(item) = self.net_reading_items.get(3) {
            set_menu_item_white(
                item,
                &format!("Total U: {}", format_bytes(net.total_transmitted_bytes)),
                mtm,
            );
        }
    }

    pub fn update(&mut self, stats: &SystemStats, config: &Config) {
        self.ensure_items();
        if self.items.is_none() {
            return;
        }
        self.last_cpu_usage = stats.cpu.global_usage;

        if self.runner.sync_config(config) {
            self.invalidate_cpu_menu();
        }

        let mtm = self.mtm;
        let items = self.items.as_ref().unwrap();

        // CPU
        let cpu_pct = format!("{:.0}%", to_total_cpu_percent(stats));
        if let Some(frame) = self.runner.current_frame() {
            self.apply_runner_frame(Some(frame.as_ref()));
        }
        set_module_title(
            &items.cpu,
            &cpu_pct,
            "CPU",
            Some(stats.cpu.global_usage),
            mtm,
        );

        // Memory
        let mem_pct = format!("{:.0}%", stats.memory.usage_percent);
        set_module_title(
            &items.mem,
            &mem_pct,
            "MEM",
            Some(stats.memory.usage_percent),
            mtm,
        );

        // Disk
        let disk_usage = stats.disks.first().map(|d| d.usage_percent).unwrap_or(0.0);
        let disk_pct = stats
            .disks
            .first()
            .map(|d| format!("{:.0}%", d.usage_percent))
            .unwrap_or_else(|| "--%".to_string());
        set_module_title(&items.disk, &disk_pct, "SSD", Some(disk_usage), mtm);

        // Network
        let net_up = format!("↑{}", format_speed(stats.network.transmitted_per_sec));
        let net_dn = format!("↓{}", format_speed(stats.network.received_per_sec));
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

        // Menus — update in-place
        self.ensure_temp_menu(stats);
        self.ensure_cpu_menu(stats, config);
        self.ensure_mem_menu(stats);
        self.ensure_disk_menu(stats);
        self.ensure_net_menu(stats);
    }
}

impl RunnerAnimator {
    fn new() -> Self {
        let run_cat_bundle = load_run_cat_bundle();
        let default_sets = discover_runcat_sets(run_cat_bundle.as_ref());
        let mut runner = Self {
            run_cat_bundle,
            icon_mode: RunnerIconMode::Original,
            active_frames_precolored_white: false,
            configured_runner_id: "runcat:cat".to_string(),
            selected_id: "runcat:cat".to_string(),
            rotation_ids: vec!["runcat:cat".to_string()],
            rotation_index: 0,
            display_secs: 600,
            frame_ms: 100,
            frame_index: 0,
            frame_accumulator: 0.0,
            last_step: Instant::now(),
            last_runner_switch: Instant::now(),
            active_frames: Vec::new(),
            default_sets,
            custom_sets_snapshot: Vec::new(),
        };
        let (frames, precolored_white) = runner.load_frames_for_id("runcat:cat", &[]);
        runner.active_frames = frames;
        runner.active_frames_precolored_white = precolored_white;
        if runner.active_frames.is_empty() {
            runner.selected_id = "fallback:runner".to_string();
            runner.active_frames = fallback_frames();
            runner.active_frames_precolored_white = false;
        }
        runner
    }

    fn sync_config(&mut self, config: &Config) -> bool {
        let mut changed = false;
        if self.custom_sets_snapshot != config.custom_runner_sets {
            self.custom_sets_snapshot = config.custom_runner_sets.clone();
            changed = true;
        }

        let display_secs = config.runner_display_secs.clamp(1, 3600);
        if self.display_secs != display_secs {
            self.display_secs = display_secs;
            changed = true;
        }

        let frame_ms = config.runner_frame_ms.clamp(40, 200);
        if self.frame_ms != frame_ms {
            self.frame_ms = frame_ms;
            changed = true;
        }

        if self.icon_mode != config.runner_icon_mode {
            self.icon_mode = config.runner_icon_mode;
            changed = true;
        }

        let preferred = if self.runner_id_exists(&config.runner_id) {
            config.runner_id.clone()
        } else if let Some(first) = self.default_sets.first() {
            first.id.clone()
        } else {
            "fallback:runner".to_string()
        };
        let config_runner_changed = self.configured_runner_id != config.runner_id;
        self.configured_runner_id = config.runner_id.clone();

        let mut rotation_ids: Vec<String> = config
            .runner_rotation_ids
            .iter()
            .filter(|id| self.runner_id_exists(id))
            .cloned()
            .collect();
        if rotation_ids.is_empty() {
            rotation_ids.push(preferred.clone());
        }
        if !rotation_ids.iter().any(|id| id == &preferred) {
            rotation_ids.insert(0, preferred.clone());
        }
        if self.rotation_ids != rotation_ids {
            self.rotation_ids = rotation_ids;
            changed = true;
        }

        let mut desired = if config_runner_changed {
            preferred
        } else if self.rotation_ids.iter().any(|id| id == &self.selected_id) {
            self.selected_id.clone()
        } else {
            self.rotation_ids
                .first()
                .cloned()
                .unwrap_or_else(|| "fallback:runner".to_string())
        };

        let desired_idx = self
            .rotation_ids
            .iter()
            .position(|id| id == &desired)
            .unwrap_or(0);
        if self.rotation_index != desired_idx {
            self.rotation_index = desired_idx;
            changed = true;
        }
        if let Some(actual) = self.rotation_ids.get(self.rotation_index) {
            desired = actual.clone();
        }

        if self.selected_id != desired {
            self.selected_id = desired.clone();
            changed = true;
        }

        if changed {
            let (frames, precolored_white) =
                self.load_frames_for_id(&self.selected_id, &self.custom_sets_snapshot);
            self.active_frames = frames;
            self.active_frames_precolored_white = precolored_white;
            if self.active_frames.is_empty() {
                self.active_frames = fallback_frames();
                self.active_frames_precolored_white = false;
            }
            self.frame_index = 0;
            self.frame_accumulator = 0.0;
            let now = Instant::now();
            self.last_step = now;
            self.last_runner_switch = now;
        }

        if self.active_frames.is_empty() {
            self.active_frames = fallback_frames();
            self.active_frames_precolored_white = false;
            return true;
        }

        changed
    }

    fn menu_options(&self) -> Vec<RunnerMenuOption> {
        let mut options = self.default_sets.clone();
        for custom in &self.custom_sets_snapshot {
            options.push(RunnerMenuOption {
                id: format!("custom:{}", custom.id),
                title: format!("Custom: {}", custom.name),
            });
        }
        options
    }

    fn preview_images(&self, options: &[RunnerMenuOption]) -> HashMap<String, Retained<NSImage>> {
        let mut map = HashMap::new();
        for opt in options {
            let (frames, _) = self.load_frames_for_id(&opt.id, &self.custom_sets_snapshot);
            if let Some(first) = frames.into_iter().next() {
                first.setSize(NSSize::new(16.0, 16.0));
                map.insert(opt.id.clone(), first);
            }
        }
        map
    }

    fn current_frame(&self) -> Option<Retained<NSImage>> {
        self.active_frames.get(self.frame_index).cloned()
    }

    fn advance(&mut self, now: Instant, cpu_usage: f32) -> Option<Retained<NSImage>> {
        self.rotate_runner_if_needed(now);

        if self.active_frames.is_empty() {
            let (frames, precolored_white) =
                self.load_frames_for_id(&self.selected_id, &self.custom_sets_snapshot);
            self.active_frames = frames;
            self.active_frames_precolored_white = precolored_white;
            if self.active_frames.is_empty() {
                self.active_frames = fallback_frames();
                self.active_frames_precolored_white = false;
            }
            self.frame_index = 0;
            self.frame_accumulator = 0.0;
            self.last_step = now;
            return self.current_frame();
        }

        let elapsed_ms = now.duration_since(self.last_step).as_secs_f64() * 1000.0;
        self.last_step = now;

        let cpu_ratio = (cpu_usage.clamp(0.0, 100.0) / 100.0) as f64;
        let speed_factor = 0.35 + cpu_ratio * 3.0;
        let effective_frame_ms = (self.frame_ms as f64 / speed_factor).max(16.0);

        self.frame_accumulator += elapsed_ms;
        while self.frame_accumulator >= effective_frame_ms {
            self.frame_accumulator -= effective_frame_ms;
            self.frame_index = (self.frame_index + 1) % self.active_frames.len();
        }
        self.current_frame()
    }

    fn rotate_runner_if_needed(&mut self, now: Instant) {
        if self.rotation_ids.len() <= 1 {
            return;
        }
        let elapsed = now.duration_since(self.last_runner_switch).as_secs_f64();
        let interval = self.display_secs.max(1) as f64;
        if elapsed < interval {
            return;
        }

        let steps = (elapsed / interval).floor() as usize;
        if steps == 0 {
            return;
        }

        self.rotation_index = (self.rotation_index + steps) % self.rotation_ids.len();
        self.last_runner_switch = now;

        let next_id = self.rotation_ids[self.rotation_index].clone();
        if next_id != self.selected_id {
            self.selected_id = next_id;
            let (frames, precolored_white) =
                self.load_frames_for_id(&self.selected_id, &self.custom_sets_snapshot);
            self.active_frames = frames;
            self.active_frames_precolored_white = precolored_white;
            if self.active_frames.is_empty() {
                self.active_frames = fallback_frames();
                self.active_frames_precolored_white = false;
            }
            self.frame_index = 0;
            self.frame_accumulator = 0.0;
        }
    }

    fn runner_id_exists(&self, runner_id: &str) -> bool {
        if runner_id == "fallback:runner" {
            return true;
        }
        if self.default_sets.iter().any(|set| set.id == runner_id) {
            return true;
        }
        if let Some(id) = runner_id.strip_prefix("custom:") {
            return self.custom_sets_snapshot.iter().any(|set| set.id == id);
        }
        false
    }

    fn load_frames_for_id(
        &self,
        runner_id: &str,
        custom_sets: &[CustomRunnerSet],
    ) -> (Vec<Retained<NSImage>>, bool) {
        if let Some(prefix) = runner_id.strip_prefix("runcat:") {
            return self.load_runcat_frames(prefix);
        }
        if let Some(custom_id) = runner_id.strip_prefix("custom:") {
            if let Some(set) = custom_sets.iter().find(|set| set.id == custom_id) {
                return (load_custom_frames(set), false);
            }
            return (Vec::new(), false);
        }
        (fallback_frames(), false)
    }

    fn load_runcat_frames(&self, prefix: &str) -> (Vec<Retained<NSImage>>, bool) {
        if self.icon_mode == RunnerIconMode::White {
            let white_exported = load_exported_runcat_frames_from_dir(
                prefix,
                EXPORTED_RUN_CAT_FRAMES_WHITE_RELATIVE,
            );
            if !white_exported.is_empty() {
                return (white_exported, true);
            }
        }

        let exported =
            load_exported_runcat_frames_from_dir(prefix, EXPORTED_RUN_CAT_FRAMES_RELATIVE);
        if !exported.is_empty() {
            return (exported, false);
        }

        let Some(bundle) = &self.run_cat_bundle else {
            return (Vec::new(), false);
        };
        let mut frames = Vec::new();
        for idx in 0..40 {
            let name = NSString::from_str(&format!("{}-page-{}", prefix, idx));
            if let Some(image) = bundle.imageForResource(&name) {
                image.setTemplate(false);
                frames.push(image);
            } else if !frames.is_empty() {
                break;
            }
        }
        (frames, false)
    }
}

fn discover_runcat_sets(bundle: Option<&Retained<NSBundle>>) -> Vec<RunnerMenuOption> {
    let mut prefixes = discover_runcat_prefixes_from_exported_frames();
    let use_bundle_probe = prefixes.is_empty();
    if use_bundle_probe {
        prefixes = resolve_runcat_assets_car_path()
            .map(|path| discover_runcat_prefixes_from_assets_car(&path))
            .unwrap_or_default();
    }
    if prefixes.is_empty() {
        prefixes = vec![
            "cat".to_string(),
            "cat-b".to_string(),
            "cat-c".to_string(),
            "cat-tail".to_string(),
            "human".to_string(),
            "engine".to_string(),
            "steam-locomotive".to_string(),
            "rabbit".to_string(),
            "horse".to_string(),
        ];
    }

    let mut options = Vec::new();
    let bundle = if use_bundle_probe { bundle } else { None };
    for prefix in prefixes {
        if let Some(bundle) = bundle {
            let probe = NSString::from_str(&format!("{}-page-0", prefix));
            if bundle.imageForResource(&probe).is_none() {
                continue;
            }
        }
        options.push(RunnerMenuOption {
            id: format!("runcat:{}", prefix),
            title: format!("RunCat {}", humanize_runner_prefix(&prefix)),
        });
    }
    options.sort_by(|a, b| a.title.cmp(&b.title));
    options
}

#[derive(Deserialize)]
struct AssetCatalogEntry {
    #[serde(rename = "AssetType")]
    asset_type: Option<String>,
    #[serde(rename = "Name")]
    name: Option<String>,
}

fn discover_runcat_prefixes_from_assets_car(assets_car: &Path) -> Vec<String> {
    if !assets_car.exists() {
        return Vec::new();
    }

    let Ok(output) = Command::new("assetutil").arg("-I").arg(assets_car).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    let Ok(entries) = serde_json::from_slice::<Vec<AssetCatalogEntry>>(&output.stdout) else {
        return Vec::new();
    };

    let mut prefixes = BTreeSet::new();
    for entry in entries {
        if entry.asset_type.as_deref() != Some("Image") {
            continue;
        }
        let Some(name) = entry.name else {
            continue;
        };
        let Some(prefix) = name.strip_suffix("-page-0") else {
            continue;
        };
        if !prefix.is_empty() {
            prefixes.insert(prefix.to_string());
        }
    }

    prefixes.into_iter().collect()
}

fn humanize_runner_prefix(prefix: &str) -> String {
    prefix
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(capitalize_ascii_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn capitalize_ascii_word(word: &str) -> String {
    let mut chars = word.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut result = String::new();
    result.push(first.to_ascii_uppercase());
    for c in chars {
        result.push(c);
    }
    result
}

fn fallback_frames() -> Vec<Retained<NSImage>> {
    let mut frames = Vec::new();
    for symbol in ["figure.run", "hare.fill", "figure.walk"] {
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(symbol),
            None,
        ) {
            frames.push(image);
        }
    }
    if frames.is_empty() {
        if let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str("circle.fill"),
            None,
        ) {
            frames.push(image);
        }
    }
    frames
}

fn load_custom_frames(set: &CustomRunnerSet) -> Vec<Retained<NSImage>> {
    let mut frames = Vec::new();
    for path in &set.frame_paths {
        if let Some(image) = load_image_from_file(Path::new(path)) {
            image.setTemplate(false);
            frames.push(image);
        }
    }
    frames
}

fn load_image_from_file(path: &Path) -> Option<Retained<NSImage>> {
    let ns_path = NSString::from_str(path.to_string_lossy().as_ref());
    NSImage::initWithContentsOfFile(NSImage::alloc(), &ns_path)
}

fn executable_contents_resources_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let macos_dir = exe.parent()?;
    let contents_dir = macos_dir.parent()?;
    let resources_dir = contents_dir.join("Resources");
    if resources_dir.exists() {
        Some(resources_dir)
    } else {
        None
    }
}

fn resolve_exported_runcat_frames_dir(relative: &str) -> Option<PathBuf> {
    let resources_dir = executable_contents_resources_dir()?;
    let exported = resources_dir.join(relative);
    if exported.exists() {
        Some(exported)
    } else {
        None
    }
}

fn discover_runcat_prefixes_from_exported_frames() -> Vec<String> {
    let Some(root) = resolve_exported_runcat_frames_dir(EXPORTED_RUN_CAT_FRAMES_RELATIVE) else {
        return Vec::new();
    };

    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };

    let mut prefixes = BTreeSet::new();
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }

        let prefix = entry.file_name().to_string_lossy().to_string();
        if prefix.is_empty() || prefix == "all-runners" || prefix == "self-made" {
            continue;
        }

        let has_frames = fs::read_dir(entry.path())
            .ok()
            .map(|files| {
                files.flatten().any(|file| {
                    file.path()
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .map(is_supported_runner_image_ext)
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        if has_frames {
            prefixes.insert(prefix);
        }
    }

    prefixes.into_iter().collect()
}

fn load_exported_runcat_frames_from_dir(prefix: &str, relative: &str) -> Vec<Retained<NSImage>> {
    let Some(root) = resolve_exported_runcat_frames_dir(relative) else {
        return Vec::new();
    };
    let dir = root.join(prefix);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .map(is_supported_runner_image_ext)
                .unwrap_or(false)
        })
        .collect();
    files.sort();

    let mut frames = Vec::new();
    for file in files {
        if let Some(image) = load_image_from_file(&file) {
            image.setTemplate(false);
            frames.push(image);
        }
    }
    frames
}

fn is_supported_runner_image_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "tiff" | "webp" | "heic"
    )
}

fn resolve_runcat_ui_bundle_path() -> Option<PathBuf> {
    let resources_dir = executable_contents_resources_dir()?;
    let embedded = resources_dir.join(EMBEDDED_RUN_CAT_UI_BUNDLE_RELATIVE);
    if embedded.exists() {
        Some(embedded)
    } else {
        None
    }
}

fn resolve_runcat_assets_car_path() -> Option<PathBuf> {
    let resources_dir = executable_contents_resources_dir()?;
    let embedded = resources_dir.join(EMBEDDED_RUN_CAT_UI_ASSETS_RELATIVE);
    if embedded.exists() {
        Some(embedded)
    } else {
        None
    }
}

fn load_run_cat_bundle() -> Option<Retained<NSBundle>> {
    let bundle_path = resolve_runcat_ui_bundle_path()?;
    NSBundle::bundleWithPath(&NSString::from_str(bundle_path.to_string_lossy().as_ref()))
}

fn custom_frames_root_dir() -> PathBuf {
    config_dir().join("custom-runners")
}

fn copy_custom_frames(files: &[PathBuf]) -> std::io::Result<(String, Vec<String>)> {
    fs::create_dir_all(custom_frames_root_dir())?;
    let set_id = CustomRunnerSet::generate_id();
    let target_dir = custom_frames_root_dir().join(&set_id);
    fs::create_dir_all(&target_dir)?;

    let mut copied = Vec::new();
    for (idx, src) in files.iter().enumerate() {
        let ext = src
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("png")
            .to_lowercase();
        let target = target_dir.join(format!("{:03}.{}", idx, ext));
        fs::copy(src, &target)?;
        copied.push(target.to_string_lossy().to_string());
    }

    Ok((set_id, copied))
}

// ── Title renderer ──

fn to_total_cpu_percent(stats: &SystemStats) -> f32 {
    stats.cpu.global_usage
}

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
            let full_len = text.encode_utf16().count();
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
            let line1_len = line1.encode_utf16().count();
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

// ── Menu builders ──

/// CPU/system menu (tags 100-199)
fn build_native_menu(
    stats: &SystemStats,
    config: &Config,
    mtm: MainThreadMarker,
    runner_options: &[RunnerMenuOption],
    runner_preview_images: &HashMap<String, Retained<NSImage>>,
    info_items: &mut Vec<Retained<NSMenuItem>>,
    login_item_out: &mut Option<Retained<NSMenuItem>>,
) -> Retained<NSMenu> {
    unsafe {
        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        let mut tag: isize = 100;
        let cpu_percent = to_total_cpu_percent(stats);
        info_items.clear();

        MENU_ACTIONS.with(|actions| {
            let mut actions = actions.borrow_mut();
            actions.retain(|k, _| *k < 100 || *k >= 400);

            // About
            let version = env!("CARGO_PKG_VERSION");
            let about_item = make_info_item(&format!("Mac State Monitor v{}", version), mtm);
            menu.addItem(&about_item);

            menu.addItem(&NSMenuItem::separatorItem(mtm));

            // CPU
            let cpu_item = make_info_item(&format!("CPU: {:.1}%", cpu_percent), mtm);
            menu.addItem(&cpu_item);
            info_items.push(cpu_item);

            // Memory
            let mem = &stats.memory;
            let mem_item = make_info_item(
                &format!(
                    "Memory: {} / {} ({:.0}%)",
                    format_bytes(mem.used_bytes),
                    format_bytes(mem.total_bytes),
                    mem.usage_percent
                ),
                mtm,
            );
            menu.addItem(&mem_item);
            info_items.push(mem_item);

            // Disk (first only)
            if let Some(disk) = stats.disks.first() {
                let name = if disk.name.is_empty() {
                    &disk.mount_point
                } else {
                    &disk.name
                };
                let disk_item = make_info_item(
                    &format!(
                        "Disk {}: {} / {} ({:.0}%)",
                        name,
                        format_bytes(disk.total_bytes - disk.available_bytes),
                        format_bytes(disk.total_bytes),
                        disk.usage_percent
                    ),
                    mtm,
                );
                menu.addItem(&disk_item);
                info_items.push(disk_item);
            }

            // Network
            let net_item = make_info_item(
                &format!(
                    "Net: D {} /s  U {} /s",
                    format_speed(stats.network.received_per_sec),
                    format_speed(stats.network.transmitted_per_sec)
                ),
                mtm,
            );
            menu.addItem(&net_item);
            info_items.push(net_item);

            menu.addItem(&NSMenuItem::separatorItem(mtm));

            // Temperature
            for reading in &stats.temperature.readings {
                let temp_item =
                    make_info_item(&format!("{}: {:.0}C", reading.label, reading.temp_c), mtm);
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
                if secs == config.poll_interval_secs {
                    item.setState(NSControlStateValueOn);
                }
                actions.insert(tag, format!("interval_{}", secs));
                tag += 1;
                interval_sub.addItem(&item);
            }
            interval_sub_item.setSubmenu(Some(&interval_sub));
            menu.addItem(&interval_sub_item);

            menu.addItem(&NSMenuItem::separatorItem(mtm));

            // Runner categorized menu
            let runner_item = NSMenuItem::new(mtm);
            runner_item.setTitle(&NSString::from_str("Runner"));
            let runner_sub = NSMenu::new(mtm);
            let effective_rotation_ids = if config.runner_rotation_ids.is_empty() {
                vec![config.runner_id.clone()]
            } else {
                config.runner_rotation_ids.clone()
            };

            // "All" option
            let all_selected = runner_options.iter().all(|opt| effective_rotation_ids.contains(&opt.id));
            let all_item = make_action_item("All", tag, mtm);
            if all_selected {
                all_item.setState(NSControlStateValueOn);
            }
            actions.insert(tag, RUNNER_ALL_ID.to_string());
            tag += 1;
            runner_sub.addItem(&all_item);
            runner_sub.addItem(&NSMenuItem::separatorItem(mtm));

            // Categorize runners
            let categories: &[(&str, &[&str])] = &[
                ("Cats", &["runcat:cat", "runcat:cat-b", "runcat:cat-c", "runcat:cat-tail", "runcat:flash-cat", "runcat:golden-cat", "runcat:metal-cluster-cat", "runcat:mock-nyan-cat", "runcat:maneki-neko"]),
                ("Dogs", &["runcat:dog", "runcat:puppy", "runcat:terrier", "runcat:welsh-corgi", "runcat:greyhound"]),
                ("Animals", &["runcat:bird", "runcat:butterfly", "runcat:chameleon", "runcat:cheetah", "runcat:chicken", "runcat:dinosaur", "runcat:dolphin", "runcat:dragon", "runcat:fishman", "runcat:fox", "runcat:frog", "runcat:hamster-wheel", "runcat:hedgehog", "runcat:horse", "runcat:mouse", "runcat:octopus", "runcat:otter", "runcat:owl", "runcat:parrot", "runcat:penguin", "runcat:penguin2", "runcat:pig", "runcat:rabbit", "runcat:reindeer-sleigh", "runcat:sheep", "runcat:squirrel", "runcat:uhooi", "runcat:whale"]),
                ("Food", &["runcat:coffee", "runcat:frypan", "runcat:mochi", "runcat:rotating-sushi", "runcat:rubber-duck", "runcat:sausage", "runcat:sushi", "runcat:tapioca-drink"]),
                ("People", &["runcat:dogeza", "runcat:human", "runcat:party-people", "runcat:push-up", "runcat:sit-up"]),
                ("Machines", &["runcat:cogwheel", "runcat:engine", "runcat:factory", "runcat:reactor", "runcat:rocket", "runcat:steam-locomotive"]),
                ("Nature", &["runcat:bonfire", "runcat:drop", "runcat:earth", "runcat:slime", "runcat:snowman", "runcat:sparkler", "runcat:wind-chime"]),
                ("Fantasy", &["runcat:ghost", "runcat:jack-o-lantern", "runcat:triforce"]),
                ("Abstract", &["runcat:city", "runcat:cradle", "runcat:dots", "runcat:entaku", "runcat:pendulum", "runcat:pulse", "runcat:sine-curve"]),
            ];

            for (cat_name, cat_ids) in categories {
                let cat_opts: Vec<&RunnerMenuOption> = runner_options.iter()
                    .filter(|opt| cat_ids.contains(&opt.id.as_str()))
                    .collect();
                if cat_opts.is_empty() {
                    continue;
                }

                let cat_menu_item = NSMenuItem::new(mtm);
                cat_menu_item.setTitle(&NSString::from_str(cat_name));
                let cat_sub = NSMenu::new(mtm);

                // Category-level toggle
                let cat_all_item = make_action_item(&format!("All {}", cat_name), tag, mtm);
                let cat_all_selected = cat_opts.iter().all(|opt| effective_rotation_ids.contains(&opt.id));
                if cat_all_selected {
                    cat_all_item.setState(NSControlStateValueOn);
                }
                actions.insert(tag, format!("{}{}", RUNNER_CATEGORY_PREFIX, cat_name));
                tag += 1;
                cat_sub.addItem(&cat_all_item);
                cat_sub.addItem(&NSMenuItem::separatorItem(mtm));

                for opt in &cat_opts {
                    let item = make_action_item(&opt.title, tag, mtm);
                    if effective_rotation_ids.contains(&opt.id) {
                        item.setState(NSControlStateValueOn);
                    }
                    if let Some(preview) = runner_preview_images.get(&opt.id) {
                        item.setImage(Some(preview));
                    }
                    actions.insert(tag, format!("{}{}", RUNNER_TOGGLE_PREFIX, opt.id));
                    tag += 1;
                    cat_sub.addItem(&item);
                }

                cat_menu_item.setSubmenu(Some(&cat_sub));
                runner_sub.addItem(&cat_menu_item);
            }

            runner_sub.addItem(&NSMenuItem::separatorItem(mtm));

            // Import custom runner
            let import_item = make_action_item("Import Custom Runner Frames…", tag, mtm);
            actions.insert(tag, RUNNER_IMPORT_ID.to_string());
            tag += 1;
            runner_sub.addItem(&import_item);

            // Display time
            let display_sub_item = NSMenuItem::new(mtm);
            display_sub_item.setTitle(&NSString::from_str("Display Time"));
            let display_sub = NSMenu::new(mtm);
            let effective_display_secs = match config.runner_display_secs {
                60 | 600 | 1800 | 3600 => config.runner_display_secs,
                _ => 600,
            };
            for (secs, label) in [
                (60_u64, "1 min"),
                (600_u64, "10 min"),
                (1800_u64, "30 min"),
                (3600_u64, "1 h"),
            ] {
                let item = make_action_item(&label, tag, mtm);
                if secs == effective_display_secs {
                    item.setState(NSControlStateValueOn);
                }
                actions.insert(tag, format!("{}{}", RUNNER_DISPLAY_PREFIX, secs));
                tag += 1;
                display_sub.addItem(&item);
            }
            display_sub_item.setSubmenu(Some(&display_sub));
            runner_sub.addItem(&display_sub_item);

            runner_item.setSubmenu(Some(&runner_sub));
            menu.addItem(&runner_item);

            menu.addItem(&NSMenuItem::separatorItem(mtm));

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

unsafe fn make_action_item(title: &str, tag: isize, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
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
        let attr_str =
            NSMutableAttributedString::initWithString(NSMutableAttributedString::alloc(), &ns_text);
        let range = NSRange::new(0, title.encode_utf16().count());
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
