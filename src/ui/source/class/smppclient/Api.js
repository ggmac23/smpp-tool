/**
 * The single point where the UI talks to the Rust backend.
 *
 * No widget touches `window.__TAURI__` directly: that keeps the bridge
 * swappable (a test or a browser-only run can install a stand-in) and means
 * every command goes through one place that reports failures the same way.
 *
 * Arguments are named, not positional, because that is how Tauri's `invoke`
 * passes them: `call("trace", { from: 0, limit: 200 })`.
 *
 * The global is read with a string key on purpose. qooxdoo's build target
 * rewrites any identifier beginning with a double underscore into a private
 * member name, so `window.__TAURI__` compiles into `window.__P_180_1` and the
 * bridge is never found. A string key cannot be rewritten.
 */
qx.Class.define("smppclient.Api", {
  extend: qx.core.Object,
  type: "singleton",

  events: {
    /** Fired when a command fails. Data: { command, message }. */
    commandError: "qx.event.type.Data"
  },

  members: {
    __bridge: null,

    /**
     * Install a stand-in bridge: an object with `invoke(command, args)` and
     * `listen(event, callback)`. Used when running outside the app.
     */
    setBridge(bridge) {
      this.__bridge = bridge;
    },

    bridge() {
      if (this.__bridge) {
        return this.__bridge;
      }
      const tauri = typeof window !== "undefined" ? window["__TAURI__"] : null;
      if (!tauri || !tauri.core || !tauri.event) {
        return null;
      }
      return {
        invoke: (command, args) => tauri.core.invoke(command, args),
        listen: (event, callback) => tauri.event.listen(event, callback)
      };
    },

    available() {
      return !!this.bridge();
    },

    /**
     * Call a command with named arguments. Rejects with an Error whose
     * message came from the backend; also fires `commandError` so a panel can
     * show it without every caller having to.
     */
    call(command, args) {
      const bridge = this.bridge();
      if (!bridge) {
        const err = new Error(
          "the backend is not reachable; this build must run inside the app"
        );
        this.fireDataEvent("commandError", { command: command, message: err.message });
        return Promise.reject(err);
      }
      return bridge.invoke(command, args || {}).catch(e => {
        const message = e && e.message ? e.message : String(e);
        this.fireDataEvent("commandError", { command: command, message: message });
        throw new Error(message);
      });
    },

    /**
     * Subscribe to a backend event. Returns an unsubscribe function at once,
     * even though Tauri resolves the real one later: a caller must be able to
     * stop listening without having to await first.
     */
    on(event, callback) {
      const bridge = this.bridge();
      if (!bridge) {
        return function () {};
      }
      let stop = null;
      let cancelled = false;
      bridge
        .listen(event, e => callback(e && "payload" in e ? e.payload : e))
        .then(unlisten => {
          if (cancelled) {
            unlisten();
          } else {
            stop = unlisten;
          }
        })
        .catch(e => {
          this.fireDataEvent("commandError", {
            command: "listen:" + event,
            message: e && e.message ? e.message : String(e)
          });
        });

      return function () {
        cancelled = true;
        if (stop) {
          stop();
        }
      };
    }
  }
});
