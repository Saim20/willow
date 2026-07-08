/**
 * WhisperModelManager.js - Sherpa-onnx model download and management
 * Handles downloading and verifying sherpa speech models for Willow
 */

import Adw from 'gi://Adw';
import Gtk from 'gi://Gtk';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

export class WhisperModelManager {
    constructor(configManager = null) {
        this._configManager = configManager;
        this._modelDir = GLib.get_home_dir() + '/.local/share/willow/models';
        this._downloadInProgress = false;

        this._bundles = [
            {
                id: 'kws',
                name: 'Keyword Spotting',
                description: 'Low-latency hotword and mode-control detection',
                size: '~15 MB',
                check: () => this._hasOnnxFiles(`${this._modelDir}/kws`),
            },
            {
                id: 'streaming',
                name: 'Streaming ASR',
                description: 'Real-time speech recognition for command and typing modes',
                size: '~120 MB',
                check: () => this._hasOnnxFiles(`${this._modelDir}/streaming`),
            },
            {
                id: 'speaker',
                name: 'Speaker Verification',
                description: 'Voice profile matching after hotword activation',
                size: '~25 MB',
                check: () => Gio.File.new_for_path(`${this._modelDir}/speaker/model.onnx`).query_exists(null),
            },
        ];
    }

    createModelGroup(window) {
        const group = new Adw.PreferencesGroup({
            title: 'Speech Models',
            description: 'Sherpa-onnx models for keyword spotting, streaming ASR, and speaker verification',
        });

        this._group = group;
        this._window = window;
        this._statusRow = null;
        this._bundleRows = new Map();

        this._buildGroupContent();
        return group;
    }

    _buildGroupContent() {
        this._statusRow = new Adw.ActionRow({
            title: 'Model Status',
            subtitle: 'Checking…',
        });
        this._group.add(this._statusRow);

        for (const bundle of this._bundles) {
            const row = new Adw.ActionRow({
                title: bundle.name,
                subtitle: `${bundle.description} • ${bundle.size}`,
            });
            this._bundleRows.set(bundle.id, row);
            this._group.add(row);
        }

        const downloadRow = new Adw.ActionRow({
            title: 'Download All Models',
            subtitle: 'Runs willow-download-model (~160 MB total)',
        });
        const downloadButton = new Gtk.Button({
            icon_name: 'folder-download-symbolic',
            label: 'Download',
            valign: Gtk.Align.CENTER,
        });
        downloadButton.connect('clicked', () => this._downloadAll(this._window, downloadButton));
        downloadRow.add_suffix(downloadButton);
        this._group.add(downloadRow);

        const dirRow = new Adw.ActionRow({
            title: 'Model Directory',
            subtitle: this._modelDir,
        });
        const openDirButton = new Gtk.Button({
            icon_name: 'folder-open-symbolic',
            valign: Gtk.Align.CENTER,
            tooltip_text: 'Open model directory',
        });
        openDirButton.connect('clicked', () => this._openModelDirectory());
        dirRow.add_suffix(openDirButton);
        this._group.add(dirRow);

        this._refreshUI();
    }

    _hasOnnxFiles(dirPath) {
        const dir = Gio.File.new_for_path(dirPath);
        if (!dir.query_exists(null)) {
            return false;
        }
        try {
            const enumerator = dir.enumerate_children('standard::name', Gio.FileQueryInfoFlags.NONE, null);
            let info;
            let hasOnnx = false;
            let hasTokens = false;
            while ((info = enumerator.next_file(null))) {
                const name = info.get_name();
                if (name.endsWith('.onnx')) {
                    hasOnnx = true;
                }
                if (name === 'tokens.txt') {
                    hasTokens = true;
                }
            }
            enumerator.close(null);
            return hasOnnx && (hasTokens || dirPath.endsWith('/speaker'));
        } catch (e) {
            return false;
        }
    }

    _allInstalled() {
        return this._bundles.every(bundle => bundle.check());
    }

    _refreshUI() {
        if (!this._statusRow) {
            return;
        }

        const installed = this._bundles.filter(b => b.check()).length;
        const total = this._bundles.length;
        const allReady = installed === total;

        this._statusRow.subtitle = allReady
            ? 'All models installed and ready'
            : `${installed}/${total} model bundles installed`;

        for (const bundle of this._bundles) {
            const row = this._bundleRows.get(bundle.id);
            if (!row) {
                continue;
            }
            const ready = bundle.check();
            row.subtitle = ready
                ? `✓ Installed • ${bundle.description}`
                : `Missing • ${bundle.description} • ${bundle.size}`;
        }
    }

    _downloadAll(window, button) {
        if (this._downloadInProgress) {
            this._showToast(window, 'Download already in progress');
            return;
        }

        this._downloadInProgress = true;
        button.sensitive = false;
        button.label = 'Downloading…';
        this._showToast(window, 'Downloading sherpa-onnx models…');

        const command = 'willow-download-model';
        try {
            GLib.spawn_command_line_async(command);
        } catch (e) {
            this._downloadInProgress = false;
            button.sensitive = true;
            button.label = 'Download';
            this._showToast(window, `Download failed: ${e.message}`);
            return;
        }

        const poll = () => {
            this._refreshUI();
            if (this._allInstalled()) {
                this._downloadInProgress = false;
                button.sensitive = true;
                button.label = 'Download';
                this._showToast(window, 'All models installed');
                return GLib.SOURCE_REMOVE;
            }
            return GLib.SOURCE_CONTINUE;
        };

        GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 3, poll);
        GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 120, () => {
            this._downloadInProgress = false;
            button.sensitive = true;
            button.label = 'Download';
            this._refreshUI();
            return GLib.SOURCE_REMOVE;
        });
    }

    _openModelDirectory() {
        try {
            GLib.spawn_command_line_sync(`mkdir -p ${this._modelDir}`);
            GLib.spawn_command_line_async(`xdg-open "${this._modelDir}"`);
        } catch (e) {
            console.error('Could not open model directory:', e);
        }
    }

    _showToast(window, message) {
        console.log(`SpeechModelManager: ${message}`);
        try {
            window.add_toast(new Adw.Toast({title: message, timeout: 3}));
        } catch (e) {
            console.log(`Toast: ${message}`);
        }
    }
}
