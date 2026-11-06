// SPDX-License-Identifier: Apache-2.0
// Copyright Kitefold Labs

// The hover card under a bar item. It is drawn by the shell because on
// Wayland only the shell knows where a panel item is.

import Clutter from 'gi://Clutter';
import GObject from 'gi://GObject';
import Pango from 'gi://Pango';
import St from 'gi://St';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';

import * as Draw from './draw.js';
import * as F from './format.js';

const GAP = 6;
const MARGIN = 8;

function label(text, styleClass, props = {}) {
    const l = new St.Label({text, style_class: styleClass, y_align: Clutter.ActorAlign.CENTER, ...props});
    l.clutter_text.ellipsize = Pango.EllipsizeMode.END;
    return l;
}

/** Vertical box params: `orientation` since GNOME 48, `vertical` before. */
export function verticalParams(params = {}) {
    return St.BoxLayout.find_property('orientation')
        ? {...params, orientation: Clutter.Orientation.VERTICAL}
        : {...params, vertical: true};
}

function area(styleClass, draw) {
    const a = new St.DrawingArea({style_class: styleClass, x_expand: true});
    a.connect('repaint', () => {
        const cr = a.get_context();
        const w = a.get_width(), h = a.get_height();
        try {
            draw(cr, w, h);
        } finally {
            cr.$dispose();
        }
    });
    return a;
}

export const Popover = GObject.registerClass(
class MastheadPopover extends St.BoxLayout {
    _init() {
        super._init(verticalParams({style_class: 'masthead-popover', reactive: false, visible: false, opacity: 0}));

        this._series = [];
        this._max = 100;
        this._extra = null;

        const head = new St.BoxLayout({style_class: 'masthead-pop-head'});
        this._name = label('', 'masthead-pop-name');
        this._dev = label('', 'masthead-pop-dev', {x_expand: true, x_align: Clutter.ActorAlign.END});
        head.add_child(this._name);
        head.add_child(this._dev);

        const big = new St.BoxLayout({style_class: 'masthead-pop-bigrow'});
        this._big = label('', 'masthead-pop-big');
        this._unit = label('', 'masthead-pop-unit', {y_align: Clutter.ActorAlign.END});
        big.add_child(this._big);
        big.add_child(this._unit);

        this._spark = area('masthead-pop-spark', (cr, w, h) =>
            Draw.sparkline(cr, w, h, this._series, this._max));
        this._extraArea = area('masthead-pop-extra', (cr, w, h) => this._extra?.(cr, w, h));

        this._rows = new St.BoxLayout(verticalParams({style_class: 'masthead-pop-rows'}));

        const foot = new St.BoxLayout({style_class: 'masthead-pop-foot'});
        this._footLeft = label('last 60 s', 'masthead-pop-foot-text', {x_expand: true});
        foot.add_child(this._footLeft);
        this._footRight = label('click to open', 'masthead-pop-foot-text');
        foot.add_child(this._footRight);

        for (const c of [head, big, this._spark, this._extraArea, this._rows, foot])
            this.add_child(c);
    }

    /** Show a plain message, used when the daemon is missing. */
    showMessage(title, lines) {
        this._name.text = title;
        this._dev.text = '';
        this._big.text = '';
        this._unit.text = '';
        this._big.visible = false;
        this._spark.visible = false;
        this._extraArea.visible = false;
        this._rows.destroy_all_children();
        for (const l of lines) {
            const t = label(l, 'masthead-pop-message');
            t.clutter_text.line_wrap = true;
            t.clutter_text.ellipsize = Pango.EllipsizeMode.NONE;
            this._rows.add_child(t);
        }
        this._footLeft.text = 'github.com/kitefold-labs/masthead';
        this._footRight.text = '';
    }

    /** Reset for a metric before its first detail arrives. */
    prepare(metric, item) {
        this._metric = metric;
        this._big.visible = true;
        this._spark.visible = true;
        this._footLeft.text = 'last 60 s';
        this._footRight.text = 'click to open';
        this._name.text = F.TITLES[metric] ?? metric;
        this._dev.text = '';
        this._big.text = item?.text ?? '';
        this._unit.text = '';
        this._series = [];
        this._extra = null;
        this._extraArea.visible = false;
        this._rows.destroy_all_children();
        this._spark.queue_repaint();
    }

    setHistory(seriesList, max) {
        this._series = seriesList;
        this._max = max ?? Draw.niceMax(seriesList);
        this._spark.queue_repaint();
    }

    _setRows(rows) {
        this._rows.destroy_all_children();
        for (const [k, v] of rows) {
            const r = new St.BoxLayout({style_class: 'masthead-pop-row'});
            r.add_child(label(k, 'masthead-pop-key', {x_expand: true}));
            r.add_child(label(v, 'masthead-pop-val'));
            this._rows.add_child(r);
        }
    }

    _setExtra(height, fn) {
        this._extra = fn;
        this._extraArea.visible = !!fn;
        if (fn) {
            this._extraArea.style = `height: ${height}px;`;
            this._extraArea.queue_repaint();
        }
    }

    /**
     * Render a Detail payload for the current metric.
     *
     * @param {object} d - parsed Detail JSON
     * @param {object} item - the bar item (id, value, text, level, summary)
     * @param {object} units - config.units
     */
    update(d, item, units) {
        const m = this._metric;
        const rows = [];
        let big = item?.text ?? '', unit = '', dev = '';
        if (m === 'cpu' && d.cpu) {
            const c = d.cpu;
            big = `${Math.round(c.total)}`;
            unit = '%';
            dev = c.model.replace(/\s+\d+-Core Processor$/, '');
            const load = d.system?.load ?? c.load;
            rows.push(['Load 1 · 5 · 15', load.map(x => x.toFixed(2)).join('  ')]);
            if (c.avg_freq_mhz > 0)
                rows.push(['Frequency', `${(c.avg_freq_mhz / 1000).toFixed(2)} GHz avg`]);
            if (c.package_temp_c !== null && c.package_temp_c !== undefined)
                rows.push(['Package', F.temp(c.package_temp_c, units)]);
            if (c.top?.length)
                rows.push(['Top', `${c.top[0].name}  ${c.top[0].cpu.toFixed(1)}%`]);
            const perCore = c.per_core ?? [];
            this._setExtra(22, perCore.length ? (cr, w, h) => Draw.bars(cr, w, h, perCore) : null);
        } else if (m === 'mem' && d.mem) {
            const x = d.mem;
            big = F.memLong(x.used, units);
            unit = `of ${F.memLong(x.total, units)}`;
            dev = `${F.memLong(x.total, units)} total`;
            rows.push(['Used', F.memLong(x.used, units)]);
            rows.push(['Cache', F.memLong(x.cache, units)]);
            rows.push(['Available', F.memLong(x.available, units)]);
            rows.push(['Swap', x.swap_total ? `${F.memLong(x.swap_used, units)} of ${F.memLong(x.swap_total, units)}` : 'none']);
            if (x.top?.length)
                rows.push(['Top', `${x.top[0].name}  ${F.memLong(x.top[0].rss, units)}`]);
            const t = x.total || 1;
            this._setExtra(8, (cr, w, h) => Draw.stack(cr, w, h, [
                [x.used / t, Draw.COLORS.accent, 1],
                [x.cache / t, Draw.COLORS.accent, 0.3],
            ]));
        } else if (m === 'gpu' && d.gpus) {
            const p = d.gpus.find(g => g.primary) ?? d.gpus[0];
            if (p) {
                dev = p.name;
                if (p.suspended) {
                    big = 'off';
                    rows.push(['State', 'suspended, not queried']);
                } else {
                    big = p.util !== null && p.util !== undefined ? `${Math.round(p.util)}` : '–';
                    unit = p.util !== null && p.util !== undefined ? '%' : '';
                    if (p.vram_total)
                        rows.push(['VRAM', `${F.memLong(p.vram_used ?? 0, units)} of ${F.memLong(p.vram_total, units)}`]);
                    if (p.temp_c !== null && p.temp_c !== undefined)
                        rows.push(['Temperature', F.temp(p.temp_c, units)]);
                    if (p.power_w !== null && p.power_w !== undefined)
                        rows.push(['Power', p.power_limit_w ? `${F.watts(p.power_w)} of ${F.watts(p.power_limit_w)}` : F.watts(p.power_w)]);
                }
            }
            for (const g of d.gpus.filter(x => x !== p)) {
                const v = g.suspended ? 'suspended' : g.util !== null && g.util !== undefined ? `${Math.round(g.util)}%` : '–';
                rows.push([g.name, v]);
            }
            this._setExtra(0, null);
        } else if (m === 'net' && d.net) {
            const n = d.net;
            big = `↓${F.rate(n.rx, units, true)}`;
            unit = `↑${F.rate(n.tx, units, true)}`;
            dev = n.interfaces.filter(i => i.counted).map(i => i.name).join(' · ');
            rows.push(['Down', F.rate(n.rx, units)]);
            rows.push(['Up', F.rate(n.tx, units)]);
            for (const i of n.interfaces.slice(0, 3)) {
                const kind = i.kind === 'wireless' ? 'Wi-Fi' : i.kind === 'ethernet' ? 'Ethernet' : i.kind;
                const idle = i.rx + i.tx < 1;
                const speed = i.speed_mbps ? ` · ${i.speed_mbps >= 1000 ? `${i.speed_mbps / 1000} Gb/s` : `${i.speed_mbps} Mb/s`}` : '';
                rows.push([i.name, idle ? `${kind} · idle` : `${kind}${speed}`]);
            }
            if (n.hidden_virtual)
                rows.push(['Virtual', `${n.hidden_virtual} hidden`]);
            this._setExtra(0, null);
        } else if (m === 'disk' && d.disk) {
            const k = d.disk;
            big = `r${F.diskRate(k.read, true)}`;
            unit = `w${F.diskRate(k.write, true)}`;
            dev = [...new Set(k.devices.map(x => x.model).filter(Boolean))].join(' · ');
            rows.push(['Read', F.diskRate(k.read)]);
            rows.push(['Write', F.diskRate(k.write)]);
            for (const x of k.devices.slice(0, 4))
                rows.push([x.name, `r ${F.diskRate(x.read, true)}  w ${F.diskRate(x.write, true)}`]);
            this._setExtra(0, null);
        } else if (m === 'temp' && d.sensors) {
            const temps = d.sensors.filter(s => s.kind === 'temp' && Number.isFinite(s.value));
            big = item ? F.temp(item.value, units, true) : '';
            dev = item?.summary ?? '';
            temps.sort((a, b) => b.value - a.value);
            for (const s of temps.slice(0, 6))
                rows.push([s.name, F.temp(s.value, units)]);
            for (const s of d.sensors.filter(x => x.kind === 'fan' && Number.isFinite(x.value)).slice(0, 2))
                rows.push([s.name, `${Math.round(s.value)} RPM`]);
            this._setExtra(0, null);
        } else if (m === 'battery' && d.batteries) {
            const b = d.batteries[0];
            if (b) {
                big = b.percent !== null && b.percent !== undefined ? `${Math.round(b.percent)}` : '–';
                unit = '%';
                dev = b.name;
                rows.push(['State', b.state]);
                if (b.watts !== null && b.watts !== undefined)
                    rows.push(['Rate', F.watts(b.watts)]);
            }
            this._setExtra(0, null);
        }
        this._big.text = big;
        this._unit.text = unit;
        this._dev.text = dev;
        this._setRows(rows);
    }

    /** Place under `actor`, centered, clamped to its monitor. */
    placeUnder(actor) {
        const [ax, ay] = actor.get_transformed_position();
        const [aw, ah] = actor.get_transformed_size();
        const monitor = Main.layoutManager.findMonitorForActor(actor) ?? Main.layoutManager.primaryMonitor;
        const [, natW] = this.get_preferred_width(-1);
        let x = Math.round(ax + aw / 2 - natW / 2);
        x = Math.max(monitor.x + MARGIN, Math.min(x, monitor.x + monitor.width - natW - MARGIN));
        this.set_position(x, Math.round(ay + ah + GAP));
    }

    open(actor) {
        // Cancel a fade-out in progress, whose completion would hide us.
        this.remove_all_transitions();
        this.show();
        this.placeUnder(actor);
        this.ease({opacity: 255, duration: 120, mode: Clutter.AnimationMode.EASE_OUT_QUAD});
    }

    hideNow() {
        this.remove_all_transitions();
        this.opacity = 0;
        this.hide();
    }

    close() {
        if (!this.visible)
            return;
        this.remove_all_transitions();
        this.ease({
            opacity: 0,
            duration: 120,
            mode: Clutter.AnimationMode.EASE_OUT_QUAD,
            onComplete: () => this.hide(),
        });
    }
});
