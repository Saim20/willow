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
        this._modelManager = new WhisperModelManager(this._configManager);
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
            'Word used to activate command mode from normal mode',
            'hotword',
            'hey willow',
            recognitionGroup
        );

        this._prefsBuilder.createSpinButtonRow(
            'Command Threshold',
            'Minimum confidence percentage to execute commands (50-100%)',
            'command-threshold',
            50, 100, 5,
            recognitionGroup
        );

        page.add(recognitionGroup);

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
                this._showToast(window, 'Settings synced to D-Bus service');
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

        const verifyGroup = this._prefsBuilder.createGroup(
            'Speaker Verification',
            'Enroll your voice so only you can activate Willow after the hotword'
        );

        this._enrollStatusRow = this._prefsBuilder.createInfoRow(
            'Enrollment Status',
            'Checking…',
            verifyGroup
        );

        this._prefsBuilder.createInfoRow(
            'How it works',
            'After saying the hotword, Willow compares your voice to the enrolled profile. Speak naturally during enrollment — 3 short samples are collected automatically.',
            verifyGroup
        );

        this._prefsBuilder.createButtonRow(
            'Start Enrollment',
            'Record voice samples while the service is running',
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
            'Delete enrolled speaker data and disable verification',
            'Remove Profile',
            'user-trash-symbolic',
            () => this._removeSpeakerProfile(window),
            verifyGroup
        );

        page.add(verifyGroup);
        window.add(page);

        this._enrollPollId = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 2, () => {
            this._refreshEnrollmentStatus();
            return GLib.SOURCE_CONTINUE;
        });
        this._refreshEnrollmentStatus();
    }

    _getProxy() {
        return this._configManager?._proxy ?? null;
    }

    _refreshEnrollmentStatus() {
        const proxy = this._getProxy();
        if (!proxy || !this._enrollStatusRow) {
            return;
        }

        try {
            proxy.GetSpeakerEnrollmentStatusRemote((result, error) => {
                if (error || !result || !result[0]) {
                    this._enrollStatusRow.subtitle = 'Service not connected';
                    return;
                }
                const status = result[0];
                const state = status.state?.unpack?.() ?? 'idle';
                const samples = status.samples?.unpack?.() ?? 0;
                const enrolled = status.enrolled?.unpack?.() ?? false;

                if (enrolled) {
                    this._enrollStatusRow.subtitle = 'Voice profile enrolled';
                } else if (state === 'recording') {
                    this._enrollStatusRow.subtitle = `Recording sample ${samples}/3 — keep speaking naturally`;
                } else if (state === 'complete') {
                    this._enrollStatusRow.subtitle = 'Enrollment complete';
                } else if (state === 'failed') {
                    this._enrollStatusRow.subtitle = 'Enrollment failed — try again';
                } else {
                    this._enrollStatusRow.subtitle = 'Not enrolled';
                }
            });
        } catch (e) {
            this._enrollStatusRow.subtitle = 'Service not available';
        }
    }

    _startSpeakerEnrollment(window) {
        const proxy = this._getProxy();
        if (!proxy) {
            this._showToast(window, 'Willow service not connected');
            return;
        }
        proxy.StartSpeakerEnrollmentRemote((result, error) => {
            if (error) {
                this._showToast(window, `Enrollment failed: ${error}`);
                return;
            }
            this._showToast(window, 'Enrollment started — speak for a few seconds');
            this._refreshEnrollmentStatus();
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
            'KWS handles hotword detection in normal mode. Streaming ASR powers command and typing modes. Speaker model enables voice verification.',
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
            'Offline sherpa-onnx speech pipeline • Speaker verification • Real-time typing • D-Bus integration • Wayland ydotool support',
            infoGroup
        );

        this._prefsBuilder.createInfoRow(
            'Technology',
            'Sherpa-onnx speech pipeline • Speaker verification • D-Bus service • Wayland ydotool support',
            infoGroup
        );

        this._prefsBuilder.createInfoRow(
            'Usage',
            'Say "hey willow" to activate • Streaming partial text in panel • Enroll voice in Voice tab',
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

    _showToast(window, message) {
        // Simple console log for compatibility
        console.log(`Willow: ${message}`);
        
        // Try to show a toast if available
        try {
            const toast = new Adw.Toast({
                title: message,
                timeout: 3,
            });
            window.add_toast(toast);
        } catch (e) {
            // Fallback: just log
            console.log(`Toast: ${message}`);
        }
    }
}