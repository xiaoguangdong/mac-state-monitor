mod alert;
mod app;
mod config;
mod launch_agent;
mod model;
mod monitor;
mod ui;

use app::App;
use config::LAUNCH_AT_LOGIN_ID;
use std::time::{Duration, Instant};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use ui::chart_window::ChartMode;
use ui::tray::{take_pending_event, QUIT_ID, SHOW_CHARTS_ID, SHOW_TEMP_CHARTS_ID, TEMP_PREFIX};

fn main() {
    let event_loop = EventLoopBuilder::<()>::with_user_event().build();

    let mut app = App::new();
    app.tick();

    let mut poll_interval = Duration::from_secs(app.config().poll_interval_secs);
    let mut last_tick = Instant::now();

    event_loop.run(move |event, event_loop, control_flow| {
        let now = Instant::now();
        if now.duration_since(last_tick) >= poll_interval {
            app.tick();
            poll_interval = Duration::from_secs(app.config().poll_interval_secs);
            last_tick = now;
        }
        *control_flow = ControlFlow::WaitUntil(last_tick + poll_interval);

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
                ..
            } => {
                if app.chart_window.window_id() == Some(window_id) {
                    app.chart_window.handle_close();
                }
            }
            Event::RedrawRequested(window_id) => {
                if app.chart_window.window_id() == Some(window_id) {
                    app.chart_window.render(&app.history);
                }
            }
            _ => {}
        }

        // Handle native menu events
        if let Some(action) = take_pending_event() {
            match action.as_str() {
                QUIT_ID => *control_flow = ControlFlow::Exit,
                SHOW_CHARTS_ID => app.toggle_charts(event_loop, ChartMode::All),
                SHOW_TEMP_CHARTS_ID => app.toggle_charts(event_loop, ChartMode::TempOnly),
                LAUNCH_AT_LOGIN_ID => app.toggle_launch_at_login(),
                _ if action.starts_with("interval_") => {
                    if let Ok(secs) = action.trim_start_matches("interval_").parse::<u64>() {
                        app.set_poll_interval(secs);
                    }
                }
                _ if action.starts_with(TEMP_PREFIX) => {
                    let component = action.trim_start_matches(TEMP_PREFIX).to_string();
                    app.set_temp_component(component);
                }
                _ => {}
            }
        }
    });
}
