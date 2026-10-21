import { InterfaceVersion, Ton, Npi } from "../../smpp/constants";
import type { BindMode, BindState } from "../../main/session";
import type { CorrelationConfig } from "../../main/dlr";
import type { ConnectRequest, BindRequest } from "../api";

export interface Option<T> {
  label: string;
  value: T;
}

/** The four interface versions the reference tool offers. */
export const INTERFACE_VERSIONS: Option<number>[] = [
  { label: "Default", value: InterfaceVersion.DEFAULT },
  { label: "v3.3", value: InterfaceVersion.V33 },
  { label: "v3.4", value: InterfaceVersion.V34 },
  { label: "v5.0", value: InterfaceVersion.V50 },
];

export const BIND_MODES: Option<BindMode>[] = [
  { label: "TRX", value: "transceiver" },
  { label: "TX", value: "transmitter" },
  { label: "RX", value: "receiver" },
];

export interface TonNpiPreset {
  label: string;
  ton: number;
  npi: number;
}

/**
 * The address-type presets the reference tool shows. "Auto" sends 0/0 and
 * leaves the choice to the SMSC.
 */
export const TON_NPI_PRESETS: TonNpiPreset[] = [
  { label: "Auto", ton: Ton.UNKNOWN, npi: Npi.UNKNOWN },
  { label: "Intl", ton: Ton.INTERNATIONAL, npi: Npi.ISDN_E163_E164 },
  { label: "Nat", ton: Ton.NATIONAL, npi: Npi.ISDN_E163_E164 },
  { label: "Alpha", ton: Ton.ALPHANUMERIC, npi: Npi.UNKNOWN },
  { label: "E164", ton: Ton.INTERNATIONAL, npi: Npi.ISDN_E163_E164 },
];

export interface ConnectionFormValues {
  host: string;
  port: number;
  tlsEnabled: boolean;
  rejectUnauthorized: boolean;
  servername: string;
  caPath: string;
  clientCertPath: string;
  clientKeyPath: string;
  clientKeyPassphrase: string;
  systemId: string;
  password: string;
  systemType: string;
  interfaceVersionLabel: string;
  bindModeLabel: string;
  addrTon: number;
  addrNpi: number;
  addressRange: string;
  windowSize: number;
  responseTimeoutMs: number;
  connectTimeoutMs: number;
  correlation: CorrelationConfig;
}

function blankToUndefined(s: string): string | undefined {
  return s && s.length > 0 ? s : undefined;
}

export function buildConnectRequest(v: ConnectionFormValues): ConnectRequest {
  return {
    host: v.host,
    port: v.port,
    tls: {
      enabled: v.tlsEnabled,
      rejectUnauthorized: v.rejectUnauthorized,
      servername: blankToUndefined(v.servername),
      caPath: blankToUndefined(v.caPath),
      clientCertPath: blankToUndefined(v.clientCertPath),
      clientKeyPath: blankToUndefined(v.clientKeyPath),
      clientKeyPassphrase: blankToUndefined(v.clientKeyPassphrase),
    },
    correlation: v.correlation,
    windowSize: v.windowSize,
    responseTimeoutMs: v.responseTimeoutMs,
    connectTimeoutMs: v.connectTimeoutMs,
  };
}

/**
 * Values straight through, with no clamping: an over-long system_type goes on
 * the wire as typed. Length limits are warnings elsewhere, never a block.
 */
export function buildBindRequest(v: ConnectionFormValues): BindRequest {
  return {
    systemId: v.systemId,
    password: v.password,
    systemType: v.systemType,
    interfaceVersion:
      INTERFACE_VERSIONS.find((o) => o.label === v.interfaceVersionLabel)?.value ??
      InterfaceVersion.DEFAULT,
    mode: BIND_MODES.find((o) => o.label === v.bindModeLabel)?.value ?? "transceiver",
    addrTon: v.addrTon,
    addrNpi: v.addrNpi,
    addressRange: v.addressRange,
  };
}

const MODE_LABEL: Record<BindMode, string> = {
  transceiver: "TRX",
  transmitter: "TX",
  receiver: "RX",
};

/** What the indicator reads. "Unbound" until a bind succeeds, as in the reference tool. */
export function stateLabel(state: BindState, mode: BindMode | null): string {
  switch (state) {
    case "unbound":
      return "Unbound";
    case "binding":
      return "Binding…";
    case "bound":
      return mode ? `Bound (${MODE_LABEL[mode]})` : "Bound";
    case "unbinding":
      return "Unbinding…";
    case "closed":
      return "Closed";
  }
}

/** Advisory length limits from the spec. Shown as warnings; never enforced. */
export const LENGTH_HINTS: Record<string, number> = {
  system_id: 15,
  password: 8,
  system_type: 12,
  address_range: 40,
  service_type: 5,
  source_addr: 20,
  destination_addr: 20,
  message_id: 64,
};

export function lengthWarnings(values: Record<string, string>): string[] {
  const out: string[] = [];
  for (const [field, max] of Object.entries(LENGTH_HINTS)) {
    const v = values[field];
    if (typeof v === "string" && v.length > max) {
      out.push(`${field} is ${v.length} characters; the spec allows ${max}. Sending it anyway.`);
    }
  }
  return out;
}
