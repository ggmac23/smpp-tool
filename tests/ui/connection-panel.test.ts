import { describe, it, expect } from "vitest";
import {
  INTERFACE_VERSIONS,
  BIND_MODES,
  TON_NPI_PRESETS,
  buildConnectRequest,
  buildBindRequest,
  stateLabel,
  lengthWarnings,
  ConnectionFormValues,
} from "../../src/shared/ui/connection";
import { DEFAULT_CORRELATION } from "../../src/main/dlr";

const FORM: ConnectionFormValues = {
  host: "smsc.example.com",
  port: 2775,
  tlsEnabled: true,
  rejectUnauthorized: false,
  servername: "sni.example.com",
  caPath: "/tmp/ca.crt",
  clientCertPath: "/tmp/client.crt",
  clientKeyPath: "/tmp/client.key",
  clientKeyPassphrase: "kp",
  systemId: "esme",
  password: "pw",
  systemType: "SMPP",
  interfaceVersionLabel: "v3.4",
  bindModeLabel: "TRX",
  addrTon: 1,
  addrNpi: 1,
  addressRange: "3933*",
  windowSize: 10,
  responseTimeoutMs: 5000,
  connectTimeoutMs: 10000,
  correlation: DEFAULT_CORRELATION,
};

describe("field option lists", () => {
  it("maps the four interface-version labels to 0x00, 0x33, 0x34 and 0x50", () => {
    expect(INTERFACE_VERSIONS.map((v) => [v.label, v.value])).toEqual([
      ["Default", 0x00],
      ["v3.3", 0x33],
      ["v3.4", 0x34],
      ["v5.0", 0x50],
    ]);
  });

  it("maps TRX, TX and RX to transceiver, transmitter and receiver", () => {
    expect(BIND_MODES.map((m) => [m.label, m.value])).toEqual([
      ["TRX", "transceiver"],
      ["TX", "transmitter"],
      ["RX", "receiver"],
    ]);
  });

  it("offers the reference tool's TON/NPI presets", () => {
    expect(TON_NPI_PRESETS.map((p) => p.label)).toEqual(["Auto", "Intl", "Nat", "Alpha", "E164"]);
  });

  it("gives Intl the international TON and the E.164 NPI", () => {
    const intl = TON_NPI_PRESETS.find((p) => p.label === "Intl")!;
    expect(intl).toMatchObject({ ton: 1, npi: 1 });
  });

  it("gives Alpha the alphanumeric TON", () => {
    expect(TON_NPI_PRESETS.find((p) => p.label === "Alpha")!.ton).toBe(5);
  });

  it("leaves Auto unset so the SMSC decides", () => {
    expect(TON_NPI_PRESETS.find((p) => p.label === "Auto")).toMatchObject({ ton: 0, npi: 0 });
  });
});

describe("buildConnectRequest", () => {
  it("reads back host, port and the TLS switches", () => {
    const r = buildConnectRequest(FORM);
    expect(r.host).toBe("smsc.example.com");
    expect(r.port).toBe(2775);
    expect(r.tls.enabled).toBe(true);
    expect(r.tls.rejectUnauthorized).toBe(false);
  });

  it("reads back the CA path, SNI, client certificate and key passphrase", () => {
    const r = buildConnectRequest(FORM);
    expect(r.tls).toMatchObject({
      servername: "sni.example.com",
      caPath: "/tmp/ca.crt",
      clientCertPath: "/tmp/client.crt",
      clientKeyPath: "/tmp/client.key",
      clientKeyPassphrase: "kp",
    });
  });

  it("omits the TLS detail fields when they are blank", () => {
    const r = buildConnectRequest({ ...FORM, servername: "", caPath: "", clientCertPath: "" });
    expect(r.tls.servername).toBeUndefined();
    expect(r.tls.caPath).toBeUndefined();
    expect(r.tls.clientCertPath).toBeUndefined();
  });

  it("carries the windowing and timeout settings", () => {
    const r = buildConnectRequest(FORM);
    expect(r.windowSize).toBe(10);
    expect(r.responseTimeoutMs).toBe(5000);
    expect(r.connectTimeoutMs).toBe(10000);
  });

  it("carries the DLR correlation settings", () => {
    expect(buildConnectRequest(FORM).correlation).toEqual(DEFAULT_CORRELATION);
  });
});

describe("buildBindRequest", () => {
  it("maps the version label to its numeric value", () => {
    expect(buildBindRequest(FORM).interfaceVersion).toBe(0x34);
    expect(buildBindRequest({ ...FORM, interfaceVersionLabel: "v5.0" }).interfaceVersion).toBe(0x50);
    expect(buildBindRequest({ ...FORM, interfaceVersionLabel: "Default" }).interfaceVersion).toBe(0);
  });

  it("maps the bind-mode label to its mode", () => {
    expect(buildBindRequest(FORM).mode).toBe("transceiver");
    expect(buildBindRequest({ ...FORM, bindModeLabel: "TX" }).mode).toBe("transmitter");
    expect(buildBindRequest({ ...FORM, bindModeLabel: "RX" }).mode).toBe("receiver");
  });

  it("carries the credentials verbatim", () => {
    const r = buildBindRequest(FORM);
    expect(r).toMatchObject({ systemId: "esme", password: "pw", systemType: "SMPP" });
  });

  it("carries the address range and its TON/NPI", () => {
    expect(buildBindRequest(FORM)).toMatchObject({ addrTon: 1, addrNpi: 1, addressRange: "3933*" });
  });

  it("sends an over-long system_type as typed rather than truncating it", () => {
    const long = { ...FORM, systemType: "TOO-LONG-SYSTEM-TYPE" };
    expect(buildBindRequest(long).systemType).toBe("TOO-LONG-SYSTEM-TYPE");
  });

  it("warns about an over-long field without changing it", () => {
    const warnings = lengthWarnings({ system_type: "TOO-LONG-SYSTEM-TYPE" });
    expect(warnings).toHaveLength(1);
    expect(warnings[0]).toMatch(/system_type/);
    expect(warnings[0]).toMatch(/anyway/i);
  });

  it("warns about nothing when every field is within its limit", () => {
    expect(lengthWarnings({ system_id: "esme", system_type: "SMPP" })).toEqual([]);
  });
});

describe("stateLabel", () => {
  it("reads Unbound before any bind", () => {
    expect(stateLabel("unbound", null)).toBe("Unbound");
  });

  it("names the mode once bound", () => {
    expect(stateLabel("bound", "transceiver")).toBe("Bound (TRX)");
    expect(stateLabel("bound", "transmitter")).toBe("Bound (TX)");
    expect(stateLabel("bound", "receiver")).toBe("Bound (RX)");
  });

  it("reads Binding while the bind is in flight", () => {
    expect(stateLabel("binding", "transceiver")).toBe("Binding…");
  });

  it("reads Closed after the connection goes", () => {
    expect(stateLabel("closed", null)).toBe("Closed");
  });

  it("reads Unbinding while unbinding", () => {
    expect(stateLabel("unbinding", "transceiver")).toBe("Unbinding…");
  });
});
