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

      const shell = new smppclient.Shell();
      this.getRoot().add(shell, { edge: 0 });
      this.__shell = shell;

      this.__panels = {};
      const attach = (label, widget) => {
        widget.addListener("notice", e => shell.showNotice(e.getData()));
        shell.addPage(label, widget);
        return widget;
      };

      this.__submitForm = attach("submit_sm", new smppclient.SubmitForm());
      this.__panels.dataSm = attach("data_sm", new smppclient.DataSmForm());
      this.__panels.messageOps = attach("query / cancel / replace", new smppclient.MessageOpsForm());
      this.__panels.multi = attach("submit_multi", new smppclient.MultiForm());

      this.__trace = new smppclient.TracePanel();
      this.__trace.addListener("notice", e => shell.showNotice(e.getData()));
      shell.getTraceSlot().add(this.__trace);

      // The flow diagram labels the far end with whatever we are bound to.
      const connection = shell.getConnectionPanel();
      connection.addListener("bound", () => {
        const values = connection.getFormValues();
        this.__trace.setPeer(values.host + ":" + values.port);
      });

      if (!smppclient.Api.getInstance().available()) {
        shell.showNotice(
          "Running outside the app: the main process is not reachable, so nothing can be sent."
        );
      }
    },

    /** The shell, for later panels to attach to. */
    getShell() {
      return this.__shell;
    },

    /** The submit_sm form. */
    getSubmitForm() {
      return this.__submitForm;
    },

    /** The other PDU panels, by name. */
    getPanels() {
      return this.__panels;
    },

    /** The PDU trace panel. */
    getTracePanel() {
      return this.__trace;
    },

    __shell: null,
    __submitForm: null,
    __panels: null,
    __trace: null
  }
});
