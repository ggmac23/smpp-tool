/**
 * The connection panel: credentials, TLS, bind mode, saved profiles and the
 * per-PDU counters. The west column of the shell.
 *
 * All value mapping lives in `SmppForms` (src/shared/ui/connection.ts) so it
 * is unit-tested without a DOM; this class is widgets and wiring only.
 *
 * @ignore(SmppForms)
 * @ignore(SmppForms.*)
 */
qx.Class.define("smppclient.ConnectionPanel", {
  extend: qx.ui.container.Composite,

  construct() {
    super(new qx.ui.layout.VBox(4));
    this.setPadding(8);
    this.setWidth(330);

    const W = smppclient.Widgets;
    this.__api = smppclient.Api.getInstance();

    this.add(this.__buildStatusStrip());
    this.add(W.group("Profile", this.__buildProfiles()));
    this.add(W.group("Server", this.__buildServer()));
    this.add(this.__buildTlsGroup());
    this.add(W.group("Bind", this.__buildBind()));
    this.add(this.__buildActions());

    this.__counters = W.mono("");
    this.__counters.setMinHeight(120);
    this.add(W.group("Counters", this.__counters, 1), { flex: 1 });

    this.__api.on("state", e => this.__onState(e));
    this.__api.on("counters", c => this.__onCounters(c));
    this.__reloadProfiles();
  },

  events: {
    /** Fired after a successful bind. Data: the bind mode. */
    bound: "qx.event.type.Data",
    /** Fired with a one-line message the shell should show. Data: string. */
    notice: "qx.event.type.Data"
  },

  members: {
    __api: null,
    __state: null,
    __dot: null,
    __counters: null,
    __fields: null,
    __profiles: null,
    __tlsGroup: null,

    /* ---------------------------------------------------------------- */
    /* construction                                                      */

    /** Bind state as a coloured dot plus its label. */
    __buildStatusStrip() {
      const box = new qx.ui.container.Composite(new qx.ui.layout.HBox(8));
      box.setPadding(2, 0, 6, 0);

      this.__dot = new qx.ui.basic.Label("\u25cf");
      this.__dot.setTextColor("#999999");
      box.add(this.__dot);

      this.__state = new qx.ui.basic.Label("Unbound");
      this.__state.setFont("bold");
      box.add(this.__state, { flex: 1 });

      return box;
    },

    __buildProfiles() {
      const W = smppclient.Widgets;
      const box = new qx.ui.container.Composite(new qx.ui.layout.HBox(4));

      this.__profiles = new qx.ui.form.SelectBox();
      this.__profiles.setWidth(170);
      box.add(this.__profiles, { flex: 1 });

      const load = new qx.ui.form.Button("Load");
      load.addListener("execute", () => this.__loadSelectedProfile());
      box.add(load);

      const save = new qx.ui.form.Button("Save");
      save.addListener("execute", () => this.__saveProfile());
      box.add(save);

      const remove = new qx.ui.form.Button("Delete");
      remove.addListener("execute", () => this.__deleteProfile());
      box.add(remove);

      void W;
      return box;
    },

    __buildServer() {
      const W = smppclient.Widgets;
      const grid = W.formGrid();
      this.__fields = this.__fields || {};

      this.__fields.host = W.row(grid, 0, "Host", W.text("", "smsc.example.com"));
      this.__fields.port = W.row(grid, 1, "Port", W.number(2775, 1, 65535));
      this.__fields.tlsEnabled = W.row(grid, 2, "", W.check("TLS", false));
      this.__fields.name = W.row(grid, 3, "Profile name", W.text("", "Aggregator A"));

      this.__fields.tlsEnabled.addListener("changeValue", e =>
        this.__tlsGroup.setVisibility(e.getData() ? "visible" : "excluded")
      );
      return grid;
    },

    __buildTlsGroup() {
      const W = smppclient.Widgets;
      const group = new qx.ui.groupbox.GroupBox("TLS");
      group.setLayout(new qx.ui.layout.VBox(4));
      const grid = W.formGrid();

      this.__fields.rejectUnauthorized = W.row(
        grid,
        0,
        "",
        W.check("Verify the server certificate", true)
      );
      this.__fields.servername = W.row(grid, 1, "SNI", W.text("", "defaults to the host"));
      this.__fields.caPath = W.row(grid, 2, "CA file", this.__filePicker("ca"));
      this.__fields.clientCertPath = W.row(grid, 3, "Client cert", this.__filePicker("cert"));
      this.__fields.clientKeyPath = W.row(grid, 4, "Client key", this.__filePicker("key"));
      this.__fields.clientKeyPassphrase = W.row(grid, 5, "Key passphrase", W.password(""));

      group.add(grid);
      group.setVisibility("excluded");
      this.__tlsGroup = group;
      return group;
    },

    /** A read-only path field with a Browse button that asks the main process. */
    __filePicker(kind) {
      const W = smppclient.Widgets;
      const box = new qx.ui.container.Composite(new qx.ui.layout.HBox(4));
      const field = W.text("", "none");
      field.setReadOnly(true);
      box.add(field, { flex: 1 });

      const browse = new qx.ui.form.Button("…");
      browse.setWidth(28);
      browse.addListener("execute", () => {
        this.__api
          .call("pickFile", kind)
          .then(picked => {
            if (picked && picked.path) {
              field.setValue(picked.path);
            }
          })
          .catch(() => undefined);
      });
      box.add(browse);

      // The caller wants the value, so expose the field's accessors on the box.
      box.getValue = () => field.getValue();
      box.setValue = v => field.setValue(v === null || v === undefined ? "" : String(v));
      return box;
    },

    __buildBind() {
      const W = smppclient.Widgets;
      const grid = W.formGrid();

      this.__fields.systemId = W.row(grid, 0, "system_id", W.text("", "esme"));
      this.__fields.password = W.row(grid, 1, "password", W.password(""));
      this.__fields.systemType = W.row(grid, 2, "system_type", W.text(""));
      this.__fields.interfaceVersion = W.row(
        grid,
        3,
        "Version",
        W.select(SmppForms.INTERFACE_VERSIONS, "v3.4")
      );
      this.__fields.bindMode = W.row(grid, 4, "Mode", W.select(SmppForms.BIND_MODES, "TRX"));
      this.__fields.addressRange = W.row(grid, 5, "address_range", W.text(""));
      this.__fields.windowSize = W.row(grid, 6, "Window", W.number(10, 1, 1000));
      this.__fields.responseTimeoutMs = W.row(grid, 7, "Resp. timeout", W.number(5000, 100, 600000));
      this.__fields.connectTimeoutMs = W.row(grid, 8, "Conn. timeout", W.number(10000, 100, 600000));
      return grid;
    },

    __buildActions() {
      const W = smppclient.Widgets;

      const bind = new qx.ui.form.Button("Bind");
      bind.addListener("execute", () => this.bind());

      const unbind = new qx.ui.form.Button("Unbind");
      unbind.addListener("execute", () => this.unbind());

      const enquire = new qx.ui.form.Button("enquire_link");
      enquire.addListener("execute", () => this.enquireLink());

      return W.buttons([bind, unbind, enquire]);
    },

    /* ---------------------------------------------------------------- */
    /* values                                                            */

    /** The form as a plain object, in the shape `SmppForms` expects. */
    getFormValues() {
      const W = smppclient.Widgets;
      const f = this.__fields;
      return {
        host: f.host.getValue() || "",
        port: f.port.getValue(),
        tlsEnabled: f.tlsEnabled.getValue(),
        rejectUnauthorized: f.rejectUnauthorized.getValue(),
        servername: f.servername.getValue() || "",
        caPath: f.caPath.getValue() || "",
        clientCertPath: f.clientCertPath.getValue() || "",
        clientKeyPath: f.clientKeyPath.getValue() || "",
        clientKeyPassphrase: f.clientKeyPassphrase.getValue() || "",
        systemId: f.systemId.getValue() || "",
        password: f.password.getValue() || "",
        systemType: f.systemType.getValue() || "",
        interfaceVersionLabel: W.selectedLabel(f.interfaceVersion),
        bindModeLabel: W.selectedLabel(f.bindMode),
        addrTon: 0,
        addrNpi: 0,
        addressRange: f.addressRange.getValue() || "",
        windowSize: f.windowSize.getValue(),
        responseTimeoutMs: f.responseTimeoutMs.getValue(),
        connectTimeoutMs: f.connectTimeoutMs.getValue(),
        correlation: {
          preferTlv: true,
          parseText: true,
          normaliseRadix: true,
          stripLeadingZeros: true
        }
      };
    },

    /**
     * Put values into the form. Only the keys present are touched, so a
     * partial profile does not blank the rest.
     */
    setFormValues(values) {
      const W = smppclient.Widgets;
      const f = this.__fields;
      const text = (field, key) => {
        if (values[key] !== undefined && values[key] !== null) {
          field.setValue(String(values[key]));
        }
      };
      const flag = (field, key) => {
        if (values[key] !== undefined) {
          field.setValue(!!values[key]);
        }
      };
      const num = (field, key) => {
        if (values[key] !== undefined && values[key] !== null) {
          field.setValue(Number(values[key]));
        }
      };

      text(f.host, "host");
      num(f.port, "port");
      flag(f.tlsEnabled, "tlsEnabled");
      flag(f.rejectUnauthorized, "rejectUnauthorized");
      text(f.servername, "servername");
      text(f.caPath, "caPath");
      text(f.clientCertPath, "clientCertPath");
      text(f.clientKeyPath, "clientKeyPath");
      text(f.clientKeyPassphrase, "clientKeyPassphrase");
      text(f.name, "name");
      text(f.systemId, "systemId");
      text(f.password, "password");
      text(f.systemType, "systemType");
      text(f.addressRange, "addressRange");
      num(f.windowSize, "windowSize");
      num(f.responseTimeoutMs, "responseTimeoutMs");
      num(f.connectTimeoutMs, "connectTimeoutMs");
      if (values.interfaceVersionLabel) {
        W.selectByLabel(f.interfaceVersion, values.interfaceVersionLabel);
      }
      if (values.bindModeLabel) {
        W.selectByLabel(f.bindMode, values.bindModeLabel);
      }
    },

    /** The text the state indicator currently shows. */
    getStateLabel() {
      return this.__state.getValue();
    },

    /** The counters block, as shown. */
    getCountersText() {
      return this.__counters.getValue();
    },

    /* ---------------------------------------------------------------- */
    /* actions                                                           */

    /** Connect and bind with what the form currently holds. */
    bind() {
      const values = this.getFormValues();

      // Length limits are advisory: warn, then send what was typed.
      const warnings = SmppForms.lengthWarnings({
        system_id: values.systemId,
        password: values.password,
        system_type: values.systemType,
        address_range: values.addressRange
      });
      warnings.forEach(w => this.fireDataEvent("notice", w));

      this.__api
        .call("connect", SmppForms.buildConnectRequest(values))
        .then(() => this.__api.call("bind", SmppForms.buildBindRequest(values)))
        .then(result => {
          this.fireDataEvent(
            "notice",
            "Bound to " + values.host + ":" + values.port +
              (result && result.systemId ? " as " + result.systemId : "")
          );
          this.fireDataEvent("bound", SmppForms.buildBindRequest(values).mode);
        })
        .catch(e => this.fireDataEvent("notice", "Bind failed: " + e.message));
    },

    /** Unbind and drop the connection. */
    unbind() {
      this.__api
        .call("unbind")
        .then(() => this.__api.call("disconnect"))
        .then(() => this.fireDataEvent("notice", "Unbound"))
        .catch(e => this.fireDataEvent("notice", "Unbind failed: " + e.message));
    },

    /** Send one enquire_link. */
    enquireLink() {
      this.__api
        .call("sendPdu", { commandId: 0x00000015 })
        .then(() => this.fireDataEvent("notice", "enquire_link answered"))
        .catch(e => this.fireDataEvent("notice", "enquire_link failed: " + e.message));
    },

    /* ---------------------------------------------------------------- */
    /* profiles                                                          */

    __reloadProfiles() {
      this.__api
        .call("listProfiles")
        .then(profiles => {
          this.__profiles.removeAll();
          (profiles || []).forEach(p => {
            const item = new qx.ui.form.ListItem(p.name || p.id);
            item.setModel(p);
            this.__profiles.add(item);
          });
        })
        .catch(() => undefined);
    },

    __loadSelectedProfile() {
      const profile = smppclient.Widgets.selectedValue(this.__profiles);
      if (!profile) {
        return;
      }
      this.setFormValues({
        host: profile.host,
        port: profile.port,
        tlsEnabled: !!(profile.tls && profile.tls.enabled),
        rejectUnauthorized: !profile.tls || profile.tls.rejectUnauthorized !== false,
        servername: (profile.tls && profile.tls.servername) || "",
        caPath: profile.caPath,
        clientCertPath: profile.clientCertPath,
        clientKeyPath: profile.clientKeyPath,
        name: profile.name,
        systemId: profile.bind && profile.bind.systemId,
        systemType: profile.bind && profile.bind.systemType
      });

      this.__api
        .call("profileSecrets", profile.id)
        .then(secrets =>
          this.setFormValues({
            password: (secrets && secrets.password) || "",
            clientKeyPassphrase: (secrets && secrets.keyPassphrase) || ""
          })
        )
        .catch(() => undefined);
    },

    __saveProfile() {
      const values = this.getFormValues();
      const existing = smppclient.Widgets.selectedValue(this.__profiles);
      const name = this.__fields.name.getValue() || values.host;
      const id = existing && existing.name === name ? existing.id : "p" + Date.now();

      const profile = {
        id: id,
        name: name,
        host: values.host,
        port: values.port,
        bind: {
          systemId: values.systemId,
          systemType: values.systemType,
          interfaceVersion: SmppForms.buildBindRequest(values).interfaceVersion,
          mode: SmppForms.buildBindRequest(values).mode
        },
        tls: {
          enabled: values.tlsEnabled,
          rejectUnauthorized: values.rejectUnauthorized,
          servername: values.servername || undefined
        },
        correlation: values.correlation,
        caPath: values.caPath || undefined,
        clientCertPath: values.clientCertPath || undefined,
        clientKeyPath: values.clientKeyPath || undefined
      };

      this.__api
        .call("saveProfile", profile, values.password, values.clientKeyPassphrase)
        .then(() => {
          this.fireDataEvent("notice", 'Saved profile "' + name + '"');
          this.__reloadProfiles();
        })
        .catch(e => this.fireDataEvent("notice", "Could not save the profile: " + e.message));
    },

    __deleteProfile() {
      const profile = smppclient.Widgets.selectedValue(this.__profiles);
      if (!profile) {
        return;
      }
      this.__api
        .call("removeProfile", profile.id)
        .then(() => {
          this.fireDataEvent("notice", 'Deleted profile "' + profile.name + '"');
          this.__reloadProfiles();
        })
        .catch(() => undefined);
    },

    /* ---------------------------------------------------------------- */
    /* events from the main process                                      */

    __onState(payload) {
      this.__state.setValue(SmppForms.stateLabel(payload.state, payload.mode));
      const colours = {
        unbound: "#999999",
        binding: "#c08000",
        bound: "#2e8b2e",
        unbinding: "#c08000",
        closed: "#b03030"
      };
      this.__dot.setTextColor(colours[payload.state] || "#999999");
    },

    __onCounters(counters) {
      const names = Object.keys(counters || {}).sort();
      const lines = names.map(name => {
        const c = counters[name];
        return name + "  sent " + c.sent + " / recv " + c.received;
      });
      this.__counters.setValue(lines.join("\n"));
    }
  }
});
