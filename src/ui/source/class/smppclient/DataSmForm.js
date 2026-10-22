/**
 * data_sm. Its body always travels in the message_payload TLV, because the
 * command has no short_message field at all.
 *
 * @ignore(SmppForms)
 * @ignore(SmppForms.*)
 */
qx.Class.define("smppclient.DataSmForm", {
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
    f.destinationAddr = W.row(grid, 4, "destination_addr", W.text(""));
    f.esmClass = W.row(grid, 5, "esm_class", W.select(SmppForms.ESM_CLASSES, "0 — Default"));
    f.registeredDelivery = W.row(
      grid,
      6,
      "registered_delivery",
      W.select(SmppForms.REGISTERED_DELIVERY, "1 — on failure and success")
    );
    f.dataCoding = W.row(grid, 7, "data_coding", W.select(SmppForms.DATA_CODINGS, "0 — GSM 03.38 default"));
    root.add(grid);

    root.add(new qx.ui.basic.Label("The body goes in the message_payload TLV."));

    f.text = new qx.ui.form.TextArea("");
    f.text.setHeight(100);
    root.add(f.text);

    const send = W.primaryButton("Send data_sm", W.ACCENT_SEND);
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

    getFormValues() {
      const W = smppclient.Widgets;
      const f = this.__fields;
      return {
        serviceType: f.serviceType.getValue() || "",
        sourceTonNpiLabel: W.selectedLabel(f.sourceTonNpi),
        sourceAddr: f.sourceAddr.getValue() || "",
        destTonNpiLabel: W.selectedLabel(f.destTonNpi),
        destinationAddr: f.destinationAddr.getValue() || "",
        esmClass: W.selectedValue(f.esmClass) || 0,
        registeredDelivery: W.selectedValue(f.registeredDelivery) || 0,
        dataCoding: W.selectedValue(f.dataCoding) || 0,
        text: f.text.getValue() || "",
        placement: "message_payload"
      };
    },

    setFormValues(values) {
      const f = this.__fields;
      ["sourceAddr", "destinationAddr", "text", "serviceType"].forEach(key => {
        if (values[key] !== undefined) {
          f[key].setValue(String(values[key]));
        }
      });
    },

    send() {
      const built = SmppForms.buildDataSmInput(this.getFormValues());
      built.warnings.forEach(w => this.fireDataEvent("notice", w));
      return this.__api
        .call("sendPdu", built.request)
        .then(result =>
          this.fireDataEvent(
            "notice",
            "data_sm answered with status " + (result ? result.commandStatus : "?")
          )
        )
        .catch(e => this.fireDataEvent("notice", "data_sm failed: " + e.message));
    }
  }
});
