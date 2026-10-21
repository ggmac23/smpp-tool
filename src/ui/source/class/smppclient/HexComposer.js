/**
 * Compose a PDU byte by byte and send it exactly as typed.
 *
 * This is the part of the tool no form can replace: a wrong command_length, a
 * truncated PDU, an unknown command_id, trailing rubbish. The preview below
 * the box says what the bytes would decode as, or why they will not — and
 * either way the Send button stays live, because sending what will not decode
 * is the point.
 *
 * @ignore(SmppForms)
 * @ignore(SmppForms.*)
 */
qx.Class.define("smppclient.HexComposer", {
  extend: qx.ui.container.Composite,

  construct() {
    super(new qx.ui.layout.VBox(6));
    this.setPadding(8);

    this.__api = smppclient.Api.getInstance();

    this.add(
      new qx.ui.basic.Label(
        "Bytes go on the wire exactly as typed. Nothing is validated and nothing is rewritten."
      )
    );

    this.__input = new qx.ui.form.TextArea("");
    this.__input.setMinHeight(110);
    this.__input.setFont(qx.bom.Font.fromString("12px monospace"));
    this.__input.addListener("input", () => this.refresh());
    this.add(this.__input);

    this.__summary = new qx.ui.basic.Label("");
    this.add(this.__summary);

    this.add(this.__buildActions());

    this.add(smppclient.Widgets.heading("Preview"));
    this.__preview = smppclient.Widgets.mono("");
    this.add(this.__preview, { flex: 1 });

    this.refresh();
  },

  events: {
    notice: "qx.event.type.Data"
  },

  members: {
    __api: null,
    __input: null,
    __summary: null,
    __preview: null,
    __sendButton: null,

    __buildActions() {
      this.__sendButton = new qx.ui.form.Button("Send bytes");
      this.__sendButton.addListener("execute", () => this.send());

      const clear = new qx.ui.form.Button("Clear");
      clear.addListener("execute", () => this.setHex(""));

      const tidy = new qx.ui.form.Button("Tidy");
      tidy.addListener("execute", () => this.tidy());

      return smppclient.Widgets.buttons([this.__sendButton, tidy, clear]);
    },

    /** Load bytes into the box, from the trace's "Edit as hex" or elsewhere. */
    setHex(hex) {
      this.__input.setValue(hex ? SmppForms.formatHex(hex) : "");
      this.refresh();
    },

    getHex() {
      return this.__input.getValue() || "";
    },

    /** Re-space the hex without changing a single byte. */
    tidy() {
      try {
        this.__input.setValue(SmppForms.formatHex(SmppForms.parseHex(this.getHex())));
      } catch (e) {
        this.fireDataEvent("notice", "Cannot tidy: " + e.message);
      }
      this.refresh();
    },

    /** The preview line, as shown. */
    getSummaryText() {
      return this.__summary.getValue();
    },

    /** The decoded field list, as shown. */
    getPreviewText() {
      return this.__preview.getValue();
    },

    isSendEnabled() {
      return this.__sendButton.getEnabled();
    },

    refresh() {
      let preview;
      try {
        preview = SmppForms.previewPdu(this.getHex());
      } catch (e) {
        this.__summary.setValue(e.message);
        this.__summary.setTextColor("#b03030");
        this.__preview.setValue("");
        this.__sendButton.setEnabled(false);
        return null;
      }

      if (preview.ok) {
        this.__summary.setValue(preview.byteCount + " bytes — " + preview.summary);
        this.__summary.setTextColor(null);
        this.__preview.setValue(
          preview.fields.map(f => f.name + " = " + f.value).join("\n")
        );
      } else {
        this.__summary.setValue(
          preview.byteCount + " bytes — will not decode: " + preview.error
        );
        this.__summary.setTextColor("#b03030");
        this.__preview.setValue(
          "These bytes do not parse as a PDU.\nThey will be sent exactly as typed."
        );
      }

      this.__sendButton.setEnabled(preview.sendable);
      return preview;
    },

    send() {
      const hex = this.getHex();
      return this.__api
        .call("sendRaw", hex)
        .then(() => {
          const bytes = SmppForms.parseHex(hex).length;
          this.fireDataEvent("notice", "Sent " + bytes + " raw bytes");
        })
        .catch(e => this.fireDataEvent("notice", "Send failed: " + e.message));
    }
  }
});
