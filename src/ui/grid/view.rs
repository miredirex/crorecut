use iced::widget::{
    button, column, container, grid, image, mouse_area, row, scrollable, space, stack, text,
};
use iced::{Alignment, Border, Element, Length};
use std::path::Path;

use super::{Grid, GridContextMenu, Message, ScanStatus, grid_scrollable_id};
use super::{ThumbnailState, VideoItem};
use crate::ui::{ContextMenuItem, context_menu_overlay, crorecut_button};
use crate::utils::format_duration_mmss;

impl Grid {
    pub fn view(&self) -> Element<'_, Message> {
        match &self.scan_status {
            ScanStatus::NotStarted | ScanStatus::Scanning => {
                container(text("Scanning videos...").size(20))
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
                    .into()
            }

            ScanStatus::Complete => {
                let main_content: Element<'_, Message> = if self.videos.is_empty() {
                    empty_state()
                } else {
                    videos_grid(&self.videos, self.context_menu.as_ref())
                };

                column![top_section(&self.videos_directory), main_content].into()
            }

            ScanStatus::Error(error_msg) => container(
                column![
                    text("Error scanning videos").size(20),
                    text(error_msg).size(14),
                ]
                .spacing(10),
            )
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .into(),
        }
    }
}

fn video_grid_item(video: &VideoItem) -> Element<'_, Message> {
    let thumbnail_content = match &video.thumbnail_state {
        ThumbnailState::Pending => container(text("...").size(40))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.2, 0.2, 0.2,
                ))),
                ..Default::default()
            }),

        ThumbnailState::Generating => container(text("...").size(40))
            .center_x(Length::Fill)
            .center_y(Length::Fill)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| container::Style {
                background: Some(iced::Background::Color(iced::Color::from_rgb(
                    0.25, 0.25, 0.25,
                ))),
                ..Default::default()
            }),

        ThumbnailState::Ready(handle) => {
            let thumbnail_image = image(handle.clone())
                .width(Length::Fill)
                .height(Length::Fill)
                .content_fit(iced::ContentFit::Cover);

            // Duration badge overlay (bottom-right corner)
            let duration_badge: Element<'_, Message> = if let Some(duration) = video.duration {
                container(
                    container(
                        text(format_duration_mmss(duration))
                            .size(11)
                            .color(iced::Color::WHITE),
                    )
                    .padding([2, 4])
                    .style(|_theme| container::Style {
                        background: Some(iced::Background::Color(iced::Color::from_rgba(
                            0.0, 0.0, 0.0, 0.7,
                        ))),
                        border: Border {
                            radius: 2.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                )
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::End)
                .align_y(Alignment::End)
                .padding(4)
                .into()
            } else {
                space().width(Length::Fill).height(Length::Fill).into()
            };

            container(stack![thumbnail_image, duration_badge])
                .width(Length::Fill)
                .height(Length::Fill)
        }

        ThumbnailState::Failed(error) => container(
            column![text("X").size(40), text(error).size(10)]
                .align_x(Alignment::Center)
                .spacing(4),
        )
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(iced::Background::Color(iced::Color::from_rgb(
                0.3, 0.1, 0.1,
            ))),
            ..Default::default()
        }),
    };

    let filename_label = text(&video.metadata.file_name)
        .size(13)
        .color(iced::Color::WHITE);
    let dir_label = text(&video.metadata.parent_dir_name)
        .size(13)
        .color(iced::Color::from_rgb(0.6, 0.6, 0.6));

    let content = column![thumbnail_content, filename_label, dir_label]
        .spacing(4)
        .width(Length::Fill)
        .height(Length::Fill);

    let video_path = video.metadata.path.clone();
    let video_path_for_context = video_path.clone();

    // Wrap in button for click handling and hover effects
    let item_button = button(content)
        .on_press(Message::VideoSelected(video_path))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(8)
        .style(|_theme, status| {
            let base_color = iced::Color::from_rgb(0.15, 0.15, 0.15);
            let hover_color = iced::Color::from_rgb(0.25, 0.25, 0.3);
            let pressed_color = iced::Color::from_rgb(0.2, 0.2, 0.25);

            button::Style {
                background: Some(iced::Background::Color(match status {
                    button::Status::Hovered => hover_color,
                    button::Status::Pressed => pressed_color,
                    _ => base_color,
                })),
                border: Border {
                    color: match status {
                        button::Status::Hovered => iced::Color::from_rgb(0.4, 0.5, 0.7),
                        button::Status::Pressed => iced::Color::from_rgb(0.3, 0.4, 0.6),
                        _ => iced::Color::from_rgb(0.2, 0.2, 0.2),
                    },
                    width: 2.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            }
        });

    // Wrap in mouse_area for right-click context menu
    mouse_area(item_button)
        .on_right_press(Message::ShowContextMenu(video_path_for_context))
        .into()
}

fn top_section<'a>(videos_dir: &Path) -> Element<'a, Message> {
    let refresh_button: Element<'_, Message> = crorecut_button(
        Some(crate::icons::REFRESH_SVG),
        "Refresh videos".into(),
        Some(videos_dir.display().to_string()),
        Some(Message::ScanVideosDirectory),
    )
    .into();

    let settings_button: Element<'_, Message> = crorecut_button(
        Some(crate::icons::GEAR_SVG),
        "Settings".into(),
        Some("Directories, FFmpeg, …".into()),
        Some(Message::ShowSettings),
    )
    .into();

    container(
        row![refresh_button, settings_button]
            .spacing(12)
            .align_y(Alignment::Center),
    )
    .padding([12, 20])
    .center_x(Length::Fill)
    .into()
}

fn empty_state<'a>() -> Element<'a, Message> {
    container(
        column![
            text("No videos found").size(20),
            text("Place video files in your Videos folder").size(14)
        ]
        .spacing(10)
        .align_x(Alignment::Center),
    )
    .center_x(Length::Fill)
    .center_y(Length::Fill)
    .into()
}

fn videos_grid<'a>(
    videos: &'a [VideoItem],
    context_menu: Option<&'a GridContextMenu>,
) -> Element<'a, Message> {
    let video_grid = grid::Grid::with_children(videos.iter().map(video_grid_item))
        .columns(3)
        .spacing(12)
        .height(grid::Sizing::AspectRatio(1.35)); // Slightly taller than 16:9 to fit text

    let grid_content = scrollable(container(video_grid).padding(20).width(Length::Fill))
        .id(grid_scrollable_id())
        .on_scroll(Message::ScrollChanged);

    // Wrap in mouse_area for cursor tracking
    let tracked_grid = mouse_area(grid_content).on_move(Message::CursorMoved);

    let menu_overlay: Option<Element<'a, Message>> = context_menu.map(|menu| {
        let items = if menu.confirming_delete {
            context_menu_delete_confirm()
        } else {
            context_menu_default(&menu.video_path)
        };
        context_menu_overlay(menu.position, items, Message::DismissContextMenu)
    });

    // Stack to keep widget tree structure consistent
    stack![tracked_grid, menu_overlay].into()
}

// Context menu items

fn context_menu_default(video_path: &Path) -> Vec<ContextMenuItem<Message>> {
    vec![
        ContextMenuItem::new("Open", Message::ContextMenuOpen(video_path.to_path_buf())),
        ContextMenuItem::new(
            "Show in files",
            Message::ContextMenuShowInFolder(video_path.to_path_buf()),
        ),
        ContextMenuItem::new("Delete", Message::ContextMenuRequestDelete)
            .color(crate::ui::color::DANGER_RED),
    ]
}

fn context_menu_delete_confirm() -> Vec<ContextMenuItem<Message>> {
    vec![
        ContextMenuItem::label("Delete video?"),
        ContextMenuItem::new("Yes, delete", Message::ContextMenuConfirmDelete)
            .color(crate::ui::color::DANGER_RED),
        ContextMenuItem::new("Cancel", Message::ContextMenuCancelDelete),
    ]
}
