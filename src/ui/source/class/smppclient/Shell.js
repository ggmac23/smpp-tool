/**
 * The window layout the user picked: connection panel west, PDU forms as tabs
 * in the centre, and the PDU trace full width along the bottom.
 */
qx.Class.define("smppclient.Shell", {
  extend: qx.ui.container.Composite,

  construct() {
    super(new qx.ui.layout.Dock());

    // The connection column scrolls on its own: with the TLS group open it is
    // taller than a short window, and it must not push the rest off screen.
    this.__connection = new smppclient.ConnectionPanel();
    const west = new qx.ui.container.Scroll();
    west.setWidth(372);
    west.setMinWidth(300);
    west.add(this.__connection);
    this.add(west, { edge: "west" });

    this.add(this.__buildStatusBar(), { edge: "south" });

    this.__tabs = new qx.ui.tabview.TabView();
    this.__tabs.setMinHeight(220);

    this.__traceSlot = new qx.ui.container.Composite(new qx.ui.layout.Grow());
    this.__traceSlot.setMinHeight(180);

    const split = new qx.ui.splitpane.Pane("vertical");
    split.add(this.__tabs, 3);
    split.add(this.__traceSlot, 2);
    this.add(split, { edge: "center" });

    this.__connection.addListener("notice", e => this.showNotice(e.getData()));
    smppclient.Api.getInstance().addListener("commandError", e =>
      this.showNotice(e.getData().command + ": " + e.getData().message)
    );
  },

  members: {
    __connection: null,
    __statusBar: null,
    __tabs: null,
    __traceSlot: null,
    __status: null,

    /** One line of feedback, set off from the panels above it. */
    __buildStatusBar() {
      const bar = new qx.ui.container.Composite(new qx.ui.layout.HBox(8));
      bar.setPadding(5, 10);
      bar.setBackgroundColor("background-medium");
      bar.setDecorator("main");

      this.__status = new qx.ui.basic.Label("");
      this.__status.setRich(false);
      bar.add(this.__status, { flex: 1 });

      this.__statusBar = bar;
      return bar;
    },

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
