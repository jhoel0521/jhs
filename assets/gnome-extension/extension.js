// jhs: combo de desbloqueo para el modo privacidad.
//
// Mientras la sesión está bloqueada, escucha las teclas en la pantalla de
// bloqueo (en `Main.uiGroup`, que es donde la pantalla de bloqueo hace su grab
// modal; en `global.stage` no llegan). Si se pulsa el combo de `~/.config/jhs/unlock-combo.json`, avisa al
// agente de jhs por su socket (`$XDG_RUNTIME_DIR/jhs.sock`). El agente decide:
// solo desbloquea si el bloqueo lo activó jhs (modo privacidad); un bloqueo
// normal (Super+L) sigue pidiendo la contraseña. La extensión no desbloquea
// nada por sí misma.

import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const COMBO_FILE = GLib.build_filenamev([GLib.get_user_config_dir(), 'jhs', 'unlock-combo.json']);
const SOCKET = GLib.build_filenamev([GLib.get_user_runtime_dir(), 'jhs.sock']);

export default class JhsUnlockExtension extends Extension {
    enable() {
        this._combo = null;
        this._eventId = 0;
        this._loadCombo();
        this._file = Gio.File.new_for_path(COMBO_FILE);
        this._monitor = this._file.monitor_file(Gio.FileMonitorFlags.NONE, null);
        this._monitorId = this._monitor.connect('changed', () => this._loadCombo());
        this._sessionId = Main.sessionMode.connect('updated', () => this._sync());
        this._sync();
    }

    disable() {
        this._stopListening();
        if (this._sessionId) {
            Main.sessionMode.disconnect(this._sessionId);
            this._sessionId = 0;
        }
        if (this._monitor) {
            this._monitor.disconnect(this._monitorId);
            this._monitor.cancel();
            this._monitor = null;
        }
        this._file = null;
        this._combo = null;
    }

    _loadCombo() {
        try {
            const [, bytes] = GLib.file_get_contents(COMBO_FILE);
            const c = JSON.parse(new TextDecoder().decode(bytes));
            this._combo = c && typeof c.keyval === 'number' ? c : null;
        } catch (e) {
            this._combo = null;
        }
    }

    _sync() {
        if (Main.sessionMode.isLocked)
            this._startListening();
        else
            this._stopListening();
    }

    _startListening() {
        if (this._eventId)
            return;
        this._grabActor = Main.uiGroup;
        this._eventId = this._grabActor.connect('captured-event', (_actor, event) => this._onEvent(event));
    }

    _stopListening() {
        if (!this._eventId)
            return;
        this._grabActor.disconnect(this._eventId);
        this._grabActor = null;
        this._eventId = 0;
    }

    _onEvent(event) {
        if (!this._combo || event.type() !== Clutter.EventType.KEY_PRESS)
            return Clutter.EVENT_PROPAGATE;
        let sym = event.get_key_symbol();
        if (sym >= 0x41 && sym <= 0x5a) // A-Z → a-z (con Shift llega en mayúscula)
            sym += 0x20;
        const s = event.get_state();
        const M = Clutter.ModifierType;
        const pressed = {
            ctrl: (s & M.CONTROL_MASK) !== 0,
            shift: (s & M.SHIFT_MASK) !== 0,
            alt: (s & M.MOD1_MASK) !== 0,
            super: (s & (M.SUPER_MASK | M.MOD4_MASK)) !== 0,
        };
        const c = this._combo;
        if (sym !== c.keyval || pressed.ctrl !== !!c.ctrl || pressed.shift !== !!c.shift ||
            pressed.alt !== !!c.alt || pressed.super !== !!c.super)
            return Clutter.EVENT_PROPAGATE;
        this._notifyAgent();
        return Clutter.EVENT_STOP;
    }

    _notifyAgent() {
        const client = new Gio.SocketClient();
        const address = Gio.UnixSocketAddress.new(SOCKET);
        client.connect_async(address, null, (_c, res) => {
            try {
                const conn = client.connect_finish(res);
                const msg = new TextEncoder().encode('{"cmd":"privacy_combo"}\n');
                conn.get_output_stream().write_all(msg, null);
                // Espera la respuesta para que el agente no vea un cliente cortado.
                const input = new Gio.DataInputStream({base_stream: conn.get_input_stream()});
                input.read_line_async(GLib.PRIORITY_DEFAULT, null, () => conn.close(null));
            } catch (e) {
                console.warn(`jhs: no se pudo avisar al agente: ${e.message}`);
            }
        });
    }
}
