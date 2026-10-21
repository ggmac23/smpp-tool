/**
 * Offset, hex and ASCII columns for one PDU's bytes.
 *
 * @ignore(SmppForms)
 * @ignore(SmppForms.*)
 */
qx.Class.define("smppclient.HexView", {
  extend: qx.ui.container.Composite,

  construct() {
    super(new qx.ui.layout.Grow());

    this.__area = smppclient.Widgets.mono("");
    this.add(this.__area);
  },

  members: {
    __area: null,
    __hex: "",

    /** Show the bytes of `hex`, which may contain whitespace. */
    setHex(hex) {
      this.__hex = hex || "";
      this.__area.setValue(SmppForms.hexDump(this.__hex));
    },

    /** The hex currently shown, cleaned of whitespace. */
    getHex() {
      return this.__hex;
    },

    /** The rendered dump, as shown. */
    getText() {
      return this.__area.getValue();
    },

    clear() {
      this.setHex("");
    }
  }
});
