//! Styled ripple button with decorative corner dots and an optional icon

use iced::widget::{button, column, container, row, stack, svg, text};
use iced::{Alignment, Color, Element, Length};

use super::color as app_colors;
use super::{corner_dots, ripple};

fn button_style(_theme: &iced::Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Disabled => app_colors::BUTTON_BG_DISABLED,
        _ => app_colors::BUTTON_BG,
    };

    button::Style {
        background: Some(iced::Background::Color(background)),
        text_color: Color::WHITE,
        border: iced::Border {
            color: app_colors::BUTTON_BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

pub struct CrorecutButton<Message> {
    icon: Option<&'static [u8]>,
    label: String,
    sublabel: Option<String>,
    on_press: Option<Message>,
    width: Length,
}

impl<Message> CrorecutButton<Message> {
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
}

pub fn crorecut_button<Message>(
    icon: Option<&'static [u8]>,
    label: String,
    sublabel: Option<String>,
    on_press: Option<Message>,
) -> CrorecutButton<Message> {
    CrorecutButton {
        icon,
        label,
        sublabel,
        on_press,
        width: Length::Shrink,
    }
}

impl<'a, Message> From<CrorecutButton<Message>> for Element<'a, Message>
where
    Message: Clone + 'a,
{
    fn from(button_config: CrorecutButton<Message>) -> Self {
        let label_content: Element<'_, Message> = match button_config.sublabel {
            Some(sub) => column![
                text(button_config.label).color(app_colors::TEXT),
                text(sub).size(13).color(app_colors::TEXT_SECONDARY)
            ]
            .into(),
            None => text(button_config.label).color(app_colors::TEXT).into(),
        };

        let content: Element<'_, Message> = match button_config.icon {
            Some(icon_data) => {
                let icon_svg = svg(svg::Handle::from_memory(icon_data))
                    .width(28)
                    .height(28);
                row![icon_svg, label_content]
                    .spacing(12)
                    .align_y(Alignment::Center)
                    .into()
            }
            None => label_content,
        };

        let width = button_config.width;
        let enabled = button_config.on_press.is_some();

        let mut btn = button(stack![
            container(content)
                .center_x(width)
                .center_y(Length::Shrink)
                .padding([12, 16]), // [vertical, horizontal]
            corner_dots(),
        ])
        .width(width)
        .style(button_style)
        .padding(2);

        if let Some(msg) = button_config.on_press {
            btn = btn.on_press(msg);
        }

        ripple(btn)
            .ripple_color(app_colors::RIPPLE_ON_GRAY)
            .enabled(enabled)
            .into()
    }
}
