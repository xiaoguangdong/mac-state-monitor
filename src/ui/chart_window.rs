use crate::model::HistoryBuffer;
use plotters::prelude::*;
use plotters_bitmap::BitMapBackend;
use std::collections::VecDeque;
use std::num::NonZeroU32;
use tao::dpi::LogicalSize;
use tao::event_loop::EventLoopWindowTarget;
use tao::window::{Window, WindowBuilder};

const WIN_WIDTH: u32 = 280;
const CHART_HEIGHT: u32 = 120;
const FIXED_COMPONENTS: [&str; 3] = ["CPU", "GPU", "SSD"];

// Modern dark theme colors
const BG_COLOR: RGBColor = RGBColor(28, 28, 32);
const GRID_COLOR: RGBColor = RGBColor(45, 45, 52);
const TEXT_COLOR: RGBColor = RGBColor(220, 220, 225);
const ACCENT_COLORS: [RGBColor; 3] = [
    RGBColor(255, 95, 87),   // CPU - Coral
    RGBColor(80, 200, 200),  // GPU - Teal
    RGBColor(255, 203, 0),   // SSD - Gold
];

pub struct ChartWindow {
    window: Option<Box<Window>>,
    context: Option<softbuffer::Context<&'static Window>>,
    surface: Option<softbuffer::Surface<&'static Window, &'static Window>>,
    visible: bool,
}

impl ChartWindow {
    pub fn new() -> Self {
        Self {
            window: None,
            context: None,
            surface: None,
            visible: false,
        }
    }

    pub fn toggle(&mut self, event_loop: &EventLoopWindowTarget<()>) {
        if self.visible {
            if let Some(w) = &self.window {
                w.set_visible(false);
            }
            self.visible = false;
        } else {
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
        if let Some(w) = &self.window {
            w.set_visible(false);
        }
        self.visible = false;
    }

    fn create_window(&mut self, event_loop: &EventLoopWindowTarget<()>) {
        let total_height = CHART_HEIGHT * 3;
        let window = Box::new(
            WindowBuilder::new()
                .with_title("Temperature Monitor")
                .with_inner_size(LogicalSize::new(WIN_WIDTH, total_height))
                .with_resizable(false)
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

        // Use physical size for pixel-accurate rendering
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

            let areas = root.split_evenly((3, 1));

            for (i, &comp_name) in FIXED_COMPONENTS.iter().enumerate() {
                let empty = VecDeque::new();
                let data = history.temps.get(comp_name).unwrap_or(&empty);
                draw_chart(&areas[i], comp_name, data, &ACCENT_COLORS[i]);
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

fn draw_chart(
    area: &DrawingArea<BitMapBackend, plotters::coord::Shift>,
    name: &str,
    data: &VecDeque<f32>,
    color: &RGBColor,
) {
    // Calculate dynamic Y-axis range
    let max_val = data
        .iter()
        .cloned()
        .fold(50.0_f32, |a, b| a.max(b))
        .max(50.0)
        * 1.12;
    let min_val = data
        .iter()
        .cloned()
        .fold(f32::MAX, |a, b| a.min(b))
        .min(30.0)
        .max(0.0);

    let current = data
        .back()
        .map(|v| format!("{:.0}°C", v))
        .unwrap_or("--".into());
    let caption = format!("{}  {}", name, current);

    let mut chart = ChartBuilder::on(area)
        .caption(&caption, ("sans-serif", 15).into_font().color(&TEXT_COLOR))
        .margin(6)
        .x_label_area_size(0)
        .y_label_area_size(38)
        .build_cartesian_2d(0..60usize, min_val..max_val)
        .unwrap();

    // Draw subtle grid
    let _ = chart
        .configure_mesh()
        .light_line_style(GRID_COLOR.mix(0.3))
        .bold_line_style(GRID_COLOR.mix(0.6))
        .y_labels(3)
        .y_label_formatter(&|v| format!("{:.0}", v))
        .label_style(("sans-serif", 10).into_font().color(&TEXT_COLOR.mix(0.7)))
        .draw();

    // Calculate offset for proper alignment
    let offset = if data.len() < 60 { 60 - data.len() } else { 0 };
    let series: Vec<(usize, f32)> = data
        .iter()
        .enumerate()
        .map(|(i, &v)| (i + offset, v))
        .collect();

    // Draw filled area with gradient effect
    if !series.is_empty() {
        let filled = color.mix(0.2);
        let _ = chart.draw_series(AreaSeries::new(
            series.iter().cloned(),
            min_val,
            filled.filled(),
        ));
    }

    // Draw the main line
    let _ = chart.draw_series(LineSeries::new(
        series.iter().cloned(),
        color.stroke_width(2),
    ));

    // Draw current value indicator
    if let Some(&(x, y)) = series.last() {
        let _ = chart.draw_series(PointSeries::of_element(
            vec![(x, y)],
            5,
            color.filled(),
            &|coord, size, style| {
                EmptyElement::at(coord)
                    + Circle::new((0, 0), size, style)
                    + Circle::new((0, 0), size - 1, BG_COLOR.filled())
            },
        ));
    }
}
