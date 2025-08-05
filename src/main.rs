use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style, Stylize},
    text::Line,
    widgets::{Block, List, ListDirection, ListState, Paragraph},
};
use thiserror::Error;

fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let terminal = ratatui::init();
    let app = App::new();
    let result = app.run(terminal);
    ratatui::restore();
    match result {
        Ok(logs) => {
            println!("Printing Glimpse log output...");
            for log_line in logs {
                println!("{}", log_line);
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

const SYS_CLASS_LEDS: &str = "/sys/class/leds";
#[derive(Debug)]
struct LED {
    file_name: String,
    name: String,
    is_on: bool,
}

#[derive(Debug, Error)]
enum NewLEDError {
    #[error("LED does not exist")]
    NotFound,
    #[error("Invalid brightness value")]
    InvalidBrightness,
    /// File name is invalid UTF-8
    #[error("Invalid encoding in file name")]
    InvalidFileName,
    #[error("I/O error: {0}")]
    IOError(std::io::Error),
}

impl From<std::io::Error> for NewLEDError {
    fn from(err: std::io::Error) -> Self {
        Self::IOError(err)
    }
}

impl LED {
    pub fn new(file_name: String) -> Result<Self, NewLEDError> {
        let led_path = PathBuf::from(SYS_CLASS_LEDS).join(&file_name);
        fs::read_dir(&led_path).map_err(|e| match e.kind() {
            ErrorKind::NotFound => NewLEDError::NotFound,
            _ => NewLEDError::IOError(e),
        })?;
        let brightness_data = fs::read_to_string(&led_path.join("brightness"))?;
        let brightness = brightness_data
            .trim()
            .parse::<u8>()
            .map_err(|_| NewLEDError::InvalidBrightness)?;
        Ok(Self {
            name: file_name.clone().replace("::", " "),
            file_name,
            is_on: brightness > 0,
        })
    }
}

fn get_all_leds() -> Result<Vec<LED>, NewLEDError> {
    let mut leds = Vec::new();
    let directories = fs::read_dir(SYS_CLASS_LEDS).map_err(NewLEDError::IOError)?;
    for directory in directories {
        let directory = directory.map_err(NewLEDError::IOError)?;
        let file_name = directory.file_name();
        leds.push(LED::new(
            file_name
                .into_string()
                .map_err(|_| NewLEDError::InvalidFileName)?,
        )?);
    }
    Ok(leds)
}

#[derive(Debug, Default, PartialEq, Eq)]
enum Pane {
    #[default]
    Sidebar,
    Mainbar,
}

/// The main application which holds the state and logic of the application.
#[derive(Debug, Default)]
pub struct App {
    /// Is the application running?
    running: bool,
    leds: Vec<LED>,
    // selected_led: Option<LED>,
    log: Vec<String>,
    focused_pane: Pane,
    led_list_state: ListState,
}

impl App {
    /// Construct a new instance of [`App`].
    pub fn new() -> Self {
        let mut log = Vec::new();
        let leds = match get_all_leds() {
            Ok(leds) => {
                log.push(format!("Successfully found {} LED(s)", leds.len()));
                leds
            }
            Err(e) => {
                log.push(format!("Error getting LEDs: {}", e));
                Vec::new()
            }
        };
        Self {
            running: false,
            focused_pane: Pane::default(),
            leds,
            log,
            led_list_state: ListState::default(),
        }
    }

    /// Run the application's main loop.
    pub fn run(mut self, mut terminal: DefaultTerminal) -> Result<Vec<String>> {
        self.running = true;
        while self.running {
            terminal.draw(|frame| self.render(frame))?;
            self.handle_crossterm_events()?;
        }
        self.log.push("Exiting Glimpse".to_string());
        Ok(self.log)
    }

    /// Renders the user interface.
    ///
    /// This is where you add new widgets. See the following resources for more information:
    ///
    /// - <https://docs.rs/ratatui/latest/ratatui/widgets/index.html>
    /// - <https://github.com/ratatui/ratatui/tree/main/ratatui-widgets/examples>
    fn render(&mut self, frame: &mut Frame) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Percentage(20), Constraint::Min(20)])
            .split(frame.area());
        // Left panel
        let left_panel_title = Line::from("LEDs").bold().blue().centered();
        let led_list = List::new(self.leds.iter().map(|led| led.name.to_string()))
            .block(Block::bordered().title(left_panel_title))
            .style(Style::new().white())
            .highlight_style(Style::new().bg(Color::Blue));
        frame.render_stateful_widget(led_list, layout[0], &mut self.led_list_state);
        // Right panel
        let title = Line::from("LED detail").bold().blue().centered();
        let text = self.log.join("\n");
        frame.render_widget(
            Paragraph::new(text)
                .block(Block::bordered().title(title))
                .left_aligned(),
            layout[1],
        );
    }

    /// Reads the crossterm events and updates the state of [`App`].
    ///
    /// If your application needs to perform work in between handling events, you can use the
    /// [`event::poll`] function to check if there are any events available with a timeout.
    fn handle_crossterm_events(&mut self) -> Result<()> {
        match event::read()? {
            // it's important to check KeyEventKind::Press to avoid handling key release events
            Event::Key(key) if key.kind == KeyEventKind::Press => self.on_key_event(key),
            Event::Mouse(_) => {}
            Event::Resize(_, _) => {}
            _ => {}
        }
        Ok(())
    }

    /// Handles the key events and updates the state of [`App`].
    fn on_key_event(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (_, KeyCode::Esc | KeyCode::Char('q'))
            | (KeyModifiers::CONTROL, KeyCode::Char('c') | KeyCode::Char('C')) => self.quit(),
            (_, KeyCode::Up) if self.focused_pane == Pane::Sidebar => {
                self.led_list_state.select_previous();
            }
            (_, KeyCode::Down) if self.focused_pane == Pane::Sidebar => {
                self.led_list_state.select_next();
            }
            _ => {}
        }
    }

    /// Set running to false to quit the application.
    fn quit(&mut self) {
        self.running = false;
    }
}
