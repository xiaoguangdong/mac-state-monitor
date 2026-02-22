mod app;
mod config;
mod model;
mod monitor;
mod ui;

use app::App;
use std::time::{Duration, Instant};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use ui::tray::{take_pending_event, QUIT_ID, SHOW_CHARTS_ID, TEMP_PREFIX};

fn main() {
    let event_loop = EventLoopBuilder::<()>::with_user_event().build();

    let mut app = App::new();
    app.tick();

    let mut poll_interval = Duration::from_secs(app.config().poll_interval_secs);

    event_loop.run(move |event, event_loop, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + poll_interval);

        match event {
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                app.tick();
                poll_interval = Duration::from_secs(app.config().poll_interval_secs);
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
                ..
            } => {
                if app.chart_window.window_id() == Some(window_id) {
                    app.chart_window.handle_close();
                }
            }
            _ => {}
        }

        // Handle native menu events
        if let Some(action) = take_pending_event() {
            match action.as_str() {
                QUIT_ID => *control_flow = ControlFlow::Exit,
                SHOW_CHARTS_ID => app.toggle_charts(event_loop),
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
