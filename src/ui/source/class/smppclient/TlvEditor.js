/**
 * Rows of TLVs: tag, value and whether the value was typed as hex or as text.
 *
 * Any tag is allowed, known or not — sending an unknown optional parameter is
 * a thing a tester needs to do.
 *
 * @ignore(SmppForms)
 * @ignore(SmppForms.*)
 */
qx.Class.define("smppclient.TlvEditor", {
  extend: qx.ui.container.Composite,

  construct() {
    super(new qx.ui.layout.VBox(4));

    this.__rows = [];
    this.__list = new qx.ui.container.Composite(new qx.ui.layout.VBox(2));
    this.add(this.__list);

    const add = new qx.ui.form.Button("Add TLV");
    add.addListener("execute", () => this.addRow());
    this.add(add);
  },

  members: {
    __rows: null,
    __list: null,

    /** Add an empty row, or one pre-filled from `{ tagHex, valueHex, valueText }`. */
    addRow(initial) {
      const W = smppclient.Widgets;
      const values = initial || {};

      const box = new qx.ui.container.Composite(new qx.ui.layout.HBox(4));

      const tag = W.text(values.tagHex || "", "tag hex, e.g. 0424");
      tag.setWidth(110);
      box.add(tag);

      const value = W.text(values.valueHex || values.valueText || "", "value");
      box.add(value, { flex: 1 });

      const asHex = W.check("hex", values.valueHex !== undefined);
      box.add(asHex);

      const remove = new qx.ui.form.Button("−");
      remove.setWidth(28);
      box.add(remove);

      const row = { box: box, tag: tag, value: value, asHex: asHex };
      this.__rows.push(row);
      this.__list.add(box);

      remove.addListener("execute", () => {
        this.__list.remove(box);
        this.__rows = this.__rows.filter(r => r !== row);
      });

      return row;
    },

    /** The rows, in the shape `SmppForms.buildSubmitInput` expects. */
    getValues() {
      return this.__rows
        .filter(r => (r.tag.getValue() || "").length > 0)
        .map(r =>
          r.asHex.getValue()
            ? { tagHex: r.tag.getValue(), valueHex: r.value.getValue() || "" }
            : { tagHex: r.tag.getValue(), valueText: r.value.getValue() || "" }
        );
    },

    /** How many rows are present. */
    getRowCount() {
      return this.__rows.length;
    },

    clear() {
      this.__list.removeAll();
      this.__rows = [];
    }
  }
});
