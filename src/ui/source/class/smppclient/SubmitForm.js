/**
 * The submit_sm form: the short set always visible, the full field set behind
 * "Show all", and a live counter under the message box.
 *
 * Every value goes to `SmppForms.buildSubmitInput`, which decides the PDUs and
 * the warnings. Warnings are shown; they never stop a send.
 *
 * @ignore(SmppForms)
 * @ignore(SmppForms.*)
 */
qx.Class.define("smppclient.SubmitForm", {
  extend: qx.ui.container.Scroll,

  construct() {
    super();

    const W = smppclient.Widgets;
    this.__api = smppclient.Api.getInstance();
    this.__fields = {};

    const root = new qx.ui.container.Composite(new qx.ui.layout.VBox(6));
    root.setPadding(8);

    root.add(this.__buildAddresses());
    root.add(this.__buildAdvancedToggle());
    root.add(this.__buildAdvanced());
    root.add(W.heading("Message"));
    root.add(this.__buildBody());
    root.add(this.__buildActions());

    this.add(root);
    this.__refreshCounter();
  },

  events: {
    /** A line for the shell's status bar. Data: string. */
    notice: "qx.event.type.Data"
  },

  members: {
    __api: null,
    __fields: null,
    __advanced: null,
    __counter: null,
    __tlvs: null,

    /* ---------------------------------------------------------------- */
    /* construction                                                      */

    __buildAddresses() {
      const W = smppclient.Widgets;
      const grid = W.compactGrid();
      const f = this.__fields;

      const tonNpiOptions = SmppForms.TON_NPI_PRESETS.map(p => ({ label: p.label, value: p.label }));

      // Type and address share a line: two rows instead of four.
      f.sourceTonNpi = W.select(tonNpiOptions, "Intl");
      f.sourceAddr = W.text("", "393331234567");
      W.wide(grid, 0, "source", W.addressCell(f.sourceTonNpi, f.sourceAddr));

      f.destTonNpi = W.select(tonNpiOptions, "Intl");
      f.destinationAddr = W.text("", "393337654321");
      W.wide(grid, 1, "destination", W.addressCell(f.destTonNpi, f.destinationAddr));

      f.registeredDelivery = W.select(SmppForms.REGISTERED_DELIVERY, "1 — on failure and success");
      f.dataCoding = W.select(SmppForms.DATA_CODINGS, "0 — GSM 03.38 default");
      W.pair(grid, 2, "registered_delivery", f.registeredDelivery, "data_coding", f.dataCoding);

      f.scheduleDelivery = W.select(
        SmppForms.SCHEDULE_DELIVERY.map(o => ({ label: o.label })),
        "Immediate"
      );
      f.validityPeriod = W.select(
        SmppForms.VALIDITY_PERIODS.map(o => ({ label: o.label })),
        "Default"
      );
      W.pair(grid, 3, "schedule", f.scheduleDelivery, "validity", f.validityPeriod);

      return grid;
    },

    __buildAdvancedToggle() {
      const toggle = new qx.ui.form.CheckBox("Show all");
      toggle.addListener("changeValue", e =>
        this.__advanced.setVisibility(e.getData() ? "visible" : "excluded")
      );
      this.__fields.showAll = toggle;
      return toggle;
    },

    __buildAdvanced() {
      const W = smppclient.Widgets;
      const group = new qx.ui.groupbox.GroupBox("All fields");
      group.setLayout(new qx.ui.layout.VBox(4));

      const grid = W.compactGrid();
      const f = this.__fields;

      f.serviceType = W.text("");
      f.esmClass = W.select(SmppForms.ESM_CLASSES, "0 — Default");
      W.pair(grid, 0, "service_type", f.serviceType, "esm_class", f.esmClass);

      f.protocolId = W.number(0, 0, 255);
      f.priorityFlag = W.select(SmppForms.PRIORITY_FLAGS, "0 — non-priority");
      W.pair(grid, 1, "protocol_id", f.protocolId, "priority_flag", f.priorityFlag);

      f.replaceIfPresent = W.number(0, 0, 1);
      f.smDefaultMsgId = W.number(0, 0, 255);
      W.pair(grid, 2, "replace_if_present", f.replaceIfPresent, "sm_default_msg_id", f.smDefaultMsgId);

      f.udhHex = W.text("", "e.g. 05 00 03 2a 03 01");
      W.wide(grid, 3, "UDH (hex)", f.udhHex);
      group.add(grid);

      group.add(W.heading("TLVs"));
      this.__tlvs = new smppclient.TlvEditor();
      group.add(this.__tlvs);

      group.setVisibility("excluded");
      this.__advanced = group;
      return group;
    },

    __buildBody() {
      const W = smppclient.Widgets;
      const box = new qx.ui.container.Composite(new qx.ui.layout.VBox(4));
      const f = this.__fields;

      const options = W.compactGrid();
      f.placement = W.select(SmppForms.BODY_PLACEMENTS, "short_message");
      f.segmentation = W.select(SmppForms.SEGMENTATION_MODES, "UDH concatenation");
      W.pair(options, 0, "body in", f.placement, "segmentation", f.segmentation);

      f.packing = W.select(SmppForms.GSM7_PACKINGS, "Unpacked (one septet per octet)");
      W.wide(options, 1, "GSM7 layout", f.packing);
      box.add(options);

      f.text = new qx.ui.form.TextArea("");
      f.text.setMinHeight(90);
      f.text.addListener("input", () => this.__refreshCounter());
      box.add(f.text);

      f.dataCoding.addListener("changeSelection", () => this.__refreshCounter());
      f.segmentation.addListener("changeSelection", () => this.__refreshCounter());
      f.packing.addListener("changeSelection", () => this.__refreshCounter());

      this.__counter = new qx.ui.basic.Label("");
      box.add(this.__counter);

      box.add(this.__buildSamples());
      return box;
    },

    __buildSamples() {
      const W = smppclient.Widgets;
      const samples = [
        { label: "GSM 1 part", key: "gsm1", coding: "0 — GSM 03.38 default" },
        { label: "UCS2 1 part", key: "ucs2_1", coding: "8 — UCS2" },
        { label: "GSM 3 parts", key: "gsm3", coding: "0 — GSM 03.38 default" },
        { label: "UCS2 3 parts", key: "ucs2_3", coding: "8 — UCS2" }
      ];

      const buttons = samples.map(sample => {
        const b = new qx.ui.form.Button(sample.label);
        b.addListener("execute", () => {
          this.__fields.text.setValue(SmppForms.SAMPLES[sample.key]);
          W.selectByLabel(this.__fields.dataCoding, sample.coding);
          this.__refreshCounter();
        });
        return b;
      });

      const clear = new qx.ui.form.Button("Clear");
      clear.addListener("execute", () => {
        this.__fields.text.setValue("");
        this.__refreshCounter();
      });
      buttons.push(clear);

      return W.buttons(buttons);
    },

    __buildActions() {
      const send = new qx.ui.form.Button("Send submit_sm");
      send.addListener("execute", () => this.send());
      return smppclient.Widgets.buttons([send]);
    },

    /* ---------------------------------------------------------------- */
    /* values                                                            */

    /** The form in the shape `SmppForms.buildSubmitInput` expects. */
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
        protocolId: f.protocolId.getValue(),
        priorityFlag: W.selectedValue(f.priorityFlag) || 0,
        registeredDelivery: W.selectedValue(f.registeredDelivery) || 0,
        scheduleDeliveryLabel: W.selectedLabel(f.scheduleDelivery),
        validityPeriodLabel: W.selectedLabel(f.validityPeriod),
        dataCoding: W.selectedValue(f.dataCoding) || 0,
        replaceIfPresent: f.replaceIfPresent.getValue(),
        smDefaultMsgId: f.smDefaultMsgId.getValue(),
        text: f.text.getValue() || "",
        placement: W.selectedValue(f.placement) || "short_message",
        segmentation: W.selectedValue(f.segmentation) || "none",
        packing: W.selectedValue(f.packing) || "unpacked",
        udhHex: f.udhHex.getValue() || "",
        tlvs: this.__tlvs.getValues()
      };
    },

    /** Put values into the form. Only the keys present are touched. */
    setFormValues(values) {
      const W = smppclient.Widgets;
      const f = this.__fields;
      if (values.sourceAddr !== undefined) {
        f.sourceAddr.setValue(String(values.sourceAddr));
      }
      if (values.destinationAddr !== undefined) {
        f.destinationAddr.setValue(String(values.destinationAddr));
      }
      if (values.text !== undefined) {
        f.text.setValue(String(values.text));
      }
      if (values.serviceType !== undefined) {
        f.serviceType.setValue(String(values.serviceType));
      }
      if (values.udhHex !== undefined) {
        f.udhHex.setValue(String(values.udhHex));
      }
      if (values.dataCodingLabel) {
        W.selectByLabel(f.dataCoding, values.dataCodingLabel);
      }
      if (values.segmentationLabel) {
        W.selectByLabel(f.segmentation, values.segmentationLabel);
      }
      if (values.placementLabel) {
        W.selectByLabel(f.placement, values.placementLabel);
      }
      if (values.showAll !== undefined) {
        f.showAll.setValue(!!values.showAll);
      }
      this.__refreshCounter();
    },

    /** What the live counter currently reads. */
    getCounterText() {
      return this.__counter.getValue();
    },

    /* ---------------------------------------------------------------- */
    /* actions                                                           */

    __refreshCounter() {
      const built = SmppForms.buildSubmitInput(this.getFormValues());
      this.__counter.setValue(SmppForms.counterText(built.measurement));
    },

    /**
     * Send the message. A multi-part body goes out as several submit_sm in
     * order; the first failure is reported and the rest are not sent, because
     * a half-delivered concatenation is worth knowing about.
     */
    send() {
      const built = SmppForms.buildSubmitInput(this.getFormValues());
      built.warnings.forEach(w => this.fireDataEvent("notice", w));

      const total = built.requests.length;
      let sent = 0;

      const next = () => {
        if (sent >= total) {
          this.fireDataEvent(
            "notice",
            "Sent " + total + " submit_sm PDU" + (total === 1 ? "" : "s")
          );
          return Promise.resolve();
        }
        const request = built.requests[sent];
        sent += 1;
        return this.__api.call("sendPdu", request).then(result => {
          if (result && result.commandStatus !== 0) {
            this.fireDataEvent(
              "notice",
              "Part " + sent + " of " + total + " came back with status " + result.commandStatus
            );
          }
          return next();
        });
      };

      return next().catch(e =>
        this.fireDataEvent("notice", "Send failed on part " + sent + ": " + e.message)
      );
    }
  }
});
