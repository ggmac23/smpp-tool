/**
 * Burst sending: N messages at a requested rate, with a row per message and
 * the aggregates beside them.
 *
 * The message itself comes from the submit_sm form, so there is one place to
 * compose a body. The rows are never capped — the trace may lose history under
 * pressure, the result table may not.
 *
 * @ignore(SmppForms)
 * @ignore(SmppForms.*)
 */
qx.Class.define("smppclient.BurstPanel", {
  extend: qx.ui.container.Composite,

  /**
   * @param submitForm {smppclient.SubmitForm} where the message body comes from
   */
  construct(submitForm) {
    super(new qx.ui.layout.VBox(6));
    this.setPadding(8);

    this.__api = smppclient.Api.getInstance();
    this.__submitForm = submitForm;
    this.__rows = [];
    this.__mode = null;

    this.add(this.__buildSettings());
    this.add(this.__buildActions());

    const split = new qx.ui.splitpane.Pane("horizontal");
    split.add(this.__buildTable(), 3);
    split.add(this.__buildStats(), 1);
    this.add(split, { flex: 1 });

    this.__api.on("burst-row", batch => this.__onRows(batch));
    this.__api.on("burst-progress", p => this.__onProgress(p));
    this.__api.on("burst-done", d => this.__onDone(d));
    this.__api.on("state", s => this.setBindMode(s.mode));

    this.__refreshLimits();
  },

  events: {
    notice: "qx.event.type.Data"
  },

  members: {
    __api: null,
    __submitForm: null,
    __fields: null,
    __table: null,
    __model: null,
    __statsList: null,
    __progress: null,
    __startButton: null,
    __limitLabel: null,
    __rows: null,
    __mode: null,
    __csvText: null,
    __csvPath: null,

    /* ---------------------------------------------------------------- */

    __buildSettings() {
      const W = smppclient.Widgets;
      const grid = W.compactGrid();
      const f = (this.__fields = {});

      f.source = W.select(
        [
          { label: "Repeat one number", value: "repeat" },
          { label: "Generate a range", value: "range" },
          { label: "Load a CSV", value: "csv" }
        ],
        "Generate a range"
      );
      f.number = W.text("393330000000", "393330000000");
      W.pair(grid, 0, "recipients", f.source, "from / number", f.number);

      // The spinners deliberately allow values above the ceilings: clamping
      // would silently change what the user asked for, and the point is to
      // refuse out loud with the limit named.
      f.count = W.number(100, 1, 10000000);
      f.rate = W.number(50, 1, 100000);
      W.pair(grid, 1, "count", f.count, "rate /sec", f.rate);

      f.awaitDlrMs = W.number(2000, 0, 600000);
      f.backoff = W.check("Back off when throttled", false);
      W.pair(grid, 2, "wait for DLRs (ms)", f.awaitDlrMs, "on throttle", f.backoff);

      f.csvPath = W.text("", "no file loaded");
      f.csvPath.setReadOnly(true);
      const browse = new qx.ui.form.Button("Load CSV…");
      browse.addListener("execute", () => this.loadCsv());
      const csvBox = new qx.ui.container.Composite(new qx.ui.layout.HBox(6));
      csvBox.add(f.csvPath, { flex: 1 });
      csvBox.add(browse);
      W.wide(grid, 3, "CSV", csvBox);

      f.count.addListener("changeValue", () => this.__refreshLimits());
      f.rate.addListener("changeValue", () => this.__refreshLimits());

      return grid;
    },

    __buildActions() {
      const W = smppclient.Widgets;

      this.__startButton = W.primaryButton("Start burst", W.ACCENT_SEND);
      this.__startButton.addListener("execute", () => this.start());

      const stop = new qx.ui.form.Button("Stop");
      stop.addListener("execute", () => this.stop());

      const exportCsv = new qx.ui.form.Button("Export CSV");
      exportCsv.addListener("execute", () => this.exportCsv());

      this.__progress = new qx.ui.basic.Label("");
      this.__limitLabel = new qx.ui.basic.Label("");
      this.__limitLabel.setTextColor("#b03030");

      const box = W.buttons([this.__startButton, stop, exportCsv]);
      box.add(this.__progress);
      box.add(new qx.ui.core.Spacer(), { flex: 1 });
      box.add(this.__limitLabel);
      return box;
    },

    __buildTable() {
      this.__model = new qx.ui.table.model.Simple();
      this.__model.setColumns(SmppForms.burstColumns(this.__mode));

      this.__table = new qx.ui.table.Table(this.__model, {
        tableColumnModel: obj => new qx.ui.table.columnmodel.Resize(obj)
      });
      this.__table.setStatusBarVisible(true);
      return this.__table;
    },

    __buildStats() {
      const box = new qx.ui.container.Composite(new qx.ui.layout.VBox(4));
      box.add(smppclient.Widgets.heading("Totals"));
      this.__statsList = smppclient.Widgets.mono("");
      box.add(this.__statsList, { flex: 1 });
      return box;
    },

    /* ---------------------------------------------------------------- */

    /** Rebuild the columns when the bind mode changes. */
    setBindMode(mode) {
      if (mode === this.__mode) {
        return;
      }
      this.__mode = mode;
      this.__model.setColumns(SmppForms.burstColumns(mode));
      this.__redrawRows();
    },

    getColumns() {
      return SmppForms.burstColumns(this.__mode);
    },

    loadCsv() {
      return this.__api
        .call("pickFile", "csv")
        .then(picked => {
          if (!picked) {
            return;
          }
          this.__csvPath = picked.path;
          this.__csvText = picked.text || "";
          this.__fields.csvPath.setValue(picked.path);
          smppclient.Widgets.selectByLabel(this.__fields.source, "Load a CSV");
          this.fireDataEvent("notice", "Loaded " + picked.path);
        })
        .catch(e => this.fireDataEvent("notice", "Could not load the CSV: " + e.message));
    },

    /** The recipient source the form currently describes. */
    getRecipientSource() {
      const W = smppclient.Widgets;
      const kind = W.selectedValue(this.__fields.source) || "range";
      const count = this.__fields.count.getValue();
      if (kind === "csv") {
        return { kind: "csv", text: this.__csvText || "" };
      }
      if (kind === "repeat") {
        return { kind: "repeat", number: this.__fields.number.getValue() || "", count: count };
      }
      return { kind: "range", from: this.__fields.number.getValue() || "", count: count };
    },

    /** Start is refused, with the limit shown, before a single PDU goes out. */
    __refreshLimits() {
      const check = SmppForms.checkBurstLimits(
        this.__fields.count.getValue(),
        this.__fields.rate.getValue()
      );
      this.__limitLabel.setValue(check.ok ? "" : check.reason);
      this.__startButton.setEnabled(check.ok);
      return check;
    },

    start() {
      const check = this.__refreshLimits();
      if (!check.ok) {
        this.fireDataEvent("notice", check.reason);
        return Promise.resolve();
      }

      this.__rows = [];
      this.__model.setData([]);

      const values = this.__submitForm.getFormValues();
      const built = SmppForms.buildSubmitInput(values);
      built.warnings.forEach(w => this.fireDataEvent("notice", w));

      const request = {
        recipients: this.getRecipientSource(),
        ratePerSec: this.__fields.rate.getValue(),
        template: {
          commandId: built.requests[0].commandId,
          fields: built.requests[0].fields,
          tlvs: (values.tlvs || []).length > 0 ? built.requests[0].tlvs : []
        },
        text: values.text,
        dataCoding: values.dataCoding,
        segmentation: values.segmentation,
        placement: values.placement,
        packing: values.packing,
        backoffOnThrottle: this.__fields.backoff.getValue(),
        awaitDlrMs: this.__fields.awaitDlrMs.getValue()
      };

      this.fireDataEvent("notice", "Burst started");
      return this.__api
        .call("startBurst", request)
        .catch(e => this.fireDataEvent("notice", "Burst failed: " + e.message));
    },

    stop() {
      return this.__api.call("stopBurst").catch(() => undefined);
    },

    exportCsv() {
      return this.__api
        .call("exportBurst")
        .then(result =>
          this.fireDataEvent(
            "notice",
            result && result.path ? "Results written to " + result.path : "Export cancelled"
          )
        )
        .catch(e => this.fireDataEvent("notice", "Export failed: " + e.message));
    },

    /* ---------------------------------------------------------------- */

    __onRows(batch) {
      if (!batch || batch.length === 0) {
        return;
      }
      const columns = this.getColumns();
      batch.forEach(row => {
        const index = this.__rows.findIndex(r => r.id === row.id);
        if (index >= 0) {
          this.__rows[index] = row;
          this.__model.setRows([SmppForms.formatBurstRow(row, columns)], index, true);
        } else {
          this.__rows.push(row);
          this.__model.addRows([SmppForms.formatBurstRow(row, columns)]);
        }
      });
    },

    __redrawRows() {
      const columns = this.getColumns();
      this.__model.setData(this.__rows.map(r => SmppForms.formatBurstRow(r, columns)));
    },

    __onProgress(p) {
      this.__progress.setValue(p.sent + " / " + p.total);
      this.__showStats(p.stats);
    },

    __onDone(d) {
      this.__showStats(d.stats);
      (d.warnings || []).forEach(w => this.fireDataEvent("notice", w));
      this.fireDataEvent("notice", "Burst " + d.reason + " — " + d.stats.submitted + " submitted");
    },

    __showStats(stats) {
      if (!stats) {
        return;
      }
      const entries = SmppForms.formatStats(stats);
      const width = entries.reduce((w, e) => Math.max(w, e.label.length), 0);
      const lines = entries.map(e => {
        const pad = e.label + new Array(width - e.label.length + 1).join(" ");
        return pad + "   " + e.value;
      });
      this.__statsList.setValue(lines.join("\n"));
    },

    /** The totals block, as shown. */
    getStatsText() {
      return this.__statsList.getValue();
    },

    getRowCount() {
      return this.__model.getRowCount();
    },

    /** Whether Start is currently allowed. */
    isStartEnabled() {
      return this.__startButton.getEnabled();
    },

    /** The refusal currently shown, or "". */
    getLimitMessage() {
      return this.__limitLabel.getValue();
    },

    /** Put values into the burst settings. Only the keys present are touched. */
    setFormValues(values) {
      const W = smppclient.Widgets;
      const f = this.__fields;
      if (values.sourceLabel) {
        W.selectByLabel(f.source, values.sourceLabel);
      }
      if (values.number !== undefined) {
        f.number.setValue(String(values.number));
      }
      if (values.count !== undefined) {
        f.count.setValue(Number(values.count));
      }
      if (values.rate !== undefined) {
        f.rate.setValue(Number(values.rate));
      }
      if (values.awaitDlrMs !== undefined) {
        f.awaitDlrMs.setValue(Number(values.awaitDlrMs));
      }
      if (values.backoff !== undefined) {
        f.backoff.setValue(!!values.backoff);
      }
      this.__refreshLimits();
    }
  }
});
