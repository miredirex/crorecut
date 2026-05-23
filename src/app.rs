use crate::ui::{editor, grid, settings};
use iced::event::{self, Event};
use iced::keyboard::{self, key};
use iced::widget::stack;
use iced::{Element, Subscription, Task, theme};
use std::path::PathBuf;

/// Screen enum - Grid state is stored separately on Crorecut since it's the home screen
#[allow(clippy::large_enum_variant)]
enum Screen {
    Grid,
    Editor(editor::Editor),
}

/// Nested message enum with screen-specific variants
#[derive(Debug, Clone)]
pub enum Message {
    // App-level navigation
    NavigateToGrid,
    NavigateToEditor(PathBuf),

    // Screen-specific messages (nested)
    Grid(grid::Message),
    Editor(editor::Message),
    Settings(settings::Message),
    HideSettings,
    // TODO: About(about::Message),
}

pub struct Crorecut {
    screen: Screen,
    grid_state: grid::Grid,
    settings: settings::Settings,
    show_settings_modal: bool,
}

impl Crorecut {
    pub fn new() -> (Self, Task<Message>) {
        let videos_dir = dirs::video_dir().unwrap_or_else(|| PathBuf::from("."));
        let grid_state = grid::Grid::new(videos_dir);

        let app = Self {
            screen: Screen::Grid,
            grid_state,
            settings: settings::Settings::new(),
            show_settings_modal: false,
        };

        let initial_task = Task::batch([
            Task::perform(
                async { Message::Grid(grid::Message::ScanVideosDirectory) },
                |msg| msg,
            ),
            iced::font::load(crate::JOST_FONT).discard(),
            iced::font::load(crate::JOST_FONT_LIGHT).discard(),
        ]);
        (app, initial_task)
    }

    pub fn title(&self) -> String {
        String::from("Crorecut")
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            // App-level navigation
            Message::NavigateToGrid => {
                let restore_scroll = !matches!(self.screen, Screen::Grid);
                self.screen = Screen::Grid;

                if restore_scroll {
                    iced::widget::operation::snap_to(
                        grid::grid_scrollable_id(),
                        self.grid_state.scroll_offset(),
                    )
                } else {
                    Task::none()
                }
            }

            Message::NavigateToEditor(video_path) => {
                self.screen = Screen::Editor(editor::Editor::new(video_path.clone()));

                // Start preparing the editor
                Task::batch([
                    Task::done(Message::Editor(editor::Message::PrepareEditor)),
                    Task::perform(
                        async move { crate::video::get_video_stream_info(&video_path).await },
                        |result| {
                            Message::Editor(editor::Message::VideoStreamInfoReceived(
                                result.map_err(|e| e.to_string()),
                            ))
                        },
                    ),
                ])
            }

            Message::Grid(msg) => {
                // Special case: VideoSelected triggers navigation to editor
                if let grid::Message::VideoSelected(ref path) = msg {
                    return self.update(Message::NavigateToEditor(path.clone()));
                }

                if matches!(msg, grid::Message::ShowSettings) {
                    self.show_settings_modal = true;
                }

                self.grid_state.update(msg).map(Message::Grid)
            }

            Message::Editor(msg) => {
                // Special case: GoBack navigates to grid
                if matches!(msg, editor::Message::GoBack) {
                    return self.update(Message::NavigateToGrid);
                }

                if let Screen::Editor(ref mut state) = self.screen {
                    state.update(msg).map(Message::Editor)
                } else {
                    Task::none()
                }
            }

            Message::Settings(msg) => self.settings.update(msg).map(Message::Settings),

            Message::HideSettings => {
                self.show_settings_modal = false;
                Task::none()
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        let content = match &self.screen {
            Screen::Grid => self.grid_state.view().map(Message::Grid),
            Screen::Editor(state) => state.view().map(Message::Editor),
        };

        let settings_overlay: Option<Element<'_, Message>> = self.show_settings_modal.then(|| {
            settings::modal_overlay(
                self.settings.view().map(Message::Settings),
                Message::HideSettings,
            )
        });

        stack![content, settings_overlay].into()
    }

    pub fn style(&self, _theme: &iced::Theme) -> iced::theme::Style {
        theme::Style {
            background_color: crate::ui::color::BACKGROUND,
            text_color: crate::ui::color::TEXT,
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let screen_subscription = match &self.screen {
            Screen::Grid => Subscription::none(),
            Screen::Editor(state) => state.subscription().map(Message::Editor),
        };

        Subscription::batch([screen_subscription, self.settings_subscription()])
    }

    fn settings_subscription(&self) -> Subscription<Message> {
        if !self.show_settings_modal {
            return Subscription::none();
        }

        let close_subscription = event::listen_with(|event, _status, _window| match event {
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(key::Named::Escape),
                ..
            }) => Some(Message::HideSettings),
            _ => None,
        });

        Subscription::batch([
            close_subscription,
            self.settings.subscription().map(Message::Settings),
        ])
    }
}
