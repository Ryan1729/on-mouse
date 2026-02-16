use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::sync::mpsc::{RecvTimeoutError};

xflags::xflags! {
    cmd on-mouse {
        /// Exectuable to run when mouse is detected to be actively moved
        optional --on-active path: PathBuf
        /// Exectuable to run when mouse is detected to be not actively moved
        optional --on-inactive path: PathBuf
        /// Whether to supress output of the current state
        optional -q,--quiet
        /// Whether to display a chart instead of default, basic print output of the current state
        optional --chart
        /// The minimum gap between two readings to consider the mouse inactive, in milliseconds.
        /// Defaults to one second.
        optional --min-movement-gap milliseconds: u64
        /// The name of a device to grap and thus block any other applications from seeing.
        /// The passed name indicates which device to grab. If passed, any other mice will be 
        /// ignored by this program.
        /// On Linux, the name for a given device can be found using the `evdev` application.
        /// Currently not supported on Windows.
        // Making the grabbing work on windows seems extremely complciated and fiddly.
        // No crate or simple working example that specifically eats the input was
        // from the grabbed device was found. Apparently what one neesd to do is set
        // a low-level mouse hook, via SetWindowsHookEx and conditionally call or not
        // call CallNextHookEx in the handler, to pass on or not pass on the input.
        // That's fine enough, but the closest thing to an example like that which I
        // was able to find acted strangely and incorrectly when run, in a way that
        // makes me suspect undefined behaviour. Miri is no help here, because it 
        // doesn't support calling things like SetWindowsHookEx.
        optional --grab-device device_name: String
        /// Output the version and exit
        optional --version
    }
}


fn main() -> Result<(), Box<dyn std::error::Error>> {
    let flags = OnMouse::from_env_or_exit();

    if flags.version {
        println!("{}", env!("CARGO_PKG_VERSION"));

        return Ok(());
    }

    match flags.grab_device.clone() {
        None => {
            let (sender, receiver) = std::sync::mpsc::channel();

            std::thread::spawn(move || {
                activity_thread_main(receiver, flags)
            });

            let listen_callback = move |event: rdev::Event| {
                use rdev::EventType;
        
                match event.event_type {
                    EventType::MouseMove {..} => {
                        // If there's an error, we assume we won't be called again.
                        if let Err(_) = sender.send(()) {
                            std::process::exit(1);
                        }
                    }
                    _ => (),
                }
            };
        
            // This will call callback endlessly.
            rdev::listen(listen_callback).map_err(|e| format!("Error: {e:?}").into())
        },
        Some(target_device_name) => {
            #[cfg(target_family = "windows")]
            {
                return Err(format!("Can't grab {target_device_name} because grabbing devices is not yet supported on Windows").into())
            }

            #[cfg(not(target_family = "windows"))]
            {
                // TODO? Use evdev-rs instead of evdev since rdev uses evdev-rs?

                // Capture the target mouse.
                let devices = evdev::enumerate();
            
                let mut device_opt = None;
            
                for (_, device) in devices {
                    if device.name() == Some(&target_device_name) {
                        device_opt = Some(device);
            
                        break
                    }
                }
            
                if let Some(mut device) = device_opt {
                    device.grab()?;
            
                    println!("Grabbed device");
            
                    let (sender, receiver) = std::sync::mpsc::channel();
            
                    std::thread::spawn(move || {
                        activity_thread_main(receiver, flags)
                    });
            
                    // Monitor the target mouse's events, sending signals to other thread in response.
                    loop {
                        use evdev::{EventType, RelativeAxisCode};
            
                        match device.fetch_events() {
                            Ok(iter) => {
                                for event in iter {
                                    if event.event_type() == EventType::RELATIVE 
                                    && RelativeAxisCode(event.code()) == RelativeAxisCode::REL_Y { 
                                        // If there's an error, we assume we won't be called again.
                                        if let Err(_) = sender.send(()) {
                                            std::process::exit(1);
                                        }
                                    }
                                }
                            }        
                            Err(e) => { dbg!(e); }
                        }
                    }
                } else {
                    Err(format!("No device named \"{target_device_name}\" found. Elevated priveldges are needed to access some devices. Consider running with `sudo`").into())
                }
            }
        }
    }
}

fn activity_thread_main(receiver: std::sync::mpsc::Receiver<()>, flags: OnMouse) {
    use std::process::Command;

    let on_activity = {
        let on_active = flags.on_active;
        let on_inactive = flags.on_inactive;
        let quiet = flags.quiet;
        let chart = flags.chart;

        enum Mode {
            Quiet,
            Print,
            Chart(std::sync::mpsc::Sender<Activity>),
        }

        let mode = match (quiet, chart) {
            (false, false) => Mode::Print,
            (false, true) => {
                let (chart_sender, chart_receiver) = std::sync::mpsc::channel();

                std::thread::spawn(move || {
                    chart_thread(chart_receiver)
                });

                Mode::Chart(chart_sender)
            }
            (true, _) => Mode::Quiet,
        };

        Box::new(move |activity| {
            match mode {
                Mode::Quiet => {},
                Mode::Print => {
                    use Activity::*;

                    match activity {
                        Inactive => {
                            println!("INACTIVE");
                        },
                        Active => {
                            println!("ACTIVE");
                        },
                    }
                },
                Mode::Chart(ref chart_sender) => {
                    // If there's an error, we assume we won't be called again.
                    chart_sender.send(activity).map_err(|e| format!("{e}"))?;
                }
            };

            match activity {
                Activity::Active => {
                    if let Some(ref on_active) = on_active {
                        if let Err(e) = Command::new::<&PathBuf>(on_active)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn() {
                            return Err(format!("Failed to run {}: {e}", on_active.display()));
                        }
                    }
                }
                Activity::Inactive => {
                    if let Some(ref on_inactive) = on_inactive {
                        if let Err(e) = Command::new::<&PathBuf>(&on_inactive)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .spawn() {
                            return Err(format!("Failed to run {}: {e}", on_inactive.display()));
                        }
                    }
                }
            }

            Ok(())
        })
    };

    let get_now = Box::new(Instant::now);

    let min_movement_gap: Duration = flags.min_movement_gap.map(Duration::from_millis).unwrap_or(Duration::from_secs(1));

    let mut handler: Handler =
        get_handler(on_activity, get_now, min_movement_gap);

    let timeout: Duration = min_movement_gap.div_f32(4.0);


    loop {
        match receiver.recv_timeout(timeout) {
            Ok(_) => {
                if let Err(e) = handler(Event::Mousemove) {
                    drop(receiver);
                    panic!("{e}");
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Err(e) = handler(Event::TimePassed) {
                    drop(receiver);
                    panic!("{e}");
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Activity {
    Inactive,
    Active,
}

fn chart_thread(receiver: std::sync::mpsc::Receiver<Activity>) {
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyModifiers};

    const COUNT: usize = 200;

    let mut window: Vec<(f64, f64)> = Vec::with_capacity(COUNT);

    let mut update_and_render = move |frame: &mut ratatui::Frame, actvity| {
        use ratatui::prelude::*;
        use ratatui::style::{Modifier, Style};
        use ratatui::text::Span;
        use ratatui::widgets::{Axis, Block, Chart, Dataset};

        if window.len() >= COUNT {
            window.remove(0);

            for (i, el) in window.iter_mut().enumerate() {
                el.0 = i as f64;
            }
        }
        window.push((
            window.len() as f64,
            match actvity {
                Activity::Inactive => -1.,
                Activity::Active => 1.,
            }
        ));

        let x_min = 0.0;
        let x_max = window.len() as f64;

        let x_labels = vec![
            Span::styled(
                format!("{}", x_min),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("{}", window.len() / 2)),
            Span::styled(
                format!("{}", x_max),
                Style::default().add_modifier(Modifier::BOLD),
            ),
        ];
        let datasets = vec![
            Dataset::default()
                .name("Activity")
                .marker(symbols::Marker::Block)
                .style(Style::default().fg(Color::Cyan))
                .data(&window),
        ];

        let chart = Chart::new(datasets)
            .block(Block::bordered())
            .x_axis(
                Axis::default()
                    .title("Index")
                    .style(Style::default().fg(Color::Gray))
                    .labels(x_labels)
                    .bounds([x_min, x_max]),
            )
            .y_axis(
                Axis::default()
                    .title("Activity")
                    .style(Style::default().fg(Color::Gray))
                    .labels(["-1".bold(), "0".into(), "1".bold()])
                    .bounds([-1.0, 1.0]),
            );

        frame.render_widget(chart, frame.area());
    };

    let mut terminal = ratatui::init();

    let per_frame = Duration::from_millis(80);
    let half_frame = per_frame.div_f32(2.);

    let mut last_activity = Activity::Inactive;

    loop {
        match receiver.recv_timeout(half_frame) {
            Ok(activity) => {
                last_activity = activity;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }

        terminal.draw(|frame| update_and_render(frame, last_activity))
            .expect("terminal drawing should work");

        if event::poll(half_frame).expect("terminal events should work") {
            match event::read().expect("terminal events should work") {
                Event::Key(key_event) => {
                    match key_event.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('c' | 'C') if key_event.modifiers == KeyModifiers::CONTROL => break,
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }

    ratatui::restore();

    std::process::exit(0);
}

enum Event {
    Mousemove,
    TimePassed,
}

type OnActivity = Box<dyn FnMut(Activity) -> Result<(), String> + Send>;
type GetNow = Box<dyn FnMut() -> Instant + Send>;
type Handler = Box<dyn FnMut(Event) -> Result<(), String> + Send>;

fn get_handler(
    mut on_activity: OnActivity,
    mut get_now: GetNow,
    min_movement_gap: Duration,
) -> Handler {
    let mut last_move_time = get_now();
    let mut last_is_active = false;

    Box::new(move |event: Event| -> Result<(), String> {
        let mut is_active = false;

        match event {
            Event::Mousemove => {
                is_active = true;
                last_move_time = get_now();
            },
            Event::TimePassed => {
                if last_is_active {
                    let now = get_now();
                    let since = now.duration_since(last_move_time);

                    if since < min_movement_gap {
                        // Okay, check again later.
                        return Ok(())
                    } else {
                        is_active = false;
                    }
                }
            },
        }

        match (last_is_active, is_active) {
            (true, true) | (false, false) => {},
            (false, true) => {
                on_activity(Activity::Active)?;
            },
            (true, false) => {
                on_activity(Activity::Inactive)?;
            },
        }

        last_is_active = is_active;

        Ok(())
    })
}

#[test]
fn this_sequence_produces_the_expected_calls() {
    use std::ops::{Deref, DerefMut};

    let min_movement_gap = Duration::from_nanos(4);
    let timeout = Duration::from_nanos(1);

    let mut base_instant = Instant::now();

    let get_now = Box::new(move || {
        base_instant = base_instant.checked_add(timeout).unwrap();
        base_instant
    });

    use std::borrow::{Borrow, BorrowMut};
    use std::sync::Arc;
    use std::sync::RwLock;

    let mut calls = Arc::new(RwLock::new(vec![]));

    let active_handle: Arc<_> = calls.clone();
    let on_activity = Box::new(move |activity| {
        let mut calls = active_handle.write().unwrap();
        calls.push(activity);
        Ok(())
    });

    let mut handler = get_handler(on_activity, get_now, min_movement_gap);

    handler(Event::Mousemove).unwrap();

    handler(Event::TimePassed).unwrap();
    handler(Event::TimePassed).unwrap();
    handler(Event::TimePassed).unwrap();

    assert_eq!(&*(calls.read().unwrap()), &vec![Activity::Active]);

    for _ in 0..5 {
        handler(Event::Mousemove).unwrap();

        handler(Event::TimePassed).unwrap();
        handler(Event::TimePassed).unwrap();
        handler(Event::TimePassed).unwrap();
    }

    // No change from before
    assert_eq!(&*(calls.read().unwrap()), &vec![Activity::Active]);

    for _ in 0..5 {
        handler(Event::TimePassed).unwrap();
    }

    assert_eq!(&*(calls.read().unwrap()), &vec![Activity::Active, Activity::Inactive]);
}