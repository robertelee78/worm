use crossterm::{
    cursor::{Hide, Show, MoveTo},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;
use std::time::{Duration, Instant};
use std::thread;

use worm::{WormGame, Direction};

fn main() -> std::io::Result<()> {
    let fd = io::stdin().as_raw_fd();
    
    let mut old_termios: libc::termios = unsafe { std::mem::zeroed() };
    unsafe { libc::tcgetattr(fd, &mut old_termios); }
    let mut raw_termios = old_termios;
    unsafe {
        libc::cfmakeraw(&mut raw_termios);
        libc::tcsetattr(fd, libc::TCSAFLUSH, &raw_termios);
    }
    
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, Hide)?;
    stdout.flush()?;

    let mut game = WormGame::new();
    let mut last_update = Instant::now();
    let mut input_buf: Vec<u8> = Vec::new();
    let mut quit = false;

    loop {
        // Read all available input (non-blocking with 1ms timeout)
        let mut fd_set: libc::fd_set = unsafe { std::mem::zeroed() };
        unsafe {
            libc::FD_ZERO(&mut fd_set);
            libc::FD_SET(fd, &mut fd_set);
        }
        let mut timeout = libc::timeval { tv_sec: 0, tv_usec: 1000 }; // 1ms
        let ret = unsafe {
            libc::select(fd + 1, &mut fd_set, std::ptr::null_mut(), std::ptr::null_mut(), &mut timeout)
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
                        let mut timeout2 = libc::timeval { tv_sec: 0, tv_usec: 0 };
                        let ret2 = unsafe {
                            libc::select(fd + 1, &mut fd_set2, std::ptr::null_mut(), std::ptr::null_mut(), &mut timeout2)
                        };
                        if ret2 <= 0 { break; }
                        match io::stdin().read(&mut buf) {
                            Ok(n) if n > 0 => input_buf.extend_from_slice(&buf[..n]),
                            _ => break,
                        }
                    }
                }
                _ => {}
            }
        }

        // Process complete input sequences
        // Parse 3-byte ESC sequences, keep incomplete ones in buffer
        let mut processed = 0;
        while processed < input_buf.len() {
            // Look for ESC [ X pattern
            if input_buf[processed] == 0x1b {
                if processed + 2 < input_buf.len() && input_buf[processed + 1] == b'[' {
                    // Have complete ESC [ X sequence
                    let cmd = input_buf[processed + 2];
                    match cmd {
                        b'A' => game.change_direction(Direction::Up),
                        b'B' => game.change_direction(Direction::Down),
                        b'C' => game.change_direction(Direction::Right),
                        b'D' => game.change_direction(Direction::Left),
                        _ => {}
                    }
                    processed += 3;
                } else {
                    // Incomplete ESC sequence - stop processing, wait for more
                    break;
                }
            } else {
                // Regular character
                match input_buf[processed] as char {
                    'q' | 'Q' => quit = true,
                    'h' => game.change_direction(Direction::Left),
                    'j' => game.change_direction(Direction::Down),
                    'k' => game.change_direction(Direction::Up),
                    'l' => game.change_direction(Direction::Right),
                    'w' => game.change_direction(Direction::Up),
                    'a' => game.change_direction(Direction::Left),
                    's' => game.change_direction(Direction::Down),
                    'd' => game.change_direction(Direction::Right),
                    ' ' => { let _ = game.fire_powerup(0); }
                    _ => {}
                }
                processed += 1;
            }
        }
        
        // Remove processed bytes from buffer
        if processed > 0 && processed <= input_buf.len() {
            input_buf.drain(0..processed);
        }

        if quit {
            unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &old_termios); }
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
            let mut t = libc::timeval { tv_sec: 0, tv_usec: 20000 };
            let r = unsafe {
                libc::select(fd + 1, &mut fd_set, std::ptr::null_mut(), std::ptr::null_mut(), &mut t)
            };
            if r > 0 {
                let mut b = [0u8; 16];
                if let Ok(n) = io::stdin().read(&mut b) {
                    if n > 0 { input_buf.extend_from_slice(&b[..n]); }
                }
            } else {
                // Timeout - standalone ESC means quit
                quit = true;
            }
        }

        // Game update
        if last_update.elapsed() >= game.frame_delay() {
            game.update();
            game.render(&mut stdout);
            stdout.flush()?;
            last_update = Instant::now();
        }

        if game.game_over {
            execute!(
                stdout,
                SetForegroundColor(Color::Red),
                MoveTo(0, game.height),
                Print(format!("GAME OVER! Score: {}  Press R to restart, Q to quit", game.score)),
                ResetColor,
            )?;
            stdout.flush()?;

            loop {
                let mut fd_set: libc::fd_set = unsafe { std::mem::zeroed() };
                unsafe {
                    libc::FD_ZERO(&mut fd_set);
                    libc::FD_SET(fd, &mut fd_set);
                }
                let mut timeout = libc::timeval { tv_sec: 1, tv_usec: 0 };
                let ret = unsafe {
                    libc::select(fd + 1, &mut fd_set, std::ptr::null_mut(), std::ptr::null_mut(), &mut timeout)
                };
                if ret > 0 {
                    let mut buf = [0u8; 16];
                    if let Ok(n) = io::stdin().read(&mut buf) {
                        if n > 0 {
                            match buf[0] as char {
                                'q' | 'Q' | '\x1b' => {
                                    unsafe { libc::tcsetattr(fd, libc::TCSAFLUSH, &old_termios); }
                                    execute!(stdout, LeaveAlternateScreen, Show)?;
                                    return Ok(());
                                }
                                'r' | 'R' => {
                                    game.restart();
                                    last_update = Instant::now();
                                    input_buf.clear();
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
        
        thread::sleep(Duration::from_millis(1));
    }
}
