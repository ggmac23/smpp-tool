import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import * as vm from "node:vm";

/**
 * The renderer has no Node globals: no Buffer, no process, no require. The
 * shared form logic is written against Buffer, so the bundle must carry its
 * own implementation. These tests load the real bundle in a context with
 * nothing Node-ish in it and exercise the functions the widgets call.
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

  it("builds a submit_sm without Buffer in the environment", () => {
    const forms = loadBundle();
    const built = forms.buildSubmitInput(FORM as never) as {
      requests: { octetFields: Record<string, string> }[];
      warnings: string[];
    };
    expect(built.requests).toHaveLength(1);
    expect(built.requests[0].octetFields.short_message).toBe("68656c6c6f");
    expect(built.warnings).toEqual([]);
  });

  it("segments a long message without Buffer in the environment", () => {
    const forms = loadBundle();
    const built = forms.buildSubmitInput({
      ...FORM,
      text: "a".repeat(400),
      segmentation: "udh",
    } as never) as { requests: unknown[] };
    expect(built.requests).toHaveLength(3);
  });

  it("measures UCS2 without Buffer in the environment", () => {
    const forms = loadBundle();
    const built = forms.buildSubmitInput({
      ...FORM,
      text: "a".repeat(71),
      dataCoding: 8,
      segmentation: "udh",
    } as never) as { measurement: { parts: number } };
    expect(built.measurement.parts).toBe(2);
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
