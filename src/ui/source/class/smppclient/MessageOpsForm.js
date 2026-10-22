/**
 * query_sm, cancel_sm, replace_sm and the SMPP v5.0 broadcast family — every
 * command that refers to a message already submitted.
 *
 * One form with a command selector, because they share almost every field.
 * There is no broadcast_multi: SMPP does not define one.
 *
 * @ignore(SmppForms)
 * @ignore(SmppForms.*)
 */
qx.Class.define("smppclient.MessageOpsForm", {
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

    f.command = W.row(
      grid,
      0,
      "Command",
      W.select(
        [
          { label: "query_sm", value: "query_sm" },
          { label: "cancel_sm", value: "cancel_sm" },
          { label: "replace_sm", value: "replace_sm" },
          { label: "broadcast_sm", value: "broadcast_sm" },
          { label: "query_broadcast_sm", value: "query_broadcast_sm" },
          { label: "cancel_broadcast_sm", value: "cancel_broadcast_sm" }
        ],
        "query_sm"
      )
    );
    f.messageId = W.row(grid, 1, "message_id", W.text("", "up to 64 characters"));
    f.serviceType = W.row(grid, 2, "service_type", W.text(""));
    f.sourceTonNpi = W.row(grid, 3, "Source type", W.select(tonNpi, "Intl"));
    f.sourceAddr = W.row(grid, 4, "source_addr", W.text(""));
    f.destTonNpi = W.row(grid, 5, "Dest type", W.select(tonNpi, "Intl"));
    f.destinationAddr = W.row(grid, 6, "destination_addr", W.text(""));
    f.scheduleDelivery = W.row(
      grid,
      7,
      "schedule_delivery_time",
      W.select(SmppForms.SCHEDULE_DELIVERY.map(o => ({ label: o.label })), "Immediate")
    );
    f.validityPeriod = W.row(
      grid,
      8,
      "validity_period",
      W.select(SmppForms.VALIDITY_PERIODS.map(o => ({ label: o.label })), "Default")
    );
    f.registeredDelivery = W.row(
      grid,
      9,
      "registered_delivery",
      W.select(SmppForms.REGISTERED_DELIVERY, "1 — on failure and success")
    );
    f.dataCoding = W.row(grid, 10, "data_coding", W.select(SmppForms.DATA_CODINGS, "0 — GSM 03.38 default"));
    f.smDefaultMsgId = W.row(grid, 11, "sm_default_msg_id", W.number(0, 0, 255));
    root.add(grid);

    root.add(W.heading("Replacement / broadcast body"));
    f.text = new qx.ui.form.TextArea("");
    f.text.setHeight(80);
    root.add(f.text);

    const send = W.primaryButton("Send");
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

    /** The selected command name. */
    getCommand() {
      return smppclient.Widgets.selectedValue(this.__fields.command);
    },

    setCommand(name) {
      smppclient.Widgets.selectByLabel(this.__fields.command, name);
    },

    getFormValues() {
      const W = smppclient.Widgets;
      const f = this.__fields;
      return {
        messageId: f.messageId.getValue() || "",
        serviceType: f.serviceType.getValue() || "",
        sourceTonNpiLabel: W.selectedLabel(f.sourceTonNpi),
        sourceAddr: f.sourceAddr.getValue() || "",
        destTonNpiLabel: W.selectedLabel(f.destTonNpi),
        destinationAddr: f.destinationAddr.getValue() || "",
        scheduleDeliveryLabel: W.selectedLabel(f.scheduleDelivery),
        validityPeriodLabel: W.selectedLabel(f.validityPeriod),
        registeredDelivery: W.selectedValue(f.registeredDelivery) || 0,
        smDefaultMsgId: f.smDefaultMsgId.getValue(),
        text: f.text.getValue() || "",
        dataCoding: W.selectedValue(f.dataCoding) || 0
      };
    },

    setFormValues(values) {
      const f = this.__fields;
      ["messageId", "serviceType", "sourceAddr", "destinationAddr", "text"].forEach(key => {
        if (values[key] !== undefined) {
          f[key].setValue(String(values[key]));
        }
      });
      if (values.command) {
        this.setCommand(values.command);
      }
    },

    /**
     * The request the form currently describes, without sending it. `bodyHex`
     * is the encoded body for the commands that carry one; query_sm and
     * cancel_sm ignore it.
     */
    build(bodyHex) {
      const values = this.getFormValues();
      const command = this.getCommand();
      switch (command) {
        case "query_sm":
          return SmppForms.buildQueryInput(values);
        case "cancel_sm":
          return SmppForms.buildCancelInput(values);
        case "replace_sm":
          return SmppForms.buildReplaceInput(values, bodyHex || "");
        default:
          return SmppForms.buildBroadcastInput(
            qx.lang.Object.mergeWith({ kind: command }, values, false),
            bodyHex || ""
          );
      }
    },

    send() {
      const values = this.getFormValues();
      // The body is encoded by the backend: no widget carries a codec.
      return this.__api
        .call("encode_body", { text: values.text, dataCoding: values.dataCoding, packing: "unpacked" })
        .then(encoded => this.__send(this.build(encoded.hex)))
        .catch(e => this.fireDataEvent("notice", this.getCommand() + " failed: " + e.message));
    },

    __send(built) {
      built.warnings.forEach(w => this.fireDataEvent("notice", w));
      return this.__api
        .call("send_pdu", { req: built.request })
        .then(result =>
          this.fireDataEvent(
            "notice",
            this.getCommand() + " answered with status " + (result ? result.commandStatus : "?")
          )
        )
        .catch(e => this.fireDataEvent("notice", this.getCommand() + " failed: " + e.message));
    }
  }
});
