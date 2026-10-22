import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import * as vm from "node:vm";

/**
 * The renderer has no Node globals: no Buffer, no process, no require. It also
 * has no codec any more — the backend encodes, decodes and segments — so the
 * bundle needs no polyfill at all. These tests load the real bundle in a
 * context with nothing Node-ish in it and exercise what the widgets call.
 */
function loadBundle(): Record<string, (...args: never[]) => unknown> {
  const source = readFileSync("compiled/build/forms.js", "utf8");
  const sandbox: Record<string, unknown> = { console };
  sandbox.globalThis = sandbox;
  sandbox.window = sandbox;
  vm.createContext(sandbox);
  vm.runInContext(source, sandbox);
  const forms = sandbox.SmppForms as Record<string, (...args: never[]) => unknown>;
  if (!forms) throw new Error("the bundle did not define SmppForms");
  return forms;
}

const FORM = {
  serviceType: "",
  sourceTonNpiLabel: "Intl",
  sourceAddr: "393331234567",
  destTonNpiLabel: "Intl",
  destinationAddr: "393337654321",
  esmClass: 0,
  protocolId: 0,
  priorityFlag: 0,
  registeredDelivery: 1,
  scheduleDeliveryLabel: "Immediate",
  validityPeriodLabel: "Default",
  dataCoding: 0,
  replaceIfPresent: 0,
  smDefaultMsgId: 0,
  text: "hello",
  placement: "short_message",
  segmentation: "none",
  packing: "unpacked",
  udhHex: "",
  tlvs: [],
};

describe("the forms bundle in a Node-free context", () => {
  it("defines SmppForms", () => {
    expect(typeof loadBundle()).toBe("object");
  });

  it("exposes the option lists the widgets read", () => {
    const forms = loadBundle() as unknown as Record<string, unknown[]>;
    expect(forms.INTERFACE_VERSIONS).toHaveLength(4);
    expect(forms.BIND_MODES).toHaveLength(3);
    expect(forms.DATA_CODINGS).toHaveLength(9);
    expect(forms.ESM_CLASSES).toHaveLength(8);
    expect(forms.REGISTERED_DELIVERY).toHaveLength(8);
  });

  it("carries no Buffer polyfill, because nothing in it needs one", () => {
    const source = readFileSync("compiled/build/forms.js", "utf8");
    expect(source).not.toMatch(/\bBuffer\b/);
  });

  it("builds the compose arguments without Buffer in the environment", () => {
    const forms = loadBundle();
    const args = forms.buildComposeArgs(FORM as never) as Record<string, unknown>;
    expect(args.sourceAddrTon).toBe(1);
    expect(args.text).toBe("hello");
    // Composing is the backend's: nothing PDU-shaped comes out of here.
    expect(args.commandId).toBeUndefined();
    expect(args.octetFields).toBeUndefined();
  });

  it("parses hand-typed hex without Buffer in the environment", () => {
    const forms = loadBundle();
    const bytes = forms.parseHex("0x de:ad be ef" as never) as Uint8Array;
    expect(Array.from(bytes)).toEqual([0xde, 0xad, 0xbe, 0xef]);
    expect(forms.formatHex(bytes as never)).toBe("de ad be ef");
  });

  it("builds a connect request without Buffer in the environment", () => {
    const forms = loadBundle();
    const req = forms.buildConnectRequest({
      host: "h",
      port: 2775,
      tlsEnabled: false,
      rejectUnauthorized: true,
      servername: "",
      caPath: "",
      clientCertPath: "",
      clientKeyPath: "",
      clientKeyPassphrase: "",
      systemId: "",
      password: "",
      systemType: "",
      interfaceVersionLabel: "v3.4",
      bindModeLabel: "TRX",
      addrTon: 0,
      addrNpi: 0,
      addressRange: "",
      windowSize: 10,
      responseTimeoutMs: 5000,
      connectTimeoutMs: 10000,
      correlation: {
        preferTlv: true,
        parseText: true,
        normaliseRadix: true,
        stripLeadingZeros: true,
      },
    } as never) as { host: string };
    expect(req.host).toBe("h");
  });
});
