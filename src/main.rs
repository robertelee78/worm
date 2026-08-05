#[cfg(not(target_arch = "wasm32"))]
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
#[cfg(not(target_arch = "wasm32"))]
use std::io::{self, Read, Write};
#[cfg(not(target_arch = "wasm32"))]
use std::os::unix::io::AsRawFd;
#[cfg(not(target_arch = "wasm32"))]
use std::thread;
#[cfg(not(target_arch = "wasm32"))]
use std::time::{Duration, Instant};

#[cfg(not(target_arch = "wasm32"))]
use worm::{Direction, WormGame};

/// Per-player brain file (the terminal player's cross-session corpus).
/// Override with WORM_BRAIN=/path/to/file.
///
/// Lives under `$XDG_DATA_HOME/worm` (or `~/.local/share/worm`). It used to
/// default to a RELATIVE `worm_brain.bin`, so launching the game from a
/// different directory silently started a brand-new brain and orphaned the
/// old one — a data-loss bug in a game whose entire premise is that the
/// opponent remembers you across sessions.
#[cfg(not(target_arch = "wasm32"))]
fn brain_path() -> std::path::PathBuf {
    if let Ok(explicit) = std::env::var("WORM_BRAIN") {
        return std::path::PathBuf::from(explicit);
    }
    let dir = std::env::var("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
                .join(".local/share")
        })
        .join("worm");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("worm_brain.bin")
}

/// Where to LOAD from: the current location, else the legacy relative file in
/// the working directory. A player upgrading past this change keeps the brain
/// they earned; the next save rewrites it to the stable location.
#[cfg(not(target_arch = "wasm32"))]
fn brain_load_path() -> std::path::PathBuf {
    let current = brain_path();
    if current.exists() {
        return current;
    }
    let legacy = std::path::PathBuf::from("worm_brain.bin");
    if legacy.exists() {
        return legacy;
    }
    current
}

#[cfg(not(target_arch = "wasm32"))]
fn save_brain(game: &WormGame) {
    let _ = game.cpu_brain.save_file(&brain_path());
}

/// What a raw read on the game-over screen asks for. Arrow keys arrive as
/// ESC-prefixed sequences in raw mode, so only a LONE ESC read (or q/Q)
/// quits — a player still steering at the moment of death must not exit
/// the app and lose the session scoreboard.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, PartialEq, Debug)]
enum GameOverKey {
    Quit,
    Restart,
    None,
}

#[cfg(not(target_arch = "wasm32"))]
fn game_over_key(buf: &[u8]) -> GameOverKey {
    let mut i = 0;
    while i < buf.len() {
        match buf[i] {
            0x1b => {
                if buf.len() == 1 {
                    return GameOverKey::Quit;
                }
                // Swallow the whole ESC-prefixed sequence: CSI (ESC [ ... with
                // a 0x40-0x7e final byte), SS3 (ESC O X), or Alt+key (ESC X).
                i += 1;
                if i < buf.len() && (buf[i] == b'[' || buf[i] == b'O') {
                    i += 1;
                    while i < buf.len() {
                        let b = buf[i];
                        i += 1;
                        if (0x40..=0x7e).contains(&b) {
                            break;
                        }
                    }
                } else {
                    i += 1;
                }
            }
            b'q' | b'Q' => return GameOverKey::Quit,
            b'r' | b'R' => return GameOverKey::Restart,
            _ => i += 1,
        }
    }
    GameOverKey::None
}

/// One key parsed from the in-play input buffer.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Copy, PartialEq, Debug)]
enum PlayKey {
    Dir(Direction),
    Fire,
    Pause,
    Quit,
    Ignore,
}

/// Parse the next key at the head of the buffer, returning what it asks for
/// and how many bytes it consumed. `None` means an incomplete ESC sequence —
/// wait for more bytes. Every COMPLETE ESC-prefixed sequence is consumed
/// whole (CSI, SS3 arrows, Alt+key): the old parser only understood ESC '['
/// and broke without consuming on anything else, so one Alt+key or
/// application-cursor arrow wedged the buffer — and every later key,
/// including 'q', queued behind it forever.
#[cfg(not(target_arch = "wasm32"))]
fn parse_play_key(buf: &[u8]) -> Option<(PlayKey, usize)> {
    match buf[0] {
        0x1b => {
            if buf.len() < 2 {
                return None; // lone ESC — the standalone-ESC timer decides
            }
            if buf[1] == b'[' || buf[1] == b'O' {
                if buf.len() < 3 {
                    return None; // incomplete CSI/SS3 — wait for the final byte
                }
                let key = match buf[2] {
                    b'A' => PlayKey::Dir(Direction::Up),
                    b'B' => PlayKey::Dir(Direction::Down),
                    b'C' => PlayKey::Dir(Direction::Right),
                    b'D' => PlayKey::Dir(Direction::Left),
                    _ => PlayKey::Ignore,
                };
                Some((key, 3))
            } else {
                // Alt+key or unknown ESC pair — discard both bytes.
                Some((PlayKey::Ignore, 2))
            }
        }
        b'q' | b'Q' => Some((PlayKey::Quit, 1)),
        b'p' | b'P' => Some((PlayKey::Pause, 1)),
        b'h' | b'a' => Some((PlayKey::Dir(Direction::Left), 1)),
        b'j' | b's' => Some((PlayKey::Dir(Direction::Down), 1)),
        b'k' | b'w' => Some((PlayKey::Dir(Direction::Up), 1)),
        b'l' | b'd' => Some((PlayKey::Dir(Direction::Right), 1)),
        b' ' => Some((PlayKey::Fire, 1)),
        _ => Some((PlayKey::Ignore, 1)),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> std::io::Result<()> {
    let fd = io::stdin().as_raw_fd();

    let mut old_termios: libc::termios = unsafe { std::mem::zeroed() };
    unsafe {
        libc::tcgetattr(fd, &mut old_termios);
    }
    let mut raw_termios = old_termios;
    unsafe {
        libc::cfmakeraw(&mut raw_termios);
        libc::tcsetattr(fd, libc::TCSAFLUSH, &raw_termios);
    }

    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, Hide)?;
    stdout.flush()?;

    let mut game = WormGame::new();
    // Per-player memory: the brain from previous sessions rides again
    // (rps-ai: "what someone opens with is a habit, it is stored").
    if let Some(brain) = worm::CpuBrain::load_file(&brain_load_path()) {
        game.cpu_brain = brain;
    }
    let mut last_update = Instant::now();
    let mut input_buf: Vec<u8> = Vec::new();
    let mut quit = false;
    let mut paused = false;

    loop {
        // Read all available input (non-blocking with 1ms timeout)
        let mut fd_set: libc::fd_set = unsafe { std::mem::zeroed() };
        unsafe {
            libc::FD_ZERO(&mut fd_set);
            libc::FD_SET(fd, &mut fd_set);
        }
        let mut timeout = libc::timeval {
            tv_sec: 0,
            tv_usec: 1000,
        }; // 1ms
        let ret = unsafe {
            libc::select(
                fd + 1,
                &mut fd_set,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut timeout,
            )
        };

        if ret > 0 {
            let mut buf = [0u8; 256];
            match io::stdin().read(&mut buf) {
                Ok(n) if n > 0 => {
                    input_buf.extend_from_slice(&buf[..n]);
                    // Read any remaining input
                    loop {
                        let mut fd_set2: libc::fd_set = unsafe { std::mem::zeroed() };
                        unsafe {
                            libc::FD_ZERO(&mut fd_set2);
                            libc::FD_SET(fd, &mut fd_set2);
                        }
                        let mut timeout2 = libc::timeval {
                            tv_sec: 0,
                            tv_usec: 0,
                        };
                        let ret2 = unsafe {
                            libc::select(
                                fd + 1,
                                &mut fd_set2,
                                std::ptr::null_mut(),
                                std::ptr::null_mut(),
                                &mut timeout2,
                            )
                        };
                        if ret2 <= 0 {
                            break;
                        }
                        match io::stdin().read(&mut buf) {
                            Ok(n) if n > 0 => input_buf.extend_from_slice(&buf[..n]),
                            _ => break,
                        }
                    }
                }
                _ => {}
            }
        }

        // Process complete input sequences; parse_play_key consumes every
        // complete ESC sequence whole, so unrecognized ones (Alt+key, SS3)
        // can never wedge the buffer.
        let mut processed = 0;
        while processed < input_buf.len() {
            match parse_play_key(&input_buf[processed..]) {
                None => break, // incomplete ESC sequence — wait for more bytes
                Some((key, n)) => {
                    match key {
                        PlayKey::Dir(d) => game.change_direction(d),
                        // Firing is gated on !paused. It was not, and the game
                        // clock is: only update() is skipped while paused, so
                        // a paused player could fire a laser in frozen time —
                        // and a laser kill ends the game outright. Direction
                        // input stays live because it only latches a pending
                        // turn that the next frame would apply anyway.
                        PlayKey::Fire => {
                            if !paused {
                                let _ = game.fire_powerup(0);
                            }
                        }
                        PlayKey::Pause => {
                            paused = !paused;
                            if paused {
                                execute!(
                                    stdout,
                                    SetForegroundColor(Color::Yellow),
                                    MoveTo(0, game.height),
                                    Print("PAUSED — press P to resume"),
                                    ResetColor,
                                )?;
                                stdout.flush()?;
                            } else {
                                // Don't bank the frozen time as owed frames.
                                last_update = Instant::now();
                            }
                        }
                        PlayKey::Quit => quit = true,
                        PlayKey::Ignore => {}
                    }
                    processed += n;
                }
            }
        }

        // Remove processed bytes from buffer
        if processed > 0 && processed <= input_buf.len() {
            input_buf.drain(0..processed);
        }

        if quit {
            save_brain(&game);
            unsafe {
                libc::tcsetattr(fd, libc::TCSAFLUSH, &old_termios);
            }
            execute!(stdout, LeaveAlternateScreen, Show)?;
            return Ok(());
        }

        // Handle standalone ESC (if it's been sitting in buffer too long)
        if input_buf.len() == 1 && input_buf[0] == 0x1b {
            // ESC sitting alone - could be standalone or first byte of sequence
            // Wait briefly for more data
            let mut fd_set: libc::fd_set = unsafe { std::mem::zeroed() };
            unsafe {
                libc::FD_ZERO(&mut fd_set);
                libc::FD_SET(fd, &mut fd_set);
            }
            let mut t = libc::timeval {
                tv_sec: 0,
                tv_usec: 20000,
            };
            let r = unsafe {
                libc::select(
                    fd + 1,
                    &mut fd_set,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &mut t,
                )
            };
            if r > 0 {
                let mut b = [0u8; 16];
                if let Ok(n) = io::stdin().read(&mut b) {
                    if n > 0 {
                        input_buf.extend_from_slice(&b[..n]);
                    }
                }
            } else {
                // Timeout - standalone ESC means quit
                quit = true;
            }
        }

        // Game update
        if !paused && last_update.elapsed() >= game.frame_delay() {
            game.update();
            game.render(&mut stdout);
            stdout.flush()?;
            last_update = Instant::now();
        }

        if game.game_over {
            // Persist the per-player brain after every game — crash-safe
            // cross-session learning (also saved on quit). The inner loop
            // below blocks until restart/quit, so this runs once per game.
            save_brain(&game);
            execute!(
                stdout,
                SetForegroundColor(Color::Red),
                MoveTo(0, game.height),
                Print(format!(
                    "GAME OVER! Score: {}  Press R to restart, Q to quit",
                    game.score
                )),
                ResetColor,
            )?;
            stdout.flush()?;

            loop {
                let mut fd_set: libc::fd_set = unsafe { std::mem::zeroed() };
                unsafe {
                    libc::FD_ZERO(&mut fd_set);
                    libc::FD_SET(fd, &mut fd_set);
                }
                let mut timeout = libc::timeval {
                    tv_sec: 1,
                    tv_usec: 0,
                };
                let ret = unsafe {
                    libc::select(
                        fd + 1,
                        &mut fd_set,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        &mut timeout,
                    )
                };
                if ret > 0 {
                    let mut buf = [0u8; 16];
                    if let Ok(n) = io::stdin().read(&mut buf) {
                        if n > 0 {
                            match game_over_key(&buf[..n]) {
                                GameOverKey::Quit => {
                                    save_brain(&game);
                                    unsafe {
                                        libc::tcsetattr(fd, libc::TCSAFLUSH, &old_termios);
                                    }
                                    execute!(stdout, LeaveAlternateScreen, Show)?;
                                    return Ok(());
                                }
                                GameOverKey::Restart => {
                                    game.restart();
                                    last_update = Instant::now();
                                    input_buf.clear();
                                    break;
                                }
                                GameOverKey::None => {}
                            }
                        }
                    }
                }
            }
        }

        thread::sleep(Duration::from_millis(1));
    }
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{game_over_key, parse_play_key, GameOverKey, PlayKey};
    use worm::Direction;

    #[test]
    fn alt_key_noise_cannot_wedge_the_input_buffer() {
        // ESC + non-'[' (Alt+x) is consumed whole; the quit key behind it
        // is reachable on the next parse step instead of queueing forever.
        assert_eq!(parse_play_key(b"\x1bxq"), Some((PlayKey::Ignore, 2)));
        assert_eq!(parse_play_key(b"q"), Some((PlayKey::Quit, 1)));
    }

    #[test]
    fn ss3_application_cursor_arrows_steer() {
        assert_eq!(
            parse_play_key(b"\x1bOA"),
            Some((PlayKey::Dir(Direction::Up), 3))
        );
    }

    #[test]
    fn incomplete_esc_sequences_wait_for_more_bytes() {
        assert_eq!(parse_play_key(b"\x1b"), None);
        assert_eq!(parse_play_key(b"\x1b["), None);
    }

    #[test]
    fn p_toggles_pause() {
        assert_eq!(parse_play_key(b"p"), Some((PlayKey::Pause, 1)));
        assert_eq!(parse_play_key(b"P"), Some((PlayKey::Pause, 1)));
    }

    #[test]
    fn arrow_keys_do_not_quit_the_game_over_screen() {
        for seq in [
            b"\x1b[A".as_slice(),
            b"\x1b[B",
            b"\x1b[C",
            b"\x1b[D",
            b"\x1bOA",
        ] {
            assert_eq!(game_over_key(seq), GameOverKey::None);
        }
    }

    #[test]
    fn quit_and_restart_keys_still_work() {
        assert_eq!(game_over_key(b"q"), GameOverKey::Quit);
        assert_eq!(game_over_key(b"Q"), GameOverKey::Quit);
        assert_eq!(
            game_over_key(b"\x1b"),
            GameOverKey::Quit,
            "a lone ESC read is a deliberate quit"
        );
        assert_eq!(game_over_key(b"r"), GameOverKey::Restart);
        assert_eq!(game_over_key(b"R"), GameOverKey::Restart);
    }

    #[test]
    fn restart_behind_a_buffered_arrow_is_seen() {
        assert_eq!(game_over_key(b"\x1b[Ar"), GameOverKey::Restart);
    }
}
