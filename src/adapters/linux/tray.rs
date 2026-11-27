//! Icono en la bandeja (StatusNotifierItem, crate `ksni`). En GNOME requiere la
//! extensión AppIndicator (Ubuntu la trae activada de serie); si no hay
//! bandeja, el agente sigue funcionando sin icono.

use std::sync::mpsc::Sender;

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::{CheckmarkItem, StandardItem, SubMenu};
use ksni::{MenuItem, ToolTip};

use crate::application::agent::Event;
use crate::domain::state;
use crate::domain::timeparse;
use crate::ports::{TrayAction, TrayModel, TrayPort};

struct SniTray {
    model: TrayModel,
    events: Sender<Event>,
}

impl SniTray {
    fn send(&self, a: TrayAction) {
        let _ = self.events.send(Event::Tray(a));
    }
}

fn item(label: impl Into<String>, action: TrayAction) -> MenuItem<SniTray> {
    StandardItem {
        label: label.into(),
        activate: Box::new(move |t: &mut SniTray| t.send(action.clone())),
        ..Default::default()
    }
    .into()
}

impl ksni::Tray for SniTray {
    fn id(&self) -> String {
        "jhs".into()
    }

    fn title(&self) -> String {
        "jhs".into()
    }

    fn icon_name(&self) -> String {
        if self.model.privacy_active { "changes-prevent" } else { "system-lock-screen" }.into()
    }

    fn tool_tip(&self) -> ToolTip {
        let mut lines = self.model.tooltip.lines();
        ToolTip {
            title: lines.next().unwrap_or("jhs").into(),
            description: lines.collect::<Vec<_>>().join("\n"),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.send(TrayAction::PrivacyOn);
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        let m = &self.model;
        let mut presets: Vec<MenuItem<Self>> =
            m.presets_min.iter().map(|&min| item(format!("En {min} min"), TrayAction::ScheduleMinutes(min))).collect();
        presets.push(MenuItem::Separator);
        presets.push(item("Personalizado…", TrayAction::ScheduleCustom));

        let privacy_label = match &m.hotkey {
            Some(h) => format!("Activar modo privacidad ({h})"),
            None => "Activar modo privacidad".into(),
        };
        let cancel: MenuItem<Self> = match m.remaining {
            Some(s) => item(format!("Cancelar apagado (faltan {})", timeparse::clock(s)), TrayAction::CancelShutdown),
            None => StandardItem { label: "Cancelar apagado".into(), enabled: false, ..Default::default() }.into(),
        };
        vec![
            item(privacy_label, TrayAction::PrivacyOn),
            MenuItem::Separator,
            SubMenu { label: "Programar apagado".into(), submenu: presets, ..Default::default() }.into(),
            cancel,
            MenuItem::Separator,
            item("Diagnóstico…", TrayAction::Doctor),
            CheckmarkItem {
                label: "Iniciar con la sesión".into(),
                checked: m.autostart,
                activate: Box::new(|t: &mut SniTray| t.send(TrayAction::ToggleAutostart)),
                ..Default::default()
            }
            .into(),
            item("Abrir configuración", TrayAction::OpenConfig),
            item("Recargar configuración", TrayAction::ReloadConfig),
            MenuItem::Separator,
            item("Salir", TrayAction::Exit),
        ]
    }
}

pub struct Tray {
    handle: Handle<SniTray>,
}

impl Tray {
    /// Publica el icono. `None` si no hay servicio de bandeja en la sesión.
    pub fn spawn(events: Sender<Event>) -> Option<Tray> {
        match (SniTray { model: TrayModel::default(), events }).spawn() {
            Ok(handle) => Some(Tray { handle }),
            Err(e) => {
                state::log(&format!("sin icono en la bandeja: {e}"));
                None
            }
        }
    }
}

impl TrayPort for Tray {
    fn update(&mut self, model: &TrayModel) {
        let model = model.clone();
        self.handle.update(move |t| t.model = model);
    }
}
