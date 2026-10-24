use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

const SHOW_ID: &str = "rustidm-show";
const EXIT_ID: &str = "rustidm-exit";

pub enum TrayAction {
    Show,
    Exit,
}

pub struct TrayController {
    _icon: TrayIcon,
}

impl TrayController {
    pub fn new() -> Result<Self, String> {
        let menu = Menu::new();
        let show = MenuItem::with_id(SHOW_ID, "Open RustIDM", true, None);
        let exit = MenuItem::with_id(EXIT_ID, "Exit", true, None);
        menu.append_items(&[&show, &exit])
            .map_err(|error| error.to_string())?;

        let icon = Icon::from_rgba(download_icon_rgba(), 32, 32)
            .map_err(|error| error.to_string())?;
        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("RustIDM")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self { _icon: tray_icon })
    }

    pub fn poll_actions(&self) -> Vec<TrayAction> {
        let mut actions = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            if event.id() == SHOW_ID {
                actions.push(TrayAction::Show);
            } else if event.id() == EXIT_ID {
                actions.push(TrayAction::Exit);
            }
        }

        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
                | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } => actions.push(TrayAction::Show),
                _ => {}
            }
        }
        actions
    }
}

fn download_icon_rgba() -> Vec<u8> {
    let mut pixels = vec![0; 32 * 32 * 4];
    for y in 4..28 {
        for x in 4..28 {
            let in_arrow = (13..19).contains(&x) && (5..20).contains(&y)
                || (9..23).contains(&x) && (17..21).contains(&y)
                || (7..25).contains(&x) && (21..25).contains(&y)
                || (5..27).contains(&x) && (26..28).contains(&y);
            if in_arrow {
                let offset = (y * 32 + x) * 4;
                pixels[offset..offset + 4].copy_from_slice(&[55, 190, 125, 255]);
            }
        }
    }
    pixels
}
