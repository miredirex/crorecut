use iced::widget::{
    button, column, container, iced as iced_logo, image, mouse_area, opaque, pin, rich_text, row,
    space, span, stack, text, text_input,
};
use iced::{Alignment, Background, Color, Element, Font, Length};

use super::{Message, Section, Settings};
use crate::ui::color as app_colors;
use crate::ui::{corner_dots, crorecut_button, styles};

const SIDEBAR_WIDTH: f32 = 150.0;
const SIDEBAR_BUTTON_HEIGHT: f32 = 48.0;
const SIDEBAR_BUTTON_SPACING: f32 = 4.0;
const INDICATOR_WIDTH: f32 = 2.0;
const INDICATOR_HEIGHT: f32 = 24.0;
const INDICATOR_BUTTON_GAP: f32 = 1.0;
const SIDEBAR_ROW_SPAN: f32 = SIDEBAR_BUTTON_HEIGHT + SIDEBAR_BUTTON_SPACING;
const INDICATOR_Y_OFFSET: f32 = (SIDEBAR_BUTTON_HEIGHT - INDICATOR_HEIGHT) / 2.0;

impl Settings {
    pub fn view(&self) -> Element<'_, Message> {
        let sidebar_buttons = column![
            sidebar_button("General", Section::General),
            sidebar_button("About", Section::About),
        ]
        .spacing(SIDEBAR_BUTTON_SPACING)
        .width(SIDEBAR_WIDTH);

        let sidebar = container(
            stack![
                sidebar_buttons,
                pin(sidebar_indicator()).x(0.0).y(self.indicator_y()),
            ]
            .width(SIDEBAR_WIDTH),
        )
        .height(Length::Fill)
        .padding(18)
        .style(|_| container::Style {
            background: Some(Background::Color(app_colors::DARK_GRAY)),
            ..Default::default()
        });

        let content = match self.active_section {
            Section::General => self.general_view(),
            Section::About => self.about_view(),
        };

        let content = container(content)
            .width(Length::Fill)
            .height(Length::Fill)
            .padding(18);

        let body = row![sidebar, content]
            .height(Length::Fill)
            .width(Length::Fill);

        container(stack![body, corner_dots()])
            .width(720)
            .height(420)
            .style(|_| container::Style {
                background: Some(Background::Color(app_colors::BUTTON_BG)),
                ..Default::default()
            })
            .padding(2)
            .into()
    }

    fn general_view(&self) -> Element<'_, Message> {
        column![
            settings_input(
                "Default directory",
                "Default videos directory",
                &self.default_directory,
                Message::DefaultDirectoryChanged,
            ),
            settings_input(
                "FFmpeg path",
                "(Leave empty to use FFmpeg from PATH)",
                &self.ffmpeg_path,
                Message::FfmpegPathChanged,
            ),
        ]
        .spacing(18)
        .width(Length::Fill)
        .into()
    }
    fn about_view(&self) -> Element<'_, Message> {
        let title = rich_text::<(), Message, _, _>(vec![
            span("Crorecut").size(16).color(app_colors::TEXT),
            span(format!(" v{}", env!("CARGO_PKG_VERSION")))
                .size(14)
                .color(app_colors::TEXT_SECONDARY)
                .font(Font::MONOSPACE),
        ]);

        column![
            row![
                image::<image::Handle>(self.window_icon.clone()),
                column![
                    title,
                    button(text("View on GitHub").size(13))
                        .on_press(Message::OpenGithub)
                        .style(styles::text_link_button)
                        .padding(0)
                ]
                .spacing(4)
                .align_x(Alignment::Start),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            space().height(Length::Fill),
            row![
                text("Made with").size(13).color(app_colors::TEXT_SECONDARY),
                iced_logo(16),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
        ]
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(Alignment::Start)
        .into()
    }

    fn indicator_y(&self) -> f32 {
        INDICATOR_Y_OFFSET + self.indicator_index() * SIDEBAR_ROW_SPAN
    }
}

pub fn modal_overlay<'a, Message>(
    content: impl Into<Element<'a, Message>>,
    on_blur: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    opaque(
        mouse_area(
            iced::widget::center(opaque(content)).style(|_| container::Style {
                background: Some(Background::Color(Color {
                    a: 0.78,
                    ..app_colors::ALMOST_BLACK
                })),
                ..Default::default()
            }),
        )
        .on_press(on_blur),
    )
}

fn sidebar_button(label: &str, section: Section) -> Element<'_, Message> {
    let button: Element<'_, Message> = crorecut_button(
        None,
        label.to_string(),
        None,
        Some(Message::SelectSection(section)),
    )
    .width(Length::Fill)
    .into();

    row![
        space().width(INDICATOR_WIDTH),
        space().width(INDICATOR_BUTTON_GAP),
        button
    ]
    .width(Length::Fill)
    .align_y(Alignment::Center)
    .height(SIDEBAR_BUTTON_HEIGHT)
    .into()
}

fn sidebar_indicator() -> Element<'static, Message> {
    container("")
        .width(INDICATOR_WIDTH)
        .height(INDICATOR_HEIGHT)
        .style(|_| container::Style {
            background: Some(Background::Color(app_colors::LIGHT_GRAY)),
            ..Default::default()
        })
        .into()
}

fn settings_input<'a>(
    label: &'a str,
    placeholder: &'a str,
    value: &'a str,
    on_input: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    column![
        text(label).size(13).color(app_colors::TEXT_SECONDARY),
        text_input(placeholder, value)
            .on_input(on_input)
            .padding(8)
            .style(styles::text_input)
            .width(Length::Fill),
    ]
    .spacing(6)
    .align_x(Alignment::Start)
    .width(Length::Fill)
    .into()
}
