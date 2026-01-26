use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::mpsc::{channel, RecvTimeoutError};
    use std::thread;

    let flags = xflags::parse_or_exit! {
        /// Exectuable to run when mouse is detected to be actively moved
        optional --on-active on_active: PathBuf
        /// Exectuable to run when mouse is detected to be not actively moved
        optional --on-inactive on_inactive: PathBuf
        /// Whether to supress output of the current state
        optional -q,--quiet
        /// The minimum gap between two readings to consider the mouse inactive, in milliseconds.
        /// Defaults to one second.
        optional --min-movement-gap min_movement_gap: u64
    };
    
    let (sender, receiver) = channel();
    let on_active_callback: Proc = {
        let quiet = flags.quiet;
        let on_active = flags.on_active;

        macro_rules! announce {
            () => {
                if !quiet {
                    println!("ACTIVE");
                }
            }
        }

        if let Some(on_active) = on_active {
            Box::new(move || {
                announce!();

                if let Err(e) = Command::new::<&PathBuf>(&on_active).spawn() {
                    return Err(format!("Failed to run {}: {e}", on_active.display()));
                }

                Ok(())
            })
        } else {
            Box::new(move || {
                announce!();

                Ok(())
            })
        }
    };

    let on_inactive_callback: Proc = {
        let quiet = flags.quiet;
        let on_inactive = flags.on_inactive;

        macro_rules! announce {
            () => {
                if !quiet {
                    println!("INACTIVE");
                }
            }
        }

        if let Some(on_inactive) = on_inactive {
            Box::new(move || {
                announce!();

                if let Err(e) = Command::new::<&PathBuf>(&on_inactive).spawn() {
                    return Err(format!("Failed to run {}: {e}", on_inactive.display()));
                }

                Ok(())
            })
        } else {
            Box::new(move || {
                announce!();

                Ok(())
            })
        }
    };

    let get_now = Box::new(Instant::now);

    let min_movement_gap: Duration = flags.min_movement_gap.map(Duration::from_millis).unwrap_or(Duration::from_secs(1));

    let mut handler: Handler =
        get_handler(on_active_callback, on_inactive_callback, get_now, min_movement_gap);

    let timeout: Duration = min_movement_gap.div_f32(4.0);

    thread::spawn(move || {
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
}

enum Event {
    Mousemove,
    TimePassed,
}

type Proc = Box<dyn FnMut() -> Result<(), String> + Send>;
type GetNow = Box<dyn FnMut() -> Instant + Send>;
type Handler = Box<dyn FnMut(Event) -> Result<(), String> + Send>;

fn get_handler(
    mut on_active_callback: Proc,
    mut on_inactive_callback: Proc,
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
                on_active_callback()?;
            },
            (true, false) => {
                on_inactive_callback()?;
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

    #[derive(Debug, PartialEq, Eq)]
    enum Call {
        Active,
        Inactive
    }

    use std::borrow::{Borrow, BorrowMut};
    use std::sync::Arc;
    use std::sync::RwLock;

    let mut calls = Arc::new(RwLock::new(vec![]));

    let active_handle: Arc<_> = calls.clone();
    let on_active_callback = Box::new(move || {
        let mut calls = active_handle.write().unwrap();
        calls.push(Call::Active);
        Ok(())
    });

    let inactive_handle: Arc<_> = calls.clone();
    let on_inactive_callback = Box::new(move || {
        let mut calls = inactive_handle.write().unwrap();
        calls.push(Call::Inactive);
        Ok(())
    });

    let mut handler = get_handler(on_active_callback, on_inactive_callback, get_now, min_movement_gap);

    handler(Event::Mousemove).unwrap();

    handler(Event::TimePassed).unwrap();
    handler(Event::TimePassed).unwrap();
    handler(Event::TimePassed).unwrap();

    assert_eq!(&*(calls.read().unwrap()), &vec![Call::Active]);

    for _ in 0..5 {
        handler(Event::Mousemove).unwrap();

        handler(Event::TimePassed).unwrap();
        handler(Event::TimePassed).unwrap();
        handler(Event::TimePassed).unwrap();
    }

    // No change from before
    assert_eq!(&*(calls.read().unwrap()), &vec![Call::Active]);

    for _ in 0..5 {
        handler(Event::TimePassed).unwrap();
    }

    assert_eq!(&*(calls.read().unwrap()), &vec![Call::Active, Call::Inactive]);
}