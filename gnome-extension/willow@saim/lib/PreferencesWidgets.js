/**
 * PreferencesWidgets.js - Reusable preference widgets
 * Common UI components for building preference pages
 */

import Adw from 'gi://Adw';
import Gtk from 'gi://Gtk';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

export class PreferencesBuilder {
    constructor(settings) {
        this._settings = settings;
    }

    /**
     * Create a basic preferences group
     */
    createGroup(title, description = null) {
        return new Adw.PreferencesGroup({
            title: title,
            description: description,
        });
    }

    /**
     * Set sync callback for automatic config sync
     */
    setSyncCallback(callback) {
        this._syncCallback = callback;
    }

    /**
     * Create a switch row
     */
    createSwitchRow(title, subtitle, settingKey, group) {
        const row = new Adw.ActionRow({
            title: title,
            subtitle: subtitle,
        });

        const switchWidget = new Gtk.Switch({
            active: this._settings.get_boolean(settingKey),
            valign: Gtk.Align.CENTER,
        });

        this._settings.bind(settingKey, switchWidget, 'active', Gio.SettingsBindFlags.DEFAULT);
        
        // Trigger sync when changed
        if (this._syncCallback) {
            this._settings.connect(`changed::${settingKey}`, () => {
                this._syncCallback();
            });
        }
        
        row.add_suffix(switchWidget);
        group.add(row);

        return { row, widget: switchWidget };
    }

    /**
     * Create an entry row
     */
    createEntryRow(title, subtitle, settingKey, placeholder, group, options = {}) {
        const syncOnChange = options.syncOnChange !== false;
        const row = new Adw.ActionRow({
            title: title,
            subtitle: subtitle,
        });

        const entry = new Gtk.Entry({
            text: this._settings.get_string(settingKey),
            placeholder_text: placeholder,
            valign: Gtk.Align.CENTER,
        });

        this._settings.bind(settingKey, entry, 'text', Gio.SettingsBindFlags.DEFAULT);
        
        // Trigger sync when changed (optional — hotword uses Apply instead)
        if (this._syncCallback && syncOnChange) {
            this._settings.connect(`changed::${settingKey}`, () => {
                this._syncCallback();
            });
        }
        
        row.add_suffix(entry);
        group.add(row);

        return { row, widget: entry };
    }

    /**
     * Create a spin button row for integers
     */
    createSpinButtonRow(title, subtitle, settingKey, min, max, step, group) {
        const row = new Adw.ActionRow({
            title: title,
            subtitle: subtitle,
        });

        const spinButton = new Gtk.SpinButton({
            adjustment: new Gtk.Adjustment({
                lower: min,
                upper: max,
                step_increment: step,
                page_increment: step * 2,
                value: this._settings.get_int(settingKey),
            }),
            valign: Gtk.Align.CENTER,
        });

        this._settings.bind(settingKey, spinButton, 'value', Gio.SettingsBindFlags.DEFAULT);
        
        // Trigger sync when changed
        if (this._syncCallback) {
            this._settings.connect(`changed::${settingKey}`, () => {
                this._syncCallback();
            });
        }
        
        row.add_suffix(spinButton);
        group.add(row);

        return { row, widget: spinButton };
    }

    /**
     * Create a spin button row for doubles/floats
     */
    createDoubleSpinButtonRow(title, subtitle, settingKey, min, max, step, digits, group) {
        const row = new Adw.ActionRow({
            title: title,
            subtitle: subtitle,
        });

        const spinButton = new Gtk.SpinButton({
            adjustment: new Gtk.Adjustment({
                lower: min,
                upper: max,
                step_increment: step,
                page_increment: step * 2,
                value: this._settings.get_double(settingKey),
            }),
            digits: digits,
            valign: Gtk.Align.CENTER,
        });

        this._settings.bind(settingKey, spinButton, 'value', Gio.SettingsBindFlags.DEFAULT);
        
        // Trigger sync when changed
        if (this._syncCallback) {
            this._settings.connect(`changed::${settingKey}`, () => {
                this._syncCallback();
            });
        }
        
        row.add_suffix(spinButton);
        group.add(row);

        return { row, widget: spinButton };
    }

    /**
     * Create an info row (non-interactive)
     */
    createInfoRow(title, subtitle, group) {
        const row = new Adw.ActionRow({
            title: title,
            subtitle: subtitle,
        });
        group.add(row);
        return row;
    }

    /**
     * Create a button row
     */
    createButtonRow(title, subtitle, buttonText, buttonIcon, onClicked, group) {
        const row = new Adw.ActionRow({
            title: title,
            subtitle: subtitle,
        });

        const button = new Gtk.Button({
            label: buttonText,
            valign: Gtk.Align.CENTER,
        });

        if (buttonIcon) {
            button.set_icon_name(buttonIcon);
        }

        button.connect('clicked', onClicked);
        row.add_suffix(button);
        group.add(row);

        return { row, widget: button };
    }

    /**
     * Create a combo box row
     */
    createComboBoxRow(title, subtitle, options, settingKey, group) {
        const row = new Adw.ActionRow({
            title: title,
            subtitle: subtitle,
        });

        const comboBox = new Gtk.ComboBoxText({
            valign: Gtk.Align.CENTER,
        });

        // Add options
        for (const [value, label] of Object.entries(options)) {
            comboBox.append(value, label);
        }

        // Set current value
        const currentValue = this._settings.get_string(settingKey);
        comboBox.set_active_id(currentValue);

        // Connect to settings
        comboBox.connect('changed', () => {
            const activeId = comboBox.get_active_id();
            if (activeId) {
                this._settings.set_string(settingKey, activeId);
            }
        });

        this._settings.connect(`changed::${settingKey}`, () => {
            const newValue = this._settings.get_string(settingKey);
            comboBox.set_active_id(newValue);
        });

        row.add_suffix(comboBox);
        group.add(row);

        return { row, widget: comboBox };
    }
}

export class StatusManager {
    constructor() {
        this._configFile = null;
        this._lastModified = 0;
    }

    /**
     * Check if configuration file exists and is accessible
     */
    checkConfigStatus() {
        try {
            const configPath = GLib.get_home_dir() + '/.config/willow/config.json';
            this._configFile = Gio.File.new_for_path(configPath);
            
            if (this._configFile.query_exists(null)) {
                const info = this._configFile.query_info('time::modified', Gio.FileQueryInfoFlags.NONE, null);
                this._lastModified = info.get_modification_date_time().to_unix();
                return {
                    exists: true,
                    accessible: true,
                    lastModified: this._lastModified,
                    path: configPath,
                };
            } else {
                return {
                    exists: false,
                    accessible: false,
                    path: configPath,
                };
            }
        } catch (e) {
            return {
                exists: false,
                accessible: false,
                error: e.message,
            };
        }
    }

    /**
     * Get file modification time
     */
    getLastModified() {
        return this._lastModified;
    }

    /**
     * Format timestamp for display
     */
    formatTimestamp(timestamp) {
        const date = new Date(timestamp * 1000);
        return date.toLocaleString();
    }
}
