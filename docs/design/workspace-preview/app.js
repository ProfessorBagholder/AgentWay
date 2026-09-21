// Isolated design artifact. All records and actions below are illustrative.
const main = document.querySelector("main");
const agents = [
  {
    id: "muse",
    name: "Podcast coordinator",
    product: "Muse",
    account: "Personal account",
    state: "Authorized",
    delivery: "Outbound requests",
    capacity: "Not reported",
    access: true,
  },
  {
    id: "editor",
    name: "Episode editor",
    product: "Claude agents",
    account: "Podcast workspace",
    state: "Authorized",
    delivery: "Worker capability unverified",
    capacity: "Shared allowance · not reported",
    access: false,
  },
  {
    id: "clips",
    name: "Clip writer",
    product: "Claude agents",
    account: "Podcast workspace",
    state: "Authorized",
    delivery: "Worker capability unverified",
    capacity: "Shared allowance · not reported",
    access: false,
  },
  {
    id: "research",
    name: "Market research",
    product: "Grok Bot",
    account: "Personal account",
    state: "Setup incomplete",
    delivery: "Not verified",
    capacity: "Not reported",
    access: false,
  },
];
let defaults = {
  category: "Entertainment",
  visibility: "Private",
  allowPublic: true,
};
let retryDone = false,
  wizard = {
    step: 1,
    product: "ChatGPT agents",
    name: "",
    publish: false,
    delegate: false,
  };
const escape = (value) =>
  String(value).replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );
const badge = (s) =>
  `<span class="badge ${s === "Connected" || s === "Authorized" || s === "Published" || s === "Complete" ? "good" : s.includes("incomplete") || s.includes("attention") ? "warn" : ""}">${escape(s)}</span>`;
const kv = (k, v) => `<div class="kv"><span>${k}</span><span>${v}</span></div>`;
const heading = (title, sub, action = "") =>
  `<div class="heading"><div><h1>${title}</h1>${sub ? `<p>${sub}</p>` : ""}</div>${action}</div>`;
const link = (href, label, primary = false) =>
  `<a class="button ${primary ? "primary" : ""}" href="#/${href}">${label}</a>`;
const back = (path, label) => `<a class="back" href="#/${path}">← ${label}</a>`;
let noticeTimer;
function notice(s) {
  clearTimeout(noticeTimer);
  document.querySelector("#notice").textContent = s;
  noticeTimer = setTimeout(
    () => (document.querySelector("#notice").textContent = ""),
    5000,
  );
}
function agentList() {
  return (
    heading("Agents", "", link("agents/connect", "Connect agent", true)) +
    `<section class="panel" aria-label="Agent connections"><div class="row head"><span>Agent</span><span>Connection</span><span>Allowance</span><span></span></div>${agents.map((a) => `<div class="row"><div><a href="#/agents/${a.id}"><strong>${escape(a.name)}</strong></a><span class="sub">${a.product} · ${a.account}</span></div><div>${badge(a.state)}<span class="sub">${a.delivery}</span></div><div>${a.capacity}<span class="sub">${a.account === "Podcast workspace" ? "Shared by 2 agents" : "No estimate available"}</span></div><a aria-label="Manage ${escape(a.name)}" href="#/agents/${a.id}">→</a></div>`).join("")}</section>`
  );
}
function agentDetail(id) {
  const a = agents.find((x) => x.id === id);
  if (!a) return missing();
  return (
    back("agents", "Agents") +
    heading(escape(a.name), `${a.product} · ${a.account}`, badge(a.state)) +
    `<div class="grid"><div><section class="panel"><div class="panel-title"><h2>Connection</h2></div><div class="pad">${kv("Access", a.state)}${kv("Task delivery", a.delivery)}<div class="actions"><button data-action="toggle" data-id="${id}">${a.state === "Paused" ? "Resume access" : "Pause access"}</button>${link("agents/connect", "Reconnect")}</div></div></section><section class="panel"><div class="panel-title"><h2>Permissions</h2></div><div class="pad"><label class="check"><input type="checkbox" id="publish" ${a.access ? "checked" : ""}> Publish to Professor Bagholder · YouTube</label><label class="check"><input type="checkbox" id="delegate" ${(a.delegate ?? id === "muse") ? "checked" : ""}> Delegate work to Episode editor and Clip writer</label><button data-action="save-access" data-id="${id}">Save permissions</button></div></section></div><div><section class="panel"><div class="panel-title"><h2>Allowance</h2></div><div class="pad"><h3>${a.capacity}</h3>${kv("Account", a.account)}${kv("On a reported limit", "Pause assignment; preserve checkpoint")}</div></section><section class="panel"><div class="pad"><h2>Activity</h2><p>${id === "muse" ? "Requested publication of Episode 12 teaser." : "No tasks yet."}</p>${id === "muse" ? link("tasks/episode", "View task") : ""}</div></section></div></div>`
  );
}
function connect() {
  const w = wizard;
  return (
    back("agents", "Agents") +
    heading("Connect an agent", "") +
    `<div class="narrow"><div class="steps">${["Agent", "Permissions", "Connect", "Verify"].map((s, i) => (i + 1 === w.step ? `<strong>${i + 1}. ${s}</strong>` : `<span>${i + 1}. ${s}</span>`)).join("")}</div><section class="panel"><form id="setup" class="pad">${w.step === 1 ? `<h2>Which agent are you connecting?</h2><label for="product">Agent platform</label><select id="product">${["Muse", "ChatGPT agents", "Claude agents", "Grok Bot"].map((p) => `<option ${p === w.product ? "selected" : ""}>${p}</option>`).join("")}</select><label for="name">Connection name</label><input id="name" required maxlength="80" placeholder="e.g. Research assistant" value="${escape(w.name)}">` : w.step === 2 ? `<h2>Choose what it can access</h2><p>${escape(w.name)} · ${w.product}</p><label class="check"><input id="publish" type="checkbox" ${w.publish ? "checked" : ""}> Publish to Professor Bagholder · YouTube</label><label class="check"><input id="delegate" type="checkbox" ${w.delegate ? "checked" : ""}> Delegate to Episode editor and Clip writer</label>` : w.step === 3 ? `<h2>Connect from ${w.product}</h2><p>Connection setup is unavailable in this preview.</p>` : `<h2>Ready to verify</h2><p>Simulate a connection to continue.</p>`}<div class="actions">${w.step > 1 ? '<button type="button" data-action="previous">Back</button>' : ""}<button class="primary" type="submit">${w.step === 4 ? "Simulate verification" : "Continue"}</button></div></form></section></div>`
  );
}
function destinations() {
  return (
    heading(
      "Destinations",
      "",
      link("destinations/connect", "Connect destination", true),
    ) +
    `<section class="panel" aria-labelledby="youtube-platform"><div class="panel-title"><h2 id="youtube-platform">YouTube</h2><span class="sub">Videos and Shorts</span></div><div class="row head destination-head"><span>Account</span><span>Account status</span><span>Agents with access</span><span></span></div><div class="row destination-row"><div><a href="#/destinations/youtube"><strong>Professor Bagholder</strong></a></div><div><span class="field-label">Account status</span>${badge("Connected")}</div><div><span class="field-label">Agents with access</span>${agents.filter((a) => a.access).length} ${agents.filter((a) => a.access).length === 1 ? "agent" : "agents"}</div><a href="#/destinations/youtube" aria-label="Manage YouTube">→</a></div></section>`
  );
}
function destination() {
  return (
    back("destinations", "Destinations") +
    heading(
      "Professor Bagholder",
      "YouTube · Videos and Shorts",
      badge("Connected"),
    ) +
    `<div class="grid"><section class="panel"><div class="panel-title"><h2>Publishing defaults</h2></div><form class="pad" id="defaults"><label for="category">Category</label><select id="category">${["Entertainment", "Comedy", "Education"].map((v) => `<option ${defaults.category === v ? "selected" : ""}>${v}</option>`).join("")}</select><label for="visibility">Default visibility</label><select id="visibility">${["Private", "Public", "Unlisted"].map((v) => `<option ${defaults.visibility === v ? "selected" : ""}>${v}</option>`).join("")}</select><label class="check"><input id="allow-public" type="checkbox" ${defaults.allowPublic ? "checked" : ""}> Allow agents with publishing permission to publish publicly</label><button type="submit">Save defaults</button></form></section><div><section class="panel"><div class="panel-title"><h2>Agents with access</h2></div><div class="pad">${
      agents
        .filter((a) => a.access)
        .map(
          (a) =>
            `<div class="kv"><a href="#/agents/${a.id}">${escape(a.name)}</a><span>Publish</span></div>`,
        )
        .join("") || "<p>No agents have publishing permission.</p>"
    }</div></section></div></div>`
  );
}
function catalog() {
  return (
    back("destinations", "Destinations") +
    heading("Connect a destination", "") +
    `<section class="panel"><div class="pad"><h2>YouTube</h2><button data-action="destination-demo">Preview account enrollment</button></div></section>`
  );
}
function tasks() {
  const f =
    new URLSearchParams(location.hash.split("?")[1] || "").get("state") ||
    "all";
  return (
    heading("Tasks", "") +
    `<div class="toolbar" aria-label="Task filters">${[
      ["all", "All tasks"],
      ["attention", "Needs attention"],
      ["complete", "Completed"],
    ]
      .map(
        ([v, l]) =>
          `<a class="${f === v ? "selected" : ""}" href="#/tasks?state=${v}">${l}</a>`,
      )
      .join(
        "",
      )}</div><section class="panel">${f !== "complete" || retryDone ? `<div class="row"><div><a href="#/tasks/episode"><strong>Publish Episode 12 teaser</strong></a><span class="sub">Podcast coordinator · Muse</span></div><div>${badge(retryDone ? "Complete" : "Needs attention")}<span class="sub">${retryDone ? "Completed" : "YouTube published · follow-up failed"}</span></div><div>Today, 10:42<span class="sub">Publishing</span></div><a href="#/tasks/episode" aria-label="Open teaser task">→</a></div>` : ""}${f !== "attention" ? `<div class="row"><div><a href="#/tasks/test"><strong>AgentWay public upload test</strong></a><span class="sub">Podcast coordinator · Muse</span></div><div>${badge("Complete")}<span class="sub">Public visibility verified</span></div><div>Yesterday<span class="sub">YouTube</span></div><a href="#/tasks/test" aria-label="Open upload task">→</a></div>` : ""}</section>`
  );
}
// Review fixtures only; production events come from the durable event journal.
const publicationEvents = [
  {
    id: "evt-101",
    time: "2026-09-21T16:42:00.000Z",
    level: "info",
    source: "Muse",
    title: "Publish request received",
    details: {
      trace_id: "trace-episode-12",
      request_id: "req-episode-12",
      task_id: "episode",
      operation: "publish_youtube",
      destination: "Professor Bagholder",
      transport: "HTTP",
    },
  },
  {
    id: "evt-102",
    time: "2026-09-21T16:42:00.020Z",
    level: "info",
    source: "AgentWay",
    title: "Permissions and video settings validated",
    details: {
      trace_id: "trace-episode-12",
      parent_event_id: "evt-101",
      principal: "Podcast coordinator",
      made_for_kids: false,
      contains_synthetic_media: true,
      requested_privacy: "public",
    },
  },
  {
    id: "evt-103",
    time: "2026-09-21T16:42:00.035Z",
    level: "info",
    source: "AgentWay",
    title: "Upload queued",
    details: {
      trace_id: "trace-episode-12",
      parent_event_id: "evt-102",
      attempt_id: "upload-1",
      media_id: "media-teaser-12",
    },
  },
  {
    id: "evt-104",
    time: "2026-09-21T16:42:01.000Z",
    level: "info",
    source: "YouTube",
    title: "Resumable upload started",
    details: {
      trace_id: "trace-episode-12",
      parent_event_id: "evt-103",
      attempt_id: "upload-1",
      operation: "videos.insert",
      http_status: 200,
    },
  },
  {
    id: "evt-105",
    time: "2026-09-21T16:43:00.000Z",
    level: "info",
    source: "YouTube",
    title: "Video upload completed",
    details: {
      trace_id: "trace-episode-12",
      parent_event_id: "evt-104",
      attempt_id: "upload-1",
      video_id: "example-video-12",
      http_status: 200,
    },
  },
  {
    id: "evt-106",
    time: "2026-09-21T16:43:04.000Z",
    level: "info",
    source: "YouTube",
    title: "Video processing completed",
    details: {
      trace_id: "trace-episode-12",
      parent_event_id: "evt-105",
      processing_status: "succeeded",
    },
  },
  {
    id: "evt-107",
    time: "2026-09-21T16:44:00.000Z",
    level: "info",
    source: "YouTube",
    title: "Visibility and disclosures verified",
    details: {
      trace_id: "trace-episode-12",
      parent_event_id: "evt-106",
      privacy: "public",
      made_for_kids: false,
      contains_synthetic_media: true,
    },
  },
  {
    id: "evt-108",
    time: "2026-09-21T16:44:01.000Z",
    level: "info",
    source: "AgentWay",
    title: "Caption upload started",
    details: {
      trace_id: "trace-episode-12",
      parent_event_id: "evt-107",
      attempt_id: "captions-1",
      operation: "captions.insert",
    },
  },
  {
    id: "evt-109",
    time: "2026-09-21T16:44:01.450Z",
    level: "error",
    source: "YouTube",
    title: "Caption upload failed",
    details: {
      trace_id: "trace-episode-12",
      parent_event_id: "evt-108",
      attempt_id: "captions-1",
      operation: "captions.insert",
      http_status: 503,
      provider_code: "backendError",
      message: "The service is temporarily unavailable.",
      retryable: true,
      duration_ms: 450,
      next_action: "Retry captions on the existing video.",
      publication_state: "published",
    },
  },
];
const connectionEvents = [
  {
    id: "evt-201",
    time: "2026-09-21T16:46:00.000Z",
    level: "error",
    source: "AgentWay",
    title: "Agent request rejected",
    details: {
      trace_id: "trace-auth-21",
      operation: "GET /v1/status",
      http_status: 401,
      code: "credential_expired",
      principal: "Market research",
      task_id: null,
      next_action: "Reconnect Market research.",
    },
  },
];
function taskEvents(id) {
  if (id !== "episode") return [];
  return [
    ...publicationEvents,
    ...(retryDone
      ? [
          {
            id: "evt-110",
            time: "2026-09-21T16:45:00.000Z",
            level: "info",
            source: "AgentWay",
            title: "Caption retry started",
            details: {
              trace_id: "trace-episode-12",
              parent_event_id: "evt-109",
              attempt_id: "captions-2",
              video_id: "example-video-12",
            },
          },
          {
            id: "evt-111",
            time: "2026-09-21T16:45:01.000Z",
            level: "info",
            source: "YouTube",
            title: "Captions attached",
            details: {
              trace_id: "trace-episode-12",
              parent_event_id: "evt-110",
              attempt_id: "captions-2",
              http_status: 200,
            },
          },
        ]
      : []),
  ];
}
function eventView(id) {
  const params = new URLSearchParams(location.hash.split("?")[1] || "");
  const errors = params.get("level") === "error";
  const all = id
    ? taskEvents(id)
    : [...taskEvents("episode"), ...connectionEvents].sort((a, b) =>
        a.time.localeCompare(b.time),
      );
  const events = all.filter((e) => !errors || e.level === "error");
  const route = id ? `tasks/${id}/events` : "events";
  return `<div class="split event-controls"><div class="toolbar"><a class="${!errors ? "selected" : ""}" href="#/${route}">All events</a><a class="${errors ? "selected" : ""}" href="#/${route}?level=error">Errors</a></div><button data-action="export-events" data-task="${id || ""}">Export diagnostics</button></div><section class="panel event-list">${events.length ? events.map((e) => `<details ${e.level === "error" ? "open" : ""}><summary><span class="event-time">${escape(e.time.replace("T", " ").replace("Z", " UTC"))}</span><span class="event-title">${escape(e.title)}<span class="sub">${escape(e.source)} · ${escape(e.id)}</span></span><span class="badge ${e.level === "error" ? "error" : ""}">${e.level === "error" ? "Error" : "Info"}</span></summary><pre>${escape(JSON.stringify(e.details, null, 2))}</pre></details>`).join("") : '<div class="pad">No events recorded.</div>'}</section>`;
}
function taskTabs(id, view) {
  return `<div class="toolbar"><a class="${!view ? "selected" : ""}" href="#/tasks/${id}">Progress</a>${id === "episode" ? `<a class="${view === "settings" ? "selected" : ""}" href="#/tasks/${id}/settings">Video settings</a>` : ""}<a class="${view === "events" ? "selected" : ""}" href="#/tasks/${id}/events">Events</a></div>`;
}
function task(id, view) {
  const settings = view === "settings";
  if (view === "events")
    return (
      back("tasks", "Tasks") +
      heading(
        id === "episode"
          ? "Publish Episode 12 teaser"
          : "AgentWay public upload test",
        "",
      ) +
      taskTabs(id, view) +
      eventView(id)
    );
  if (id === "test")
    return (
      back("tasks", "Tasks") +
      heading(
        "AgentWay public upload test",
        "Podcast coordinator · Muse",
        badge("Complete"),
      ) +
      `${taskTabs(id, view)}<section class="panel"><div class="pad">${kv("Destination", "Professor Bagholder · YouTube")}${kv("Requested visibility", "Public")}${kv("Observed visibility", "Public")}${kv("Disclosure verification", "Not checked")}</div></section>`
    );
  return (
    back("tasks", "Tasks") +
    heading(
      "Publish Episode 12 teaser",
      "Requested by Podcast coordinator · Muse",
      badge(retryDone ? "Complete" : "Needs attention"),
    ) +
    `${taskTabs(id, view)}${
      settings
        ? `<section class="panel"><div class="panel-title"><h2>Video settings</h2></div><div class="pad table-wrap"><table><thead><tr><th>Setting</th><th>Effective request</th><th>Provider result</th></tr></thead><tbody>${[
            ["Visibility", "Public", "Public · verified"],
            [
              "Altered or synthetic content",
              "Yes · agent declaration",
              "Yes · verified",
            ],
            ["Made for kids", "No · agent declaration", "No · verified"],
            [
              "Category",
              "Entertainment · account default",
              "Entertainment · verified",
            ],
            [
              "Notify subscribers",
              "No · agent choice",
              "Accepted · not readable",
            ],
            ["Paid promotion", "Not requested", "Write support unverified"],
          ]
            .map((r) => `<tr>${r.map((v) => `<td>${v}</td>`).join("")}</tr>`)
            .join(
              "",
            )}</tbody></table><details><summary>Metadata and assets</summary>${kv("Title", "When “buy the dip” becomes a lifestyle")}${kv("Description", "Agent-supplied description and full-episode link")}${kv("Captions", retryDone ? "Attached" : "Upload failed · recoverable")}</details></div></section>`
        : `${!retryDone ? '<div class="callout"><strong>The video is public. Captions need attention.</strong><p>The caption request failed. Retrying this operation will reuse the existing video; it will not upload another copy.</p></div>' : ""}<div class="grid"><section class="panel"><div class="panel-title"><h2>Progress</h2></div><div class="pad"><ol class="timeline"><li><strong>Request accepted</strong><p>Podcast coordinator submitted video, metadata and declarations.</p><span class="sub">10:42</span></li><li><strong>Video uploaded and processed</strong><span class="sub">10:43</span></li><li><strong>Required settings verified</strong><p>Visibility, audience and synthetic-content declaration match.</p><span class="sub">10:44</span></li><li><strong>${retryDone ? "Optional captions attached" : "Optional captions failed"}</strong><p>${retryDone ? "Captions attached." : "Provider temporarily unavailable. The video remains published."}</p>${!retryDone ? '<button data-action="retry">Simulate caption retry</button>' : ""}</li></ol></div></section><section class="panel"><div class="pad"><h2>Publication</h2>${kv("Destination", "Professor Bagholder · YouTube")}${kv("Visibility", "Public")}${kv("Settings", "Verified")}${link("tasks/episode/settings", "View video settings")}</div></section></div>`
    }`
  );
}
function settings() {
  return (
    heading("Settings", "") +
    `<div class="narrow"><section class="panel"><div class="panel-title"><h2>Agent connections</h2></div><div class="pad">${kv("Public endpoint", "https://bridge.example/mcp")}</div></section></div>`
  );
}
function missing() {
  return heading("Page not found", "Choose a section from the navigation.");
}
function render() {
  if (location.hash === "#content") {
    document.querySelector("main").focus();
    return;
  }
  let [area, id, tab] = (
    location.hash.replace(/^#\//, "").split("?")[0] || "agents"
  ).split("/");
  document.querySelectorAll("nav a").forEach((a) => {
    if (a.hash === `#/${area}`) a.setAttribute("aria-current", "page");
    else a.removeAttribute("aria-current");
  });
  main.innerHTML =
    area === "agents"
      ? id === "connect"
        ? connect()
        : id
          ? agentDetail(id)
          : agentList()
      : area === "destinations"
        ? id === "connect"
          ? catalog()
          : id
            ? destination()
            : destinations()
        : area === "tasks"
          ? id
            ? task(id, tab)
            : tasks()
          : area === "events"
            ? heading("Events", "") + eventView()
            : area === "settings"
              ? settings()
              : missing();
  document.title = `${main.querySelector("h1")?.textContent || "AgentWay"} · Design preview`;
}
window.addEventListener("hashchange", () => {
  render();
  main.focus({ preventScroll: true });
});
main.addEventListener("click", (e) => {
  const b = e.target.closest("[data-action]");
  if (!b) return;
  const a = agents.find((a) => a.id === b.dataset.id);
  if (b.dataset.action === "toggle") {
    a.state = a.state === "Paused" ? "Authorized" : "Paused";
    render();
    notice(`${a.name}: ${a.state.toLowerCase()} in preview only.`);
  }
  if (b.dataset.action === "export-events") {
    const events = b.dataset.task
      ? taskEvents(b.dataset.task)
      : [...taskEvents("episode"), ...connectionEvents];
    const blob = new Blob(
      [JSON.stringify({ source: "design-fixture", events }, null, 2)],
      { type: "application/json" },
    );
    const url = URL.createObjectURL(blob);
    const download = document.createElement("a");
    download.href = url;
    download.download = "agentway-diagnostics.json";
    download.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  }
  if (b.dataset.action === "save-access") {
    a.access = document.querySelector("#publish").checked;
    a.delegate = document.querySelector("#delegate").checked;
    notice("Permissions saved in this preview only.");
  }
  if (b.dataset.action === "previous") {
    wizard.step--;
    render();
  }
  if (b.dataset.action === "retry") {
    retryDone = true;
    render();
    notice("Caption retry simulated. Existing video retained.");
  }
  if (b.dataset.action === "destination-demo")
    notice("Account enrollment is unavailable in this preview.");
});
main.addEventListener("submit", (e) => {
  e.preventDefault();
  if (e.target.id === "defaults") {
    defaults = {
      category: document.querySelector("#category").value,
      visibility: document.querySelector("#visibility").value,
      allowPublic: document.querySelector("#allow-public").checked,
    };
    notice("Defaults saved in this preview only; no account was changed.");
    return;
  }
  if (e.target.id !== "setup") return;
  if (wizard.step === 1) {
    wizard.name = document.querySelector("#name").value.trim();
    if (!wizard.name) return;
    wizard.product = document.querySelector("#product").value;
  }
  if (wizard.step === 2) {
    wizard.publish = document.querySelector("#publish").checked;
    wizard.delegate = document.querySelector("#delegate").checked;
  }
  if (wizard.step < 4) {
    wizard.step++;
    render();
    main.querySelector("h2")?.scrollIntoView({ block: "nearest" });
  } else {
    const id = `sample-${agents.length}`;
    agents.push({
      id,
      name: wizard.name,
      product: wizard.product,
      account: "Example account",
      state: "Authorized",
      delivery: "Not verified",
      capacity: "Not reported",
      access: wizard.publish,
      delegate: wizard.delegate,
    });
    wizard = {
      step: 1,
      product: "ChatGPT agents",
      name: "",
      publish: false,
      delegate: false,
    };
    location.hash = `/agents/${id}`;
    notice("Sample connection created. No real agent is connected.");
  }
});
render();
