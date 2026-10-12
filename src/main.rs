// SPDX-License-Identifier: GPL-3.0-only

mod config;
mod fill;
mod indicator;
mod providers;
mod settings;
mod usage;
mod view;
mod watch;

use cosmic::iced::platform_specific::shell::wayland::commands::popup::destroy_popup;
use cosmic::iced::window::Id;
use cosmic::cosmic_config::CosmicConfigEntry;
use cosmic::iced::{time, Subscription};
use cosmic::prelude::*;
use std::path::PathBuf;
use std::time::Duration;

use config::{Config, ResetDisplay, Scope, Style};
use indicator::{indicator_state, IndicatorState};
use providers::Provider;
use usage::UsageSample;

/// Which content the popup surface is showing.
#[derive(Clone, Copy, PartialEq)]
enum PopupKind {
    Info,
    Settings,
}

fn main() -> cosmic::iced::Result {
    // Start the applet's event loop with `()` as the application's flags.
    cosmic::applet::run::<Window>(())
}

/// Current unix time in seconds (saturating to 0 on a pre-epoch clock).
fn unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A detected provider and its most recent usage sample.
struct ProviderState {
    provider: Provider,
    sample: Option<UsageSample>,
}

/// Detect providers and read each one's latest sample.
fn load_providers(config: &Config) -> Vec<ProviderState> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let claude_override = config.history_path.as_deref().map(PathBuf::from);
    providers::detect(&home, claude_override.as_deref())
        .into_iter()
        .map(|provider| {
            let sample = usage::read_latest(&provider.path);
            ProviderState { provider, sample }
        })
        .collect()
}

/// The applet model. The COSMIC runtime manages the `core`.
struct Window {
    core: cosmic::Core,
    config: Config,
    providers: Vec<ProviderState>,
    /// Id of the provider tab in focus; falls back to the first provider.
    selected: Option<String>,
    now: i64,
    /// The open details popup, if any (set while the popup window is shown).
    popup: Option<Id>,
    /// Which content the open popup renders (info details vs. settings).
    popup_kind: PopupKind,
}

/// Messages emitted by the applet.
#[derive(Debug, Clone)]
pub enum Message {
    /// Re-read the usage history file (wired to a watcher in Task 6).
    Reload,
    /// Toggle the details popup (implemented in Task 8).
    TogglePopup,
    /// The popup window was closed by the compositor (e.g. click-away).
    PopupClosed(Id),
    /// Toggle the settings popup (right-click, or the ⚙ button).
    ToggleSettings,
    /// Switch the open popup back to the info view.
    ShowInfo,
    /// Select a provider tab by id.
    SelectProvider(String),
    /// Settings mutations (each persists config immediately).
    SetScope(Scope),
    SetStyle(Style),
    SetShowPercent(bool),
    SetPercentInsideRing(bool),
    SetResetDisplay(ResetDisplay),
    SetAmber(f32),
    SetRed(f32),
    SetStaleAfterMins(u64),
    SetHistoryPath(String),
}

impl cosmic::Application for Window {
    /// The async executor used to run the application's commands.
    type Executor = cosmic::executor::Default;

    /// Data the application receives in its `init` method.
    type Flags = ();

    /// Messages the application and its widgets will emit.
    type Message = Message;

    /// Unique identifier in RDNN (reverse domain name notation) format.
    const APP_ID: &'static str = "co.osterberg.AiUsage";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, Task<cosmic::Action<Self::Message>>) {
        // Load persisted config from cosmic-config (defaults if absent).
        let config = Config::load();
        let providers = load_providers(&config);
        (
            Window {
                core,
                config,
                providers,
                selected: None,
                now: unix_now(),
                popup: None,
                popup_kind: PopupKind::Info,
            },
            Task::none(),
        )
    }

    fn update(&mut self, message: Self::Message) -> Task<cosmic::Action<Self::Message>> {
        match message {
            Message::Reload => {
                self.refresh();
            }
            Message::SelectProvider(id) => {
                self.selected = Some(id);
            }
            Message::TogglePopup => {
                // Refresh data/clock when opening so the popup is current.
                self.refresh();
                return if let Some(id) = self.popup.take() {
                    destroy_popup(id)
                } else {
                    Self::open_popup(PopupKind::Info)
                };
            }
            Message::ToggleSettings => {
                return match self.popup {
                    // Settings already open: close the popup.
                    Some(id) if self.popup_kind == PopupKind::Settings => {
                        self.popup = None;
                        destroy_popup(id)
                    }
                    // Info popup open: switch the same surface to settings.
                    Some(_) => {
                        self.popup_kind = PopupKind::Settings;
                        Task::none()
                    }
                    // Closed: open a fresh popup directly in settings mode.
                    None => Self::open_popup(PopupKind::Settings),
                };
            }
            Message::ShowInfo => {
                self.popup_kind = PopupKind::Info;
                return Task::none();
            }
            Message::SetScope(scope) => {
                self.config.scope = scope;
                self.save_config();
                return Task::none();
            }
            Message::SetStyle(style) => {
                self.config.style = style;
                // Drop a reset mode the new style can't render (e.g. Dual ring on
                // a bar), so the stored value stays consistent with the UI.
                if !settings::reset_valid(style, self.config.reset_display) {
                    self.config.reset_display = ResetDisplay::None;
                }
                self.save_config();
                return Task::none();
            }
            Message::SetShowPercent(v) => {
                self.config.show_percent = v;
                self.save_config();
                return Task::none();
            }
            Message::SetPercentInsideRing(v) => {
                self.config.percent_inside_ring = v;
                self.save_config();
                return Task::none();
            }
            Message::SetResetDisplay(v) => {
                self.config.reset_display = v;
                self.save_config();
                return Task::none();
            }
            Message::SetAmber(v) => {
                self.config.thresholds.amber = v.clamp(0.0, 1.0);
                self.save_config();
                return Task::none();
            }
            Message::SetRed(v) => {
                self.config.thresholds.red = v.clamp(0.0, 1.0);
                self.save_config();
                return Task::none();
            }
            Message::SetStaleAfterMins(m) => {
                self.config.stale_after = m * 60;
                self.save_config();
                return Task::none();
            }
            Message::SetHistoryPath(s) => {
                let trimmed = s.trim();
                self.config.history_path = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                };
                self.refresh();
                self.save_config();
                return Task::none();
            }
            Message::PopupClosed(id) => {
                if self.popup == Some(id) {
                    self.popup = None;
                    self.popup_kind = PopupKind::Info;
                }
            }
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        // inotify watch on the usage file: emits Reload (near-instant) on change.
        // The path doubles as the subscription's identity (run_with hashes it).
        let files = self.providers.iter().map(|ps| {
            Subscription::run_with(ps.provider.path.clone(), |p: &PathBuf| {
                watch::file_stream(p.clone())
            })
        });
        // 30s tick: re-reads the file and refreshes `now`, so staleness and
        // countdowns advance even when the file itself is idle.
        let tick = time::every(Duration::from_secs(30)).map(|_| Message::Reload);
        Subscription::batch(files.chain(std::iter::once(tick)))
    }

    /// The applet's button in the panel renders the real indicator. Hovering
    /// shows a tooltip; clicking toggles the details popup.
    fn view(&self) -> Element<'_, Self::Message> {
        let sample = self.current().and_then(|ps| ps.sample.as_ref());
        let state: IndicatorState = indicator_state(sample, self.now, &self.config);
        // Reset context (elapsed/remaining) for the soonest budget; indicator_view
        // renders it per the reset_display mode (text / compact / glow / ring arcs).
        let reset = sample
            .map(|s| view::reset_info(s, self.now, self.config.scope));
        let content = view::indicator_view(&state, &self.config, reset);

        // Content-sized button — NOT button_from_element, which forces a fixed
        // symbolic-icon size that clips wider content (fill bars, two
        // percentages). on_press_down so cosmic-panel's hover-synthesized
        // press-down opens the popup; right-click opens settings.
        let button = cosmic::widget::button::custom(content)
            .on_press_down(Message::TogglePopup)
            .class(cosmic::theme::Button::AppletIcon);
        let target = cosmic::widget::mouse_area(button).on_right_press(Message::ToggleSettings);

        // autosize_window lets the panel surface grow to fit the content width.
        self.core.applet.autosize_window(target).into()
    }

    /// The popup window: info details or the settings panel.
    fn view_window(&self, _id: Id) -> Element<'_, Self::Message> {
        match self.popup_kind {
            PopupKind::Settings => view::popup_surface(view::settings_view(&self.config)),
            PopupKind::Info => {
                let body: Element<'_, Message> = match self.current().and_then(|ps| ps.sample.as_ref()) {
                    Some(s) => view::popup_view(s, self.now, &self.config),
                    None => cosmic::widget::text("No usage data yet").size(14).into(),
                };
                let mut col = cosmic::widget::Column::new().spacing(8);
                if self.providers.len() > 1 {
                    col = col.push(self.tab_bar());
                }
                view::popup_surface(col.push(body))
            }
        }
    }

    /// Notified when the compositor closes the popup (e.g. click-away).
    fn on_close_requested(&self, id: Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}

impl Window {
    /// Open a popup surface the way the stock applets do (`surface_task` with
    /// `app_popup`). The popup content comes from `view_window`.
    fn open_popup(kind: PopupKind) -> Task<cosmic::Action<Message>> {
        cosmic::surface::surface_task(cosmic::surface::action::app_popup(
            |_| Default::default(),
            move |app: &mut Self| {
                let new_id = Id::unique();
                app.popup = Some(new_id);
                app.popup_kind = kind;
                app.core.applet.get_popup_settings(
                    app.core.main_window_id().unwrap(),
                    new_id,
                    None,
                    None,
                    None,
                )
            },
            None,
        ))
    }

    /// Re-detect providers and re-read their samples, plus the clock.
    fn refresh(&mut self) {
        self.providers = load_providers(&self.config);
        self.now = unix_now();
    }

    /// The provider in focus, or the first detected one.
    fn current(&self) -> Option<&ProviderState> {
        self.selected
            .as_ref()
            .and_then(|id| self.providers.iter().find(|ps| &ps.provider.id == id))
            .or_else(|| self.providers.first())
    }

    /// One button per detected provider; the one in focus is highlighted.
    fn tab_bar(&self) -> Element<'_, Message> {
        let current = self.current().map(|ps| ps.provider.id.clone());
        let mut row = cosmic::widget::Row::new().spacing(6).padding([8, 8, 0, 8]);
        for ps in &self.providers {
            let active = current.as_deref() == Some(ps.provider.id.as_str());
            let class = if active {
                cosmic::theme::Button::Suggested
            } else {
                cosmic::theme::Button::Standard
            };
            row = row.push(
                cosmic::widget::button::text(ps.provider.name.clone())
                    .class(class)
                    .on_press(Message::SelectProvider(ps.provider.id.clone())),
            );
        }
        row.into()
    }

    /// Persist the current config back to cosmic-config (best-effort; logs on
    /// failure). Mirrors the stock-applet pattern: open a `Config` handler for
    /// the app id + schema version, then `write_entry`.
    fn save_config(&self) {
        if let Ok(h) = cosmic::cosmic_config::Config::new(config::CONFIG_ID, config::CONFIG_VERSION) {
            if let Err(e) = self.config.write_entry(&h) {
                eprintln!("config write failed: {e:?}");
            }
        }
    }
}
