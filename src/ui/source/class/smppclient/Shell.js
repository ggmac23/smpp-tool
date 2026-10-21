/**
 * The window layout the user picked: connection panel west, PDU forms as tabs
 * in the centre, and the PDU trace full width along the bottom. The bar along
 * the bottom carries the bind state, the last notice and the theme picker.
 *
 * @ignore(SmppForms)
 * @ignore(SmppForms.*)
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
    this.__connection.addListener("stateChanged", e => this.setConnectionState(e.getData()));
    this.applyTheme(this.__currentTheme());
    smppclient.Api.getInstance().addListener("commandError", e =>
      this.showNotice(e.getData().command + ": " + e.getData().message)
    );
  },

  members: {
    __connection: null,
    __statusBar: null,
    __themes: null,
    __state: null,
    __dot: null,
    __tabs: null,
    __traceSlot: null,
    __status: null,

    /** One line of feedback, set off from the panels above it. */
    __buildStatusBar() {
      const bar = new qx.ui.container.Composite(new qx.ui.layout.HBox(8));
      bar.setPadding(5, 10);
      bar.setBackgroundColor("background-medium");
      bar.setDecorator("main");

      this.__dot = new qx.ui.basic.Label("\u25cf");
      this.__dot.setTextColor("#9a9a9a");
      bar.add(this.__dot);

      this.__state = new qx.ui.basic.Label("Unbound");
      this.__state.setFont("bold");
      bar.add(this.__state);

      bar.add(new qx.ui.basic.Label("|"), { flex: 0 });

      this.__status = new qx.ui.basic.Label("");
      this.__status.setRich(false);
      bar.add(this.__status, { flex: 1 });

      bar.add(new qx.ui.basic.Label("Theme"));
      this.__themes = smppclient.Widgets.select(
        SmppForms.THEMES,
        SmppForms.themeLabel(this.__currentTheme())
      );
      this.__themes.setWidth(130);
      this.__themes.addListener("changeSelection", () => {
        const value = smppclient.Widgets.selectedValue(this.__themes);
        if (value) {
          this.applyTheme(value);
        }
      });
      bar.add(this.__themes);

      this.__statusBar = bar;
      return bar;
    },

    __currentTheme() {
      let remembered = null;
      try {
        remembered = window.localStorage.getItem(SmppForms.THEME_STORAGE_KEY);
      } catch (e) {
        remembered = null;
      }
      return SmppForms.resolveTheme(remembered);
    },

    /** Swap the theme live and remember the choice for next time. */
    applyTheme(value) {
      const theme = qx.theme[value];
      if (!theme) {
        return false;
      }
      qx.theme.manager.Meta.getInstance().setTheme(theme);
      try {
        window.localStorage.setItem(SmppForms.THEME_STORAGE_KEY, value);
      } catch (e) {
        // A private window or blocked storage: the choice just does not stick.
      }
      return true;
    },

    /** Bind state, shown in the bar. */
    setConnectionState(payload) {
      this.__state.setValue(payload.label);
      const colours = {
        unbound: "#9a9a9a",
        binding: "#c08000",
        bound: "#2e8b2e",
        unbinding: "#c08000",
        closed: "#b03030"
      };
      this.__dot.setTextColor(colours[payload.state] || "#9a9a9a");
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

    /** Bring the page holding `widget` to the front. */
    selectPage(widget) {
      const page = this.__tabs.getChildren().filter(p => p.getChildren().indexOf(widget) >= 0);
      if (page.length > 0) {
        this.__tabs.setSelection([page[0]]);
      }
    },

    /** One line of feedback along the bottom. */
    showNotice(text) {
      this.__status.setValue(text === undefined || text === null ? "" : String(text));
    }
  }
});
