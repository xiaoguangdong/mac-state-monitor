use crate::model::HistoryBuffer;
use plotters::prelude::*;
use plotters_bitmap::BitMapBackend;
use std::collections::VecDeque;
use std::num::NonZeroU32;
use tao::dpi::LogicalSize;
use tao::event_loop::EventLoopWindowTarget;
use tao::window::{Window, WindowBuilder};

const WIN_WIDTH: u32 = 800;
const WIN_HEIGHT: u32 = 520;
const FIXED_TEMPS: [&str; 3] = ["CPU", "GPU", "SSD"];

// Modern dark theme colors
const BG_COLOR: RGBColor = RGBColor(28, 28, 32);
const GRID_COLOR: RGBColor = RGBColor(45, 45, 52);
const TEXT_COLOR: RGBColor = RGBColor(220, 220, 225);
const TEMP_COLORS: [RGBColor; 3] = [
    RGBColor(255, 95, 87),  // CPU - Coral
    RGBColor(80, 200, 200), // GPU - Teal
    RGBColor(255, 203, 0),  // SSD - Gold
];
const CPU_COLOR: RGBColor = RGBColor(90, 200, 250);
const MEM_COLOR: RGBColor = RGBColor(175, 130, 255);
const NET_DOWN_COLOR: RGBColor = RGBColor(50, 215, 75);
const NET_UP_COLOR: RGBColor = RGBColor(255, 159, 10);

#[derive(Clone, Copy, PartialEq)]
pub enum ChartMode {
    All,
    TempOnly,
}

pub struct ChartWindow {
    window: Option<Box<Window>>,
    context: Option<softbuffer::Context<&'static Window>>,
    surface: Option<softbuffer::Surface<&'static Window, &'static Window>>,
    visible: bool,
    mode: ChartMode,
}

impl ChartWindow {
    pub fn new() -> Self {
        Self {
            window: None,
            context: None,
            surface: None,
            visible: false,
            mode: ChartMode::All,
        }
    }

    pub fn toggle(&mut self, event_loop: &EventLoopWindowTarget<()>, mode: ChartMode) {
        if self.visible && self.mode == mode {
            if let Some(w) = &self.window {
                w.set_visible(false);
            }
            self.visible = false;
        } else {
            self.mode = mode;
            if self.window.is_none() {
                self.create_window(event_loop);
            }
            if let Some(w) = &self.window {
                w.set_visible(true);
                w.request_redraw();
            }
            self.visible = true;
        }
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn window_id(&self) -> Option<tao::window::WindowId> {
        self.window.as_ref().map(|w| w.id())
    }

    pub fn handle_close(&mut self) {
        self.surface = None;
        self.context = None;
        self.window = None;
        self.visible = false;
    }

    fn create_window(&mut self, event_loop: &EventLoopWindowTarget<()>) {
        let window = Box::new(
            WindowBuilder::new()
                .with_title("System Monitor")
                .with_inner_size(LogicalSize::new(WIN_WIDTH, WIN_HEIGHT))
                .with_min_inner_size(LogicalSize::new(400u32, 240u32))
                .with_resizable(true)
                .with_visible(false)
                .build(event_loop)
                .expect("failed to create chart window"),
        );

        let window_ref: &'static Window = unsafe { &*(window.as_ref() as *const Window) };

        let context =
            softbuffer::Context::new(window_ref).expect("failed to create softbuffer context");
        let surface =
            softbuffer::Surface::new(&context, window_ref).expect("failed to create surface");

        self.window = Some(window);
        self.context = Some(unsafe { std::mem::transmute(context) });
        self.surface = Some(unsafe { std::mem::transmute(surface) });
    }

    pub fn render(&mut self, history: &HistoryBuffer) {
        if !self.visible {
            return;
        }

        let window = match &self.window {
            Some(w) => w,
            None => return,
        };

        let phys = window.inner_size();
        let width = phys.width;
        let height = phys.height;
        if width == 0 || height == 0 {
            return;
        }

        let surface = match &mut self.surface {
            Some(s) => s,
            None => return,
        };

        let _ = surface.resize(
            NonZeroU32::new(width).unwrap(),
            NonZeroU32::new(height).unwrap(),
        );

        let (w, h) = (width as usize, height as usize);
        let mut pixel_buf = vec![0u8; w * h * 3];

        {
            let backend = BitMapBackend::with_buffer(&mut pixel_buf, (width, height));
            let root = backend.into_drawing_area();
            let _ = root.fill(&BG_COLOR);

            match self.mode {
                ChartMode::All => {
                    // 3 rows x 2 cols layout
                    let rows = root.split_evenly((3, 1));
                    let top = rows[0].split_evenly((1, 2));
                    let mid = rows[1].split_evenly((1, 2));

                    draw_percent_chart(&top[0], "CPU", &history.cpu_usage, &CPU_COLOR);
                    draw_percent_chart(&top[1], "MEM", &history.mem_usage, &MEM_COLOR);
                    draw_net_chart(&mid[0], "NET Down", &history.net_down, &NET_DOWN_COLOR);
                    draw_net_chart(&mid[1], "NET Up", &history.net_up, &NET_UP_COLOR);
                    draw_temp_combined(&rows[2], history);
                }
                ChartMode::TempOnly => {
                    draw_temp_combined(&root, history);
                }
            }

            let _ = root.present();
        }

        // Copy RGB to softbuffer (ARGB format)
        let mut buf = surface.buffer_mut().unwrap();
        for i in 0..w * h {
            let r = pixel_buf[i * 3] as u32;
            let g = pixel_buf[i * 3 + 1] as u32;
            let b = pixel_buf[i * 3 + 2] as u32;
            buf[i] = (255 << 24) | (r << 16) | (g << 8) | b;
        }
        let _ = buf.present();

        window.request_redraw();
    }
}

fn draw_percent_chart(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    name: &str,
    data: &VecDeque<f32>,
    color: &RGBColor,
) {
    let current = data
        .back()
        .map(|v| format!("{:.1}%", v))
        .unwrap_or("--".into());
    let caption = format!("{}  {}", name, current);

    let mut chart = ChartBuilder::on(area)
        .caption(&caption, ("sans-serif", 36).into_font().color(&TEXT_COLOR))
        .margin(6)
        .x_label_area_size(0)
        .y_label_area_size(34)
        .build_cartesian_2d(0..data.len().max(1), 0.0f32..100.0)
        .unwrap();

    let _ = chart
        .configure_mesh()
        .light_line_style(GRID_COLOR.mix(0.3))
        .bold_line_style(GRID_COLOR.mix(0.6))
        .y_labels(3)
        .y_label_formatter(&|v| format!("{:.0}%", v))
        .label_style(("sans-serif", 24).into_font().color(&TEXT_COLOR.mix(0.7)))
        .draw();

    let series: Vec<(usize, f32)> = data
        .iter()
        .enumerate()
        .map(|(i, &v)| (i, v))
        .collect();

    if !series.is_empty() {
        let _ = chart.draw_series(AreaSeries::new(
            series.iter().cloned(),
            0.0,
            color.mix(0.2).filled(),
        ));
        let _ = chart.draw_series(LineSeries::new(
            series.iter().cloned(),
            color.stroke_width(2),
        ));
    }
}

fn draw_net_chart(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    name: &str,
    data: &VecDeque<f64>,
    color: &RGBColor,
) {
    let max_val = data
        .iter()
        .cloned()
        .fold(10.0_f64, |a, b| a.max(b))
        * 1.2;

    let current = data
        .back()
        .map(|v| {
            if *v >= 1024.0 {
                format!("{:.1} MB/s", v / 1024.0)
            } else {
                format!("{:.0} KB/s", v)
            }
        })
        .unwrap_or("--".into());
    let caption = format!("{}  {}", name, current);

    let mut chart = ChartBuilder::on(area)
        .caption(&caption, ("sans-serif", 36).into_font().color(&TEXT_COLOR))
        .margin(6)
        .x_label_area_size(0)
        .y_label_area_size(42)
        .build_cartesian_2d(0..data.len().max(1), 0.0..max_val)
        .unwrap();

    let _ = chart
        .configure_mesh()
        .light_line_style(GRID_COLOR.mix(0.3))
        .bold_line_style(GRID_COLOR.mix(0.6))
        .y_labels(3)
        .y_label_formatter(&|v| {
            if *v >= 1024.0 {
                format!("{:.0}M", v / 1024.0)
            } else {
                format!("{:.0}K", v)
            }
        })
        .label_style(("sans-serif", 24).into_font().color(&TEXT_COLOR.mix(0.7)))
        .draw();

    let series: Vec<(usize, f64)> = data
        .iter()
        .enumerate()
        .map(|(i, &v)| (i, v))
        .collect();

    if !series.is_empty() {
        let _ = chart.draw_series(AreaSeries::new(
            series.iter().cloned(),
            0.0,
            color.mix(0.2).filled(),
        ));
        let _ = chart.draw_series(LineSeries::new(
            series.iter().cloned(),
            color.stroke_width(2),
        ));
    }
}

fn draw_temp_combined(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    history: &HistoryBuffer,
) {
    let empty = VecDeque::new();
    let all_data: Vec<(&str, &VecDeque<f32>, &RGBColor)> = FIXED_TEMPS
        .iter()
        .enumerate()
        .map(|(i, &name)| {
            let data = history.temps.get(name).unwrap_or(&empty);
            (name, data, &TEMP_COLORS[i])
        })
        .collect();

    // Find max data length
    let mut max_len = 1usize;
    for (_, data, _) in &all_data {
        max_len = max_len.max(data.len());
    }

    let mut chart = ChartBuilder::on(area)
        .caption(
            "TEMP",
            ("sans-serif", 36).into_font().color(&TEXT_COLOR),
        )
        .margin(6)
        .x_label_area_size(0)
        .y_label_area_size(34)
        .build_cartesian_2d(0..max_len, 0.0f32..100.0)
        .unwrap();

    let _ = chart
        .configure_mesh()
        .light_line_style(GRID_COLOR.mix(0.3))
        .bold_line_style(GRID_COLOR.mix(0.6))
        .y_labels(3)
        .y_label_formatter(&|v| format!("{:.0}", v))
        .label_style(("sans-serif", 24).into_font().color(&TEXT_COLOR.mix(0.7)))
        .draw();

    for (name, data, color) in &all_data {
        let series: Vec<(usize, f32)> = data
            .iter()
            .enumerate()
            .map(|(i, &v)| (i, v))
            .collect();
        if !series.is_empty() {
            let val = data.back().map(|v| format!("{:.0}", v)).unwrap_or("--".into());
            let label = format!("{} {}", name, val);
            let _ = chart.draw_series(AreaSeries::new(
                series.iter().cloned(),
                0.0,
                color.mix(0.2).filled(),
            ));
            let _ = chart
                .draw_series(LineSeries::new(
                    series.iter().cloned(),
                    (*color).stroke_width(2),
                ))
                .unwrap()
                .label(label)
                .legend(move |(x, y)| {
                    PathElement::new(vec![(x, y), (x + 30, y)], (*color).stroke_width(3))
                });
        }
    }

    let _ = chart
        .configure_series_labels()
        .position(SeriesLabelPosition::UpperLeft)
        .background_style(BG_COLOR.mix(0.8))
        .border_style(GRID_COLOR)
        .label_font(("sans-serif", 24).into_font().color(&TEXT_COLOR))
        .draw();
}
