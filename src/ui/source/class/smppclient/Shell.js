/**
 * The window layout the user picked: connection panel west, PDU forms as tabs
 * in the centre, and the PDU trace full width along the bottom.
 */
qx.Class.define("smppclient.Shell", {
  extend: qx.ui.container.Composite,

  construct() {
    super(new qx.ui.layout.Dock());

    this.__connection = new smppclient.ConnectionPanel();
    this.add(this.__connection, { edge: "west" });

    this.__status = new qx.ui.basic.Label("");
    this.__status.setPadding(4, 8);
    this.add(this.__status, { edge: "south" });

    this.__tabs = new qx.ui.tabview.TabView();

    const split = new qx.ui.splitpane.Pane("vertical");
    split.add(this.__tabs, 2);
    this.__traceSlot = new qx.ui.container.Composite(new qx.ui.layout.Grow());
    split.add(this.__traceSlot, 1);
    this.add(split, { edge: "center" });

    this.__connection.addListener("notice", e => this.showNotice(e.getData()));
    smppclient.Api.getInstance().addListener("commandError", e =>
      this.showNotice(e.getData().command + ": " + e.getData().message)
    );
  },

  members: {
    __connection: null,
    __tabs: null,
    __traceSlot: null,
    __status: null,

    /** The tab view the PDU forms live in. */
    getTabs() {
      return this.__tabs;
    },

    /** The container the trace panel goes into. */
    getTraceSlot() {
      return this.__traceSlot;
    },

    getConnectionPanel() {
      return this.__connection;
    },

    /** Add a page to the tab view. */
    addPage(label, widget) {
      const page = new qx.ui.tabview.Page(label);
      page.setLayout(new qx.ui.layout.Grow());
      page.add(widget);
      this.__tabs.add(page);
      return page;
    },

    /** One line of feedback along the bottom. */
    showNotice(text) {
      this.__status.setValue(text === undefined || text === null ? "" : String(text));
    }
  }
});
