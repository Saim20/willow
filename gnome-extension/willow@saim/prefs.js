/**
 * Willow Preferences
 * Configuration UI for D-Bus service parameters and command management
 */

import Adw from 'gi://Adw';
import Gtk from 'gi://Gtk';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import {ExtensionPreferences} from 'resource:///org/gnome/Shell/Extensions/js/extensions/prefs.js';

// Import our modular components
import {ConfigManager} from './lib/ConfigManager.js';
import {CommandManager} from './lib/CommandEditor.js';
import {PreferencesBuilder, StatusManager} from './lib/PreferencesWidgets.js';
import {WhisperModelManager} from './lib/WhisperModelManager.js';
import {LogViewer} from './lib/LogViewer.js';

export default class VoiceAssistantExtensionPreferences extends ExtensionPreferences {
    fillPreferencesWindow(window) {
        const settings = this.getSettings();
        
        // Initialize managers
        this._configManager = new ConfigManager(settings);
        this._commandManager = new CommandManager(this._configManager);
        this._prefsBuilder = new PreferencesBuilder(settings);
        this._statusManager = new StatusManager();
        this._modelManager = new WhisperModelManager(this._configManager, this.dir);
        this._logViewer = new LogViewer();
        
        // Setup automatic sync with debouncing
        this._syncTimeout = null;
        this._prefsBuilder.setSyncCallback(() => {
            if (this._syncTimeout) {
                GLib.source_remove(this._syncTimeout);
            }
            this._syncTimeout = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 500, () => {
                this._configManager.syncSettingsToConfig();
                this._syncTimeout = null;
                return GLib.SOURCE_REMOVE;
            });
        });
        
        // Create pages
        this._createGeneralPage(window, settings);
        this._createVoicePage(window, settings);
        this._createModelsPage(window, settings);
        this._createCommandsPage(window, settings);
        this._createLogsPage(window, settings);
        this._createAboutPage(window, settings);

        this._setupServiceSignals(window);
    }

    _setupServiceSignals(window) {
        const proxy = this._configManager?._proxy;
        if (!proxy) {
            return;
        }

        proxy.connectSignal('Error', (_proxy, _sender, [message, details]) => {
            this._showToast(window, `${message}: ${details}`, 8);
            this._refreshVoicePageStatus();
        });

        proxy.connectSignal('Notification', (_proxy, _sender, [title, message]) => {
            // No banners — status refresh only if needed.
            this._refreshVoicePageStatus();
            console.log(`Willow: ${title}: ${message}`);
        });

        proxy.connectSignal('StatusChanged', () => {
            this._refreshVoicePageStatus();
        });
    }

    _createGeneralPage(window, settings) {
        const page = new Adw.PreferencesPage({
            title: 'General',
            icon_name: 'preferences-system-symbolic',
        });

        // Voice Recognition Settings group
        const recognitionGroup = this._prefsBuilder.createGroup(
            'Voice Recognition',
            'Configure voice command processing parameters via D-Bus service'
        );

        this._prefsBuilder.createEntryRow(
            'Activation Hotword',
            'Saved locally — open the Voice tab and click Apply Hotword to activate',
            'hotword',
            'hey willow',
            recognitionGroup,
            {syncOnChange: false}
        );
        this._hotwordHintRow = this._prefsBuilder.createInfoRow(
            'Hotword Status',
            'Open the Voice tab to apply changes to the running service',
            recognitionGroup
        );

        this._prefsBuilder.createSpinButtonRow(
            'Command Threshold',
            'Minimum confidence percentage to execute commands (50-100%)',
            'command-threshold',
            50, 100, 5,
            recognitionGroup
        );

        this._prefsBuilder.createDoubleSpinButtonRow(
            'Command Silence Timeout',
            'Streaming ASR trailing silence (rule2) — lower closes utterances sooner',
            'command-endpoint-silence',
            0.15, 1.50, 0.05, 2,
            recognitionGroup
        );

        this._prefsBuilder.createDoubleSpinButtonRow(
            'Workflow Session Timeout',
            'Seconds to wait for missing slots (e.g. “Open which app?”) before clearing',
            'workflow-session-timeout',
            2.0, 60.0, 1.0, 0,
            recognitionGroup
        );

        this._prefsBuilder.createSwitchRow(
            'Early Command Fire',
            'Run exact phrases as soon as streaming ASR matches (recommended)',
            'early-fire',
            recognitionGroup
        );

        this._prefsBuilder.createSwitchRow(
            'Typing Mode Auto-Revert',
            'Return to normal after idle timeout (off = stay in typing until exit phrase)',
            'typing-auto-revert',
            recognitionGroup
        );

        this._prefsBuilder.createSwitchRow(
            'GPU Acceleration',
            'Prefer CUDA when Willow was built with GPU sherpa-onnx (falls back to CPU)',
            'gpu-acceleration',
            recognitionGroup
        );

        page.add(recognitionGroup);

        const llmGroup = this._prefsBuilder.createGroup(
            'Local LLM Fallback',
            'Optional llama-cli + GGUF for ambiguous phrasing after endpoint (default off)'
        );

        this._prefsBuilder.createSwitchRow(
            'Enable LLM Fallback',
            'Rewrite unmatched utterances to structured intents (never free-form shell)',
            'llm-enabled',
            llmGroup
        );

        this._prefsBuilder.createEntryRow(
            'GGUF Model Path',
            'Absolute path to a local .gguf model for llama-cli',
            'llm-model-path',
            '',
            llmGroup
        );

        this._prefsBuilder.createSpinButtonRow(
            'Max Tokens',
            'Cap LLM output length (16–256)',
            'llm-max-tokens',
            16, 256, 8,
            llmGroup
        );

        this._prefsBuilder.createSpinButtonRow(
            'Timeout (ms)',
            'Hard timeout for LLM fallback (100–5000)',
            'llm-timeout-ms',
            100, 5000, 50,
            llmGroup
        );

        page.add(llmGroup);

        // Interface Settings group
        const interfaceGroup = this._prefsBuilder.createGroup(
            'Interface Settings',
            'Configure extension display and refresh behavior'
        );

        this._prefsBuilder.createSpinButtonRow(
            'Status Update Interval',
            'How often to refresh D-Bus status display (1-10 seconds)',
            'update-interval',
            1, 10, 1,
            interfaceGroup
        );

        page.add(interfaceGroup);

        // D-Bus Service Control group
        const serviceGroup = this._prefsBuilder.createGroup(
            'D-Bus Service Control',
            'Manage synchronization with Willow service'
        );

        this._prefsBuilder.createButtonRow(
            'Sync to Service',
            'Push current settings to the D-Bus service configuration',
            'Sync Now',
            'document-save-symbolic',
            () => {
                this._configManager.syncSettingsToConfig();
                this._configManager.pushConfigToService((ok, error) => {
                    if (!ok) {
                        this._showToast(window, `Sync failed: ${error}`, 8);
                        return;
                    }
                    this._showToast(window, 'Settings synced to D-Bus service');
                });
            },
            serviceGroup
        );

        this._prefsBuilder.createButtonRow(
            'Load from Service',
            'Load configuration from D-Bus service to extension settings',
            'Load from Service',
            'document-open-symbolic',
            () => {
                this._configManager.loadConfigFromService((config, error) => {
                    if (error) {
                        this._showToast(window, 'Failed to load config from service');
                        console.error('Failed to load config from service:', error);
                        return;
                    }
                    this._configManager.syncConfigToSettings();
                    this._showToast(window, 'Configuration loaded from service');
                });
            },
            serviceGroup
        );

        page.add(serviceGroup);

        // Status group
        const statusGroup = this._prefsBuilder.createGroup(
            'Status',
            'Current voice recognition configuration'
        );

        const configStatus = this._statusManager.checkConfigStatus();
        const statusText = configStatus.exists ? 
            `Configuration file: ${configStatus.path}` :
            'Configuration file not found - will be created by service';

        this._prefsBuilder.createInfoRow(
            'Configuration',
            statusText,
            statusGroup
        );

        if (configStatus.exists && configStatus.lastModified) {
            this._prefsBuilder.createInfoRow(
                'Last Modified',
                this._statusManager.formatTimestamp(configStatus.lastModified),
                statusGroup
            );
        }

        page.add(statusGroup);

        // Smart Workflows group
        const smartGroup = this._prefsBuilder.createGroup(
            'Smart Workflows',
            'Context-aware voice commands for opening apps and searching'
        );

        this._prefsBuilder.createInfoRow(
            'Smart Open',
            'Say "open [app]" or "launch [app]" in command mode to open any application (e.g., "open firefox", "launch spotify")',
            smartGroup
        );

        this._prefsBuilder.createInfoRow(
            'Smart Search',
            'Say "search [engine] for [query]" in command mode (e.g., "search youtube for music videos", "search google for python tutorials")',
            smartGroup
        );

        this._prefsBuilder.createInfoRow(
            'Context Configuration',
            'Smart workflows use ~/.config/willow/context.json to configure default apps, search engines, and app aliases',
            smartGroup
        );

        this._prefsBuilder.createButtonRow(
            'Edit Context File',
            'Open the context configuration file to customize smart workflow behavior',
            'Edit Context',
            'document-edit-symbolic',
            () => {
                try {
                    const contextPath = GLib.get_home_dir() + '/.config/willow/context.json';
                    GLib.spawn_command_line_async(`xdg-open "${contextPath}"`);
                } catch (e) {
                    this._showToast(window, 'Could not open context file');
                }
            },
            smartGroup
        );

        page.add(smartGroup);
        window.add(page);
    }

    _createVoicePage(window, settings) {
        const page = new Adw.PreferencesPage({
            title: 'Voice',
            icon_name: 'audio-input-microphone-symbolic',
        });

        const statusGroup = this._prefsBuilder.createGroup(
            'Live Service Status',
            'Shows what the Willow service is doing right now'
        );

        this._micStatusRow = this._prefsBuilder.createInfoRow(
            'Microphone',
            'Checking…',
            statusGroup
        );
        this._hotwordStatusRow = this._prefsBuilder.createInfoRow(
            'Active Hotword',
            'Checking…',
            statusGroup
        );
        this._modelsStatusRow = this._prefsBuilder.createInfoRow(
            'Speech Models',
            'Checking…',
            statusGroup
        );
        this._encodingStatusRow = this._prefsBuilder.createInfoRow(
            'Hotword Encoding',
            'Checking…',
            statusGroup
        );

        page.add(statusGroup);

        const hotwordGroup = this._prefsBuilder.createGroup(
            'Hotword',
            'Say this phrase in normal mode to activate Willow'
        );

        this._prefsBuilder.createEntryRow(
            'Activation Hotword',
            'Use short, clear phrases with common English words',
            'hotword',
            'hey willow',
            hotwordGroup,
            {syncOnChange: false}
        );

        this._applyHotwordButton = this._prefsBuilder.createButtonRow(
            'Apply Hotword',
            'Encode and activate the hotword on the running service',
            'Apply Now',
            'emblem-ok-symbolic',
            () => this._applyHotword(window),
            hotwordGroup
        );

        page.add(hotwordGroup);

        this._speakerVerificationEnabled =
            this._configManager.getConfig().speaker_verification?.enabled ?? false;

        if (this._speakerVerificationEnabled) {
            const verifyGroup = this._prefsBuilder.createGroup(
                'Speaker Verification',
                'Enroll your voice so only you can activate Willow after the hotword'
            );

            this._enrollStatusRow = this._prefsBuilder.createInfoRow(
                'Enrollment Status',
                'Checking…',
                verifyGroup
            );

            this._enrollProgressRow = new Adw.ActionRow({
                title: 'Enrollment Progress',
                subtitle: 'Press Start, then follow the on-screen prompts',
                visible: false,
            });
            this._enrollProgressBar = new Gtk.ProgressBar({
                valign: Gtk.Align.CENTER,
                width_request: 220,
                show_text: true,
                fraction: 0,
            });
            this._enrollProgressRow.add_suffix(this._enrollProgressBar);
            verifyGroup.add(this._enrollProgressRow);

            this._enrollPromptRow = this._prefsBuilder.createInfoRow(
                'What to say',
                'No magic phrase — Willow records 3 short samples (~2 seconds each). Try your hotword, a command, then anything else in your normal voice.',
                verifyGroup
            );

            this._prefsBuilder.createInfoRow(
                'How it works',
                'Press Start, then speak when prompted. Keep talking steadily until each sample completes. A desktop notification shows example phrases for each sample.',
                verifyGroup
            );

            this._enrollStartButton = this._prefsBuilder.createButtonRow(
                'Start Enrollment',
                'Record or replace your voice profile (3 short speech samples)',
                'Start',
                'microphone-sensitivity-high-symbolic',
                () => this._startSpeakerEnrollment(window),
                verifyGroup
            );

            this._prefsBuilder.createButtonRow(
                'Cancel Enrollment',
                'Stop an in-progress enrollment session',
                'Cancel',
                'process-stop-symbolic',
                () => this._cancelSpeakerEnrollment(window),
                verifyGroup
            );

            this._prefsBuilder.createButtonRow(
                'Remove Voice Profile',
                'Delete enrolled speaker data',
                'Remove Profile',
                'user-trash-symbolic',
                () => this._removeSpeakerProfile(window),
                verifyGroup
            );

            page.add(verifyGroup);
        }

        window.add(page);

        this._voicePollFast = false;
        this._voicePollId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 1, () => {
            this._refreshVoicePageStatus();
            return GLib.SOURCE_CONTINUE;
        });
        this._refreshVoicePageStatus();
    }

    _applyHotword(window) {
        const hotword = this._settings.get_string('hotword').trim();
        if (!hotword) {
            this._showToast(window, 'Enter a hotword first');
            return;
        }

        this._hotwordStatusRow.subtitle = 'Applying hotword…';
        this._configManager.syncSettingsToConfig();
        this._configManager.applyHotwordToService(hotword, (ok, error) => {
            if (!ok) {
                this._hotwordStatusRow.subtitle = `Failed: ${error}`;
                this._showToast(window, `Hotword failed: ${error}`, 8);
                return;
            }
            this._hotwordStatusRow.subtitle = `Listening for "${hotword}"`;
            if (this._hotwordHintRow) {
                this._hotwordHintRow.subtitle = `Active on service: ${hotword}`;
            }
            this._showToast(window, `Hotword applied: ${hotword}`, 5);
            this._refreshVoicePageStatus();
        });
    }

    _refreshVoicePageStatus() {
        const proxy = this._getProxy();

        if (!proxy) {
            const disconnected = 'Service not connected — start willow.service';
            if (this._micStatusRow) this._micStatusRow.subtitle = disconnected;
            if (this._hotwordStatusRow) this._hotwordStatusRow.subtitle = disconnected;
            if (this._modelsStatusRow) this._modelsStatusRow.subtitle = disconnected;
            if (this._encodingStatusRow) this._encodingStatusRow.subtitle = disconnected;
            if (this._enrollStatusRow) this._enrollStatusRow.subtitle = disconnected;
            return;
        }

        proxy.GetStatusRemote((statusResult, statusError) => {
            if (statusError || !statusResult || !statusResult[0]) {
                const busy = statusError
                    ? `Service busy or unavailable (${statusError})`
                    : 'Could not read service status';
                if (this._micStatusRow) this._micStatusRow.subtitle = busy;
                if (this._enrollStatusRow) this._enrollStatusRow.subtitle = busy;
                return;
            }
            const status = statusResult[0];
            const audioActive = status.audio_active?.unpack?.() ?? false;
            const modelsLoaded = status.models_loaded?.unpack?.() ?? false;
            const hotword = status.hotword?.unpack?.() ?? '';
            const encodingReady = status.keyword_encoding_ready?.unpack?.() ?? false;
            const enrolled = status.speaker_enrolled?.unpack?.() ?? false;
            const speakerVerificationEnabled =
                status.speaker_verification_enabled?.unpack?.() ?? this._speakerVerificationEnabled;
            this._speakerVerificationEnabled = speakerVerificationEnabled;
            const enrollState = status.enrollment_state?.unpack?.() ?? 'idle';
            const enrollSamples = status.enrollment_samples?.unpack?.() ?? 0;
            const enrollBuffer = status.enrollment_buffer_fraction?.unpack?.() ?? 0;
            const reenrolling = status.enrollment_reenrolling?.unpack?.() ?? false;
            const enrollmentPrompt = status.enrollment_prompt?.unpack?.() ?? '';

            if (this._micStatusRow) {
                this._micStatusRow.subtitle = audioActive
                    ? 'Listening — microphone is active'
                    : modelsLoaded
                        ? 'Not listening — restart the service or use the panel menu'
                        : 'Unavailable — download models first';
            }

            if (this._hotwordStatusRow) {
                this._hotwordStatusRow.subtitle = hotword
                    ? `Service is listening for "${hotword}"`
                    : 'No hotword configured';
            }

            if (this._modelsStatusRow) {
                this._modelsStatusRow.subtitle = modelsLoaded
                    ? 'Loaded and ready'
                    : 'Missing — download models on the Models tab';
            }

            if (this._encodingStatusRow) {
                this._encodingStatusRow.subtitle = encodingReady
                    ? 'Ready — hotword changes can be encoded'
                    : 'Unavailable — install python-sentencepiece and reinstall Willow';
            }

            if (this._hotwordHintRow) {
                this._hotwordHintRow.subtitle = hotword
                    ? `Active on service: ${hotword}`
                    : 'Apply hotword from the Voice tab';
            }

            const recording = enrollState === 'recording';
            const sampleFraction = Math.min(enrollBuffer, 1);
            if (this._enrollProgressRow) {
                this._enrollProgressRow.visible = recording || enrollState === 'complete' || enrollState === 'failed';
            }
            if (this._enrollProgressBar) {
                const fraction = Math.min((enrollSamples + sampleFraction) / 3, 1);
                this._enrollProgressBar.fraction = fraction;
                const pct = Math.round(fraction * 100);
                this._enrollProgressBar.text = recording
                    ? `${enrollSamples}/3 (${pct}%)`
                    : `${enrollSamples}/3`;
            }
            if (this._enrollProgressRow) {
                if (recording) {
                    const nextSample = enrollSamples + 1;
                    const within = Math.round(sampleFraction * 100);
                    this._enrollProgressRow.subtitle = enrollmentPrompt
                        || `Recording sample ${nextSample}/3 (${within}% of current sample) — keep speaking`;
                } else if (enrollState === 'complete' || enrolled) {
                    this._enrollProgressRow.subtitle = 'Enrollment complete';
                } else if (enrollState === 'failed') {
                    this._enrollProgressRow.subtitle = 'Enrollment failed — try again';
                } else {
                    this._enrollProgressRow.subtitle = 'Press Start, then speak naturally';
                }
            }

            if (this._enrollStatusRow) {
                if (recording) {
                    this._enrollStatusRow.subtitle = enrollmentPrompt
                        || (reenrolling
                            ? `Re-enrolling — sample ${enrollSamples + 1}/3 (speak clearly)`
                            : `Recording sample ${enrollSamples + 1}/3 — speak clearly`);
                } else if (enrolled) {
                    this._enrollStatusRow.subtitle = 'Voice profile enrolled — press Start to re-enroll';
                } else if (enrollState === 'complete') {
                    this._enrollStatusRow.subtitle = 'Enrollment complete';
                } else if (enrollState === 'failed') {
                    this._enrollStatusRow.subtitle = 'Enrollment failed — ensure the mic is active and try again';
                } else if (!audioActive) {
                    this._enrollStatusRow.subtitle = 'Ready — microphone will start when you press Start';
                } else {
                    this._enrollStatusRow.subtitle = 'Not enrolled — press Start and speak for ~6 seconds total';
                }
            }

            if (this._enrollStartButton?.widget) {
                const canStart = modelsLoaded && enrollState !== 'recording';
                this._enrollStartButton.widget.sensitive = canStart;
                this._enrollStartButton.widget.set_label(
                    enrolled || reenrolling ? 'Re-enroll' : 'Start'
                );
            }

            if (this._applyHotwordButton?.widget) {
                this._applyHotwordButton.widget.sensitive = encodingReady && Boolean(hotword || this._settings.get_string('hotword').trim());
            }
        });
    }

    _getProxy() {
        return this._configManager?._proxy ?? null;
    }

    _refreshEnrollmentStatus() {
        this._refreshVoicePageStatus();
    }

    _startSpeakerEnrollment(window) {
        const proxy = this._getProxy();
        if (!proxy) {
            this._showToast(window, 'Willow service not connected');
            return;
        }

        if (this._enrollProgressRow) {
            this._enrollProgressRow.visible = true;
        }
        if (this._enrollProgressBar) {
            this._enrollProgressBar.fraction = 0;
            this._enrollProgressBar.text = '0/3';
        }
        if (this._enrollStatusRow) {
            this._enrollStatusRow.subtitle = 'Starting enrollment…';
        }

        proxy.StartSpeakerEnrollmentRemote((result, error) => {
            if (error) {
                this._showToast(window, `Enrollment failed: ${error}`, 8);
                this._refreshVoicePageStatus();
                return;
            }
            this._showToast(window, 'Enrollment started — follow the on-screen prompts and speak steadily', 6);
            this._refreshVoicePageStatus();
        });
    }

    _cancelSpeakerEnrollment(window) {
        const proxy = this._getProxy();
        if (!proxy) {
            return;
        }
        proxy.CancelSpeakerEnrollmentRemote(() => {
            this._showToast(window, 'Enrollment cancelled');
            this._refreshEnrollmentStatus();
        });
    }

    _removeSpeakerProfile(window) {
        const proxy = this._getProxy();
        if (!proxy) {
            this._showToast(window, 'Willow service not connected');
            return;
        }
        proxy.RemoveSpeakerProfileRemote(() => {
            this._showToast(window, 'Voice profile removed');
            this._refreshEnrollmentStatus();
        });
    }

    _createLogsPage(window, settings) {
        const page = new Adw.PreferencesPage({
            title: 'Logs',
            icon_name: 'text-x-generic-symbolic',
        });

        // Add log viewer group from LogViewer
        const logGroup = this._logViewer.createLogViewerGroup(window);
        page.add(logGroup);

        // Log configuration group
        const configGroup = this._prefsBuilder.createGroup(
            'Log Configuration',
            'Logging settings and information'
        );

        this._prefsBuilder.createInfoRow(
            'Log Location',
            'Service logs are written to /tmp/willow.log',
            configGroup
        );

        this._prefsBuilder.createInfoRow(
            'Systemd Journal',
            'Complete service logs (including stdout/stderr) are available via systemd journal',
            configGroup
        );

        this._prefsBuilder.createInfoRow(
            'Log Rotation',
            'Log files in /tmp are automatically cleared on system reboot',
            configGroup
        );

        page.add(configGroup);

        window.add(page);
    }

    _createModelsPage(window, settings) {
        const page = new Adw.PreferencesPage({
            title: 'Models',
            icon_name: 'folder-download-symbolic',
        });

        // Add model management group from WhisperModelManager
        const modelGroup = this._modelManager.createModelGroup(window);
        page.add(modelGroup);

        // Model information group
        const infoGroup = this._prefsBuilder.createGroup(
            'Model Information',
            'Understanding sherpa-onnx models for Willow speech pipeline'
        );

        this._prefsBuilder.createInfoRow(
            'Model Bundles',
            this._speakerVerificationEnabled
                ? 'KWS wakes Willow. Streaming ASR drives command partials and early fire. Whisper is for typing only.'
                : 'KWS wakes Willow. Streaming ASR drives command partials and early fire. Whisper is for typing only.',
            infoGroup
        );

        this._prefsBuilder.createInfoRow(
            'Download',
            'Run willow-download-model or use the Download button above. Restart the service after installing models.',
            infoGroup
        );

        this._prefsBuilder.createInfoRow(
            'Model Source',
            'Models are downloaded from the sherpa-onnx GitHub releases (k2-fsa/sherpa-onnx)',
            infoGroup
        );

        page.add(infoGroup);

        // Service restart info
        const restartGroup = this._prefsBuilder.createGroup(
            'Apply Changes',
            'Service must be restarted to use a different model'
        );

        this._prefsBuilder.createButtonRow(
            'Restart Service',
            'Restart the Willow service to apply model changes',
            'Restart Now',
            'view-refresh-symbolic',
            () => {
                try {
                    GLib.spawn_command_line_async('systemctl --user restart willow.service');
                    this._showToast(window, 'Restarting Willow service...');
                } catch (e) {
                    this._showToast(window, 'Failed to restart service');
                }
            },
            restartGroup
        );

        page.add(restartGroup);
        window.add(page);
    }

    
    _createCommandsPage(window, settings) {
        const page = new Adw.PreferencesPage({
            title: 'Commands',
            icon_name: 'utilities-terminal-symbolic',
        });

        // Statistics group
        const statsGroup = this._prefsBuilder.createGroup(
            'Command Statistics',
            'Overview of your current voice commands'
        );

        const stats = this._getCommandStats();
        this._prefsBuilder.createInfoRow(
            'Total Commands',
            `${stats.totalCommands} commands configured`,
            statsGroup
        );

        this._prefsBuilder.createInfoRow(
            'Total Phrases',
            `${stats.totalPhrases} voice phrases available`,
            statsGroup
        );

        page.add(statsGroup);

        // Commands management group
        const commandsGroup = this._commandManager.createCommandsGroup();
        page.add(commandsGroup);

        // Actions group
        const actionsGroup = this._prefsBuilder.createGroup(
            'Command Actions',
            'Manage your voice command configuration'
        );

        this._prefsBuilder.createButtonRow(
            'Reset to Defaults',
            'Restore all commands to default configuration',
            'Reset Commands',
            'edit-clear-all-symbolic',
            () => this._showResetConfirmation(window),
            actionsGroup
        );

        this._prefsBuilder.createButtonRow(
            'Open Config Directory',
            'Open configuration directory in file manager',
            'Open Directory',
            'folder-open-symbolic',
            () => this._openConfigDirectory(),
            actionsGroup
        );

        page.add(actionsGroup);
        window.add(page);
    }

    _getCommandStats() {
        const config = this._configManager.getConfig();
        const commands = config.commands || [];
        
        const totalCommands = commands.length;
        const totalPhrases = commands.reduce((sum, cmd) => {
            return sum + (cmd.phrases ? cmd.phrases.length : 0);
        }, 0);

        return {
            totalCommands,
            totalPhrases
        };
    }

    _createAboutPage(window, settings) {
        const page = new Adw.PreferencesPage({
            title: 'About',
            icon_name: 'help-info-symbolic',
        });

        // Info group
        const infoGroup = this._prefsBuilder.createGroup(
            'Willow Voice Assistant',
            'Offline voice control for GNOME using sherpa-onnx'
        );

        this._prefsBuilder.createInfoRow(
            'Features',
            'Offline sherpa-onnx speech pipeline • Real-time typing • D-Bus integration • Wayland ydotool support',
            infoGroup
        );

        this._prefsBuilder.createInfoRow(
            'Technology',
            'Sherpa-onnx speech pipeline • D-Bus service • Wayland ydotool support',
            infoGroup
        );

        this._prefsBuilder.createInfoRow(
            'Usage',
            'Say "hey willow" to activate • Streaming partial text in panel • Configure hotword in Voice tab',
            infoGroup
        );

        this._prefsBuilder.createInfoRow(
            'Mode Indicators',
            'Normal: Microphone (listening for hotword) • Command: Red pulsing (processing commands) • Typing: Keyboard (speech-to-text)',
            infoGroup
        );

        page.add(infoGroup);

        // D-Bus Service group
        const serviceGroup = this._prefsBuilder.createGroup(
            'D-Bus Service',
            'Information about the Willow D-Bus service'
        );

        this._prefsBuilder.createInfoRow(
            'Service Name',
            'com.github.saim.Willow',
            serviceGroup
        );

        this._prefsBuilder.createInfoRow(
            'Object Path',
            '/com/github/saim/VoiceAssistant',
            serviceGroup
        );

        this._prefsBuilder.createInfoRow(
            'Service Control',
            'Use systemctl --user {start|stop|restart|status} willow.service',
            serviceGroup
        );

        this._prefsBuilder.createInfoRow(
            'Smart Features',
            'Command mode supports smart open (any app) and smart search (configurable engines) via context.json',
            serviceGroup
        );

        page.add(serviceGroup);

        // Support group
        const supportGroup = this._prefsBuilder.createGroup(
            'Support & Resources',
            'Get help and additional information'
        );

        this._prefsBuilder.createButtonRow(
            'Documentation',
            'View the complete documentation and setup guide',
            'View Docs',
            'text-x-readme-symbolic',
            () => this._openDocumentation(),
            supportGroup
        );

        this._prefsBuilder.createButtonRow(
            'Report Issue',
            'Report a bug or request a new feature',
            'Report Issue',
            'bug-symbolic',
            () => this._openIssueTracker(),
            supportGroup
        );

        page.add(supportGroup);
        window.add(page);
    }

    // Helper methods
    _showResetConfirmation(window) {
        const dialog = new Gtk.MessageDialog({
            modal: true,
            transient_for: window,
            message_type: Gtk.MessageType.QUESTION,
            buttons: Gtk.ButtonsType.YES_NO,
            text: 'Reset Commands to Defaults?',
            secondary_text: 'This will restore all commands to their default configuration. Custom commands will be lost. This action cannot be undone.',
        });

        dialog.connect('response', (dialog, response) => {
            if (response === Gtk.ResponseType.YES) {
                this._configManager.saveConfig(this._configManager._getDefaultConfig());
                this._showToast(window, 'Commands reset to defaults');
            }
            dialog.close();
        });

        dialog.present();
    }

    _openConfigDirectory() {
        try {
            const configDir = GLib.get_home_dir() + '/.config/willow';
            GLib.spawn_command_line_async(`nautilus "${configDir}"`);
        } catch (e) {
            console.log('Could not open config directory');
        }
    }

    _openDocumentation() {
        try {
            GLib.spawn_command_line_async('xdg-open https://github.com/Saim20/willow');
        } catch (e) {
            console.log('Could not open documentation');
        }
    }

    _openIssueTracker() {
        try {
            GLib.spawn_command_line_async('xdg-open https://github.com/Saim20/willow/issues');
        } catch (e) {
            console.log('Could not open issue tracker');
        }
    }

    _showToast(window, message, timeoutSeconds = 3) {
        console.log(`Willow: ${message}`);

        try {
            const toast = new Adw.Toast({
                title: message,
                timeout: timeoutSeconds,
            });
            window.add_toast(toast);
        } catch (e) {
            console.log(`Toast: ${message}`);
        }
    }
}