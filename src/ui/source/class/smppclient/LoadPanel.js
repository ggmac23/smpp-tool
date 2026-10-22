/**
 * Load test: N connections, M messages, and the question of how much the
 * SMSC takes before it gives way.
 *
 * It opens its own connections rather than using the one being inspected: at
 * these rates a shared trace would be useless, and the trace is the one thing
 * that must stay readable. The connection and bind details therefore come
 * from the connection panel's form, not from a live session.
 *
 * The report leads with the sustained rate, which is the number the whole
 * exercise exists to produce. Requested, achieved and sustained stay apart:
 * asking for 500, managing 480 and holding 150 without errors are three
 * different facts, and only the last one answers "what does it take".
 *
 * @ignore(SmppForms)
 * @ignore(SmppForms.*)
 */
qx.Class.define("smppclient.LoadPanel", {
  extend: qx.ui.container.Composite,

  /**
   * @param connectionPanel {smppclient.ConnectionPanel} where the host, the
   *   TLS settings and the bind credentials come from
   * @param submitForm {smppclient.SubmitForm} where the message body comes from
   */
  construct(connectionPanel, submitForm) {
    // Tight: the table and the report are what the panel is for, and every
    // pixel the form takes is one they do not get.
    super(new qx.ui.layout.VBox(4));
    this.setPadding(6);

    this.__api = smppclient.Api.getInstance();
    this.__connectionPanel = connectionPanel;
    this.__submitForm = submitForm;
    this.__steps = [];

    this.add(this.__buildSettings());
    this.add(this.__buildActions());

    const split = new qx.ui.splitpane.Pane("horizontal");
    split.add(this.__buildTable(), 3);
    split.add(this.__buildReport(), 2);
    this.add(split, { flex: 1 });

    this.__api.on("load-progress", p => this.__onProgress(p));
    this.__api.on("load-step", s => this.__onStep(s));
    this.__api.on("load-done", r => this.__onDone(r));

    this.__refreshMode();
    this.__refreshLimits();
  },

  events: {
    notice: "qx.event.type.Data"
  },

  members: {
    __api: null,
    __connectionPanel: null,
    __submitForm: null,
    __fields: null,
    __table: null,
    __model: null,
    __reportList: null,
    __stepList: null,
    __progress: null,
    __startButton: null,
    __limitLabel: null,
    __limitsLabel: null,
    __steps: null,
    __running: false,

    /* ---------------------------------------------------------------- */
    /* layout                                                           */

    __buildSettings() {
      const W = smppclient.Widgets;
      const grid = W.compactGrid();
      const f = {};
      this.__fields = f;

      f.connections = W.number(4, 1, SmppForms.MAX_CONNECTIONS);
      f.messages = W.number(1000, 1, 50000);
      W.pair(grid, 0, "Connections", f.connections, "Messages", f.messages);

      f.mode = W.select(SmppForms.LOAD_MODES, "Fixed rate");
      f.ratePerSec = W.number(200, 1, 320000);
      W.pair(grid, 1, "Mode", f.mode, "Rate /s", f.ratePerSec);

      f.startRate = W.number(50, 1, 320000);
      f.step = W.number(50, 1, 320000);
      W.pair(grid, 2, "Ramp from /s", f.startRate, "Step /s", f.step);

      f.maxRate = W.number(500, 1, 320000);
      f.stepSeconds = W.number(10, 1, 600);
      W.pair(grid, 3, "Up to /s", f.maxRate, "Step seconds", f.stepSeconds);

      f.errorSharePercent = W.number(2, 0, 100);
      f.latencyP95Ms = W.number(2000, 0, 600000);
      W.pair(grid, 4, "Give way at % errors", f.errorSharePercent, "or p95 ms", f.latencyP95Ms);

      f.destination = W.text("393331234567", "destination_addr");
      W.wide(grid, 5, "Destination", f.destination);

      f.mode.addListener("changeSelection", () => {
        this.__refreshMode();
        this.__refreshLimits();
      });
      [
        "connections",
        "messages",
        "ratePerSec",
        "startRate",
        "step",
        "maxRate",
        "stepSeconds",
        "errorSharePercent",
        "latencyP95Ms"
      ].forEach(key => f[key].addListener("changeValue", () => this.__refreshLimits()));

      return smppclient.Widgets.tightGroup("Load", grid);
    },

    __buildActions() {
      const W = smppclient.Widgets;

      this.__startButton = W.primaryButton("Start load test");
      this.__startButton.addListener("execute", () => this.start());

      const stop = new qx.ui.form.Button("Stop");
      stop.addListener("execute", () => this.stop());

      this.__progress = new qx.ui.basic.Label("");

      // What this build allows, always on screen. Crossing a ceiling and
      // reading the refusal is a poor way to learn what you may ask for,
      // and these are this build's numbers, not the protocol's.
      this.__limitsLabel = new qx.ui.basic.Label("");
      this.__limitsLabel.setTextColor("#6b6b6b");

      this.__limitLabel = new qx.ui.basic.Label("");
      this.__limitLabel.setTextColor("#b03030");

      const box = W.buttons([this.__startButton, stop]);
      box.add(this.__progress);
      box.add(new qx.ui.core.Spacer(), { flex: 1 });
      box.add(this.__limitLabel);

      const wrap = new qx.ui.container.Composite(new qx.ui.layout.VBox(2));
      wrap.add(box);
      wrap.add(this.__limitsLabel);
      return wrap;
    },

    /** One row per connection: a balancer that favours one shows up here. */
    __buildTable() {
      this.__model = new qx.ui.table.model.Simple();
      this.__model.setColumns(SmppForms.CONNECTION_COLUMNS, SmppForms.CONNECTION_COLUMNS);

      this.__table = new qx.ui.table.Table(this.__model, {
        tableColumnModel: obj => new qx.ui.table.columnmodel.Resize(obj)
      });
      this.__table.setStatusBarVisible(false);
      this.__table.setMinHeight(140);
      return smppclient.Widgets.tightGroup("Per connection", this.__table, 1);
    },

    __buildReport() {
      const box = new qx.ui.container.Composite(new qx.ui.layout.VBox(4));

      this.__reportList = smppclient.Widgets.mono("Nothing run yet.");
      box.add(smppclient.Widgets.tightGroup("Report", this.__reportList, 1), { flex: 1 });

      this.__stepList = smppclient.Widgets.mono("");
      box.add(smppclient.Widgets.tightGroup("Ramp steps", this.__stepList, 1), { flex: 1 });
      return box;
    },

    /* ---------------------------------------------------------------- */
    /* form                                                             */

    /** The form as a plain object, in the shape `SmppForms` expects. */
    getFormValues() {
      const W = smppclient.Widgets;
      const f = this.__fields;
      return {
        connections: f.connections.getValue(),
        messages: f.messages.getValue(),
        modeLabel: W.selectedLabel(f.mode),
        ratePerSec: f.ratePerSec.getValue(),
        startRate: f.startRate.getValue(),
        step: f.step.getValue(),
        stepSeconds: f.stepSeconds.getValue(),
        maxRate: f.maxRate.getValue(),
        errorSharePercent: f.errorSharePercent.getValue(),
        latencyP95Ms: f.latencyP95Ms.getValue(),
        destination: f.destination.getValue() || ""
      };
    },

    setFormValues(values) {
      const W = smppclient.Widgets;
      const f = this.__fields;

      // The mode is a selection, not a value, so it is set by its label and
      // not by the loop below. Leaving it to the loop would quietly ignore it,
      // which is a load test that silently runs at a fixed rate.
      if (values.modeLabel !== undefined) {
        W.selectByLabel(f.mode, values.modeLabel);
      }

      Object.keys(values).forEach(key => {
        if (key !== "modeLabel" && f[key] && values[key] !== undefined) {
          f[key].setValue(values[key]);
        }
      });
      this.__refreshMode();
      this.__refreshLimits();
    },

    /** Grey out the fields the chosen mode does not use. */
    __refreshMode() {
      const ramp = smppclient.Widgets.selectedValue(this.__fields.mode) === "ramp";
      this.__fields.ratePerSec.setEnabled(!ramp);
      ["startRate", "step", "maxRate", "stepSeconds", "errorSharePercent", "latencyP95Ms"].forEach(
        key => this.__fields[key].setEnabled(ramp)
      );
    },

    /** Start is refused, with the limit shown, before a single bind goes out. */
    __refreshLimits() {
      const values = this.getFormValues();
      const check = SmppForms.checkLoadLimits(values);
      this.__limitsLabel.setValue(SmppForms.limitsText(values));
      this.__limitLabel.setValue(check.ok ? "" : check.reason);
      this.__startButton.setEnabled(check.ok && !this.__running);
      return check;
    },

    isStartEnabled() {
      return this.__startButton.getEnabled();
    },

    /* ---------------------------------------------------------------- */
    /* running                                                          */

    start() {
      const check = this.__refreshLimits();
      if (!check.ok) {
        this.fireDataEvent("notice", check.reason);
        return Promise.resolve();
      }

      const values = this.getFormValues();
      const connection = this.__connectionPanel.getFormValues();
      const submit = this.__submitForm.getFormValues();

      this.__steps = [];
      this.__stepList.setValue("");
      this.__model.setData([]);
      this.__reportList.setValue("Running…");
      this.__running = true;
      this.__startButton.setEnabled(false);

      // The template comes from the backend's own composition of the submit
      // form, so a load test sends what the single-send tab would have sent.
      return this.__api
        .call("compose_submit", { values: SmppForms.buildComposeArgs(submit) })
        .then(built =>
          this.__api.call("start_load_test", {
            req: {
              connect: SmppForms.buildConnectRequest(connection),
              bind: SmppForms.buildBindRequest(connection),
              connections: values.connections,
              messages: values.messages,
              mode: SmppForms.buildLoadMode(values),
              template: {
                commandId: built.requests[0].commandId,
                fields: built.requests[0].fields,
                tlvs: []
              },
              destination: values.destination,
              text: submit.text,
              dataCoding: submit.dataCoding,
              segmentation: submit.segmentation,
              placement: submit.placement,
              packing: submit.packing
            }
          })
        )
        .then(report => {
          this.__onDone(report);
          return report;
        })
        .catch(e => {
          this.__running = false;
          this.__reportList.setValue("Did not run: " + e.message);
          this.__refreshLimits();
          this.fireDataEvent("notice", "Load test failed: " + e.message);
        });
    },

    stop() {
      return this.__api.call("stop_load_test").catch(() => undefined);
    },

    __onProgress(p) {
      this.__progress.setValue(SmppForms.formatLoadProgress(p));
    },

    __onStep(step) {
      this.__steps.push(step);
      this.__stepList.setValue(this.__steps.map(s => SmppForms.formatRampStep(s)).join("\n"));
    },

    __onDone(report) {
      if (!report) {
        return;
      }
      this.__running = false;
      this.__refreshLimits();
      this.showReport(report);
      this.fireDataEvent(
        "notice",
        "Load test: " +
          SmppForms.endReasonText(report.ended) +
          " — sustained " +
          report.sustainedRatePerSec +
          "/s"
      );
    },

    /** Draw a report. Separate from the event so a test can hand one in. */
    showReport(report) {
      const entries = SmppForms.formatLoadReport(report);
      const width = entries.reduce((w, e) => Math.max(w, e.label.length), 0);
      this.__reportList.setValue(
        entries
          .map(e => e.label + new Array(width - e.label.length + 1).join(" ") + "   " + e.value)
          .join("\n")
      );

      this.__model.setData(
        (report.perConnection || []).map(c =>
          SmppForms.formatConnectionRow(c, SmppForms.CONNECTION_COLUMNS)
        )
      );

      this.__steps = report.steps || [];
      this.__stepList.setValue(
        this.__steps.length === 0
          ? "No ramp: this was a fixed-rate run."
          : this.__steps.map(s => SmppForms.formatRampStep(s)).join("\n")
      );
    },

    /** What the ceilings line currently reads, for the checks. */
    getLimitsText() {
      return this.__limitsLabel.getValue();
    },

    /** What the report pane currently reads, for the checks. */
    getReportText() {
      return this.__reportList.getValue();
    },

    /** What the ramp-step pane currently reads, for the checks. */
    getStepText() {
      return this.__stepList.getValue();
    },

    /** The per-connection rows as drawn, for the checks. */
    getConnectionRows() {
      const rows = [];
      for (let i = 0; i < this.__model.getRowCount(); i++) {
        rows.push(this.__model.getRowData(i));
      }
      return rows;
    }
  }
});
