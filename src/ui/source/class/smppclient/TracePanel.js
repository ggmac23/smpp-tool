/**
 * The PDU trace: one row per PDU in both directions, with the hex and the
 * decoded field list of whatever is selected.
 *
 * Rows arrive in batches from the main process — 500 PDUs a second must not be
 * 500 messages a second — and a batch is appended in order, never reordered.
 * When the main process has discarded history to stay inside its cap, the
 * panel says so rather than quietly showing a partial trace.
 *
 * @ignore(SmppForms)
 * @ignore(SmppForms.*)
 */
qx.Class.define("smppclient.TracePanel", {
  extend: qx.ui.container.Composite,

  construct() {
    super(new qx.ui.layout.VBox(4));

    this.__api = smppclient.Api.getInstance();
    this.__rowsBySeq = {};
    this.__seqByRow = [];

    this.add(this.__buildToolbar());

    this.__dropped = new qx.ui.basic.Label("");
    this.__dropped.setVisibility("excluded");
    this.add(this.__dropped);

    const split = new qx.ui.splitpane.Pane("horizontal");
    split.add(this.__buildTable(), 3);
    split.add(this.__buildDetail(), 2);
    this.add(split, { flex: 1 });

    this.__api.on("pdu", batch => this.appendBatch(batch));
  },

  events: {
    /** The user asked to edit a PDU as hex. Data: the hex string. */
    editAsHex: "qx.event.type.Data",
    notice: "qx.event.type.Data"
  },

  members: {
    __api: null,
    __table: null,
    __model: null,
    __hexView: null,
    __fieldList: null,
    __dropped: null,
    __rowsBySeq: null,
    /**
     * Trace row ordinal per table row. The table model keeps only as many
     * columns as it was given, so the ordinal cannot ride along inside a row.
     */
    __seqByRow: null,
    __compact: false,
    __flow: null,
    __peer: null,

    /* ---------------------------------------------------------------- */

    __buildToolbar() {
      const bar = new qx.ui.container.Composite(new qx.ui.layout.HBox(4));
      bar.setPadding(4);

      const top = new qx.ui.form.Button("Top");
      top.addListener("execute", () => this.scrollToTop());
      bar.add(top);

      const bottom = new qx.ui.form.Button("Bottom");
      bottom.addListener("execute", () => this.scrollToBottom());
      bar.add(bottom);

      this.__compactButton = new qx.ui.form.Button("Compact");
      this.__compactButton.addListener("execute", () => this.toggleCompact());
      bar.add(this.__compactButton);

      const clear = new qx.ui.form.Button("Clear");
      clear.addListener("execute", () => this.clear());
      bar.add(clear);

      bar.add(new qx.ui.core.Spacer(), { flex: 1 });

      const flow = new qx.ui.form.Button("Flow diagram");
      flow.addListener("execute", () => this.showFlow());
      bar.add(flow);

      const editHex = new qx.ui.form.Button("Edit as hex");
      editHex.addListener("execute", () => this.editSelectedAsHex());
      bar.add(editHex);

      ["csv", "json", "text"].forEach(format => {
        const b = new qx.ui.form.Button("Export " + format.toUpperCase());
        b.addListener("execute", () => this.export(format));
        bar.add(b);
      });

      return bar;
    },

    __buildTable() {
      this.__model = new qx.ui.table.model.Simple();
      this.__model.setColumns(SmppForms.TRACE_COLUMNS);

      this.__table = new qx.ui.table.Table(this.__model, {
        tableColumnModel: obj => new qx.ui.table.columnmodel.Resize(obj)
      });
      this.__table.setStatusBarVisible(true);

      // The PDU column takes whatever is spare; the rest are fixed, because a
      // trace is scanned down the left edge and across one wide column.
      const widths = this.__table.getTableColumnModel().getBehavior();
      widths.setWidth(0, 100);
      widths.setWidth(1, 64);
      widths.setWidth(2, 38);
      widths.setWidth(3, "1*", 260);
      widths.setWidth(4, 120);

      // The selection model is what announces a change — a click on the table
      // raises nothing on the table itself, so listening there left the hex
      // and field panes empty for every mouse selection.
      this.__table
        .getSelectionModel()
        .addListener("changeSelection", () => this.__onSelect());
      return this.__table;
    },

    __buildDetail() {
      const box = new qx.ui.container.Composite(new qx.ui.layout.VBox(4));

      box.add(smppclient.Widgets.heading("PDU (hex)"));
      this.__hexView = new smppclient.HexView();
      box.add(this.__hexView, { flex: 1 });

      box.add(smppclient.Widgets.heading("Fields"));
      this.__fieldList = smppclient.Widgets.mono("");
      box.add(this.__fieldList, { flex: 1 });

      return box;
    },

    /* ---------------------------------------------------------------- */

    /** Append a batch of rows, in the order the main process sent them. */
    appendBatch(batch) {
      if (!batch || batch.length === 0) {
        return;
      }
      const rows = batch.map(pdu => {
        this.__rowsBySeq[pdu.seq] = pdu;
        this.__seqByRow.push(pdu.seq);
        return this.__toRow(pdu);
      });
      this.__model.addRows(rows);
      this.__refreshDropped();
    },

    __toRow(pdu) {
      const summary = pdu.decodeError ? "decode error: " + pdu.decodeError : pdu.summary;
      return [
        SmppForms.formatTime(pdu.at),
        pdu.sequenceNumber,
        SmppForms.directionArrow(pdu.direction),
        this.__compact ? pdu.commandName : summary,
        pdu.commandStatus === 0 ? "" : pdu.statusName
      ];
    },

    /** Swap between the full summary and just the command name. */
    toggleCompact() {
      this.__compact = !this.__compact;
      this.__compactButton.setLabel(this.__compact ? "Expand" : "Compact");
      this.__model.setData(this.__seqByRow.map(seq => this.__toRow(this.__rowsBySeq[seq])));
    },

    scrollToTop() {
      this.__table.setScrollY(0);
    },

    scrollToBottom() {
      const rows = this.__model.getRowCount();
      if (rows > 0) {
        this.__table.scrollCellVisible(0, rows - 1);
      }
    },

    clear() {
      this.__model.setData([]);
      this.__rowsBySeq = {};
      this.__seqByRow = [];
      this.__hexView.clear();
      this.__fieldList.setValue("");
      this.__api.call("clearTrace").catch(() => undefined);
      this.__refreshDropped();
    },

    export(format) {
      this.__api
        .call("exportTrace", format)
        .then(result =>
          this.fireDataEvent(
            "notice",
            result && result.path ? "Trace written to " + result.path : "Export cancelled"
          )
        )
        .catch(e => this.fireDataEvent("notice", "Export failed: " + e.message));
    },

    /** Select a row by its index and show its detail. */
    selectRow(index) {
      this.__table.getSelectionModel().setSelectionInterval(index, index);
    },

    /** Whether the panel holds the PDU for a trace row ordinal. */
    hasPdu(seq) {
      return !!this.__rowsBySeq[seq];
    },

    /** The trace row ordinal currently selected, or null. */
    getSelectedSeq() {
      const selection = this.__table.getSelectionModel();
      const ranges = selection.getSelectedRanges();
      if (ranges.length === 0) {
        return null;
      }
      const seq = this.__seqByRow[ranges[0].minIndex];
      return seq === undefined ? null : seq;
    },

    editSelectedAsHex() {
      const hex = this.__hexView.getHex();
      if (!hex) {
        this.fireDataEvent("notice", "Select a PDU in the trace first.");
        return;
      }
      this.fireDataEvent("editAsHex", hex);
    },

    /**
     * Open the ladder diagram for everything in the trace. The window is kept
     * so reopening it does not lose its position.
     */
    showFlow() {
      if (!this.__flow) {
        this.__flow = new smppclient.FlowDiagram();
        qx.core.Init.getApplication().getRoot().add(this.__flow);
      }
      const rows = this.__seqByRow.map(seq => this.__rowsBySeq[seq]).filter(Boolean);
      this.__flow.setPeers("ESME", this.__peer || "SMSC");
      this.__flow.display(rows);
      return this.__flow;
    },

    /** Name the far end, so the diagram can label its lifeline. */
    setPeer(label) {
      this.__peer = label;
      if (this.__flow) {
        this.__flow.setPeers("ESME", label);
      }
    },

    /** The flow window, once opened. */
    getFlowDiagram() {
      return this.__flow;
    },

    /** The hex dump currently shown, for verification. */
    getHexText() {
      return this.__hexView.getText();
    },

    /** The field list currently shown, for verification. */
    getFieldsText() {
      return this.__fieldList.getValue();
    },

    getRowCount() {
      return this.__model.getRowCount();
    },

    /** The PDU column of a row, as shown. */
    getRowSummary(index) {
      const row = this.__model.getRowData(index);
      return row ? row[3] : "";
    },

    __onSelect() {
      const seq = this.getSelectedSeq();
      if (seq === null || seq === undefined) {
        this.__hexView.clear();
        this.__fieldList.setValue("");
        return;
      }
      const pdu = this.__rowsBySeq[seq];
      this.__hexView.setHex(pdu ? pdu.hex : "");

      this.__api
        .call("pduFields", seq)
        .then(fields => {
          const rows = Array.isArray(fields) ? fields : [];
          this.__fieldList.setValue(rows.map(f => f.name + " = " + f.value).join("\n"));
        })
        .catch(() => this.__fieldList.setValue(""));
    },

    __refreshDropped() {
      this.__api
        .call("status")
        .then(status => {
          const parts = [
            SmppForms.droppedNotice(status ? status.dropped : 0),
            SmppForms.pendingNotice(status ? status.pending : null)
          ].filter(Boolean);
          const notice = parts.join("  ");
          this.__dropped.setValue(notice);
          this.__dropped.setVisibility(notice ? "visible" : "excluded");
        })
        .catch(() => undefined);
    },

    __compactButton: null
  }
});
