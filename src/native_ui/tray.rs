use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use std::sync::mpsc::{self, Receiver, Sender};

const SHOW_ID: &str = "rustidm-show";
const EXIT_ID: &str = "rustidm-exit";

pub enum TrayAction {
    Show,
    Exit,
}

pub struct TrayController {
    _icon: TrayIcon,
    actions: Receiver<TrayAction>,
}

impl TrayController {
    pub fn new(ctx: eframe::egui::Context) -> Result<Self, String> {
        let menu = Menu::new();
        let show = MenuItem::with_id(SHOW_ID, "Open RustIDM", true, None);
        let exit = MenuItem::with_id(EXIT_ID, "Exit", true, None);
        menu.append_items(&[&show, &exit])
            .map_err(|error| error.to_string())?;

        let icon = Icon::from_rgba(download_icon_rgba(), 32, 32)
            .map_err(|error| error.to_string())?;
        let (action_tx, actions) = mpsc::channel();
        install_event_handlers(action_tx, ctx);
        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("RustIDM")
            .with_icon(icon)
            .with_menu(Box::new(menu))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            _icon: tray_icon,
            actions,
        })
    }

    pub fn poll_actions(&self) -> Vec<TrayAction> {
        self.actions.try_iter().collect()
    }
}

fn install_event_handlers(action_tx: Sender<TrayAction>, ctx: eframe::egui::Context) {
    let menu_action_tx = action_tx.clone();
    let menu_ctx = ctx.clone();
    MenuEvent::set_event_handler(Some(move |event| {
        let action = if event.id() == SHOW_ID {
            Some(TrayAction::Show)
        } else if event.id() == EXIT_ID {
            Some(TrayAction::Exit)
        } else {
            None
        };
        if let Some(action) = action {
            let _ = menu_action_tx.send(action);
            menu_ctx.request_repaint();
        }
    }));

    TrayIconEvent::set_event_handler(Some(move |event| {
        if matches!(
            event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
        ) {
            let _ = action_tx.send(TrayAction::Show);
            ctx.request_repaint();
        }
    }));
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
