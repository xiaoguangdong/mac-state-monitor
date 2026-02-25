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
use ui::tray::{
    take_pending_event, QUIT_ID, RUNNER_ALL_ID, RUNNER_CATEGORY_PREFIX, RUNNER_DISPLAY_PREFIX,
    RUNNER_IMPORT_ID, RUNNER_TOGGLE_PREFIX, SHOW_CHARTS_ID, SHOW_TEMP_CHARTS_ID, TEMP_PREFIX,
};

fn main() {
    let event_loop = EventLoopBuilder::<()>::with_user_event().build();

    let mut app = App::new();
    app.tick();

    let mut poll_interval = Duration::from_secs(app.config().poll_interval_secs);
    let mut last_tick = Instant::now();
    let animation_interval = Duration::from_millis(40);
    let mut last_animation = Instant::now();

    event_loop.run(move |event, event_loop, control_flow| {
        let now = Instant::now();
        if now.duration_since(last_tick) >= poll_interval {
            app.tick();
            poll_interval = Duration::from_secs(app.config().poll_interval_secs);
            last_tick = now;
        }
        if now.duration_since(last_animation) >= animation_interval {
            app.animate(now);
            last_animation = now;
        }

        let next_poll = last_tick + poll_interval;
        let next_animation = last_animation + animation_interval;
        *control_flow = ControlFlow::WaitUntil(next_poll.min(next_animation));

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
                RUNNER_ALL_ID => app.select_all_runners(),
                RUNNER_IMPORT_ID => app.import_custom_runner(),
                _ if action.starts_with("interval_") => {
                    if let Ok(secs) = action.trim_start_matches("interval_").parse::<u64>() {
                        app.set_poll_interval(secs);
                    }
                }
                _ if action.starts_with(RUNNER_DISPLAY_PREFIX) => {
                    if let Ok(secs) = action
                        .trim_start_matches(RUNNER_DISPLAY_PREFIX)
                        .parse::<u64>()
                    {
                        app.set_runner_display_secs(secs);
                    }
                }
                _ if action.starts_with(RUNNER_CATEGORY_PREFIX) => {
                    let category = action.trim_start_matches(RUNNER_CATEGORY_PREFIX).to_string();
                    app.select_runner_category(category);
                }
                _ if action.starts_with(RUNNER_TOGGLE_PREFIX) => {
                    let runner_id = action.trim_start_matches(RUNNER_TOGGLE_PREFIX).to_string();
                    app.toggle_runner_in_rotation(runner_id);
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
