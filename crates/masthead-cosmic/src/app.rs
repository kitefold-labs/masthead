//! The COSMIC applet: bar items in the panel and the hover popover.

use crate::dbus::{self, Cmd, Event};
use crate::hover::{self, Effect, Hover};
use crate::model;
use crate::style::{self, Palette};
use cosmic::app::{Core, Task};
use cosmic::applet::cosmic_panel_config::PanelAnchor;
use cosmic::iced::font::Font;
use cosmic::iced::platform_specific::shell::wayland::commands::popup::destroy_popup;
use cosmic::iced::widget::{canvas, column, row};
use cosmic::iced::{Alignment, Border, Color, Length, Limits, Point, Rectangle, Size, Subscription, mouse, window};
use cosmic::widget::rectangle_tracker::{RectangleTracker, RectangleUpdate, rectangle_tracker_subscription};
use cosmic::widget::{self, autosize, container, icon, mouse_area, space, text};
use cosmic::{Element, Renderer, Theme};
use masthead_api::PanelItem;
use masthead_api::detail::{Describe, Detail};
use masthead_core::config::{BarStyle, Config};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;
use std::sync::LazyLock;
use tokio::sync::mpsc::UnboundedSender;

pub const APP_ID: &str = "com.kitefoldlabs.Masthead.CosmicApplet";
static AUTOSIZE_MAIN: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("masthead-main"));
static AUTOSIZE_POPUP: LazyLock<widget::Id> = LazyLock::new(|| widget::Id::new("masthead-popup"));

const POPUP_WIDTH: f32 = 304.0;
const TREND_POINTS: usize = 30;
/// Space between the panel and the popover card.
const GAP: f32 = 6.0;

#[derive(Debug, Clone)]
pub enum Message {
    Dbus(Event),
    Enter(String),
    Exit(String),
    OpenTimer(u64),
    CloseTimer(u64),
    Click(String),
    Rect(RectangleUpdate<String>),
    PopupClosed(window::Id),
    /// The pointer left the applet surface.
    PointerLeft,
    /// Surface requests from libcosmic widgets (the offline tooltip).
    Surface(cosmic::surface::Action<Message>),
}

pub struct Applet {
    core: Core,
    items: Vec<PanelItem>,
    describe: Describe,
    config: Config,
    cmd: Option<UnboundedSender<Cmd>>,
    hover: Hover,
    popup: Option<(window::Id, String)>,
    detail: Option<Detail>,
    history: Vec<Vec<f64>>,
    tracker: Option<RectangleTracker<String>>,
    rects: HashMap<String, Rectangle>,
    trends: HashMap<String, VecDeque<f64>>,
    font: Font,
    status: Option<String>,
}

fn icon_handle(id: &str) -> icon::Handle {
    let bytes: &'static [u8] = match id {
        "cpu" => include_bytes!("../icons/cpu.svg"),
        "mem" => include_bytes!("../icons/mem.svg"),
        "gpu" => include_bytes!("../icons/gpu.svg"),
        "net" => include_bytes!("../icons/net.svg"),
        "disk" => include_bytes!("../icons/disk.svg"),
        "temp" => include_bytes!("../icons/temp.svg"),
        "battery" => include_bytes!("../icons/battery.svg"),
        _ => include_bytes!("../icons/mark.svg"),
    };
    let mut h = icon::from_svg_bytes(bytes);
    h.symbolic = true;
    h
}

/// A symbolic icon tinted with `color`, or with the panel's text color.
fn tinted_icon(id: &str, size: u16, color: Option<Color>) -> widget::Icon {
    icon(icon_handle(id)).size(size).class(cosmic::theme::Svg::Custom(Rc::new(move |theme: &Theme| cosmic::iced::widget::svg::Style {
        color: Some(color.unwrap_or_else(|| theme.cosmic().background(theme.transparent).on.into())),
    })))
}

/// A line chart of up to two series; the first is filled.
struct Spark {
    series: Vec<Vec<f64>>,
    max: f64,
    palette: Palette,
}

impl<M> canvas::Program<M, Theme, Renderer> for Spark {
    type State = ();

    fn draw(&self, (): &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry<Renderer>> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);
        let grid = canvas::Path::line(Point::new(0.0, h - 0.5), Point::new(w, h - 0.5));
        frame.stroke(&grid, canvas::Stroke::default().with_color(self.palette.grid).with_width(1.0));
        for (si, s) in self.series.iter().enumerate() {
            let pts: Vec<Point> = s
                .iter()
                .enumerate()
                .filter(|(_, v)| v.is_finite())
                .map(|(i, v)| {
                    let x = if s.len() > 1 { i as f32 / (s.len() - 1) as f32 * w } else { w };
                    let y = h - 1.0 - ((*v / self.max).clamp(0.0, 1.0) as f32) * (h - 2.0);
                    Point::new(x, y)
                })
                .collect();
            if pts.len() < 2 {
                continue;
            }
            let line = canvas::Path::new(|b| {
                b.move_to(pts[0]);
                for p in &pts[1..] {
                    b.line_to(*p);
                }
            });
            if si == 0 {
                let area = canvas::Path::new(|b| {
                    b.move_to(Point::new(pts[0].x, h));
                    for p in &pts {
                        b.line_to(*p);
                    }
                    b.line_to(Point::new(pts[pts.len() - 1].x, h));
                    b.close();
                });
                frame.fill(&area, self.palette.accent_soft);
                frame.stroke(&line, canvas::Stroke::default().with_color(self.palette.accent).with_width(1.4));
                let end = pts[pts.len() - 1];
                frame.fill(&canvas::Path::circle(end, 2.5), self.palette.accent);
            } else {
                frame.stroke(&line, canvas::Stroke::default().with_color(self.palette.muted).with_width(1.0));
            }
        }
        vec![frame.into_geometry()]
    }
}

/// Per-thread load as a strip of bars.
struct Cores {
    load: Vec<f32>,
    palette: Palette,
}

impl<M> canvas::Program<M, Theme, Renderer> for Cores {
    type State = ();

    fn draw(&self, (): &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry<Renderer>> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let n = self.load.len().max(1) as f32;
        let gap = 2.0;
        let bw = ((bounds.width - gap * (n - 1.0)) / n).max(1.0);
        for (i, l) in self.load.iter().enumerate() {
            let hgt = (l / 100.0).clamp(0.08, 1.0) * bounds.height;
            let x = i as f32 * (bw + gap);
            let mut c = self.palette.accent;
            c.a = 0.35 + l.clamp(0.0, 100.0) / 160.0;
            frame.fill_rectangle(Point::new(x, bounds.height - hgt), Size::new(bw, hgt), c);
        }
        vec![frame.into_geometry()]
    }
}

/// Used and cache memory as one stacked bar.
struct Stack {
    parts: Vec<(f32, Color)>,
    track: Color,
}

impl<M> canvas::Program<M, Theme, Renderer> for Stack {
    type State = ();

    fn draw(&self, (): &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<canvas::Geometry<Renderer>> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), self.track);
        let mut x = 0.0;
        for (f, c) in &self.parts {
            let w = f.clamp(0.0, 1.0) * bounds.width;
            frame.fill_rectangle(Point::new(x, 0.0), Size::new(w, bounds.height), *c);
            x += w;
        }
        vec![frame.into_geometry()]
    }
}

impl Applet {
    fn send(&self, c: Cmd) {
        if let Some(tx) = &self.cmd {
            let _ = tx.send(c);
        }
    }

    fn horizontal(&self) -> bool {
        matches!(self.core.applet.anchor, PanelAnchor::Top | PanelAnchor::Bottom)
    }

    fn apply(&mut self, effects: Vec<Effect>) -> Task<Message> {
        let mut tasks = Vec::new();
        for e in effects {
            match e {
                Effect::OpenTimer(seq) => tasks.push(cosmic::task::future(async move {
                    tokio::time::sleep(hover::OPEN_DELAY).await;
                    Message::OpenTimer(seq)
                })),
                Effect::CloseTimer(seq) => tasks.push(cosmic::task::future(async move {
                    tokio::time::sleep(hover::CLOSE_DELAY).await;
                    Message::CloseTimer(seq)
                })),
                Effect::Open(id) => tasks.push(self.open_popup(id)),
                Effect::Close => {
                    if let Some((pid, _)) = self.popup.take() {
                        tasks.push(destroy_popup(pid));
                    }
                    self.detail = None;
                    self.history.clear();
                }
                Effect::Subscribe(m) => self.send(Cmd::Hover(m)),
                Effect::Unsubscribe(h) => self.send(Cmd::Unhover(h)),
            }
        }
        Task::batch(tasks)
    }

    fn open_popup(&mut self, id: String) -> Task<Message> {
        self.detail = None;
        self.history.clear();
        self.send(Cmd::History(id.clone()));
        cosmic::surface::surface_task(cosmic::surface::action::app_popup::<Self>(
            |_| cosmic::surface::action::LiveSettings::default(),
            move |app: &mut Self| {
                let pid = window::Id::unique();
                app.popup = Some((pid, id.clone()));
                let main = app.core.main_window_id().unwrap_or(window::Id::RESERVED);
                let mut s = app.core.applet.get_popup_settings(main, pid, None, None, None);
                if let Some(r) = app.rects.get(&id) {
                    s.positioner.anchor_rect = Rectangle {
                        x: r.x.max(0.0) as i32,
                        y: r.y.max(0.0) as i32,
                        width: r.width.max(1.0) as i32,
                        height: r.height.max(1.0) as i32,
                    };
                }
                // xdg-shell requires a popup to touch its parent, and mutter
                // dismisses one that does not; libcosmic's 4 px offset relies on
                // cosmic-comp being lenient. Anchor flush and draw the gap inside.
                s.positioner.offset = (0, 0);
                s.positioner.size = None;
                s.positioner.size_limits = Limits::NONE.min_width(1.0).min_height(1.0).max_width(POPUP_WIDTH + GAP).max_height(900.0);
                // A hover card: no input grab, and no input region, so the
                // pointer passes through and leaving the item closes it.
                s.grab = false;
                s.input_zone = Some(vec![Rectangle::new(Point::new(-1000.0, -1000.0), Size::default())]);
                s
            },
            None,
        ))
    }

    fn item_view(&self, it: &PanelItem) -> Element<'_, Message> {
        let p = style::palette();
        let color = style::level_color(&p, model::level(it));
        let (icon_size, _) = self.core.applet.suggested_size(true);
        let (pad_major, pad_minor) = self.core.applet.suggested_padding(true);
        let show_value = self.config.bar.style != BarStyle::Compact;
        let show_trend = self.config.bar.style != BarStyle::Values;
        let value = |s: String, size: Option<f32>| {
            let mut t = self.core.applet.text(s).font(self.font);
            if let Some(sz) = size {
                t = t.size(sz);
            }
            if let Some(c) = color {
                t = t.class(cosmic::theme::Text::Color(c));
            }
            t
        };
        let trend = || -> Element<'_, Message> {
            let series = vec![self.trends.get(&it.id).map(|d| d.iter().copied().collect()).unwrap_or_default()];
            let max = if matches!(it.id.as_str(), "cpu" | "gpu" | "temp" | "battery") {
                100.0
            } else if it.id == "mem" && model::mem_total(&self.describe, None) > 0 {
                model::mem_total(&self.describe, None) as f64
            } else {
                model::nice_max(&series)
            };
            canvas(Spark { series, max, palette: p }).width(Length::Fixed(28.0)).height(Length::Fixed(14.0)).into()
        };
        let content: Element<'_, Message> = if self.horizontal() {
            let mut r = row![tinted_icon(&it.id, icon_size, color)].spacing(6).align_y(Alignment::Center);
            if show_value {
                r = r.push(value(it.text.clone(), None));
            }
            if show_trend {
                r = r.push(trend());
            }
            r = r.push(space::vertical().height(Length::Fixed(f32::from(icon_size + 2 * pad_minor))));
            r.into()
        } else {
            let mut c = column![tinted_icon(&it.id, icon_size, color)].spacing(2).align_x(Alignment::Center);
            if show_value {
                c = c.push(value(model::compact_text(it, self.config.units), Some(10.0)));
            }
            c.into()
        };
        let button = widget::button::custom(content)
            .padding(if self.horizontal() { [0, pad_major / 2] } else { [pad_major / 2, 0] })
            .on_press(Message::Click(it.id.clone()))
            .class(cosmic::theme::Button::AppletIcon);
        let area = mouse_area(button).on_enter(Message::Enter(it.id.clone())).on_exit(Message::Exit(it.id.clone()));
        match &self.tracker {
            Some(t) => t.container(it.id.clone(), area).ignore_bounds(true).into(),
            None => area.into(),
        }
    }

    fn popup_view(&self, id: &str) -> Element<'_, Message> {
        let p = style::palette();
        let Some(it) = self.items.iter().find(|i| i.id == id) else {
            return text("").into();
        };
        let units = self.config.units;
        let mono = self.font;
        let muted = |s: String, size: f32| text(s).size(size).class(cosmic::theme::Text::Color(p.muted));
        let big = model::big(it, self.detail.as_ref(), units);
        let level_color = style::level_color(&p, model::level(it)).unwrap_or(p.fg);

        let header = row![
            muted(model::title(id).to_owned(), 13.0),
            space::horizontal().width(Length::Fill),
            muted(model::device(it, &self.describe, self.detail.as_ref()), 10.5).font(mono),
        ]
        .align_y(Alignment::Center)
        .spacing(8);
        let big_row =
            row![text(big.value).size(30).font(mono).class(cosmic::theme::Text::Color(level_color)), muted(big.suffix, 13.0).font(mono),]
                .spacing(6)
                .align_y(Alignment::End);

        let series = if self.history.is_empty() {
            vec![self.trends.get(id).map(|d| d.iter().copied().collect()).unwrap_or_default()]
        } else {
            self.history.clone()
        };
        let max = model::spark_max(id, &series, model::mem_total(&self.describe, self.detail.as_ref()));
        let spark = canvas(Spark { series, max, palette: p }).width(Length::Fill).height(Length::Fixed(46.0));

        let mut body = column![header, big_row, spark].spacing(8);

        if let Some(d) = &self.detail {
            if id == "cpu"
                && let Some(c) = d.cpu.as_ref().filter(|c| !c.per_core.is_empty())
            {
                body = body.push(canvas(Cores { load: c.per_core.clone(), palette: p }).width(Length::Fill).height(Length::Fixed(22.0)));
            }
            if id == "mem"
                && let Some(m) = d.mem.as_ref().filter(|m| m.total > 0)
            {
                let t = m.total as f32;
                let parts = vec![(m.used as f32 / t, p.accent), (m.cache as f32 / t, p.accent_soft)];
                body = body.push(canvas(Stack { parts, track: p.grid }).width(Length::Fill).height(Length::Fixed(8.0)));
            }
            let mut rows = column![].spacing(4);
            for (k, v) in model::rows(id, d, units) {
                rows = rows.push(
                    row![
                        muted(k, 12.5),
                        space::horizontal().width(Length::Fill),
                        text(v).size(12).font(mono).class(cosmic::theme::Text::Color(p.fg))
                    ]
                    .align_y(Alignment::Center)
                    .spacing(12),
                );
            }
            body = body.push(rows);
        } else if !it.summary.is_empty() {
            body = body.push(muted(it.summary.clone(), 12.5));
        }

        let foot = row![
            muted("last 60 s".into(), 10.5).font(mono),
            space::horizontal().width(Length::Fill),
            muted("click to open".into(), 10.5).font(mono)
        ];
        body = body.push(container(space::horizontal().width(Length::Fill).height(Length::Fixed(1.0))).class(
            cosmic::theme::Container::custom(move |_| cosmic::iced::widget::container::Style {
                background: Some(p.border.into()),
                ..Default::default()
            }),
        ));
        body = body.push(foot);

        let card = container(body).padding([14, 14]).width(Length::Fixed(POPUP_WIDTH)).class(cosmic::theme::Container::custom(move |_| {
            cosmic::iced::widget::container::Style {
                text_color: Some(p.fg),
                background: Some(p.surface.into()),
                border: Border { radius: 12.0.into(), width: 1.0, color: p.border },
                ..Default::default()
            }
        }));
        let gap = match self.core.applet.anchor {
            PanelAnchor::Top => [GAP, 0.0, 0.0, 0.0],
            PanelAnchor::Bottom => [0.0, 0.0, GAP, 0.0],
            PanelAnchor::Left => [0.0, 0.0, 0.0, GAP],
            PanelAnchor::Right => [0.0, GAP, 0.0, 0.0],
        };
        let card = container(card).padding(cosmic::iced::Padding { top: gap[0], right: gap[1], bottom: gap[2], left: gap[3] });
        autosize::autosize(card, AUTOSIZE_POPUP.clone())
            .limits(Limits::NONE.min_width(1.0).min_height(1.0).max_width(POPUP_WIDTH + GAP).max_height(900.0))
            .into()
    }
}

impl cosmic::Application for Applet {
    type Executor = cosmic::SingleThreadExecutor;
    type Flags = ();
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, (): ()) -> (Self, Task<Message>) {
        let app = Self {
            core,
            items: Vec::new(),
            describe: Describe::default(),
            config: Config::default(),
            cmd: None,
            hover: Hover::default(),
            popup: None,
            detail: None,
            history: Vec::new(),
            tracker: None,
            rects: HashMap::new(),
            trends: HashMap::new(),
            font: style::value_font(),
            status: Some("Connecting to masthead-daemon".into()),
        };
        (app, Task::none())
    }

    fn on_close_requested(&self, id: window::Id) -> Option<Message> {
        Some(Message::PopupClosed(id))
    }

    fn subscription(&self) -> Subscription<Message> {
        let left = cosmic::iced::event::listen_with(|e, _, _| match e {
            cosmic::iced::Event::Mouse(mouse::Event::CursorLeft) => Some(Message::PointerLeft),
            _ => None,
        });
        Subscription::batch([
            dbus::subscription().map(Message::Dbus),
            rectangle_tracker_subscription(0u8).map(|(_, u)| Message::Rect(u)),
            left,
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        if crate::debug_enabled() && !matches!(message, Message::Dbus(Event::Panel(_) | Event::Detail(..) | Event::History(..))) {
            eprintln!("masthead-cosmic: {message:?}");
        }
        match message {
            Message::Dbus(ev) => match ev {
                Event::Ready(tx) => {
                    self.cmd = Some(tx);
                    // A popover opened before the connection came up needs its tier.
                    if let Some(m) = self.hover.open().map(ToOwned::to_owned) {
                        self.send(Cmd::Hover(m));
                    }
                }
                Event::Connected { describe, config } => {
                    self.describe = *describe;
                    self.config = *config;
                    self.status = None;
                }
                Event::Panel(items) => {
                    for it in &items {
                        model::push_trend(self.trends.entry(it.id.clone()).or_default(), it.value, TREND_POINTS);
                    }
                    self.items = items;
                    let refresh = self.popup.as_ref().map(|(_, m)| m.clone());
                    if let Some(m) = refresh
                        && self.detail.is_none()
                    {
                        self.send(Cmd::History(m));
                    }
                }
                Event::Detail(m, d) => {
                    if self.popup.as_ref().is_some_and(|(_, open)| *open == m) {
                        self.detail = Some(*d);
                        // The daemon samples once a second; fetch the line to match.
                        self.send(Cmd::History(m));
                    }
                }
                Event::History(m, s) => {
                    if self.popup.as_ref().is_some_and(|(_, open)| *open == m) {
                        self.history = s;
                    }
                }
                Event::Subscribed(m, h) => {
                    let e = self.hover.subscribed(&m, h);
                    return self.apply(e);
                }
                Event::Config(c) => self.config = *c,
                Event::Restarted => {
                    let e = self.hover.daemon_restarted();
                    return self.apply(e);
                }
                Event::Disconnected(e) => {
                    self.cmd = None;
                    self.items.clear();
                    self.status = Some(format!("masthead-daemon is not reachable: {e}"));
                }
            },
            Message::Enter(id) => {
                let e = self.hover.enter(&id);
                return self.apply(e);
            }
            Message::Exit(id) => {
                let e = self.hover.exit(&id);
                return self.apply(e);
            }
            Message::OpenTimer(seq) => {
                let e = self.hover.open_timer(seq);
                return self.apply(e);
            }
            Message::CloseTimer(seq) => {
                let e = self.hover.close_timer(seq);
                return self.apply(e);
            }
            Message::Click(id) => {
                self.send(Cmd::ToggleWindow(id));
                let e = self.hover.click();
                return self.apply(e);
            }
            Message::Rect(u) => match u {
                RectangleUpdate::Rectangle((id, r)) => {
                    self.rects.insert(id, r);
                }
                RectangleUpdate::Init(t) => self.tracker = Some(t),
            },
            Message::PointerLeft => {
                let e = self.hover.left_surface();
                return self.apply(e);
            }
            Message::Surface(a) => return cosmic::task::message(cosmic::Action::Surface(a)),
            Message::PopupClosed(id) => {
                if self.popup.as_ref().is_some_and(|(p, _)| *p == id) {
                    self.popup = None;
                    let e = self.hover.closed_externally();
                    return self.apply(e);
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let content: Element<'_, Message> = if self.items.is_empty() {
            let tip = self.status.clone().unwrap_or_else(|| "Masthead".into());
            let (s, _) = self.core.applet.suggested_size(true);
            let btn = self.core.applet.button_from_element(tinted_icon("mark", s, None), true).on_press(Message::Click("cpu".into()));
            Element::from(self.core.applet.applet_tooltip::<Message>(btn, tip, false, Message::Surface, None))
        } else if self.horizontal() {
            let mut r = cosmic::widget::row::with_capacity(self.items.len()).align_y(Alignment::Center);
            for it in &self.items {
                r = r.push(self.item_view(it));
            }
            r.into()
        } else {
            let mut c = cosmic::widget::column::with_capacity(self.items.len()).align_x(Alignment::Center).spacing(4);
            for it in &self.items {
                c = c.push(self.item_view(it));
            }
            c.into()
        };
        autosize::autosize(content, AUTOSIZE_MAIN.clone()).into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Message> {
        match &self.popup {
            Some((pid, metric)) if *pid == id => self.popup_view(metric),
            _ => text("").into(),
        }
    }

    fn style(&self) -> Option<cosmic::iced::theme::Style> {
        Some(cosmic::applet::style())
    }
}
