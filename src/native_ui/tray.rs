use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem},
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
};
use std::sync::mpsc::{self, Receiver, Sender};

const SHOW_ID: &str = "stormget-show";
const EXIT_ID: &str = "stormget-exit";

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
        let show = MenuItem::with_id(SHOW_ID, "فتح StormGet (Open StormGet)", true, None);
        let exit = MenuItem::with_id(EXIT_ID, "خروج نهائي (Exit StormGet)", true, None);
        menu.append_items(&[&show, &exit])
            .map_err(|error| error.to_string())?;

        let (rgba, w, h) = super::stormget_app_icon_rgba();
        let icon = Icon::from_rgba(rgba, w, h).map_err(|error| error.to_string())?;
        let (action_tx, actions) = mpsc::channel();
        install_event_handlers(action_tx, ctx);
        let tray_icon = TrayIconBuilder::new()
            .with_tooltip("StormGet Download Manager")
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
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
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

    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
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
