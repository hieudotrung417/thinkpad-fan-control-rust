use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    path::Path,
    rc::Rc,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};

use glib::ControlFlow;
use gtk::{gdk, prelude::*};
use libappindicator::{AppIndicator, AppIndicatorStatus};

use fanspeed::{
    mode::FanMode,
    sensors::{FanState, SensorReader},
    session::PrivilegedSession,
    status::{format_number, tray_status},
    APP_ID, HELPER_PATH,
};

pub fn run(arguments: &[String]) -> i32 {
    let application = gtk::Application::new(Some(APP_ID), Default::default());
    let state = AppState::new(application.clone());
    let startup_state = Rc::clone(&state);
    application.connect_startup(move |_| startup_state.startup());
    let activate_state = Rc::clone(&state);
    application.connect_activate(move |_| activate_state.activate());
    let shutdown_state = Rc::clone(&state);
    application.connect_shutdown(move |_| {
        if let Ok(mut session) = shutdown_state.session.try_lock() {
            session.close();
        }
    });
    application.run_with_args(arguments).value()
}

struct AppState {
    application: gtk::Application,
    reader: SensorReader,
    session: Arc<Mutex<PrivilegedSession>>,
    window: RefCell<Option<WindowWidgets>>,
    tray: RefCell<Option<TrayWidgets>>,
    hold: RefCell<Option<gio::ApplicationHoldGuard>>,
    busy: Cell<bool>,
    updating_tray: Cell<bool>,
}

struct WindowWidgets {
    window: gtk::ApplicationWindow,
    mode_label: gtk::Label,
    rpm_label: gtk::Label,
    temperature_label: gtk::Label,
    message_label: gtk::Label,
    auto_button: gtk::Button,
    level_dropdown: gtk::ComboBoxText,
    apply_button: gtk::Button,
    full_button: gtk::Button,
}

struct TrayWidgets {
    _menu: gtk::Menu,
    indicator: AppIndicator,
    status_item: gtk::MenuItem,
    mode_items: HashMap<String, gtk::RadioMenuItem>,
    quit_item: gtk::MenuItem,
}

impl AppState {
    fn new(application: gtk::Application) -> Rc<Self> {
        Rc::new(Self {
            application,
            reader: SensorReader::default(),
            session: Arc::new(Mutex::new(PrivilegedSession::default())),
            window: RefCell::new(None),
            tray: RefCell::new(None),
            hold: RefCell::new(None),
            busy: Cell::new(false),
            updating_tray: Cell::new(false),
        })
    }

    fn startup(self: &Rc<Self>) {
        *self.hold.borrow_mut() = Some(self.application.hold());
        self.install_css();
        self.create_tray();
    }

    fn install_css(&self) {
        let provider = gtk::CssProvider::new();
        let _ = provider.load_from_data(
            b".card { background: alpha(@theme_fg_color, 0.06); border-radius: 12px; padding: 18px; }
              .status-title { font-size: 20px; font-weight: 700; }
              .section-title { font-size: 16px; font-weight: 700; }
              .warning { color: #c47f00; }",
        );
        if let Some(screen) = gdk::Screen::default() {
            gtk::StyleContext::add_provider_for_screen(
                &screen,
                &provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
    }

    fn activate(self: &Rc<Self>) {
        if self.window.borrow().is_none() {
            self.create_window();
        }
        if let Some(widgets) = self.window.borrow().as_ref() {
            widgets.window.show_all();
            widgets.window.present();
        }
    }

    fn create_window(self: &Rc<Self>) {
        let window = gtk::ApplicationWindow::new(&self.application);
        window.set_title("ThinkPad Fan Control");
        window.set_default_size(520, 480);
        window.set_resizable(false);

        let header = gtk::HeaderBar::new();
        header.set_show_close_button(true);
        let title = gtk::Label::new(Some("ThinkPad Fan Control"));
        title.style_context().add_class("title");
        header.set_custom_title(Some(&title));
        window.set_titlebar(Some(&header));

        let root = gtk::Box::new(gtk::Orientation::Vertical, 18);
        root.set_margin_top(24);
        root.set_margin_bottom(24);
        root.set_margin_start(24);
        root.set_margin_end(24);
        window.add(&root);

        let status_card = gtk::Box::new(gtk::Orientation::Vertical, 12);
        status_card.style_context().add_class("card");
        root.pack_start(&status_card, false, false, 0);
        let mode_label = gtk::Label::new(Some("Reading fan status…"));
        mode_label.set_xalign(0.0);
        mode_label.style_context().add_class("status-title");
        status_card.pack_start(&mode_label, false, false, 0);
        let rpm_label = gtk::Label::new(Some("Fan speed: —"));
        rpm_label.set_xalign(0.0);
        status_card.pack_start(&rpm_label, false, false, 0);
        let temperature_label = gtk::Label::new(Some("Temperature: —"));
        temperature_label.set_xalign(0.0);
        status_card.pack_start(&temperature_label, false, false, 0);

        let controls = gtk::Box::new(gtk::Orientation::Vertical, 12);
        controls.style_context().add_class("card");
        root.pack_start(&controls, false, false, 0);
        let heading = gtk::Label::new(Some("Cooling mode"));
        heading.set_xalign(0.0);
        heading.style_context().add_class("section-title");
        controls.pack_start(&heading, false, false, 0);

        let auto_button = gtk::Button::with_label("Automatic (recommended)");
        auto_button.style_context().add_class("suggested-action");
        controls.pack_start(&auto_button, false, false, 0);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        controls.pack_start(&row, false, false, 0);
        let level_dropdown = gtk::ComboBoxText::new();
        for level in 1..=7 {
            level_dropdown.append_text(&format!("Manual level {level}"));
        }
        level_dropdown.set_active(Some(6));
        level_dropdown.set_hexpand(true);
        row.pack_start(&level_dropdown, true, true, 0);
        let apply_button = gtk::Button::with_label("Apply");
        row.pack_start(&apply_button, false, false, 0);
        let full_button = gtk::Button::with_label("Full speed (maximum cooling)");
        controls.pack_start(&full_button, false, false, 0);

        let message_label = gtk::Label::new(None);
        message_label.set_xalign(0.0);
        message_label.set_line_wrap(true);
        message_label.style_context().add_class("dim-label");
        root.pack_start(&message_label, false, false, 0);
        let warning = gtk::Label::new(Some(
            "Manual control remains active until you select Automatic or reboot. The app never offers fan-off mode.",
        ));
        warning.set_xalign(0.0);
        warning.set_line_wrap(true);
        warning.style_context().add_class("warning");
        root.pack_start(&warning, false, false, 0);

        let hide_window = window.clone();
        let hidden_message = message_label.clone();
        window.connect_delete_event(move |_, _| {
            hide_window.hide();
            hidden_message.set_text("Still running in the status bar.");
            glib::Propagation::Stop
        });

        let auto_state = Rc::clone(self);
        auto_button.connect_clicked(move |_| auto_state.apply_mode(FanMode::Auto));
        let apply_state = Rc::clone(self);
        let dropdown = level_dropdown.clone();
        apply_button.connect_clicked(move |_| {
            if let Some(index) = dropdown.active() {
                if let Some(mode) = FanMode::ALL.get(index as usize + 1).copied() {
                    apply_state.apply_mode(mode);
                }
            }
        });
        let full_state = Rc::clone(self);
        full_button.connect_clicked(move |_| full_state.apply_mode(FanMode::FullSpeed));

        *self.window.borrow_mut() = Some(WindowWidgets {
            window,
            mode_label,
            rpm_label,
            temperature_label,
            message_label,
            auto_button,
            level_dropdown,
            apply_button,
            full_button,
        });

        self.refresh_status();
        let refresh_state = Rc::clone(self);
        glib::timeout_add_local(Duration::from_secs(2), move || {
            refresh_state.refresh_status();
            ControlFlow::Continue
        });
        if std::env::var_os("APPIMAGE").is_some() || Path::new(HELPER_PATH).exists() {
            self.begin_authorization();
        }
    }

    fn create_tray(self: &Rc<Self>) {
        let mut menu = gtk::Menu::new();
        let status_item = gtk::MenuItem::with_label("Reading fan status…");
        status_item.set_sensitive(false);
        menu.append(&status_item);
        menu.append(&gtk::SeparatorMenuItem::new());

        let mut mode_items = HashMap::new();
        let mut first_item: Option<gtk::RadioMenuItem> = None;
        self.updating_tray.set(true);
        for mode in FanMode::ALL {
            let item = if let Some(first) = &first_item {
                gtk::RadioMenuItem::with_label_from_widget(first, Some(mode.menu_label()))
            } else {
                let item = gtk::RadioMenuItem::builder()
                    .label(mode.menu_label())
                    .build();
                first_item = Some(item.clone());
                item
            };
            let state = Rc::clone(self);
            item.connect_toggled(move |item| {
                if item.is_active() && !state.updating_tray.get() {
                    state.apply_mode(mode);
                }
            });
            menu.append(&item);
            mode_items.insert(mode.kernel_value().to_owned(), item);
        }
        self.updating_tray.set(false);

        menu.append(&gtk::SeparatorMenuItem::new());
        let show_item = gtk::MenuItem::with_label("Open Fan Control");
        let show_state = Rc::clone(self);
        show_item.connect_activate(move |_| show_state.activate());
        menu.append(&show_item);
        let quit_item = gtk::MenuItem::with_label("Quit");
        let quit_state = Rc::clone(self);
        quit_item.connect_activate(move |_| quit_state.quit());
        menu.append(&quit_item);
        menu.show_all();

        let mut indicator = if let Some(appdir) = std::env::var_os("APPDIR") {
            let path = Path::new(&appdir).join("usr/share/icons/hicolor/scalable/apps");
            AppIndicator::with_path(APP_ID, APP_ID, &path.to_string_lossy())
        } else {
            AppIndicator::new(APP_ID, APP_ID)
        };
        indicator.set_status(AppIndicatorStatus::Active);
        indicator.set_title("ThinkPad Fan Control");
        indicator.set_menu(&mut menu);
        *self.tray.borrow_mut() = Some(TrayWidgets {
            _menu: menu,
            indicator,
            status_item,
            mode_items,
            quit_item,
        });
    }

    fn begin_authorization(self: &Rc<Self>) {
        if self.busy.replace(true) {
            return;
        }
        self.set_sensitive(false);
        self.set_message("Authorize once to unlock fan control for this session…");
        self.run_session_task(None);
    }

    fn apply_mode(self: &Rc<Self>, mode: FanMode) {
        if self.busy.get() {
            return;
        }
        if std::env::var_os("APPIMAGE").is_none() && !Path::new(HELPER_PATH).exists() {
            self.show_error(
                "The privileged helper is not installed.",
                "Run sudo ./install.sh from the project directory, then reopen the app.",
            );
            return;
        }
        self.busy.set(true);
        self.set_sensitive(false);
        let ready = self
            .session
            .try_lock()
            .is_ok_and(|mut session| session.ready());
        self.set_message(if ready {
            "Changing fan mode…"
        } else {
            "Waiting for authorization…"
        });
        self.run_session_task(Some(mode));
    }

    fn run_session_task(self: &Rc<Self>, mode: Option<FanMode>) {
        let (sender, receiver) = mpsc::channel();
        let session = Arc::clone(&self.session);
        thread::spawn(move || {
            let result = session
                .lock()
                .map_err(|_| "The privileged helper session failed.".to_owned())
                .and_then(|mut session| match mode {
                    Some(mode) => session.set_mode(mode).map_err(|error| error.to_string()),
                    None => session
                        .open()
                        .map(|()| "Fan control unlocked.".to_owned())
                        .map_err(|error| error.to_string()),
                });
            let _ = sender.send(result);
        });
        let state = Rc::clone(self);
        glib::timeout_add_local(Duration::from_millis(50), move || {
            match receiver.try_recv() {
                Ok(result) => {
                    state.finish_task(mode.is_some(), result);
                    ControlFlow::Break
                }
                Err(mpsc::TryRecvError::Empty) => ControlFlow::Continue,
                Err(mpsc::TryRecvError::Disconnected) => {
                    state.finish_task(
                        mode.is_some(),
                        Err("The privileged helper session ended.".to_owned()),
                    );
                    ControlFlow::Break
                }
            }
        });
    }

    fn finish_task(&self, was_mode_change: bool, result: Result<String, String>) {
        self.busy.set(false);
        self.set_sensitive(true);
        match result {
            Ok(message) => self.set_message(&message),
            Err(error) => {
                if was_mode_change {
                    self.set_message("");
                    self.show_error("Could not change fan mode", &error);
                } else {
                    self.set_message("Fan control is locked. Select a mode to retry.");
                }
            }
        }
        self.refresh_status();
    }

    fn refresh_status(&self) {
        let state = self.reader.read();
        if let Some(widgets) = self.window.borrow().as_ref() {
            widgets.mode_label.set_text(&state.mode.label());
            if state.rpms.is_empty() {
                widgets.rpm_label.set_text("Fan speed: unavailable");
            } else {
                widgets.rpm_label.set_text(&format!(
                    "Fans: {}",
                    state
                        .rpms
                        .iter()
                        .map(|rpm| format_number(*rpm))
                        .collect::<Vec<_>>()
                        .join(" / ")
                ));
            }
            let mut temperatures = Vec::new();
            if let Some(value) = state.cpu_temp {
                temperatures.push(format!("CPU {value:.1}°C"));
            }
            if let Some(value) = state.gpu_temp {
                temperatures.push(format!("GPU {value:.1}°C"));
            }
            widgets.temperature_label.set_text(&format!(
                "Temperatures: {}",
                if temperatures.is_empty() {
                    "unavailable".to_owned()
                } else {
                    temperatures.join("  •  ")
                }
            ));
        }
        self.update_tray(&state);
    }

    fn update_tray(&self, state: &FanState) {
        let status = tray_status(state);
        let mut tray = self.tray.borrow_mut();
        let Some(tray) = tray.as_mut() else { return };
        tray.status_item.set_label(&status.details);
        tray.indicator.set_label(&status.short_mode, "MAX");
        tray.indicator
            .set_title(&format!("ThinkPad Fan Control — {}", status.details));
        if let Some(selected) = status.selected_mode {
            if let Some(item) = tray.mode_items.get(&selected) {
                self.updating_tray.set(true);
                item.set_active(true);
                self.updating_tray.set(false);
            }
        }
    }

    fn set_sensitive(&self, sensitive: bool) {
        if let Some(widgets) = self.window.borrow().as_ref() {
            widgets.auto_button.set_sensitive(sensitive);
            widgets.level_dropdown.set_sensitive(sensitive);
            widgets.apply_button.set_sensitive(sensitive);
            widgets.full_button.set_sensitive(sensitive);
        }
        if let Some(tray) = self.tray.borrow().as_ref() {
            for item in tray.mode_items.values() {
                item.set_sensitive(sensitive);
            }
            tray.quit_item.set_sensitive(sensitive);
            if !sensitive {
                tray.status_item.set_label("Changing fan mode…");
            }
        }
    }

    fn set_message(&self, message: &str) {
        if let Some(widgets) = self.window.borrow().as_ref() {
            widgets.message_label.set_text(message);
        }
    }

    fn show_error(&self, title: &str, detail: &str) {
        let parent = self.window.borrow();
        let Some(parent) = parent.as_ref() else {
            return;
        };
        let dialog = gtk::MessageDialog::new(
            Some(&parent.window),
            gtk::DialogFlags::MODAL,
            gtk::MessageType::Error,
            gtk::ButtonsType::Close,
            title,
        );
        dialog.set_secondary_text(Some(detail));
        dialog.connect_response(|dialog, _| dialog.close());
        dialog.show_all();
    }

    fn quit(&self) {
        self.application.quit();
    }
}
