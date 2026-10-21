/**
 * A ladder diagram of the trace: time down the page, the ESME on the left and
 * the SMSC on the right, one labelled arrow per PDU, with the offset from the
 * first packet and from the previous one beside each.
 *
 * Opens as a window so it can be moved aside and left open while traffic runs.
 *
 * @ignore(SmppForms)
 * @ignore(SmppForms.*)
 */
qx.Class.define("smppclient.FlowDiagram", {
  extend: qx.ui.window.Window,

  construct() {
    super("SMPP flow");

    this.setLayout(new qx.ui.layout.VBox(4));
    this.setWidth(1020);
    this.setHeight(620);
    this.setShowMinimize(false);
    this.setContentPadding(0);

    this.add(this.__buildToolbar());

    this.__scroll = new qx.ui.container.Scroll();
    this.__html = new qx.ui.embed.Html("");
    this.__html.setOverflowX("auto");
    this.__html.setOverflowY("auto");
    this.__scroll.add(this.__html);
    this.add(this.__scroll, { flex: 1 });

    this.__status = new qx.ui.basic.Label("");
    this.__status.setPadding(4, 8);
    this.add(this.__status);
  },

  members: {
    __html: null,
    __scroll: null,
    __status: null,
    __rows: null,
    __left: "ESME",
    __right: "SMSC",

    __buildToolbar() {
      const bar = new qx.ui.container.Composite(new qx.ui.layout.HBox(6));
      bar.setPadding(6, 8);

      const refresh = new qx.ui.form.Button("Refresh");
      refresh.addListener("execute", () => this.redraw());
      bar.add(refresh);

      bar.add(new qx.ui.core.Spacer(), { flex: 1 });

      const close = new qx.ui.form.Button("Close");
      close.addListener("execute", () => this.close());
      bar.add(close);

      return bar;
    },

    /** Name the two sides, so the diagram says which SMSC it is. */
    setPeers(left, right) {
      this.__left = left || "ESME";
      this.__right = right || "SMSC";
    },

    /**
     * Draw `rows` (WirePdu records, oldest first) and bring the window up.
     *
     * Deliberately not called `show`: Window already has one, and open()
     * calls it, so overriding it recurses until the stack gives out.
     */
    display(rows) {
      this.__rows = rows || [];
      this.redraw();
      this.open();
      this.center();
    },

    redraw() {
      const rows = this.__rows || [];
      const svg = SmppForms.buildFlowSvg(rows, {
        leftLabel: this.__left,
        rightLabel: this.__right,
        width: 960
      });
      this.__html.setHtml(svg);
      this.__status.setValue(
        rows.length === 1 ? "1 PDU" : rows.length + " PDUs"
      );
    },

    /** The SVG currently drawn, for verification. */
    getSvg() {
      return this.__html.getHtml();
    }
  }
});
