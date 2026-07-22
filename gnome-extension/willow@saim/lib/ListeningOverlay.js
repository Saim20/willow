/**
 * Siri-style floating listening HUD for Command mode.
 * Non-closable; clicks pass through.
 */

import Clutter from 'gi://Clutter';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

export class ListeningOverlay {
    constructor() {
        this._visible = false;
        this._card = null;
        this._promptLabel = null;
        this._transcriptLabel = null;
        this._monitorsChangedId = 0;
    }

    show() {
        if (!this._card) {
            this._build();
        }
        if (!this._visible) {
            // Direct top-chrome card (same pattern as other Shell OSDs / color-picker).
            Main.layoutManager.addTopChrome(this._card, {
                affectsInputRegion: false,
                affectsStruts: false,
            });
            this._visible = true;
        }
        this._card.visible = true;
        this._card.opacity = 255;
        this._relayout();
        const parent = this._card.get_parent();
        if (parent) {
            parent.set_child_above_sibling(this._card, null);
        }
    }

    hide() {
        if (!this._visible || !this._card) {
            return;
        }
        Main.layoutManager.removeChrome(this._card);
        this._visible = false;
    }

    destroy() {
        this.hide();
        if (this._monitorsChangedId) {
            Main.layoutManager.disconnect(this._monitorsChangedId);
            this._monitorsChangedId = 0;
        }
        if (this._card) {
            this._card.destroy();
            this._card = null;
        }
        this._promptLabel = null;
        this._transcriptLabel = null;
    }

    setContent({prompt = '', transcript = ''} = {}) {
        if (!this._card) {
            this._build();
        }
        const hasPrompt = Boolean(prompt);
        this._promptLabel.visible = hasPrompt;
        this._promptLabel.text = prompt || '';
        this._transcriptLabel.text = transcript || 'Listening…';
        if (this._visible) {
            this._relayout();
        }
    }

    _build() {
        // Inline styles so the HUD stays visible even if stylesheet reload lags.
        this._card = new St.BoxLayout({
            name: 'willow-listening-overlay',
            style_class: 'willow-listening-card',
            style: 'background-color: rgba(20, 22, 28, 0.94); border-radius: 28px; padding: 28px 40px; min-width: 280px;',
            vertical: true,
            reactive: false,
            can_focus: false,
            track_hover: false,
            x_expand: false,
            y_expand: false,
        });

        const title = new St.Label({
            text: 'Willow',
            style_class: 'willow-listening-title',
            style: 'font-size: 28px; font-weight: 700; color: #f4f4f5;',
            x_align: Clutter.ActorAlign.CENTER,
        });
        this._promptLabel = new St.Label({
            text: '',
            style_class: 'willow-listening-prompt',
            style: 'font-size: 15px; color: #a1a1aa; margin-top: 8px;',
            visible: false,
            x_align: Clutter.ActorAlign.CENTER,
        });
        this._transcriptLabel = new St.Label({
            text: 'Listening…',
            style_class: 'willow-listening-transcript',
            style: 'font-size: 18px; color: #fafafa; margin-top: 14px; font-style: italic;',
            x_align: Clutter.ActorAlign.CENTER,
        });

        this._card.add_child(title);
        this._card.add_child(this._promptLabel);
        this._card.add_child(this._transcriptLabel);

        this._monitorsChangedId = Main.layoutManager.connect('monitors-changed', () => {
            if (this._visible) {
                this._relayout();
            }
        });
    }

    _relayout() {
        if (!this._card) {
            return;
        }
        const monitor = Main.layoutManager.primaryMonitor;
        if (!monitor) {
            return;
        }

        this._card.queue_relayout();
        const [, natW] = this._card.get_preferred_width(-1);
        const [, natH] = this._card.get_preferred_height(Math.max(natW, 280));
        const cardW = Math.max(natW, 280);
        const cardH = Math.max(natH, 96);
        this._card.set_size(cardW, cardH);

        // Absolute stage coordinates (monitor origin + inset).
        const x = Math.floor(monitor.x + (monitor.width - cardW) / 2);
        const y = Math.floor(monitor.y + monitor.height * 0.22);
        this._card.set_position(x, y);
    }
}
