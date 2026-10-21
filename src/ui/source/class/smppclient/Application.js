/**
 * Main application class of the SMPP client.
 *
 * @asset(smppclient/*)
 */
qx.Class.define("smppclient.Application", {
  extend: qx.application.Standalone,

  members: {
    main() {
      super.main();

      if (qx.core.Environment.get("qx.debug")) {
        qx.log.appender.Native;
        qx.log.appender.Console;
      }

      this.getRoot().add(new qx.ui.basic.Label("SMPP Client"), {
        left: 20,
        top: 20
      });
    }
  }
});
