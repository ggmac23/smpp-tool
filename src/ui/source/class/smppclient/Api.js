/**
 * The single point where the UI talks to the main process.
 *
 * No widget touches `window.smpp` directly: that keeps the bridge swappable
 * (a test or a browser-only run can install a stand-in) and means every
 * command goes through one place that reports failures the same way.
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

    /** Install a stand-in bridge. Used when running outside Electron. */
    setBridge(bridge) {
      this.__bridge = bridge;
    },

    bridge() {
      return this.__bridge || (typeof window !== "undefined" ? window.smpp : null);
    },

    available() {
      return !!this.bridge();
    },

    /**
     * Call a command. Rejects with an Error whose message came from the main
     * process; also fires `commandError` so a panel can show it without
     * every caller having to.
     */
    call(command, ...args) {
      const bridge = this.bridge();
      if (!bridge) {
        const err = new Error(
          "the main process is not reachable; this build must run inside the app"
        );
        this.fireDataEvent("commandError", { command: command, message: err.message });
        return Promise.reject(err);
      }
      return bridge[command](...args).catch(e => {
        this.fireDataEvent("commandError", { command: command, message: e.message });
        throw e;
      });
    },

    /** Subscribe to a main-process event. Returns an unsubscribe function. */
    on(event, callback) {
      const bridge = this.bridge();
      return bridge ? bridge.on(event, callback) : function () {};
    }
  }
});
