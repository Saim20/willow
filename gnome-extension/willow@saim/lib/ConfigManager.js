/**
 * ConfigManager.js - Configuration management utilities
 * Handles loading, saving, and synchronizing configuration with the D-Bus service
 * Config file syncs between GNOME extension preferences and Willow service
 */

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import {createVoiceAssistantProxy} from './DbusInterface.js';

export class ConfigManager {
    constructor(settings) {
        this._settings = settings;
        this._configPath = GLib.get_home_dir() + '/.config/willow/config.json';
        this._configFile = Gio.File.new_for_path(this._configPath);
        this._config = null;
        this._proxy = null;
        this._initDbusProxy();
    }

    /**
     * Initialize D-Bus proxy for live updates
     */
    _initDbusProxy() {
        try {
            this._proxy = createVoiceAssistantProxy((proxy, error) => {
                if (error) {
                    console.log(`ConfigManager: Could not connect to D-Bus service: ${error}`);
                }
            });
        } catch (e) {
            console.log(`ConfigManager: Could not connect to D-Bus service: ${e}`);
        }
    }

    /**
     * Notify D-Bus service of config changes
     */
    _notifyServiceConfigChanged(config) {
        if (!this._proxy) {
            return;
        }
        
        try {
            const configJson = JSON.stringify(config);
            this._proxy.UpdateConfigRemote(configJson, (result, error) => {
                if (error) {
                    console.log(`ConfigManager: Failed to notify service of config change: ${error}`);
                } else {
                    console.log('ConfigManager: Service notified of config change');
                }
            });
        } catch (e) {
            console.log(`ConfigManager: Error notifying service: ${e}`);
        }
    }

    /**
     * Load configuration from file
     */
    loadConfig() {
        try {
            if (this._configFile.query_exists(null)) {
                let [success, contents] = this._configFile.load_contents(null);
                if (success) {
                    this._config = JSON.parse(new TextDecoder().decode(contents));
                    // Filter out comment fields (they start with _)
                    this._config = this._filterComments(this._config);
                    return this._config;
                }
            }
            return this._getDefaultConfig();
        } catch (e) {
            console.log(`ConfigManager: Error loading config: ${e}`);
            return this._getDefaultConfig();
        }
    }

    /**
     * Save configuration to file
     */
    saveConfig(config, notifyService = true) {
        try {
            // Ensure parent directory exists
            const parentDir = this._configFile.get_parent();
            if (!parentDir.query_exists(null)) {
                parentDir.make_directory_with_parents(null);
            }

            // Read existing config to preserve comments
            let existingConfig = {};
            if (this._configFile.query_exists(null)) {
                try {
                    let [success, contents] = this._configFile.load_contents(null);
                    if (success) {
                        existingConfig = JSON.parse(new TextDecoder().decode(contents));
                    }
                } catch (e) {
                    console.log(`ConfigManager: Could not load existing config for comment preservation: ${e}`);
                }
            }

            // Merge: preserve comment fields from existing, update actual values from new config
            const mergedConfig = this._mergePreservingComments(existingConfig, config);

            const configJson = JSON.stringify(mergedConfig, null, 2);
            this._configFile.replace_contents(configJson, null, false, Gio.FileCreateFlags.NONE, null);
            this._config = config; // Store the clean version internally
            
            if (notifyService) {
                this._notifyServiceConfigChanged(config);
            }

            return true;
        } catch (e) {
            console.log(`ConfigManager: Error saving config: ${e}`);
            return false;
        }
    }

    /**
     * Get current config (load if needed)
     */
    getConfig() {
        if (!this._config) {
            this._config = this.loadConfig();
        }
        return this._config;
    }

    /**
     * Update specific config section
     */
    updateConfigSection(section, data) {
        const config = this.getConfig();
        config[section] = data;
        return this.saveConfig(config);
    }

    /**
     * Sync extension settings to config file
     */
    syncSettingsToConfig() {
        const config = this.getConfig();
        
        // Update basic settings
        config.hotword = this._settings.get_string('hotword');
        config.command_threshold = this._settings.get_int('command-threshold');

        // Persist locally only; push hotword via Apply button, threshold via SetConfigValue.
        return this.saveConfig(config, false);
    }

    /**
     * Sync config file to extension settings
     */
    syncConfigToSettings() {
        const config = this.getConfig();
        
        this._settings.set_string('hotword', config.hotword || 'hey willow');
        this._settings.set_int('command-threshold', config.command_threshold || 80);
    }

    /**
     * Apply config pushed from the D-Bus service without writing back to disk
     */
    applyConfigFromService(config) {
        if (!config) {
            return;
        }

        this._config = this._filterComments(config);
        this.syncConfigToSettings();
    }

    /**
     * Push the current config JSON to the running service (explicit user action).
     */
    pushConfigToService(callback) {
        if (!this._proxy) {
            callback(false, 'Service not connected');
            return;
        }

        const config = this.getConfig();
        const configJson = JSON.stringify(config);
        this._proxy.UpdateConfigRemote(configJson, (result, error) => {
            if (error) {
                callback(false, String(error));
                return;
            }
            callback(true, null);
        });
    }

    /**
     * Push hotword directly to the running service for immediate feedback.
     */
    applyHotwordToService(hotword, callback) {
        if (!this._proxy) {
            callback(false, 'Service not connected');
            return;
        }

        this._proxy.SetConfigValueRemote('hotword', new GLib.Variant('s', hotword), (result, error) => {
            if (error) {
                callback(false, String(error));
                return;
            }
            callback(true, null);
        });
    }

    /**
     * Load config from the running service via D-Bus
     */
    loadConfigFromService(callback) {
        if (!this._proxy) {
            callback(this.getConfig(), new Error('Service not connected'));
            return;
        }

        this._proxy.GetConfigRemote((result, error) => {
            if (error) {
                callback(this.getConfig(), error);
                return;
            }

            try {
                const config = JSON.parse(result[0]);
                this._config = this._filterComments(config);
                callback(this._config, null);
            } catch (e) {
                callback(this.getConfig(), e);
            }
        });
    }

    /**
     * Filter out comment fields from config (fields starting with _)
     */
    _filterComments(obj) {
        if (Array.isArray(obj)) {
            return obj.map(item => this._filterComments(item));
        } else if (obj !== null && typeof obj === 'object') {
            const filtered = {};
            for (const [key, value] of Object.entries(obj)) {
                // Skip fields starting with underscore (comments)
                if (!key.startsWith('_')) {
                    filtered[key] = this._filterComments(value);
                }
            }
            return filtered;
        }
        return obj;
    }

    /**
     * Merge new config with existing, preserving comment fields
     */
    _mergePreservingComments(existing, updated) {
        if (Array.isArray(updated)) {
            // For arrays (like commands), just return the updated array
            return updated;
        } else if (updated !== null && typeof updated === 'object') {
            const merged = {};
            
            // First, copy all comment fields from existing
            for (const [key, value] of Object.entries(existing)) {
                if (key.startsWith('_')) {
                    merged[key] = value;
                }
            }
            
            // Then, copy all non-comment fields from updated
            for (const [key, value] of Object.entries(updated)) {
                if (!key.startsWith('_')) {
                    if (typeof value === 'object' && value !== null && !Array.isArray(value)) {
                        // Recursively merge objects
                        merged[key] = this._mergePreservingComments(existing[key] || {}, value);
                    } else {
                        merged[key] = value;
                    }
                }
            }
            
            return merged;
        }
        return updated;
    }

    /**
     * Get default configuration structure
     */
    _getDefaultConfig() {
        return {
            "hotword": "hey willow",
            "command_threshold": 80,
            "commands": [],
            "typing_mode": {
                "exit_phrases": [
                    "stop typing",
                    "exit typing",
                    "normal mode",
                    "go to normal mode"
                ],
                "check_recent_chars": 100
            }
        };
    }
}
