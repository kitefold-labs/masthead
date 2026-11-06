// SPDX-License-Identifier: Apache-2.0
// Copyright Kitefold Labs

// The extension's only link to the system: the com.kitefoldlabs.Masthead1
// D-Bus service run by masthead-daemon. Nothing here samples or polls.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

export const BUS_NAME = 'com.kitefoldlabs.Masthead1';
const OBJECT_PATH = '/com/kitefoldlabs/Masthead1';
const INTERFACE = 'com.kitefoldlabs.Masthead1';

/** Retry delays, in seconds, for waking a daemon that went away. */
const RETRY = [2, 4, 8, 16, 30, 60];

/**
 * Call a method on the daemon. Without DO_NOT_AUTO_START, the first call
 * D-Bus-activates masthead-daemon.
 *
 * @param {string} method - D-Bus method name
 * @param {GLib.Variant|null} params - arguments tuple
 * @param {string|null} replyType - reply signature, e.g. '(u)'
 * @returns {Promise<any[]>} the unpacked reply tuple
 */
export function call(method, params = null, replyType = null) {
    return new Promise((resolve, reject) => {
        Gio.DBus.session.call(BUS_NAME, OBJECT_PATH, INTERFACE, method, params,
            replyType ? new GLib.VariantType(replyType) : null,
            Gio.DBusCallFlags.NONE, 5000, null, (conn, res) => {
                try {
                    resolve(conn.call_finish(res).deepUnpack());
                } catch (e) {
                    reject(e);
                }
            });
    });
}

/** Whether a D-Bus service file for the daemon is installed. */
function isActivatable() {
    return new Promise(resolve => {
        Gio.DBus.session.call('org.freedesktop.DBus', '/org/freedesktop/DBus',
            'org.freedesktop.DBus', 'ListActivatableNames', null,
            new GLib.VariantType('(as)'), Gio.DBusCallFlags.NONE, 2000, null, (conn, res) => {
                try {
                    const [names] = conn.call_finish(res).deepUnpack();
                    resolve(names.includes(BUS_NAME));
                } catch {
                    resolve(false);
                }
            });
    });
}

/**
 * Tracks the daemon on the bus and owns the bar's `panel` subscription.
 *
 * Callbacks: onReady() when a daemon appears and the panel tier is held,
 * onMissing() when no daemon is running or installed, onPanel(items),
 * onConfig(json).
 */
export class MastheadClient {
    constructor(callbacks) {
        this._cb = callbacks;
        this._owner = null;
        this._panelHandle = null;
        this._watchId = 0;
        this._signalIds = [];
        this._retry = 0;
        this._retryId = 0;
        this._destroyed = false;
    }

    start() {
        const conn = Gio.DBus.session;
        this._signalIds.push(conn.signal_subscribe(null, INTERFACE, 'Panel', OBJECT_PATH, null,
            Gio.DBusSignalFlags.NONE, (_c, _s, _p, _i, _n, params) => {
                const [items] = params.deepUnpack();
                this._cb.onPanel?.(items);
            }));
        this._signalIds.push(conn.signal_subscribe(null, INTERFACE, 'ConfigChanged', OBJECT_PATH, null,
            Gio.DBusSignalFlags.NONE, (_c, _s, _p, _i, _n, params) => {
                const [json] = params.deepUnpack();
                this._cb.onConfig?.(json);
            }));
        // AUTO_START asks the bus to activate the daemon if it is installed.
        this._watchId = Gio.bus_watch_name(Gio.BusType.SESSION, BUS_NAME,
            Gio.BusNameWatcherFlags.AUTO_START,
            (_c, _name, owner) => this._appeared(owner),
            () => this._vanished());
    }

    get connected() {
        return this._owner !== null;
    }

    async _appeared(owner) {
        if (owner === this._owner)
            return;
        this._owner = owner;
        this._retry = 0;
        this._clearRetry();
        try {
            const [handle] = await call('Subscribe', new GLib.Variant('(s)', ['panel']), '(u)');
            if (this._destroyed || owner !== this._owner) {
                call('Unsubscribe', new GLib.Variant('(u)', [handle])).catch(() => {});
                return;
            }
            this._panelHandle = handle;
            this._cb.onReady?.();
        } catch (e) {
            console.warn(`Masthead: Subscribe failed: ${e.message}`);
        }
    }

    async _vanished() {
        const hadDaemon = this._owner !== null;
        this._owner = null;
        this._panelHandle = null;
        if (this._destroyed)
            return;
        if (!await isActivatable()) {
            if (!this._destroyed)
                this._cb.onMissing?.();
            return;
        }
        if (hadDaemon)
            this._cb.onStopped?.();
        this._scheduleRetry();
    }

    /** Wake an activatable daemon that stopped, backing off up to a minute. */
    _scheduleRetry() {
        this._clearRetry();
        const delay = RETRY[Math.min(this._retry, RETRY.length - 1)];
        this._retry++;
        this._retryId = GLib.timeout_add_seconds(GLib.PRIORITY_LOW, delay, () => {
            this._retryId = 0;
            call('Describe', null, '(s)').catch(() => this._scheduleRetry());
            return GLib.SOURCE_REMOVE;
        });
    }

    _clearRetry() {
        if (this._retryId) {
            GLib.source_remove(this._retryId);
            this._retryId = 0;
        }
    }

    /**
     * Subscribe to a Detail signal for one metric (arg0 match, so the window's
     * large `all` payloads never reach the shell).
     *
     * @param {string} metric - metric id
     * @param {Function} cb - called with the parsed detail object
     * @returns {number} id for unwatchDetail
     */
    watchDetail(metric, cb) {
        return Gio.DBus.session.signal_subscribe(null, INTERFACE, 'Detail', OBJECT_PATH, metric,
            Gio.DBusSignalFlags.NONE, (_c, _s, _p, _i, _n, params) => {
                const [, json] = params.deepUnpack();
                try {
                    cb(JSON.parse(json));
                } catch (e) {
                    console.warn(`Masthead: bad detail payload: ${e.message}`);
                }
            });
    }

    unwatchDetail(id) {
        if (id)
            Gio.DBus.session.signal_unsubscribe(id);
    }

    destroy() {
        this._destroyed = true;
        this._clearRetry();
        for (const id of this._signalIds)
            Gio.DBus.session.signal_unsubscribe(id);
        this._signalIds = [];
        if (this._watchId) {
            Gio.bus_unwatch_name(this._watchId);
            this._watchId = 0;
        }
        if (this._panelHandle !== null && this._owner !== null)
            call('Unsubscribe', new GLib.Variant('(u)', [this._panelHandle])).catch(() => {});
        this._panelHandle = null;
        this._owner = null;
    }
}

export async function subscribe(tier) {
    const [handle] = await call('Subscribe', new GLib.Variant('(s)', [tier]), '(u)');
    return handle;
}

export function unsubscribe(handle) {
    return call('Unsubscribe', new GLib.Variant('(u)', [handle])).catch(() => {});
}

export async function snapshot() {
    const [items] = await call('Snapshot', null, '(a(sdsus))');
    return items;
}

export async function getDetail(metric) {
    const [json] = await call('GetDetail', new GLib.Variant('(s)', [metric]), '(s)');
    return JSON.parse(json);
}

/** @returns {Promise<number[]>} the last `seconds` values (max per step), NaN for gaps */
export async function history(series, seconds) {
    const [, , max] = await call('History', new GLib.Variant('(su)', [series, seconds]), '(dadad)');
    return max;
}

export function toggleWindow(metric) {
    return call('ToggleWindow', new GLib.Variant('(s)', [metric])).catch(e =>
        console.warn(`Masthead: ToggleWindow failed: ${e.message}`));
}

export async function getConfig() {
    const [json] = await call('GetConfig', null, '(s)');
    return JSON.parse(json);
}

export function setConfig(config) {
    return call('SetConfig', new GLib.Variant('(s)', [JSON.stringify(config)]));
}
