// SPDX-License-Identifier: Apache-2.0
// Copyright Kitefold Labs

// Formatting for popover values. Mirrors crates/masthead-core/src/format.rs so
// the popover reads like every other Masthead face. Bar labels arrive already
// formatted from the daemon.

const GIB = 1024 ** 3;

function scaled(v, base) {
    let i = 0;
    v = Math.max(0, v);
    while (v >= base && i < 4) {
        v /= base;
        i++;
    }
    return [v, i];
}

function trimNum(v, i) {
    return i > 0 && v < 10 ? v.toFixed(1) : `${Math.round(v)}`;
}

export function percent(v) {
    return `${Math.round(Math.min(100, Math.max(0, v)))}%`;
}

export function memLong(n, units = {}) {
    const binary = (units.memory ?? 'binary') === 'binary';
    const g = n / (binary ? GIB : 1e9);
    if (g < 1) {
        const m = n / (binary ? 1024 ** 2 : 1e6);
        return `${Math.round(m)} ${binary ? 'MiB' : 'MB'}`;
    }
    return `${g.toFixed(1)} ${binary ? 'GiB' : 'GB'}`;
}

export function rate(bytesPerSec, units = {}, short = false) {
    const bits = units.network === 'bits';
    const [v, i] = scaled(bytesPerSec * (bits ? 8 : 1), 1000);
    const n = trimNum(v, i);
    if (bits)
        return short ? `${n}${['b', 'k', 'm', 'g', 't'][i]}` : `${n} ${['b/s', 'kb/s', 'Mb/s', 'Gb/s', 'Tb/s'][i]}`;
    return short ? `${n}${['B', 'K', 'M', 'G', 'T'][i]}` : `${n} ${['B/s', 'kB/s', 'MB/s', 'GB/s', 'TB/s'][i]}`;
}

export function diskRate(bytesPerSec, short = false) {
    return rate(bytesPerSec, {network: 'bytes'}, short);
}

export function temp(celsius, units = {}, short = false) {
    const f = units.temperature === 'fahrenheit';
    const v = Math.round(f ? celsius * 9 / 5 + 32 : celsius);
    return short ? `${v}°` : `${v} ${f ? '°F' : '°C'}`;
}

export function watts(w) {
    return w < 10 ? `${w.toFixed(1)} W` : `${Math.round(w)} W`;
}

export function bytes(b) {
    const [v, i] = scaled(b, 1000);
    return `${trimNum(v, i)} ${['B', 'kB', 'MB', 'GB', 'TB'][i]}`;
}

export function duration(secs) {
    const s = Math.max(0, Math.floor(secs));
    const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60);
    if (d > 0)
        return `${d}d ${h}h`;
    if (h > 0)
        return `${h}h ${m}m`;
    return `${m}m`;
}

export const TITLES = {
    cpu: 'CPU',
    mem: 'Memory',
    gpu: 'GPU',
    net: 'Network',
    disk: 'Disk',
    temp: 'Temperature',
    battery: 'Battery',
};

/** History series drawn for each metric: [primary, secondary?]. */
export const SERIES = {
    cpu: ['cpu'],
    mem: ['mem'],
    gpu: ['gpu'],
    net: ['net.rx', 'net.tx'],
    disk: ['disk.read', 'disk.write'],
    temp: ['temp'],
    battery: ['battery'],
};
