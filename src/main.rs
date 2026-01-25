fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;
    use std::sync::mpsc::channel;
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
    
    thread::spawn(move || {
        use std::process::Command;
        use std::sync::mpsc::RecvTimeoutError;
        use std::time::{Duration, Instant};
        let mut last_move_time = Instant::now();
        let mut last_is_active = false;

        let min_movement_gap: Duration = flags.min_movement_gap.map(Duration::from_millis).unwrap_or(Duration::from_secs(1));
        let timeout: Duration = min_movement_gap.div_f32(4.0);

        loop {
            let mut is_active = false;

            match receiver.recv_timeout(timeout) {
                Ok(_) => {
                    is_active = true;
                    last_move_time = Instant::now();
                }
                Err(RecvTimeoutError::Timeout) => {
                    if last_is_active {
                        let now = Instant::now();
                        let since = now.duration_since(last_move_time);
    
                        if since < min_movement_gap {
                            // Okay, check again later.
                            continue
                        } else {
                            is_active = false;
                        }
                    }
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }

            match (last_is_active, is_active) {
                (true, true) | (false, false) => {},
                (false, true) => {
                    if let Some(ref on_active) = flags.on_active {
                        if let Err(e) = Command::new::<&PathBuf>(on_active).spawn() {
                            drop(receiver);
                            panic!("Failed to run {}: {e}", on_active.display());
                        }
                    }
                    if !flags.quiet {
                        println!("ACTIVE");
                    }
                },
                (true, false) => {
                    if let Some(ref on_inactive) = flags.on_inactive {
                        if let Err(e) = Command::new::<&PathBuf>(on_inactive).spawn() {
                            drop(receiver);
                            panic!("Failed to run {}: {e}", on_inactive.display());
                        }
                    }
                    if !flags.quiet {
                        println!("INACTIVE");
                    }
                },
            }

            last_is_active = is_active;
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
