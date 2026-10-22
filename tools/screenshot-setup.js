/**
 * Fills the interface with plausible traffic for the README screenshots.
 *
 * It installs a stand-in bridge through `Api.setBridge`, the same seam the
 * tests use, so nothing here touches a socket. Not part of the app: it is
 * appended to a copy of index.html by tools/screenshots.sh.
 */
(function () {
  var listeners = {};

  function bridge() {
    smppclient.Api.getInstance().setBridge({
      invoke: function (c) {
        if (c === "status") {
          return Promise.resolve({ state: "bound", mode: "transceiver", connected: true,
            dropped: 0, pendingBuffered: 0, pendingDeclared: null, inflight: 0 });
        }
        if (c === "compose_submit") {
          return Promise.resolve({ requests: [{ commandId: 4, fields: {}, octetFields: {}, tlvs: [] }],
            warnings: [], measurement: { chars: 24, bits: 168, bytes: 24, parts: 1, unmappable: [] } });
        }
        if (c === "list_profiles") return Promise.resolve([]);
        if (c === "pdu_fields") {
          return Promise.resolve([
            { name: "command_length", value: "61" },
            { name: "command_id", value: "submit_sm (0x4)" },
            { name: "command_status", value: "ESME_ROK (0)" },
            { name: "sequence_number", value: "2" },
            { name: "service_type", value: "" },
            { name: "source_addr_ton", value: "5" },
            { name: "source_addr", value: "INFOSMS" },
            { name: "dest_addr_ton", value: "1" },
            { name: "destination_addr", value: "447700900123" },
            { name: "esm_class", value: "0" },
            { name: "registered_delivery", value: "1" },
            { name: "data_coding", value: "0" },
            { name: "sm_length", value: "24" },
            { name: "short_message", value: "hello from the smpp tool" }
          ]);
        }
        return Promise.resolve(null);
      },
      listen: function (n, cb) {
        (listeners[n] = listeners[n] || []).push(cb);
        return Promise.resolve(function () {});
      }
    });
  }

  function fire(n, p) {
    (listeners[n] || []).forEach(function (cb) { cb({ payload: p }); });
  }

  function traffic() {
    var t = Date.now() - 9000;
    return [
      { seq: 1, at: t, direction: "out", commandName: "bind_transceiver", commandStatus: 0, statusName: "ESME_ROK", sequenceNumber: 1, summary: "bind_transceiver esme-01", hex: "0000002b00000009000000000000000165736d652d30310073656372657400534d50500034000000" },
      { seq: 2, at: t + 140, direction: "in", commandName: "bind_transceiver_resp", commandStatus: 0, statusName: "ESME_ROK", sequenceNumber: 1, summary: "bind_transceiver_resp SMSC-GW", hex: "00000019800000090000000000000001534d53432d475700" },
      { seq: 3, at: t + 1200, direction: "out", commandName: "submit_sm", commandStatus: 0, statusName: "ESME_ROK", sequenceNumber: 2, summary: "submit_sm INFOSMS -> 447700900123 24 bytes", hex: "0000003d0000000400000000000000020005004 94e464f534d530001013434373730303930303132330000000000000100180068656c6c6f" },
      { seq: 4, at: t + 1390, direction: "in", commandName: "submit_sm_resp", commandStatus: 0, statusName: "ESME_ROK", sequenceNumber: 2, summary: "submit_sm_resp id=525333619", hex: "0000001a8000000400000000000000023532353333333631393900" },
      { seq: 5, at: t + 2050, direction: "out", commandName: "submit_sm", commandStatus: 0, statusName: "ESME_ROK", sequenceNumber: 3, summary: "submit_sm INFOSMS -> 447700900124 24 bytes", hex: "0000003d000000040000000000000003" },
      { seq: 6, at: t + 2240, direction: "in", commandName: "submit_sm_resp", commandStatus: 88, statusName: "ESME_RTHROTTLED", sequenceNumber: 3, summary: "submit_sm_resp", hex: "00000010800000040000005800000003" },
      { seq: 7, at: t + 3600, direction: "in", commandName: "deliver_sm", commandStatus: 0, statusName: "ESME_ROK", sequenceNumber: 9, summary: "deliver_sm 447700900123 -> INFOSMS 101 bytes receipt id=525333619 DELIVRD", hex: "00000081000000050000000000000009" },
      { seq: 8, at: t + 3610, direction: "out", commandName: "deliver_sm_resp", commandStatus: 0, statusName: "ESME_ROK", sequenceNumber: 9, summary: "deliver_sm_resp", hex: "00000011800000050000000000000009" },
      { seq: 9, at: t + 5000, direction: "in", commandName: "(undecodable)", commandStatus: 0, statusName: "ESME_ROK", sequenceNumber: 0, summary: "", hex: "00000008ffffffff", decodeError: "command_length 8 is below the 16-byte header" },
      { seq: 10, at: t + 7000, direction: "out", commandName: "enquire_link", commandStatus: 0, statusName: "ESME_ROK", sequenceNumber: 4, summary: "enquire_link", hex: "00000010000000150000000000000004" },
      { seq: 11, at: t + 7150, direction: "in", commandName: "enquire_link_resp", commandStatus: 0, statusName: "ESME_ROK", sequenceNumber: 4, summary: "enquire_link_resp", hex: "00000010800000150000000000000004" }
    ];
  }

  var report = {
    connections: 8, requestedRatePerSec: 12000, achievedRatePerSec: 7421.6,
    sustainedRatePerSec: 7000, messages: 29000, pdus: 29000,
    submitted: 29000, accepted: 29000, errorsByStatus: {},
    latencyP50: 433, latencyP95: 1382, latencyP99: 1539, durationMs: 3907,
    ended: "breached",
    steps: [
      { requestedRate: 5000, achievedRate: 4998.1, submitted: 10000, accepted: 10000, errorShare: 0, latencyP95: 361, breached: false, reason: null },
      { requestedRate: 6000, achievedRate: 5996.4, submitted: 12000, accepted: 12000, errorShare: 0, latencyP95: 402, breached: false, reason: null },
      { requestedRate: 7000, achievedRate: 6999.3, submitted: 14000, accepted: 14000, errorShare: 0, latencyP95: 498, breached: false, reason: null },
      { requestedRate: 8000, achievedRate: 7494.6, submitted: 15000, accepted: 15000, errorShare: 0, latencyP95: 1470, breached: true, reason: "p95 latency reached 1470ms, above the 1000ms allowed" }
    ],
    perConnection: [0, 1, 2, 3, 4, 5, 6, 7].map(function (i) {
      var p95 = [1290, 1344, 1401, 1312, 1433, 1388, 1356, 1370][i];
      return { index: i, bound: true, bindError: null, submitted: i === 0 ? 3630 : 3625,
        accepted: i === 0 ? 3630 : 3625, errorsByStatus: {},
        latencyP50: 410 + i * 6, latencyP95: p95, latencyP99: p95 + 150, lost: false };
    })
  };

  function populate() {
    var app = qx.core.Init.getApplication();
    var shell = app.getShell();

    shell.getConnectionPanel().setFormValues({
      host: "smsc.aggregator.example", port: 2775, systemId: "esme-01",
      password: "secret", systemType: "SMPP", name: "Aggregator A (staging)"
    });
    app.getSubmitForm().setFormValues({
      sourceAddr: "INFOSMS", destinationAddr: "447700900123",
      text: "hello from the smpp tool"
    });

    fire("state", { state: "bound", mode: "transceiver" });
    fire("counters", {
      bind_transceiver: { sent: 1, received: 0 }, bind_transceiver_resp: { sent: 0, received: 1 },
      submit_sm: { sent: 3, received: 0 }, submit_sm_resp: { sent: 0, received: 3 },
      deliver_sm: { sent: 0, received: 2 }, deliver_sm_resp: { sent: 2, received: 0 },
      enquire_link: { sent: 1, received: 0 }, enquire_link_resp: { sent: 0, received: 1 }
    });
    fire("pdu", traffic());

    app.getTracePanel().setPeer("smsc.aggregator.example:2775");
    app.getTracePanel().selectRow(2);

    var load = app.getPanels().load;
    load.setFormValues({
      connections: 8, messages: 20000, modeLabel: "Ramp until it breaks",
      startRate: 5000, step: 1000, maxRate: 12000, stepSeconds: 2,
      errorSharePercent: 2, latencyP95Ms: 1000, destination: "447700900123"
    });
    load.showReport(report);

    var which = (location.search.match(/shot=([a-z]+)/) || [])[1] || "main";
    if (which === "load") {
      shell.selectPage(load);
      shell.showNotice("Load test: the SMSC gave way — sustained 7000/s");
    } else if (which === "flow") {
      app.getTracePanel().showFlow();
    } else {
      shell.selectPage(app.getSubmitForm());
      shell.showNotice("Bound to smsc.aggregator.example:2775 as SMSC-GW");
    }
    document.title = "ready";
  }

  function waitForApp() {
    if (typeof qx !== "object" || !qx.core || !qx.core.Init || !qx.core.Init.getApplication()) {
      return window.setTimeout(waitForApp, 50);
    }
    bridge();
    window.setTimeout(populate, 300);
  }
  waitForApp();
})();
