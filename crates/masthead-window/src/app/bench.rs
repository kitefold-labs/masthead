// Copyright 2026 Kitefold Labs
// SPDX-License-Identifier: Apache-2.0

//! Frame-time benchmark. Builds each page headlessly with synthetic data at
//! the window's default size and times the four steps a frame is made of:
//! building the widget tree with layout, the per-frame event pass, drawing
//! into the renderer's layers, and rasterizing every pixel in software.
//! Scrolling pages also get a scroll frame, and every page a 1 Hz sample.
//!
//! ```sh
//! cargo test --release -p masthead-window -- --ignored --nocapture bench
//! ```
//!
//! With `MASTHEAD_BENCH_PNG=<dir>`, the first frame of every page at 1× is
//! written there as a PNG.

use super::*;
use crate::bus::Event as Bus;
use crate::disk::Tab;
use crate::model::{Page, Range, Series};
use cosmic::iced::advanced::graphics::Viewport;
use cosmic::iced::advanced::{clipboard, renderer};
use cosmic::iced::runtime::user_interface::{Cache as UiCache, UserInterface};
use cosmic::iced::{Event, Pixels, Point, Rectangle, Size, mouse, window};
use masthead_api::detail::*;
use masthead_disk::findings::{Action, Group, GroupId, Home, Item};
use masthead_disk::mounts::Space;
use masthead_disk::{Kind, Node, Scan};
use std::path::PathBuf;
use std::sync::Once;
use std::time::Instant;

/// Window sizes to bench: the default, the default on a 2× display, and a
/// window maximized on 1080p and 4K displays at 1×. Heights leave out the
/// header bar.
const SIZES: [(f32, f32, f32); 4] = [(1040.0, 674.0, 1.0), (1040.0, 674.0, 2.0), (1920.0, 1034.0, 1.0), (3840.0, 2114.0, 1.0)];
const FRAMES: usize = 12;

fn fonts() {
    static ONCE: Once = Once::new();
    ONCE.call_once(crate::load_fonts);
}

fn procs(n: usize, t: u64) -> Vec<ProcDetail> {
    const NAMES: [&str; 12] = [
        "firefox",
        "rustc",
        "gnome-shell",
        "code",
        "cargo",
        "Xwayland",
        "pipewire",
        "systemd",
        "containerd",
        "node",
        "masthead-daemon",
        "slack",
    ];
    (0..n)
        .map(|i| ProcDetail {
            pid: 1000 + i as u32 * 7,
            name: format!("{}-{i}", NAMES[i % NAMES.len()]),
            user: if i % 5 == 0 { "root".into() } else { "nate".into() },
            cpu: ((i * 37 + t as usize * 11) % 1000) as f64 / 10.0,
            rss: (i as u64 * 7_340_032 + t * 65_536) % (8 << 30),
            threads: 4,
        })
        .collect()
}

fn describe() -> Describe {
    Describe {
        version: "0.1.0".into(),
        hostname: "bench".into(),
        kernel: "7.0.0".into(),
        cpu_model: "AMD Ryzen 9 9950X3D 16-Core Processor".into(),
        cores: 16,
        threads: 32,
        mem_total: 64 << 30,
        gpus: vec![GpuInfo { pci: "0000:01:00.0".into(), name: "NVIDIA GeForce RTX 5090".into(), vendor: "nvidia".into(), discrete: true }],
        sensors: (0..20)
            .map(|i| SensorInfo { key: format!("k10temp/temp{i}"), name: format!("Sensor {i}"), kind: "temp".into() })
            .collect(),
        interfaces: vec![],
        disks: (0..3).map(|i| DiskInfo { name: format!("nvme{i}n1"), model: "Samsung SSD 990 PRO 2TB".into(), size: 2 << 40 }).collect(),
        has_battery: true,
        metrics: MetricId::ALL.iter().map(|m| m.as_str().to_owned()).collect(),
    }
}

fn detail(t: u64) -> Detail {
    let wave = |k: f64| (t as f64 / k).sin() * 0.5 + 0.5;
    Detail {
        t,
        system: Some(SystemDetail { uptime: 86_400.0 * 3.0 + t as f64, load: [1.2, 1.5, 1.9], procs_total: 612, procs_running: 3 }),
        cpu: Some(CpuDetail {
            model: "AMD Ryzen 9 9950X3D".into(),
            cores: 16,
            threads: 32,
            total: 12.0 + 40.0 * wave(7.0),
            per_core: (0..32).map(|i| ((i * 13 + t as usize * 5) % 100) as f32).collect(),
            freq_mhz: vec![4200.0; 32],
            avg_freq_mhz: 4200.0 + 500.0 * wave(3.0),
            package_temp_c: Some(55.0 + 10.0 * wave(5.0)),
            load: [1.2, 1.5, 1.9],
            top: procs(8, t),
        }),
        mem: Some(MemDetail {
            total: 64 << 30,
            used: (18u64 << 30) + ((t % 7) << 28),
            cache: 20 << 30,
            available: 46 << 30,
            free: 26 << 30,
            swap_total: 8 << 30,
            swap_used: 1 << 20,
            top: procs(8, t),
        }),
        gpus: Some(vec![GpuDetail {
            pci: "0000:01:00.0".into(),
            name: "NVIDIA GeForce RTX 5090".into(),
            vendor: "nvidia".into(),
            driver: "nvidia 580.95".into(),
            discrete: true,
            primary: true,
            util: Some(30.0 * wave(4.0)),
            vram_used: Some(3 << 30),
            vram_total: Some(32 << 30),
            temp_c: Some(41.0),
            power_w: Some(60.0 + 100.0 * wave(9.0)),
            power_limit_w: Some(575.0),
            suspended: false,
        }]),
        net: Some(NetDetail {
            rx: 2_000_000.0 * wave(2.0),
            tx: 300_000.0 * wave(6.0),
            interfaces: (0..3)
                .map(|i| IfaceDetail {
                    name: ["enp5s0", "wlp4s0", "tailscale0"][i].into(),
                    kind: ["ethernet", "wireless", "virtual"][i].into(),
                    driver: "r8169".into(),
                    speed_mbps: Some(2500),
                    rx: 1_000_000.0 * wave(2.0 + i as f64),
                    tx: 100_000.0,
                    rx_total: 50 << 30,
                    tx_total: 5 << 30,
                    counted: i < 2,
                })
                .collect(),
            hidden_virtual: 4,
        }),
        disk: Some(DiskDetail {
            read: 50_000_000.0 * wave(3.0),
            write: 20_000_000.0 * wave(8.0),
            devices: (0..3)
                .map(|i| DiskDevDetail {
                    name: format!("nvme{i}n1"),
                    model: "Samsung SSD 990 PRO 2TB".into(),
                    size: 2 << 40,
                    read: 10_000_000.0 * wave(3.0 + f64::from(i)),
                    write: 5_000_000.0,
                })
                .collect(),
            filesystems: ["/", "/home", "/boot/efi", "/mnt/data"]
                .iter()
                .enumerate()
                .map(|(i, m)| FsDetail {
                    mount: (*m).into(),
                    device: format!("/dev/nvme{i}n1p2"),
                    fstype: "ext4".into(),
                    total: 2 << 40,
                    used: (i as u64 + 1) * (400 << 30),
                })
                .collect(),
        }),
        sensors: Some(
            (0..20)
                .map(|i| SensorDetail {
                    key: format!("k10temp/temp{i}"),
                    name: format!("Sensor {i}"),
                    kind: ["temp", "temp", "fan", "voltage", "power"][i % 5].into(),
                    value: Some(40.0 + i as f64 + 5.0 * wave(2.0)),
                    unit: "°C".into(),
                })
                .collect(),
        ),
        batteries: Some(vec![BatteryDetail { name: "BAT0".into(), percent: Some(76.0), watts: Some(12.5), state: "discharging".into() }]),
        processes: Some(procs(300, t)),
    }
}

fn dir(name: String, size: u64, children: Vec<Node>) -> Node {
    let mut n = Node::new(name, Kind::Dir);
    n.children = children;
    n.sum_children(size);
    n
}

/// A tree like a real scan: a root, two mounts, and folders of folders.
fn scan() -> Scan {
    let leaf = |name: String, size: u64| -> Node {
        let kids = (0..24).map(|j| Node::file(format!("{name}-{j}.bin"), size / (j as u64 + 2), 1_700_000_000)).collect();
        dir(name, 0, kids)
    };
    let mount = |path: &str, scale: u64| -> Node {
        let mut m = Node::new(path, Kind::Mount);
        m.children = (0..80)
            .map(|i| {
                let size = scale * 1000 / (i as u64 + 1);
                let kids = (0..24).map(|j| leaf(format!("sub{i}_{j}"), size / (j as u64 + 1))).collect();
                dir(format!("folder{i}"), 0, kids)
            })
            .collect();
        m.children.push(Node {
            name: "Not readable without root".into(),
            kind: Kind::Unscanned,
            size: scale * 200,
            files: 0,
            newest: 0,
            children: vec![],
        });
        m.sum_children(0);
        m
    };
    let mut root = Node::new("This computer", Kind::Computer);
    root.children = vec![mount("/home", 1 << 30), mount("/", 1 << 29)];
    root.sum_children(0);
    Scan { tree: root, caches: vec![], downloads: vec![], elapsed: std::time::Duration::from_secs(4), cancelled: false }
}

fn groups(home: &Home) -> Vec<Group> {
    let items = |n: usize, g: GroupId| -> Vec<Item> {
        (0..n)
            .map(|i| {
                let path = home.home.join(format!("work/project-{i}/target"));
                Item {
                    title: format!("project-{i}/target"),
                    detail: format!("Cargo build output · changed {} days ago", i * 3),
                    size: (1u64 << 30) / (i as u64 + 1),
                    stale: i % 3 == 0,
                    action: match g {
                        GroupId::Downloads => Action::MoveToTrash(path),
                        GroupId::Trash => Action::EmptyTrash,
                        _ => Action::DeleteCache(path),
                    },
                }
            })
            .collect()
    };
    vec![
        Group {
            id: GroupId::BuildOutputs,
            title: "Build outputs",
            about: "Cargo target folders and node_modules.".into(),
            items: items(30, GroupId::BuildOutputs),
        },
        Group {
            id: GroupId::AppCaches,
            title: "Application caches",
            about: "Caches in ~/.cache.".into(),
            items: items(20, GroupId::AppCaches),
        },
        Group {
            id: GroupId::Downloads,
            title: "Old downloads",
            about: "Large downloads unchanged for a month.".into(),
            items: items(10, GroupId::Downloads),
        },
        Group { id: GroupId::Trash, title: "Trash", about: "Files in the Trash.".into(), items: items(1, GroupId::Trash) },
    ]
}

fn app(page: Page) -> App {
    fonts();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let (mut app, _) =
        App::init(cosmic::Core::default(), Flags { page: Page::Metric(MetricId::Cpu), dark: true, theme: Theme::System, requests: tx });
    let _ = app.update(Message::Bus(Bus::Describe(Box::new(describe()))));
    let _ = app.update(Message::Bus(Bus::Config(Box::default())));
    for t in 0..60 {
        let _ = app.update(Message::Bus(Bus::Detail(Box::new(detail(t)))));
    }
    let _ = app.update(Message::Select(page));
    let _ = app.update(Message::Range(Range::M15));
    if let Page::Metric(m) = page {
        for (index, _) in model::series_for(m).iter().enumerate() {
            let pts: Vec<f64> = (0..900).map(|i| 30.0 + 25.0 * (f64::from(i) / 17.0).sin() + f64::from(i % 7)).collect();
            let series = Series::from_history(1.0, pts.clone(), pts);
            let _ = app.update(Message::Bus(Bus::History { generation: app.generation, index, series }));
        }
    }
    if page == Page::DiskUsage {
        let home = Home::from_env().expect("HOME");
        app.disk.groups = groups(&home);
        app.disk.cache_paths = app
            .disk
            .groups
            .iter()
            .flat_map(|g| &g.items)
            .filter_map(|i| match &i.action {
                Action::DeleteCache(p) => Some(p.clone()),
                _ => None,
            })
            .collect();
        app.disk.home = Some(home);
        app.disk.scan = Some(scan());
        app.disk.path = vec![0];
        app.disk.scanned_at = masthead_disk::time::now();
        app.disk.space = vec![
            (PathBuf::from("/home"), Space { total: 2 << 40, used: 900 << 30, free: 1100 << 30 }),
            (PathBuf::from("/"), Space { total: 1 << 40, used: 300 << 30, free: 700 << 30 }),
        ];
    }
    app
}

struct Frame {
    renderer: cosmic::Renderer,
    pixmap: tiny_skia::Pixmap,
    mask: tiny_skia::Mask,
    viewport: Viewport,
    theme: cosmic::Theme,
    clipboard: clipboard::Null,
    messages: Vec<Message>,
}

impl Frame {
    fn new((width, height, scale): (f32, f32, f32), pal: &Palette) -> Self {
        let (w, h) = ((width * scale) as u32, (height * scale) as u32);
        Self {
            renderer: cosmic::Renderer::new(views::SANS, Pixels(14.0)),
            pixmap: tiny_skia::Pixmap::new(w, h).unwrap(),
            mask: tiny_skia::Mask::new(w, h).unwrap(),
            viewport: Viewport::with_physical_size(Size::new(w, h), f64::from(scale)),
            theme: palette::cosmic_theme(pal),
            clipboard: clipboard::Null,
            messages: Vec::new(),
        }
    }

    fn size(&self) -> Size {
        self.viewport.logical_size()
    }

    fn update(&mut self, ui: &mut UserInterface<'_, Message, cosmic::Theme, cosmic::Renderer>, events: &[Event], cursor: mouse::Cursor) {
        let _ = ui.update(events, cursor, &mut self.renderer, &mut self.clipboard, &mut self.messages);
    }

    fn redraw(&mut self, ui: &mut UserInterface<'_, Message, cosmic::Theme, cosmic::Renderer>, cursor: mouse::Cursor) {
        self.update(ui, &[Event::Window(window::Event::RedrawRequested(Instant::now()))], cursor);
    }

    fn draw(&mut self, ui: &mut UserInterface<'_, Message, cosmic::Theme, cosmic::Renderer>, cursor: mouse::Cursor) {
        let style = renderer::Style {
            icon_color: cosmic::iced::Color::WHITE,
            text_color: cosmic::iced::Color::WHITE,
            scale_factor: self.viewport.scale_factor(),
        };
        ui.draw(&mut self.renderer, &self.theme, &style, cursor);
    }

    /// Rasterizes the whole window, as the real renderer does when every
    /// pixel is damaged (the first frame, and a scroll of the main area is
    /// close to it).
    fn raster(&mut self) {
        let full = Rectangle::with_size(self.size());
        self.renderer.draw(&mut self.pixmap.as_mut(), &mut self.mask, &self.viewport, &[full], cosmic::iced::Color::BLACK);
    }

    /// The frame as a PNG. The renderer writes the window's BGRA pixel order
    /// into the pixmap, so red and blue are swapped back first.
    fn save_png(&self, path: &std::path::Path) {
        let mut rgba = self.pixmap.clone();
        for p in rgba.pixels_mut() {
            *p = tiny_skia::PremultipliedColorU8::from_rgba(p.blue(), p.green(), p.red(), 255).unwrap_or(*p);
        }
        if let Err(e) = rgba.save_png(path) {
            eprintln!("could not write {}: {e}", path.display());
        }
    }

    fn counts(&mut self) -> String {
        let layers = self.renderer.layers();
        let quads: usize = layers.iter().map(|l| l.quads.len()).sum();
        let text: usize = layers.iter().map(|l| l.text.iter().map(|t| t.as_slice().len()).sum::<usize>()).sum();
        let prims: usize = layers.iter().map(|l| l.primitives.iter().map(|p| p.as_slice().len()).sum::<usize>()).sum();
        let images: usize = layers.iter().map(|l| l.images.len()).sum();
        format!("{} layers, {quads} quads, {text} texts, {prims} paths, {images} images", layers.len())
    }
}

fn ms(d: std::time::Duration) -> String {
    format!("{:6.2}", d.as_secs_f64() * 1000.0)
}

fn median(mut v: Vec<std::time::Duration>) -> std::time::Duration {
    v.sort();
    v[v.len() / 2]
}

/// Runs one page and prints a row of timings.
fn bench(name: &str, page: Page, tab: Option<Tab>, size: (f32, f32, f32)) {
    let mut app = app(page);
    if let Some(t) = tab {
        app.disk.tab = t;
    }
    let mut f = Frame::new(size, &app.pal);
    let size = f.size();
    let cursor = mouse::Cursor::Available(Point::new(620.0, 420.0));

    // Build: the widget tree plus layout, as after every message.
    let t0 = Instant::now();
    let mut ui = UserInterface::build(app.view(), size, UiCache::default(), &mut f.renderer);
    let build = t0.elapsed();
    // Warm the text and glyph caches, as a window that has drawn once has.
    f.redraw(&mut ui, cursor);
    f.draw(&mut ui, cursor);
    f.raster();
    if let Ok(dir) = std::env::var("MASTHEAD_BENCH_PNG")
        && (f.viewport.scale_factor() - 1.0).abs() < f64::EPSILON
        && size.width < 1100.0
    {
        f.save_png(&std::path::Path::new(&dir).join(format!("{}.png", name.replace(" · ", "-").to_lowercase())));
    }

    let t0 = Instant::now();
    f.redraw(&mut ui, cursor);
    let redraw = t0.elapsed();
    let t0 = Instant::now();
    f.draw(&mut ui, cursor);
    let draw = t0.elapsed();
    let t0 = Instant::now();
    f.raster();
    let raster = t0.elapsed();
    let counts = f.counts();

    // A wheel step with the pointer over the main area, then the frame it causes.
    let mut scroll = Vec::new();
    for i in 0..FRAMES {
        let dy = if i % 2 == 0 { -40.0 } else { 40.0 };
        let t0 = Instant::now();
        f.update(&mut ui, &[Event::Mouse(mouse::Event::WheelScrolled { delta: mouse::ScrollDelta::Pixels { x: 0.0, y: dy } })], cursor);
        f.redraw(&mut ui, cursor);
        f.draw(&mut ui, cursor);
        f.raster();
        scroll.push(t0.elapsed());
    }

    // A 1 Hz sample: update, rebuild, lay out, draw, rasterize.
    let mut tick = Vec::new();
    let mut cache = ui.into_cache();
    for t in 60..60 + FRAMES as u64 {
        let t0 = Instant::now();
        let _ = app.update(Message::Bus(Bus::Detail(Box::new(detail(t)))));
        let mut ui = UserInterface::build(app.view(), size, cache, &mut f.renderer);
        f.redraw(&mut ui, cursor);
        f.draw(&mut ui, cursor);
        f.raster();
        tick.push(t0.elapsed());
        cache = ui.into_cache();
    }

    println!(
        "{name:<22} {} {} {} {} | scroll {} tick {} | {counts}",
        ms(build),
        ms(redraw),
        ms(draw),
        ms(raster),
        ms(median(scroll)),
        ms(median(tick)),
    );
}

#[test]
#[ignore = "benchmark; run with --ignored --nocapture"]
fn bench_pages() {
    for size in SIZES {
        println!("\n{}×{} at scale {}: ms per step", size.0, size.1, size.2);
        println!("{:<22} {:>6} {:>6} {:>6} {:>6}", "page", "build", "events", "draw", "raster");
        bench("CPU", Page::Metric(MetricId::Cpu), None, size);
        bench("Memory", Page::Metric(MetricId::Mem), None, size);
        bench("GPU", Page::Metric(MetricId::Gpu), None, size);
        bench("Network", Page::Metric(MetricId::Net), None, size);
        bench("Disk", Page::Metric(MetricId::Disk), None, size);
        bench("Temperature", Page::Metric(MetricId::Temp), None, size);
        bench("Battery", Page::Metric(MetricId::Battery), None, size);
        bench("Processes", Page::Processes, None, size);
        bench("Settings", Page::Settings, None, size);
        bench("Disk usage · map", Page::DiskUsage, Some(Tab::Map), size);
        bench("Disk usage · clean up", Page::DiskUsage, Some(Tab::Cleanup), size);
    }
}

/// libcosmic calls `malloc_trim(0)` after every `view`. Its cost grows with
/// the heap, so this times it with a scan tree in memory.
#[test]
#[ignore = "benchmark; run with --ignored --nocapture"]
#[allow(unsafe_code)] // glibc's malloc_trim, as libcosmic calls it
fn bench_malloc_trim() {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> i32;
    }
    let tree = scan();
    let procs: Vec<Vec<ProcDetail>> = (0..10).map(|t| procs(300, t)).collect();
    let mut times = Vec::new();
    for _ in 0..FRAMES {
        let t0 = Instant::now();
        // SAFETY: malloc_trim takes an integer and touches only the allocator's own state.
        unsafe { malloc_trim(0) };
        times.push(t0.elapsed());
    }
    println!(
        "malloc_trim(0) with {} tree nodes and {} processes in memory: {} ms",
        tree.tree.count(),
        procs.len() * 300,
        ms(median(times))
    );
}

/// The renderer's fixed per-frame costs: clearing a damage region to the
/// background, and rebuilding the window-sized clip mask, which it does once
/// per layer and twice per canvas drawn.
#[test]
#[ignore = "benchmark; run with --ignored --nocapture"]
#[allow(clippy::too_many_lines)] // one block per primitive
fn bench_mask() {
    for (width, height, scale) in SIZES {
        let (w, h) = ((width * scale) as u32, (height * scale) as u32);
        let mut pixmap = tiny_skia::Pixmap::new(w, h).unwrap();
        let mut mask = tiny_skia::Mask::new(w, h).unwrap();
        let rect = tiny_skia::Rect::from_xywh(0.0, 0.0, w as f32, h as f32).unwrap();
        let path = tiny_skia::PathBuilder::from_rect(rect);
        let mut fills = Vec::new();
        let mut masks = Vec::new();
        for _ in 0..FRAMES {
            let t0 = Instant::now();
            pixmap.fill_path(
                &path,
                &tiny_skia::Paint { blend_mode: tiny_skia::BlendMode::Source, anti_alias: false, ..tiny_skia::Paint::default() },
                tiny_skia::FillRule::Winding,
                tiny_skia::Transform::identity(),
                None,
            );
            fills.push(t0.elapsed());
            let t0 = Instant::now();
            mask.clear();
            mask.fill_path(&path, tiny_skia::FillRule::EvenOdd, false, tiny_skia::Transform::identity());
            masks.push(t0.elapsed());
        }
        // A full-window quad with anti-aliasing, as a container background is drawn.
        let mut quads = Vec::new();
        let rounded = tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_xywh(212.0 * scale, 0.0, (width - 212.0) * scale, height * scale).unwrap(),
        );
        for _ in 0..FRAMES {
            let t0 = Instant::now();
            pixmap.fill_path(
                &rounded,
                &tiny_skia::Paint { anti_alias: true, ..tiny_skia::Paint::default() },
                tiny_skia::FillRule::EvenOdd,
                tiny_skia::Transform::identity(),
                None,
            );
            quads.push(t0.elapsed());
        }
        // The same quad through a clip mask, as inside a scrollable.
        let mut masked = Vec::new();
        for _ in 0..FRAMES {
            let t0 = Instant::now();
            pixmap.fill_path(
                &rounded,
                &tiny_skia::Paint { anti_alias: true, ..tiny_skia::Paint::default() },
                tiny_skia::FillRule::EvenOdd,
                tiny_skia::Transform::identity(),
                Some(&mask),
            );
            masked.push(t0.elapsed());
        }
        // 1000 glyph blits, 8×12 px each.
        let glyph = tiny_skia::Pixmap::new((8.0 * scale) as u32, (12.0 * scale) as u32).unwrap();
        let mut glyphs = Vec::new();
        for _ in 0..FRAMES {
            let t0 = Instant::now();
            for i in 0..1000 {
                pixmap.draw_pixmap(
                    (i % 100) * 9,
                    (i / 100) * 14,
                    glyph.as_ref(),
                    &tiny_skia::PixmapPaint::default(),
                    tiny_skia::Transform::identity(),
                    None,
                );
            }
            glyphs.push(t0.elapsed());
        }
        // A 900-point chart line and its gradient area, as the 15 m chart draws.
        let mut pb = tiny_skia::PathBuilder::new();
        for i in 0..900 {
            let x = i as f32 * 0.8 * scale;
            let y = (100.0 + 60.0 * (i as f32 / 17.0).sin()) * scale;
            if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
        }
        let line = pb.finish().unwrap();
        let mut strokes = Vec::new();
        for _ in 0..FRAMES {
            let t0 = Instant::now();
            pixmap.stroke_path(
                &line,
                &tiny_skia::Paint { anti_alias: true, ..tiny_skia::Paint::default() },
                &tiny_skia::Stroke { width: 1.6 * scale, ..tiny_skia::Stroke::default() },
                tiny_skia::Transform::identity(),
                None,
            );
            strokes.push(t0.elapsed());
        }
        // Blitting a bitmap the width of the window and 340 px tall, as the disk map is drawn.
        let map = tiny_skia::Pixmap::new(((width - 234.0) * scale) as u32, (340.0 * scale) as u32).unwrap();
        let mut blits = Vec::new();
        for _ in 0..FRAMES {
            let t0 = Instant::now();
            pixmap.draw_pixmap(
                (234.0 * scale) as i32,
                (100.0 * scale) as i32,
                map.as_ref(),
                &tiny_skia::PixmapPaint { quality: tiny_skia::FilterQuality::Nearest, ..tiny_skia::PixmapPaint::default() },
                tiny_skia::Transform::identity(),
                None,
            );
            blits.push(t0.elapsed());
        }
        println!("{w}×{h}: {}×{} bitmap blit {} ms", map.width(), map.height(), ms(median(blits)));
        println!(
            "{w}×{h}: background {} ms · clip mask {} ms · main-area quad {} ms, masked {} ms · 1000 glyphs {} ms · 900-pt stroke {} ms",
            ms(median(fills)),
            ms(median(masks)),
            ms(median(quads)),
            ms(median(masked)),
            ms(median(glyphs)),
            ms(median(strokes))
        );
    }
}
