/**
 * Small widget helpers shared by the panels: labelled rows, section headings
 * and selects built from the option lists in `SmppForms`.
 */
qx.Class.define("smppclient.Widgets", {
  type: "static",

  statics: {
    /** Blue for "open or close the connection". */
    ACCENT_BIND: "#1f6fd0",
    /** Green for "put this on the wire". */
    ACCENT_SEND: "#1e8e3e",

    /** A bold, small-caps section heading. */
    heading(text) {
      const label = new qx.ui.basic.Label(text);
      label.setFont("bold");
      label.setMarginTop(8);
      return label;
    },

    /** A grid suitable for label/field rows. */
    formGrid() {
      const layout = new qx.ui.layout.Grid(8, 5);
      layout.setColumnFlex(1, 1);
      layout.setColumnAlign(0, "right", "middle");
      const grid = new qx.ui.container.Composite(layout);
      // Fields stay readable instead of stretching across a wide window.
      grid.setMaxWidth(760);
      return grid;
    },

    /**
     * A two-field-per-row grid: label, field, label, field. Halves the height
     * of a long form so it fits without scrolling.
     */
    compactGrid() {
      const layout = new qx.ui.layout.Grid(8, 5);
      layout.setColumnFlex(1, 1);
      layout.setColumnFlex(3, 1);
      layout.setColumnAlign(0, "right", "middle");
      layout.setColumnAlign(2, "right", "middle");
      const grid = new qx.ui.container.Composite(layout);
      grid.setMaxWidth(980);
      return grid;
    },

    /** Two labelled fields on one row of a `compactGrid`. */
    pair(grid, row, label1, field1, label2, field2) {
      grid.add(new qx.ui.basic.Label(label1), { row: row, column: 0 });
      grid.add(field1, { row: row, column: 1 });
      if (field2) {
        grid.add(new qx.ui.basic.Label(label2), { row: row, column: 2 });
        grid.add(field2, { row: row, column: 3 });
      }
      return [field1, field2];
    },

    /** One labelled field spanning both field columns of a `compactGrid`. */
    wide(grid, row, label, field) {
      grid.add(new qx.ui.basic.Label(label), { row: row, column: 0 });
      grid.add(field, { row: row, column: 1, colSpan: 3 });
      return field;
    },

    /**
     * An address cell: the TON/NPI selector and the address on one line, so
     * "Source type" and "source_addr" stop costing two rows.
     */
    addressCell(typeSelect, addressField) {
      const box = new qx.ui.container.Composite(new qx.ui.layout.HBox(6));
      typeSelect.setWidth(96);
      box.add(typeSelect);
      box.add(addressField, { flex: 1 });
      return box;
    },

    /** Put `field` in `grid` at `row`, labelled. Returns the field. */
    row(grid, row, labelText, field) {
      grid.add(new qx.ui.basic.Label(labelText), { row: row, column: 0 });
      grid.add(field, { row: row, column: 1 });
      return field;
    },

    text(value, placeholder) {
      const f = new qx.ui.form.TextField(value === undefined ? "" : String(value));
      if (placeholder) {
        f.setPlaceholder(placeholder);
      }
      return f;
    },

    password(value) {
      return new qx.ui.form.PasswordField(value === undefined ? "" : String(value));
    },

    number(value, min, max) {
      const f = new qx.ui.form.Spinner(min === undefined ? 0 : min, value, max === undefined ? 999999 : max);
      f.setMaxWidth(120);
      return f;
    },

    check(label, value) {
      const f = new qx.ui.form.CheckBox(label);
      f.setValue(!!value);
      return f;
    },

    /**
     * A select built from `[{label, value}]`. The model carries the value, so
     * callers read `getSelection()[0].getModel()`.
     */
    select(options, selectedLabel) {
      const box = new qx.ui.form.SelectBox();
      options.forEach(option => {
        const item = new qx.ui.form.ListItem(option.label);
        item.setModel(option.value === undefined ? option.label : option.value);
        box.add(item);
        if (selectedLabel !== undefined && option.label === selectedLabel) {
          box.setSelection([item]);
        }
      });
      return box;
    },

    /** The label of the current selection, or "" when there is none. */
    selectedLabel(box) {
      const selection = box.getSelection();
      return selection.length > 0 ? selection[0].getLabel() : "";
    },

    /** The model of the current selection, or undefined. */
    selectedValue(box) {
      const selection = box.getSelection();
      return selection.length > 0 ? selection[0].getModel() : undefined;
    },

    selectByLabel(box, label) {
      const match = box.getChildren().filter(item => item.getLabel() === label);
      if (match.length > 0) {
        box.setSelection([match[0]]);
      }
    },

    /**
     * A button for the one action a panel is really for. The accent is set
     * explicitly rather than taken from the theme: these two buttons are
     * meant to be the ones your eye lands on, which is a decision about this
     * panel and not about the palette.
     */
    primaryButton(label, colour) {
      const b = new qx.ui.form.Button(label);
      // The theme's button decorator paints over a background colour, which
      // left white text on a pale box. Dropping the decorator lets the accent
      // through.
      b.setDecorator(null);
      b.setBackgroundColor(colour || smppclient.Widgets.ACCENT_SEND);
      b.setTextColor("#ffffff");
      b.setFont("bold");
      b.setPadding(7, 18);
      b.setMinWidth(130);
      b.setCenter(true);
      return b;
    },

    /** A row of buttons. */
    buttons(buttons) {
      const box = new qx.ui.container.Composite(new qx.ui.layout.HBox(6));
      buttons.forEach(b => box.add(b));
      return box;
    },

    /**
     * A titled group around `content`. Pass `flex` when the content should
     * take the spare height.
     */
    group(title, content, flex) {
      const box = new qx.ui.groupbox.GroupBox(title);
      box.setLayout(new qx.ui.layout.VBox(4));
      box.setContentPadding(8, 8, 8, 8);
      box.add(content, flex ? { flex: flex } : undefined);
      return box;
    },

    /** A monospaced read-only area, for hex and counters. */
    mono(text) {
      const area = new qx.ui.form.TextArea(text === undefined ? "" : text);
      area.setReadOnly(true);
      area.setFont(qx.bom.Font.fromString("11px monospace"));
      area.setWrap(false);
      return area;
    }
  }
});
