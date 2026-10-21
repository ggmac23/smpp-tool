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

      this.__submitForm = new smppclient.SubmitForm();
      this.__submitForm.addListener("notice", e => shell.showNotice(e.getData()));
      shell.addPage("submit_sm", this.__submitForm);

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

    __shell: null,
    __submitForm: null
  }
});
