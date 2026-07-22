import GObject from 'gi://GObject';
import St from 'gi://St';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

import {ConfigManager} from './lib/ConfigManager.js';
import {createVoiceAssistantProxy} from './lib/DbusInterface.js';
import {ListeningOverlay} from './lib/ListeningOverlay.js';

const VoiceAssistantIndicator = GObject.registerClass(
class VoiceAssistantIndicator extends PanelMenu.Button {
    _init(settings) {
        super._init(0.0, 'Willow');
        
        this._box = new St.BoxLayout({
            style_class: 'panel-status-menu-box willow-indicator-box'
        });
        this.add_child(this._box);
        
        this._icon = new St.Icon({
            icon_name: 'microphone-sensitivity-medium-symbolic',
            style_class: 'system-status-icon willow-normal'
        });
        this._box.add_child(this._icon);
        
        this._currentMode = 'normal';
        this._currentBuffer = '';
        this._isRunning = false;
        this._modelsLoaded = false;
        this._bufferIsPartial = false;
        this._speakerEnrolled = false;
        this._workflowPrompt = '';
        this._enrollmentState = 'idle';
        this._enrollmentSamples = 0;
        this._enrollmentBufferFraction = 0;
        this._enrollmentReenrolling = false;
        this._speakerVerificationEnabled = false;
        this._enrollmentPrompt = '';
        this._activeHotword = '';
        this._audioActive = false;
        this._autoStartAttempted = false;
        this._dbusConnected = false;
        this._reconnectTimer = null;
        this._statusTimer = null;
        this._lastCommandPhrase = '';
        
        this._settings = settings;
        this._configManager = new ConfigManager(this._settings);
        this._listeningOverlay = new ListeningOverlay();
        
        this._setupDBus();
        this._setupMenu();
        this._setupSettingsHandlers();
        
        console.log('Willow: Extension initialized with D-Bus');
    }
    
    _setupDBus() {
        try {
            this._proxy = createVoiceAssistantProxy((proxy, error) => {
                if (error) {
                    console.error('Willow: D-Bus connection error:', error);
                    this._dbusConnected = false;
                    this._scheduleReconnect();
                    return;
                }
                
                this._onDBusConnected();
            });
        } catch (e) {
            console.error('Willow: Failed to create D-Bus proxy:', e);
            this._scheduleReconnect();
        }
    }

    _scheduleReconnect() {
        if (this._reconnectTimer) {
            return;
        }

        this._reconnectTimer = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, 5, () => {
            this._reconnectTimer = null;
            if (!this._dbusConnected) {
                console.log('Willow: Retrying D-Bus connection...');
                this._setupDBus();
            }
            return GLib.SOURCE_REMOVE;
        });
    }

    _clearReconnectTimer() {
        if (this._reconnectTimer) {
            GLib.source_remove(this._reconnectTimer);
            this._reconnectTimer = null;
        }
    }
    
    _onDBusConnected() {
        console.log('Willow: Connected to D-Bus service');
        this._dbusConnected = true;
        this._clearReconnectTimer();
        
        this._proxy.connectSignal('ModeChanged', (proxy, sender, [newMode, oldMode]) => {
            this._onModeChanged(newMode, oldMode);
        });
        
        this._proxy.connectSignal('BufferChanged', (proxy, sender, [buffer]) => {
            this._onBufferChanged(buffer);
        });

        this._proxy.connectSignal('PartialBufferChanged', (proxy, sender, [partial, isFinal]) => {
            this._onPartialBufferChanged(partial, isFinal);
        });

        this._proxy.connectSignal('CommandPending', (proxy, sender, [phrase, blocked]) => {
            this._onCommandPending(phrase, blocked);
        });

        this._proxy.connectSignal('SpeakerVerificationFailed', (proxy, sender, [reason]) => {
            console.warn('Willow: Speaker verification failed:', reason);
        });
        
        this._proxy.connectSignal('CommandExecuted', (proxy, sender, [command, phrase, confidence]) => {
            this._onCommandExecuted(command, phrase, confidence);
        });
        
        this._proxy.connectSignal('StatusChanged', (proxy, sender, [status]) => {
            this._onStatusChanged(status);
        });
        
        this._proxy.connectSignal('Error', (proxy, sender, [message, details]) => {
            this._onError(message, details);
        });
        
        // Notification signal kept for D-Bus compat; prompts go to HUD only (no banners).
        this._proxy.connectSignal('Notification', (proxy, sender, [title, message, _urgency]) => {
            this._onNotification(title, message);
        });

        this._proxy.connectSignal('ConfigChanged', (proxy, sender, [configJson]) => {
            this._onConfigChanged(configJson);
        });
        
        this._updateStatus();
        this._resetStatusTimer();
    }

    _resetStatusTimer() {
        if (this._statusTimer) {
            GLib.source_remove(this._statusTimer);
            this._statusTimer = null;
        }

        const interval = this._settings.get_int('update-interval') || 2;
        this._statusTimer = GLib.timeout_add_seconds(GLib.PRIORITY_DEFAULT, interval, () => {
            if (!this._dbusConnected) {
                return GLib.SOURCE_CONTINUE;
            }
            this._updateStatus();
            return GLib.SOURCE_CONTINUE;
        });
    }
    
    _setupMenu() {
        this._modeItem = new PopupMenu.PopupMenuItem(`Mode: NORMAL`, {
            reactive: false
        });
        this.menu.addMenuItem(this._modeItem);
        
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        
        this._serviceStatusItem = new PopupMenu.PopupMenuItem('Service: Checking...', {
            reactive: false
        });
        this.menu.addMenuItem(this._serviceStatusItem);
        
        this._startItem = new PopupMenu.PopupMenuItem('Start Service');
        this._startItem.connect('activate', () => this._startService());
        this.menu.addMenuItem(this._startItem);

        this._stopItem = new PopupMenu.PopupMenuItem('Stop Service');
        this._stopItem.connect('activate', () => this._stopService());
        this.menu.addMenuItem(this._stopItem);

        this._restartItem = new PopupMenu.PopupMenuItem('Restart Service');
        this._restartItem.connect('activate', () => this._restartService());
        this.menu.addMenuItem(this._restartItem);

        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        
        this._normalModeItem = new PopupMenu.PopupMenuItem('Switch to Normal Mode');
        this._normalModeItem.connect('activate', () => this._setMode('normal'));
        this.menu.addMenuItem(this._normalModeItem);
        
        this._commandModeItem = new PopupMenu.PopupMenuItem('Switch to Command Mode');
        this._commandModeItem.connect('activate', () => this._setMode('command'));
        this.menu.addMenuItem(this._commandModeItem);
        
        this._typingModeItem = new PopupMenu.PopupMenuItem('Switch to Typing Mode');
        this._typingModeItem.connect('activate', () => this._setMode('typing'));
        this.menu.addMenuItem(this._typingModeItem);
        
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        
        this._bufferItem = new PopupMenu.PopupMenuItem('Buffer: (empty)', {
            reactive: false
        });
        this.menu.addMenuItem(this._bufferItem);

        this._lastCommandItem = new PopupMenu.PopupMenuItem('Last command: (none)', {
            reactive: false
        });
        this.menu.addMenuItem(this._lastCommandItem);
        
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        
        this._smartInfoItem = new PopupMenu.PopupMenuItem('Smart: Say "open [app]" or "search [engine] for [query]"', {
            reactive: false,
            style_class: 'willow-smart-info'
        });
        this.menu.addMenuItem(this._smartInfoItem);
        this._smartInfoItem.visible = false;
        
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        this._speakerSeparator = new PopupMenu.PopupSeparatorMenuItem();
        this.menu.addMenuItem(this._speakerSeparator);

        this._speakerStatusItem = new PopupMenu.PopupMenuItem('Voice: Not enrolled', {
            reactive: false
        });
        this.menu.addMenuItem(this._speakerStatusItem);

        this._enrollItem = new PopupMenu.PopupMenuItem('Enroll Voice Profile');
        this._enrollItem.connect('activate', () => this._startSpeakerEnrollment());
        this.menu.addMenuItem(this._enrollItem);

        this._updateSpeakerVerificationVisibility();
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        
        this._prefsItem = new PopupMenu.PopupMenuItem('Preferences');
        this._prefsItem.connect('activate', () => {
            try {
                GLib.spawn_command_line_async('gnome-extensions prefs willow@saim');
            } catch (e) {
                console.error('Willow: Error opening preferences:', e);
            }
        });
        this.menu.addMenuItem(this._prefsItem);
    }
    
    _setupSettingsHandlers() {
        const syncableKeys = [
            'command-threshold',
            'gpu-acceleration',
            'command-endpoint-silence',
            'workflow-session-timeout',
            'early-fire',
            'llm-enabled',
            'llm-model-path',
            'llm-max-tokens',
            'llm-timeout-ms',
            'typing-auto-revert',
        ];
        
        syncableKeys.forEach(key => {
            this._settings.connect(`changed::${key}`, () => {
                this._syncSettingsToService();
            });
        });

        this._settings.connect('changed::update-interval', () => {
            this._resetStatusTimer();
        });
    }
    
    _syncSettingsToService() {
        if (!this._proxy || !this._dbusConnected) return;
        
        try {
            const threshold = this._settings.get_int('command-threshold') / 100.0;
            this._proxy.SetConfigValueRemote('command_threshold', new GLib.Variant('d', threshold));
            const provider = this._settings.get_boolean('gpu-acceleration') ? 'auto' : 'cpu';
            this._proxy.SetConfigValueRemote('inference.provider', new GLib.Variant('s', provider));
            const endpoint = this._settings.get_double('command-endpoint-silence');
            this._proxy.SetConfigValueRemote(
                'command_mode.endpoint_silence',
                new GLib.Variant('d', endpoint)
            );
            this._proxy.SetConfigValueRemote(
                'workflows.session_timeout',
                new GLib.Variant('d', this._settings.get_double('workflow-session-timeout'))
            );
            this._proxy.SetConfigValueRemote(
                'intent.early_fire',
                new GLib.Variant('b', this._settings.get_boolean('early-fire'))
            );
            const llmEnabled = this._settings.get_boolean('llm-enabled');
            this._proxy.SetConfigValueRemote(
                'intent.llm_fallback',
                new GLib.Variant('b', llmEnabled)
            );
            this._proxy.SetConfigValueRemote(
                'inference.llm.enabled',
                new GLib.Variant('b', llmEnabled)
            );
            this._proxy.SetConfigValueRemote(
                'inference.llm.model_path',
                new GLib.Variant('s', this._settings.get_string('llm-model-path'))
            );
            this._proxy.SetConfigValueRemote(
                'inference.llm.max_tokens',
                new GLib.Variant('i', this._settings.get_int('llm-max-tokens'))
            );
            this._proxy.SetConfigValueRemote(
                'inference.llm.timeout_ms',
                new GLib.Variant('i', this._settings.get_int('llm-timeout-ms'))
            );
            this._proxy.SetConfigValueRemote(
                'typing_mode.auto_revert',
                new GLib.Variant('b', this._settings.get_boolean('typing-auto-revert'))
            );
            console.log('Willow: Settings synced to service');
        } catch (e) {
            console.error('Willow: Error syncing settings:', e);
        }
    }
    
    _setMode(mode) {
        if (!this._proxy || !this._dbusConnected) {
            console.error('Willow: Service not connected');
            return;
        }
        
        try {
            this._proxy.SetModeRemote(mode, (result, error) => {
                if (error) {
                    console.error('Willow: SetMode error:', error);
                }
            });
        } catch (e) {
            console.error('Willow: SetMode exception:', e);
        }
    }
    
    _startService() {
        if (!this._proxy || !this._dbusConnected) {
            console.error('Willow: Service not connected');
            return;
        }
        
        try {
            this._proxy.StartRemote((result, error) => {
                if (error) {
                    console.error('Willow: Start error:', error);
                }
            });
        } catch (e) {
            console.error('Willow: Start exception:', e);
        }
    }
    
    _stopService() {
        if (!this._proxy || !this._dbusConnected) return;
        
        try {
            this._proxy.StopRemote((result, error) => {
                if (error) {
                    console.error('Willow: Stop error:', error);
                }
            });
        } catch (e) {
            console.error('Willow: Stop exception:', e);
        }
    }
    
    _restartService() {
        if (!this._proxy || !this._dbusConnected) return;
        
        try {
            this._proxy.RestartRemote((result, error) => {
                if (error) {
                    console.error('Willow: Restart error:', error);
                }
            });
        } catch (e) {
            console.error('Willow: Restart exception:', e);
        }
    }

    _updateSpeakerVerificationVisibility() {
        const visible = this._speakerVerificationEnabled;
        if (this._speakerSeparator) {
            this._speakerSeparator.visible = visible;
        }
        if (this._speakerStatusItem) {
            this._speakerStatusItem.visible = visible;
        }
        if (this._enrollItem) {
            this._enrollItem.visible = visible;
        }
    }

    _startSpeakerEnrollment() {
        if (!this._proxy || !this._dbusConnected) {
            console.error('Willow: Service not connected');
            return;
        }
        try {
            this._proxy.StartSpeakerEnrollmentRemote((result, error) => {
                if (error) {
                    console.error('Willow: StartSpeakerEnrollment error:', error);
                }
            });
        } catch (e) {
            console.error('Willow: StartSpeakerEnrollment exception:', e);
        }
    }
    
    _updateStatus() {
        if (!this._proxy || !this._dbusConnected) return;
        
        try {
            this._proxy.GetStatusRemote((result, error) => {
                if (error) {
                    console.error('Willow: GetStatus error:', error);
                    this._dbusConnected = false;
                    this._isRunning = false;
                    this._modelsLoaded = false;
                    if (this._serviceStatusItem) {
                        this._serviceStatusItem.label.text = 'Service: Not connected';
                    }
                    this._scheduleReconnect();
                    return;
                }
                
                if (result && result[0]) {
                    this._onStatusChanged(result[0]);
                }
            });
        } catch (e) {
            console.error('Willow: GetStatus exception:', e);
        }
    }
    
    _onModeChanged(newMode, oldMode) {
        this._currentMode = newMode;
        if (newMode !== 'command') {
            this._workflowPrompt = '';
        }
        this._updateDisplay();
        console.log(`Willow: Mode changed from ${oldMode} to ${newMode}`);
    }
    
    _onPartialBufferChanged(partial, isFinal) {
        this._currentBuffer = partial;
        this._bufferIsPartial = !isFinal;
        this._updateDisplay();
    }

    _onCommandPending(phrase, blocked) {
        if (blocked) {
            this._currentBuffer = `${phrase} (waiting…)`;
            if (phrase && !this._workflowPrompt) {
                this._workflowPrompt = phrase;
            }
        } else if (phrase) {
            this._currentBuffer = phrase;
        }
        this._updateDisplay();
    }

    _onBufferChanged(buffer) {
        this._currentBuffer = buffer;
        this._updateDisplay();
    }
    
    _onCommandExecuted(command, phrase, confidence) {
        this._lastCommandPhrase = phrase || command;
        this._workflowPrompt = '';
        console.log(`Willow: Command executed: ${phrase} (${(confidence * 100).toFixed(1)}%)`);
        this._updateDisplay();
    }
    
    _onStatusChanged(status) {
        if (status.is_running !== undefined) {
            this._isRunning = status.is_running.unpack();
        }
        if (status.audio_active !== undefined) {
            this._audioActive = status.audio_active.unpack();
        }
        if (status.models_loaded !== undefined) {
            this._modelsLoaded = status.models_loaded.unpack();
        } else if (status.whisper_loaded !== undefined) {
            this._modelsLoaded = status.whisper_loaded.unpack();
        }
        if (status.speaker_enrolled !== undefined) {
            this._speakerEnrolled = status.speaker_enrolled.unpack();
        }
        if (status.enrollment_state !== undefined) {
            this._enrollmentState = status.enrollment_state.unpack();
        }
        if (status.enrollment_samples !== undefined) {
            this._enrollmentSamples = status.enrollment_samples.unpack();
        }
        if (status.enrollment_buffer_fraction !== undefined) {
            this._enrollmentBufferFraction = status.enrollment_buffer_fraction.unpack();
        }
        if (status.enrollment_reenrolling !== undefined) {
            this._enrollmentReenrolling = status.enrollment_reenrolling.unpack();
        }
        if (status.speaker_verification_enabled !== undefined) {
            this._speakerVerificationEnabled = status.speaker_verification_enabled.unpack();
        }
        if (status.enrollment_prompt !== undefined) {
            this._enrollmentPrompt = status.enrollment_prompt.unpack();
        }
        if (status.hotword !== undefined) {
            this._activeHotword = status.hotword.unpack();
        }
        if (status.current_mode !== undefined) {
            this._currentMode = status.current_mode.unpack();
        }
        if (status.current_buffer !== undefined) {
            this._currentBuffer = status.current_buffer.unpack();
        }
        if (status.workflow_prompt !== undefined) {
            this._workflowPrompt = status.workflow_prompt.unpack();
        }

        this._maybeAutoStart();
        this._updateDisplay();
    }

    _onConfigChanged(configJson) {
        try {
            const config = JSON.parse(configJson);
            this._configManager.applyConfigFromService(config);
        } catch (e) {
            console.error('Willow: Failed to apply config from service:', e);
        }
    }

    _maybeAutoStart() {
        const needsStart = this._modelsLoaded && !this._audioActive;
        if (!needsStart || this._autoStartAttempted) {
            return;
        }

        this._autoStartAttempted = true;
        console.log('Willow: Auto-starting voice assistant');
        this._startService();
    }
    
    _onError(message, details) {
        console.error('Willow:', message, details);
    }
    
    _onNotification(_title, _message) {
        // Notifications removed — prompts/transcripts go to the HUD only.
    }
    
    _updateDisplay() {
        let iconName = 'microphone-sensitivity-medium-symbolic';
        let modeClass = 'willow-normal';

        if (!this._isRunning) {
            iconName = 'microphone-disabled-symbolic';
            modeClass = 'willow-stopped';
        } else if (!this._audioActive) {
            iconName = 'microphone-sensitivity-muted-symbolic';
            modeClass = 'willow-stopped';
        } else if (this._currentMode === 'command') {
            iconName = 'microphone-sensitivity-high-symbolic';
            modeClass = 'willow-command';
        } else if (this._currentMode === 'typing') {
            iconName = 'input-keyboard-symbolic';
            modeClass = 'willow-typing';
        }

        this._icon.icon_name = iconName;
        this._icon.style_class = `system-status-icon ${modeClass}`;
        
        if (this._smartInfoItem) {
            this._smartInfoItem.visible = (this._currentMode === 'command');
        }
        
        if (this._modeItem) {
            this._modeItem.label.text = `Mode: ${this._currentMode.toUpperCase()}`;
        }
        
        if (this._bufferItem) {
            this._bufferItem.label.text = this._currentBuffer 
                ? `Heard: ${this._currentBuffer}` 
                : 'Heard: (empty)';
        }

        if (this._lastCommandItem) {
            this._lastCommandItem.label.text = this._lastCommandPhrase
                ? `Last command: ${this._lastCommandPhrase}`
                : 'Last command: (none)';
        }
        
        if (this._serviceStatusItem) {
            if (this._audioActive) {
                this._serviceStatusItem.label.text = 'Service: Listening';
            } else if (!this._modelsLoaded) {
                this._serviceStatusItem.label.text = 'Service: No model (run willow-download-model)';
            } else {
                this._serviceStatusItem.label.text = 'Service: Waiting for audio...';
            }
        }
        
        if (this._startItem) this._startItem.setSensitive(!this._audioActive);
        if (this._stopItem) this._stopItem.setSensitive(this._audioActive);
        if (this._restartItem) this._restartItem.setSensitive(this._modelsLoaded);

        this._updateSpeakerVerificationVisibility();

        if (this._speakerStatusItem && this._speakerVerificationEnabled) {
            if (this._enrollmentState === 'recording') {
                const next = this._enrollmentSamples + 1;
                const within = Math.round(Math.min(this._enrollmentBufferFraction, 1) * 100);
                this._speakerStatusItem.label.text = this._enrollmentReenrolling
                    ? `Voice: Re-enrolling ${next}/3 (${within}%)`
                    : `Voice: Enrolling ${next}/3 (${within}%)`;
                if (this._enrollmentPrompt) {
                    this._speakerStatusItem.label.text += ` — ${this._enrollmentPrompt}`;
                }
            } else if (this._speakerEnrolled) {
                this._speakerStatusItem.label.text = 'Voice: Enrolled';
            } else if (this._enrollmentState === 'failed') {
                this._speakerStatusItem.label.text = 'Voice: Enrollment failed';
            } else {
                this._speakerStatusItem.label.text = 'Voice: Not enrolled';
            }
        }
        if (this._enrollItem && this._speakerVerificationEnabled) {
            this._enrollItem.label.text = this._speakerEnrolled
                ? 'Re-enroll Voice Profile'
                : 'Enroll Voice Profile';
            this._enrollItem.setSensitive(this._modelsLoaded &&
                this._enrollmentState !== 'recording');
        }

        this._syncListeningOverlay();
    }

    _syncListeningOverlay() {
        if (!this._listeningOverlay) {
            return;
        }
        // Show for whole Command session (HUD replaces the old panel buffer text).
        if (this._currentMode === 'command') {
            try {
                this._listeningOverlay.show();
                this._listeningOverlay.setContent({
                    prompt: this._workflowPrompt || '',
                    transcript: this._currentBuffer || 'Listening…',
                });
            } catch (e) {
                console.error('Willow: ListeningOverlay error:', e);
            }
        } else {
            this._listeningOverlay.hide();
        }
    }
    
    destroy() {
        if (this._listeningOverlay) {
            this._listeningOverlay.destroy();
            this._listeningOverlay = null;
        }
        if (this._statusTimer) {
            GLib.source_remove(this._statusTimer);
            this._statusTimer = null;
        }

        this._clearReconnectTimer();
        
        if (this._proxy) {
            this._proxy = null;
        }

        this._dbusConnected = false;
        
        super.destroy();
    }
});

export default class VoiceAssistantExtension extends Extension {
    constructor(metadata) {
        super(metadata);
        this._indicator = null;
    }

    enable() {
        console.log('Willow: Enabling extension');
        const settings = this.getSettings();
        this._indicator = new VoiceAssistantIndicator(settings);
        Main.panel.addToStatusArea('willow', this._indicator);
    }

    disable() {
        console.log('Willow: Disabling extension');
        if (this._indicator) {
            this._indicator.destroy();
            this._indicator = null;
        }
    }
}
