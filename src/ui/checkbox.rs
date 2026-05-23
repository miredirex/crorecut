use iced::widget::{container, mouse_area, row, space, stack, text};
use iced::{Alignment, Background, Border, Element, Length, mouse};

use super::color as app_colors;

const CHECKBOX_SIZE: f32 = 24.0;
const CHECKBOX_INNER_SIZE: f32 = 12.0;

fn outer_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(app_colors::BUTTON_BG)),
        border: Border {
            color: app_colors::BUTTON_BORDER,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

fn inner_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(app_colors::CONTROL_ACCENT)),
        ..Default::default()
    }
}

fn checkbox_box<'a, Message>(is_checked: bool) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    let outer = container(space::Space::new())
        .width(Length::Fixed(CHECKBOX_SIZE))
        .height(Length::Fixed(CHECKBOX_SIZE))
        .style(outer_style);

    if !is_checked {
        return outer.into();
    }

    let inner = container(space::Space::new())
        .width(Length::Fixed(CHECKBOX_INNER_SIZE))
        .height(Length::Fixed(CHECKBOX_INNER_SIZE))
        .style(inner_style);

    stack![
        outer,
        container(inner)
            .width(Length::Fixed(CHECKBOX_SIZE))
            .height(Length::Fixed(CHECKBOX_SIZE))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
    ]
    .into()
}

pub fn crorecut_checkbox<'a, Message>(
    is_checked: bool,
    label: impl Into<String>,
    on_toggle: Message,
) -> Element<'a, Message>
where
    Message: Clone + 'a,
{
    mouse_area(
        row![
            checkbox_box(is_checked),
            text(label.into()).color(app_colors::TEXT)
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .on_press(on_toggle)
    .interaction(mouse::Interaction::Pointer)
    .into()
}
