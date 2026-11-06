// SPDX-License-Identifier: Apache-2.0
// Copyright Kitefold Labs

// Masthead for GNOME Shell. Presentation only: masthead-daemon samples the
// system and publishes bar items over D-Bus; this extension draws them, shows
// a popover on hover and asks the daemon to open the window on click.
//
// It holds no sampling timers. The only timeouts are the hover open/close
// delays and the reconnect backoff in client.js.

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import St from 'gi://St';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';

import * as Client from './client.js';
import * as Draw from './draw.js';
import * as F from './format.js';
import {Popover} from './popover.js';

const OPEN_DELAY_MS = 300;
const CLOSE_DELAY_MS = 150;
const SPARK_SECONDS = 60;
const ROLE_PREFIX = 'masthead-';

/** Label width class per metric, so a changing value never shifts the bar. */
const WIDTH = {cpu: 'w4', gpu: 'w4', temp: 'w4', battery: 'w4', mem: 'w5', net: 'w11', disk: 'w11'};

/** One metric in the top bar: icon, value and optional trend line. */
const MetricItem = GObject.registerClass({
    Signals: {
        'hover-changed': {param_types: [GObject.TYPE_BOOLEAN]},
        'activate-metric': {},
        'context-menu': {},
    },
}, class MastheadMetricItem extends PanelMenu.Button {
    _init(id, iconsDir) {
        super._init(0.5, `Masthead ${F.TITLES[id] ?? id}`, true);
        this.metric = id;
        this.item = null;
        this.add_style_class_name('masthead-item');

        const box = new St.BoxLayout({style_class: 'masthead-item-box'});
        this._icon = new St.Icon({
            gicon: Gio.icon_new_for_string(iconsDir.get_child(`masthead-${id === 'missing' ? 'mark' : id}-symbolic.svg`).get_path()),
            style_class: 'masthead-icon',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._label = new St.Label({
            text: '',
            style_class: `masthead-value masthead-${WIDTH[id] ?? 'w4'}`,
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._values = [];
        this._max = 100;
        this._spark = new St.DrawingArea({style_class: 'masthead-spark', y_align: Clutter.ActorAlign.CENTER, visible: false});
        this._spark.connect('repaint', a => {
            const cr = a.get_context();
            try {
                Draw.sparkline(cr, a.get_width(), a.get_height(), this._values, this._max,
                    Draw.levelColor(this.item?.level ?? 0));
            } finally {
                cr.$dispose();
            }
        });
        box.add_child(this._icon);
        box.add_child(this._label);
        box.add_child(this._spark);
        this.add_child(box);

        this.connect('notify::hover', () => this.emit('hover-changed', this.hover));
        this.connect('key-focus-in', () => this.emit('hover-changed', true));
        this.connect('key-focus-out', () => this.emit('hover-changed', false));
        this.connect('button-release-event', (_a, event) => {
            const b = event.get_button();
            if (b === Clutter.BUTTON_PRIMARY)
                this.emit('activate-metric');
            else if (b === Clutter.BUTTON_SECONDARY)
                this.emit('context-menu');
            return Clutter.EVENT_STOP;
        });
        this.connect('touch-event', (_a, event) => {
            if (event.type() === Clutter.EventType.TOUCH_END)
                this.emit('activate-metric');
            return Clutter.EVENT_STOP;
        });
        this.connect('key-press-event', (_a, event) => {
            const k = event.get_key_symbol();
            if (k === Clutter.KEY_Return || k === Clutter.KEY_KP_Enter || k === Clutter.KEY_space) {
                this.emit('activate-metric');
                return Clutter.EVENT_STOP;
            }
            if (k === Clutter.KEY_Menu) {
                this.emit('context-menu');
                return Clutter.EVENT_STOP;
            }
            return Clutter.EVENT_PROPAGATE;
        });
    }

    /** Apply a bar item. Only touches the label when its text changed. */
    setItem(item) {
        const prev = this.item;
        this.item = item;
        this.remove_style_class_name('masthead-dim');
        if (this._label.text !== item.text)
            this._label.text = item.text;
        if (prev?.level !== item.level) {
            this.remove_style_class_name('masthead-warn');
            this.remove_style_class_name('masthead-crit');
            if (item.level === 1)
                this.add_style_class_name('masthead-warn');
            else if (item.level === 2)
                this.add_style_class_name('masthead-crit');
            this._spark.queue_repaint();
        }
        this.accessible_name = `${F.TITLES[item.id] ?? item.id} ${item.text}`;
    }

    setPlaceholder(text) {
        this._label.text = text;
        this.add_style_class_name('masthead-dim');
    }

    setStyle(style) {
        this._spark.visible = style === 'trend' || style === 'compact';
        this._label.visible = style !== 'compact';
    }

    get sparkVisible() {
        return this._spark.visible;
    }

    setTrend(values, max) {
        this._values = values;
        this._max = max;
        this._spark.queue_repaint();
    }
});

export default class MastheadExtension extends Extension {
    enable() {
        this._iconsDir = this.dir.get_child('icons');
        this._items = new Map();
        this._menus = new Map();
        this._destroyingItems = false;
        this._memTotal = null;
        this._config = null;
        this._style = 'values';
        this._hover = {item: null, handle: null, signal: 0, gen: 0};
        this._openTimer = 0;
        this._closeTimer = 0;
        this._trendBusy = false;

        // Like panel menus, the popover lives in uiGroup above windows. It is
        // not chrome: it takes no input and adds nothing to the input region.
        this._popover = new Popover();
        this._popover.connect('destroy', () => {
            this._popover = null;
        });
        Main.uiGroup.add_child(this._popover);

        this._client = new Client.MastheadClient({
            onReady: () => this._onReady(),
            onPanel: items => this._onPanel(items),
            onConfig: json => this._onConfig(json),
            onMissing: () => this._showMissing(),
            onStopped: () => this._onStopped(),
        });
        this._client.start();
    }

    disable() {
        this._closePopover();
        this._stopClient();
        this._destroyItems();
        this._popover?.destroy();
        this._popover = null;
        this._items = null;
        this._menus = null;
        this._config = null;
    }

    _stopClient() {
        this._clearTimers();
        this._client?.destroy();
        this._client = null;
    }

    // --- daemon events -----------------------------------------------------

    async _onReady() {
        try {
            this._config = await Client.getConfig();
            this._style = this._config?.bar?.style ?? 'values';
        } catch (e) {
            console.warn(`Masthead: GetConfig failed: ${e.message}`);
        }
        try {
            const [json] = await Client.call('Describe', null, '(s)');
            this._memTotal = JSON.parse(json).mem_total || null;
        } catch (e) {
            console.warn(`Masthead: Describe failed: ${e.message}`);
        }
        try {
            const items = await Client.snapshot();
            if (this._client)
                this._onPanel(items);
        } catch (e) {
            console.warn(`Masthead: Snapshot failed: ${e.message}`);
        }
    }

    _onPanel(raw) {
        if (!this._items || !this._client)
            return;
        const items = raw.map(([id, value, text, level, summary]) => ({id, value, text, level, summary}));
        if (items.length === 0) {
            // Every metric hidden in settings: show nothing.
            this._closePopover();
            this._destroyItems();
            return;
        }
        const ids = items.map(i => i.id);
        const current = [...this._items.keys()];
        if (ids.join() !== current.join())
            this._rebuild(ids);
        for (const it of items)
            this._items.get(it.id)?.setItem(it);
        this._refreshTrends();
    }

    /** The daemon went away: keep the last values, dimmed, until it returns. */
    _onStopped() {
        this._closePopover();
        for (const item of this._items?.values() ?? [])
            item.add_style_class_name('masthead-dim');
    }

    _onConfig(json) {
        try {
            this._config = JSON.parse(json);
        } catch {
            return;
        }
        this._style = this._config?.bar?.style ?? 'values';
        for (const item of this._items?.values() ?? [])
            item.setStyle(this._style);
        this._refreshTrends();
    }

    _units() {
        return this._config?.units ?? {};
    }

    // --- bar items ---------------------------------------------------------

    _rebuild(ids) {
        this._closePopover();
        this._destroyItems();
        ids.forEach((id, i) => {
            const item = new MetricItem(id, this._iconsDir);
            item.setStyle(this._style);
            item.connect('hover-changed', (_a, on) => this._onHover(item, on));
            item.connect('activate-metric', () => {
                this._closePopover();
                if (id !== 'missing')
                    Client.toggleWindow(id);
            });
            item.connect('context-menu', () => this._openMenu(item));
            item.connect('destroy', () => {
                if (this._items?.get(id) === item)
                    this._items.delete(id);
                // Only the shell destroys an item behind our back, when it
                // tears the panel down at shutdown. Stop listening so a late
                // Panel signal does not rebuild into a disposed panel.
                if (!this._destroyingItems)
                    this._stopClient();
            });
            Main.panel.addToStatusArea(`${ROLE_PREFIX}${id}`, item, i, 'right');
            this._items.set(id, item);
        });
    }

    _destroyItems() {
        for (const menu of this._menus?.values() ?? [])
            menu.destroy();
        this._menus?.clear();
        this._destroyingItems = true;
        for (const item of [...this._items?.values() ?? []])
            item.destroy();
        this._destroyingItems = false;
        this._items?.clear();
    }

    _showMissing() {
        if (!this._items)
            return;
        this._rebuild(['missing']);
        this._items.get('missing').setPlaceholder('Masthead');
    }

    async _refreshTrends() {
        if (this._trendBusy || !this._items || (this._style !== 'trend' && this._style !== 'compact'))
            return;
        this._trendBusy = true;
        try {
            for (const item of this._items.values()) {
                const series = F.SERIES[item.metric];
                if (!series || !item.sparkVisible)
                    continue;
                const values = await Client.history(series[0], SPARK_SECONDS);
                if (!this._items)
                    return;
                item.setTrend(values, this._maxFor(item.metric, [values]));
            }
        } catch (e) {
            console.debug(`Masthead: History failed: ${e.message}`);
        } finally {
            this._trendBusy = false;
        }
    }

    _maxFor(metric, seriesList) {
        if (metric === 'mem')
            return this._memTotal ?? Draw.niceMax(seriesList);
        if (metric === 'net' || metric === 'disk')
            return Draw.niceMax(seriesList);
        return 100;
    }

    // --- context menu ------------------------------------------------------

    _openMenu(item) {
        this._closePopover();
        if (item.metric === 'missing')
            return;
        let menu = this._menus.get(item);
        if (!menu) {
            menu = new PopupMenu.PopupMenu(item, 0.5, St.Side.TOP);
            menu.actor.add_style_class_name('panel-menu');
            menu.addAction('Open Masthead', () => Client.toggleWindow(item.metric));
            menu.addAction('Settings', () => Client.toggleWindow('settings'));
            menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
            menu.addAction(`Hide ${F.TITLES[item.metric] ?? item.metric}`, () => this._hideMetric(item.metric));
            Main.uiGroup.add_child(menu.actor);
            menu.actor.hide();
            Main.panel.menuManager.addMenu(menu);
            this._menus.set(item, menu);
        }
        menu.toggle();
    }

    async _hideMetric(metric) {
        try {
            const config = this._config ?? await Client.getConfig();
            const next = {...config, bar: {...config.bar, metrics: config.bar.metrics.filter(m => m !== metric)}};
            await Client.setConfig(next);
        } catch (e) {
            Main.notifyError('Masthead', `Could not hide ${F.TITLES[metric] ?? metric}: ${e.message}`);
        }
    }

    // --- popover -----------------------------------------------------------

    _clearTimers() {
        if (this._openTimer) {
            GLib.source_remove(this._openTimer);
            this._openTimer = 0;
        }
        if (this._closeTimer) {
            GLib.source_remove(this._closeTimer);
            this._closeTimer = 0;
        }
    }

    _onHover(item, on) {
        if (on) {
            if (this._closeTimer) {
                GLib.source_remove(this._closeTimer);
                this._closeTimer = 0;
            }
            if (this._hover.item === item)
                return;
            if (this._hover.item) {
                // Moving between items with a popover open switches at once.
                this._openPopover(item);
                return;
            }
            if (this._openTimer)
                GLib.source_remove(this._openTimer);
            this._openTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, OPEN_DELAY_MS, () => {
                this._openTimer = 0;
                if (item.hover || item.has_key_focus())
                    this._openPopover(item);
                return GLib.SOURCE_REMOVE;
            });
        } else {
            if (this._openTimer) {
                GLib.source_remove(this._openTimer);
                this._openTimer = 0;
            }
            if (this._hover.item !== item || this._closeTimer)
                return;
            this._closeTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, CLOSE_DELAY_MS, () => {
                this._closeTimer = 0;
                this._closePopover();
                return GLib.SOURCE_REMOVE;
            });
        }
    }

    async _openPopover(item) {
        if (this._menus.get(item)?.isOpen)
            return;
        this._closePopover(false);
        const gen = ++this._hover.gen;
        this._hover.item = item;
        this._lastDetail = null;

        if (item.metric === 'missing' || !this._client?.connected) {
            this._popover.showMessage('Masthead', [
                'masthead-daemon is not installed or not running.',
                'This extension only draws; the daemon does the sampling. Install the masthead package, or from a source checkout run "just install".',
            ]);
            this._popover.open(item);
            return;
        }

        const metric = item.metric;
        this._popover.prepare(metric, item.item);
        this._popover.open(item);
        this._hover.signal = this._client.watchDetail(metric, d => {
            if (gen !== this._hover.gen)
                return;
            this._renderDetail(d);
        });
        try {
            const handle = await Client.subscribe(`hover:${metric}`);
            if (gen !== this._hover.gen) {
                Client.unsubscribe(handle);
                return;
            }
            this._hover.handle = handle;
            const d = await Client.getDetail(metric);
            if (gen === this._hover.gen)
                this._renderDetail(d);
        } catch (e) {
            console.warn(`Masthead: hover subscribe failed: ${e.message}`);
        }
    }

    async _renderDetail(d) {
        const item = this._hover.item;
        if (!item || !this._popover)
            return;
        this._lastDetail = d;
        if (d.mem?.total)
            this._memTotal = d.mem.total;
        this._popover.update(d, item.item, this._units());
        this._popover.placeUnder(item);
        const gen = this._hover.gen;
        try {
            const series = F.SERIES[item.metric] ?? [];
            const lists = [];
            for (const s of series) {
                lists.push(await Client.history(s, SPARK_SECONDS));
            }
            if (gen === this._hover.gen && this._popover)
                this._popover.setHistory(lists, this._maxFor(item.metric, lists));
        } catch (e) {
            console.debug(`Masthead: History failed: ${e.message}`);
        }
    }

    _closePopover(animate = true) {
        this._hover.gen++;
        if (this._hover.signal) {
            this._client?.unwatchDetail(this._hover.signal);
            this._hover.signal = 0;
        }
        if (this._hover.handle !== null) {
            Client.unsubscribe(this._hover.handle);
            this._hover.handle = null;
        }
        this._hover.item = null;
        if (animate)
            this._popover?.close();
        else
            this._popover?.hideNow();
    }
}
