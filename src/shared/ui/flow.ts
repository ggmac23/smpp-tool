/**
 * A ladder diagram of a PDU trace: time down the page, the ESME on the left,
 * the SMSC on the right, one labelled arrow per PDU.
 *
 * Built as an SVG string so the drawing logic is unit-tested without a DOM and
 * the widget only has to embed the result.
 */

export interface FlowInput {
  at: number;
  direction: "in" | "out";
  commandName: string;
  sequenceNumber: number;
  decodeError?: string;
}

export interface FlowStep {
  /** Seconds since the first PDU, to six decimals. */
  offsetFirst: string;
  /** Seconds since the previous PDU, to six decimals. */
  offsetPrev: string;
  label: string;
  sequenceNumber: number;
  /** true when the ESME sent it. */
  toRight: boolean;
  colour: string;
}

export interface FlowOptions {
  leftLabel?: string;
  rightLabel?: string;
  width?: number;
}

export function formatOffset(seconds: number): string {
  const n = seconds === 0 ? 0 : seconds;
  return n.toFixed(6);
}

const COLOURS = {
  submit: "#c026a8",
  deliver: "#8b2fc9",
  response: "#1f9d3a",
  ack: "#767676",
  bind: "#1f6fd0",
  error: "#c0392b",
  other: "#555555",
};

/** Arrows are coloured by family so a flow reads without squinting at names. */
export function arrowColour(commandName: string): string {
  if (commandName.startsWith("(undecodable)") || commandName.startsWith("unknown(")) {
    return COLOURS.error;
  }
  if (commandName === "deliver_sm_resp" || commandName === "data_sm_resp") return COLOURS.ack;
  if (commandName.endsWith("_resp")) return COLOURS.response;
  if (commandName.startsWith("deliver_sm")) return COLOURS.deliver;
  if (commandName.startsWith("submit") || commandName.startsWith("data_sm")) return COLOURS.submit;
  if (commandName.startsWith("bind") || commandName === "unbind") return COLOURS.bind;
  return COLOURS.other;
}

export function flowSteps(rows: FlowInput[]): FlowStep[] {
  if (rows.length === 0) return [];
  const first = rows[0].at;

  return rows.map((row, i) => {
    const previous = i === 0 ? row.at : rows[i - 1].at;
    const label = row.decodeError ? `${row.commandName} — decode error` : row.commandName;
    return {
      offsetFirst: formatOffset((row.at - first) / 1000),
      offsetPrev: formatOffset((row.at - previous) / 1000),
      label,
      sequenceNumber: row.sequenceNumber,
      toRight: row.direction === "out",
      colour: arrowColour(row.commandName),
    };
  });
}

function escapeXml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

const ROW_HEIGHT = 30;
const HEADER_HEIGHT = 56;
const FOOTER = 20;
const COL_FIRST_X = 10;
const COL_PREV_X = 108;
const LANE_LEFT = 230;

export function buildFlowSvg(rows: FlowInput[], opts: FlowOptions = {}): string {
  const width = opts.width ?? 940;
  const laneRight = width - 40;
  const steps = flowSteps(rows);

  if (steps.length === 0) {
    return [
      `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="80" viewBox="0 0 ${width} 80">`,
      `<text x="20" y="44" font-family="sans-serif" font-size="13" fill="#666666">`,
      `Nothing to draw yet — no PDUs in the trace.</text>`,
      `</svg>`,
    ].join("\n");
  }

  const height = HEADER_HEIGHT + steps.length * ROW_HEIGHT + FOOTER;
  const parts: string[] = [];

  parts.push(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}" font-family="sans-serif">`
  );
  parts.push(`<rect x="0" y="0" width="${width}" height="${height}" fill="#ffffff"/>`);

  // Column headings.
  parts.push(
    `<text x="${COL_FIRST_X}" y="20" font-size="11" fill="#444444">Offset from first</text>`,
    `<text x="${COL_PREV_X}" y="20" font-size="11" fill="#444444">Offset from prev.</text>`,
    `<text x="${LANE_LEFT}" y="20" font-size="12" font-weight="bold" fill="#1f6fd0">${escapeXml(opts.leftLabel ?? "ESME")}</text>`,
    `<text x="${laneRight}" y="20" font-size="12" font-weight="bold" fill="#1f6fd0" text-anchor="end">${escapeXml(opts.rightLabel ?? "SMSC")}</text>`
  );
  parts.push(
    `<line x1="${COL_FIRST_X}" y1="28" x2="${width - 10}" y2="28" stroke="#cccccc" stroke-width="1"/>`
  );

  // Lifelines.
  const lifelineTop = 34;
  const lifelineBottom = height - 8;
  parts.push(
    `<line x1="${LANE_LEFT}" y1="${lifelineTop}" x2="${LANE_LEFT}" y2="${lifelineBottom}" stroke="#999999" stroke-width="2"/>`,
    `<line x1="${laneRight}" y1="${lifelineTop}" x2="${laneRight}" y2="${lifelineBottom}" stroke="#999999" stroke-width="2"/>`
  );

  steps.forEach((step, i) => {
    const y = HEADER_HEIGHT + i * ROW_HEIGHT;

    parts.push(
      `<text x="${COL_FIRST_X}" y="${y + 4}" font-size="11" font-family="monospace" fill="#333333">${step.offsetFirst}</text>`,
      `<text x="${COL_PREV_X}" y="${y + 4}" font-size="11" font-family="monospace" fill="#333333">${step.offsetPrev}</text>`
    );

    const x1 = step.toRight ? LANE_LEFT : laneRight;
    const x2 = step.toRight ? laneRight : LANE_LEFT;
    const head = step.toRight ? -1 : 1;

    parts.push(
      `<line class="flow-arrow" x1="${x1}" y1="${y}" x2="${x2}" y2="${y}" stroke="${step.colour}" stroke-width="2"/>`,
      `<polygon points="${x2},${y} ${x2 + head * 9},${y - 4} ${x2 + head * 9},${y + 4}" fill="${step.colour}"/>`,
      `<text x="${(LANE_LEFT + laneRight) / 2}" y="${y - 6}" font-size="12" font-weight="bold" fill="${step.colour}" text-anchor="middle">${escapeXml(step.label)}</text>`,
      `<text x="${x2 + head * 14}" y="${y - 5}" font-size="10" font-family="monospace" fill="#888888" text-anchor="${step.toRight ? "end" : "start"}">seq ${step.sequenceNumber}</text>`
    );
  });

  parts.push(`</svg>`);
  return parts.join("\n");
}
