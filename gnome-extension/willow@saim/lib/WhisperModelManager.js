/**
 * WhisperModelManager.js - Sherpa-onnx model download and management
 * Handles downloading and verifying sherpa speech models for Willow
 */

import Adw from 'gi://Adw';
import Gtk from 'gi://Gtk';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

export class WhisperModelManager {
    constructor(configManager = null, extensionDir = null) {
        this._configManager = configManager;
        this._extensionDir = extensionDir;
        this._modelDir = GLib.get_home_dir() + '/.local/share/willow/models';
        this._downloadInProgress = false;
        this._pollSourceId = null;
        this._subprocess = null;

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
        this._downloadButton = null;

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

        this._progressRow = new Adw.ActionRow({
            title: 'Download Progress',
            subtitle: 'Waiting to start…',
            visible: false,
        });
        this._progressBar = new Gtk.ProgressBar({
            valign: Gtk.Align.CENTER,
            width_request: 200,
            show_text: true,
        });
        this._progressRow.add_suffix(this._progressBar);
        this._group.add(this._progressRow);

        const downloadRow = new Adw.ActionRow({
            title: 'Download All Models',
            subtitle: 'Downloads sherpa-onnx models (~160 MB total)',
        });
        const downloadButton = new Gtk.Button({
            icon_name: 'folder-download-symbolic',
            label: 'Download',
            valign: Gtk.Align.CENTER,
        });
        downloadButton.connect('clicked', () => this._downloadAll(this._window, downloadButton));
        downloadRow.add_suffix(downloadButton);
        this._downloadButton = downloadButton;
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

        if (this._downloadInProgress && !this._allInstalled()) {
            const bundleFraction = installed / total;
            if (bundleFraction > this._progressBar.fraction) {
                this._progressBar.fraction = bundleFraction;
                this._progressBar.text = `${Math.round(bundleFraction * 100)}%`;
            }
            if (!this._progressRow.subtitle || this._progressRow.subtitle === 'Waiting to start…') {
                this._progressRow.subtitle = `Installed ${installed}/${total} bundles…`;
            }
        }
    }

    _resolveDownloadScript() {
        const installed = Gio.File.new_for_path('/usr/bin/willow-download-model');
        if (installed.query_exists(null)) {
            return installed.get_path();
        }

        if (this._extensionDir) {
            const devScript = this._extensionDir.resolve('../../download-model.sh');
            if (devScript.query_exists(null)) {
                return devScript.get_path();
            }
        }

        return null;
    }

    _downloadAll(window, button) {
        if (this._downloadInProgress) {
            this._showToast(window, 'Download already in progress');
            return;
        }

        const scriptPath = this._resolveDownloadScript();
        if (!scriptPath) {
            this._showToast(window, 'Download script not found. Install willow or run willow-download-model manually.');
            return;
        }

        this._downloadInProgress = true;
        button.sensitive = false;
        button.label = 'Downloading…';
        this._progressRow.visible = true;
        this._progressRow.subtitle = 'Starting download…';
        this._progressBar.fraction = 0;
        this._progressBar.text = '0%';
        this._showToast(window, 'Downloading sherpa-onnx models…');

        try {
            this._subprocess = Gio.Subprocess.new(
                [scriptPath],
                Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_MERGE,
            );
        } catch (e) {
            this._finishDownload(window, button, false, `Failed to start download: ${e.message}`);
            return;
        }

        const stdout = this._subprocess.get_stdout_pipe();
        const inputStream = new Gio.DataInputStream({
            base_stream: new Gio.UnixInputStream({
                fd: stdout.steal_fd(),
                close_fd: true,
            }),
        });

        const readLine = () => {
            inputStream.read_line_async(GLib.PRIORITY_DEFAULT, null, (stream, result) => {
                try {
                    const [line, length_] = stream.read_line_finish(result);
                    if (length_ === 0) {
                        return;
                    }
                    if (line) {
                        this._handleDownloadOutput(line);
                    }
                    readLine();
                } catch (e) {
                    console.error('SpeechModelManager: output read error:', e);
                }
            });
        };
        readLine();

        this._pollSourceId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 2, () => {
            this._refreshUI();
            return GLib.SOURCE_CONTINUE;
        });

        this._subprocess.wait_async(null, (subprocess, result) => {
            try {
                const success = subprocess.wait_finish(result);
                if (success && this._allInstalled()) {
                    this._finishDownload(window, button, true, 'All models installed. Restart the service to use them.');
                } else if (success) {
                    this._finishDownload(window, button, false, 'Download finished but some models are still missing.');
                } else {
                    this._finishDownload(window, button, false, 'Download failed. Check your network connection and try again.');
                }
            } catch (e) {
                this._finishDownload(window, button, false, `Download failed: ${e.message}`);
            }
        });
    }

    _handleDownloadOutput(line) {
        if (line.startsWith('WILLOW_PROGRESS:')) {
            const match = line.match(/^WILLOW_PROGRESS:(\d+):(\d+):(.+)$/);
            if (match) {
                const step = parseInt(match[1], 10);
                const total = parseInt(match[2], 10);
                const message = match[3];
                if (total > 0) {
                    const fraction = Math.min(step / total, 1);
                    if (fraction >= this._progressBar.fraction) {
                        this._progressBar.fraction = fraction;
                        this._progressBar.text = `${Math.round(fraction * 100)}%`;
                    }
                }
                this._progressRow.subtitle = message;
            }
            this._refreshUI();
            return;
        }

        console.log(`SpeechModelManager: ${line}`);
    }

    _finishDownload(window, button, success, message) {
        this._downloadInProgress = false;
        this._subprocess = null;

        if (this._pollSourceId) {
            GLib.source_remove(this._pollSourceId);
            this._pollSourceId = null;
        }

        button.sensitive = true;
        button.label = 'Download';
        this._refreshUI();

        if (success) {
            this._progressBar.fraction = 1;
            this._progressBar.text = '100%';
            this._progressRow.subtitle = 'Complete';
        } else {
            this._progressRow.subtitle = message;
        }

        this._showToast(window, message, success ? 5 : 8);
    }

    _openModelDirectory() {
        try {
            GLib.spawn_command_line_sync(`mkdir -p ${this._modelDir}`);
            GLib.spawn_command_line_async(`xdg-open "${this._modelDir}"`);
        } catch (e) {
            console.error('Could not open model directory:', e);
        }
    }

    _showToast(window, message, timeout = 3) {
        console.log(`SpeechModelManager: ${message}`);
        try {
            window.add_toast(new Adw.Toast({title: message, timeout}));
        } catch (e) {
            console.log(`Toast: ${message}`);
        }
    }
}
