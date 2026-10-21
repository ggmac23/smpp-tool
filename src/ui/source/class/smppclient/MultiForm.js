/**
 * submit_multi: the submit_sm field set plus a list of destinations, one per
 * line, and distribution-list names alongside them.
 *
 * @ignore(SmppForms)
 * @ignore(SmppForms.*)
 */
qx.Class.define("smppclient.MultiForm", {
  extend: qx.ui.container.Scroll,

  construct() {
    super();

    const W = smppclient.Widgets;
    this.__api = smppclient.Api.getInstance();
    this.__fields = {};

    const root = new qx.ui.container.Composite(new qx.ui.layout.VBox(6));
    root.setPadding(8);

    const grid = W.formGrid();
    const f = this.__fields;
    const tonNpi = SmppForms.TON_NPI_PRESETS.map(p => ({ label: p.label, value: p.label }));

    f.serviceType = W.row(grid, 0, "service_type", W.text(""));
    f.sourceTonNpi = W.row(grid, 1, "Source type", W.select(tonNpi, "Intl"));
    f.sourceAddr = W.row(grid, 2, "source_addr", W.text(""));
    f.destTonNpi = W.row(grid, 3, "Dest type", W.select(tonNpi, "Intl"));
    f.esmClass = W.row(grid, 4, "esm_class", W.select(SmppForms.ESM_CLASSES, "0 — Default"));
    f.protocolId = W.row(grid, 5, "protocol_id", W.number(0, 0, 255));
    f.priorityFlag = W.row(grid, 6, "priority_flag", W.select(SmppForms.PRIORITY_FLAGS, "0 — non-priority"));
    f.registeredDelivery = W.row(
      grid,
      7,
      "registered_delivery",
      W.select(SmppForms.REGISTERED_DELIVERY, "1 — on failure and success")
    );
    f.scheduleDelivery = W.row(
      grid,
      8,
      "schedule_delivery_time",
      W.select(SmppForms.SCHEDULE_DELIVERY.map(o => ({ label: o.label })), "Immediate")
    );
    f.validityPeriod = W.row(
      grid,
      9,
      "validity_period",
      W.select(SmppForms.VALIDITY_PERIODS.map(o => ({ label: o.label })), "Default")
    );
    f.dataCoding = W.row(grid, 10, "data_coding", W.select(SmppForms.DATA_CODINGS, "0 — GSM 03.38 default"));
    f.replaceIfPresent = W.row(grid, 11, "replace_if_present_flag", W.number(0, 0, 1));
    f.smDefaultMsgId = W.row(grid, 12, "sm_default_msg_id", W.number(0, 0, 255));
    root.add(grid);

    root.add(W.heading("Destinations — one address per line"));
    f.destinations = new qx.ui.form.TextArea("");
    f.destinations.setHeight(100);
    root.add(f.destinations);

    root.add(W.heading("Distribution lists — one name per line"));
    f.distributionLists = new qx.ui.form.TextArea("");
    f.distributionLists.setHeight(60);
    root.add(f.distributionLists);

    root.add(W.heading("Message"));
    f.text = new qx.ui.form.TextArea("");
    f.text.setHeight(80);
    root.add(f.text);

    this.__count = new qx.ui.basic.Label("0 destinations");
    root.add(this.__count);
    f.destinations.addListener("input", () => this.__refreshCount());
    f.distributionLists.addListener("input", () => this.__refreshCount());

    const send = new qx.ui.form.Button("Send submit_multi");
    send.addListener("execute", () => this.send());
    root.add(W.buttons([send]));

    this.add(root);
  },

  events: {
    notice: "qx.event.type.Data"
  },

  members: {
    __api: null,
    __fields: null,
    __count: null,

    getFormValues() {
      const W = smppclient.Widgets;
      const f = this.__fields;
      return {
        serviceType: f.serviceType.getValue() || "",
        sourceTonNpiLabel: W.selectedLabel(f.sourceTonNpi),
        sourceAddr: f.sourceAddr.getValue() || "",
        destTonNpiLabel: W.selectedLabel(f.destTonNpi),
        destinations: f.destinations.getValue() || "",
        distributionLists: f.distributionLists.getValue() || "",
        esmClass: W.selectedValue(f.esmClass) || 0,
        protocolId: f.protocolId.getValue(),
        priorityFlag: W.selectedValue(f.priorityFlag) || 0,
        registeredDelivery: W.selectedValue(f.registeredDelivery) || 0,
        scheduleDeliveryLabel: W.selectedLabel(f.scheduleDelivery),
        validityPeriodLabel: W.selectedLabel(f.validityPeriod),
        dataCoding: W.selectedValue(f.dataCoding) || 0,
        replaceIfPresent: f.replaceIfPresent.getValue(),
        smDefaultMsgId: f.smDefaultMsgId.getValue(),
        text: f.text.getValue() || ""
      };
    },

    setFormValues(values) {
      const f = this.__fields;
      ["sourceAddr", "destinations", "distributionLists", "text", "serviceType"].forEach(key => {
        if (values[key] !== undefined) {
          f[key].setValue(String(values[key]));
        }
      });
      this.__refreshCount();
    },

    getDestinationCount() {
      return SmppForms.buildMultiInput(this.getFormValues()).request.destList.length;
    },

    __refreshCount() {
      const n = this.getDestinationCount();
      this.__count.setValue(n + (n === 1 ? " destination" : " destinations"));
    },

    send() {
      const built = SmppForms.buildMultiInput(this.getFormValues());
      built.warnings.forEach(w => this.fireDataEvent("notice", w));
      return this.__api
        .call("sendPdu", built.request)
        .then(result =>
          this.fireDataEvent(
            "notice",
            "submit_multi to " +
              built.request.destList.length +
              " destination(s), status " +
              (result ? result.commandStatus : "?")
          )
        )
        .catch(e => this.fireDataEvent("notice", "submit_multi failed: " + e.message));
    }
  }
});
