const { invoke } = window.__TAURI__.tauri;
const { listen } = window.__TAURI__.event;

const urlEl = document.getElementById("url");
const saveDirEl = document.getElementById("saveDir");
const outNameEl = document.getElementById("outName");
const pickDirBtn = document.getElementById("pickDirBtn");
const maxPagesEl = document.getElementById("maxPages");
const maxDepthEl = document.getElementById("maxDepth");
const concurrencyEl = document.getElementById("concurrency");
const externalAssetsEl = document.getElementById("externalAssets");
const followExternalEl = document.getElementById("followExternal");
const zipEl = document.getElementById("zip");
const downloadBtn = document.getElementById("downloadBtn");
const openBtn = document.getElementById("openBtn");
const useForServerBtn = document.getElementById("useForServerBtn");
const logEl = document.getElementById("log");
const downloadBadge = document.getElementById("downloadBadge");

const projectDirEl = document.getElementById("projectDir");
const pickProjectBtn = document.getElementById("pickProjectBtn");
const projectScanEl = document.getElementById("projectScan");
const scanTagsEl = document.getElementById("scanTags");
const serverBackendEl = document.getElementById("serverBackend");
const serverPortEl = document.getElementById("serverPort");
const backendNoteEl = document.getElementById("backendNote");
const serverStatusTextEl = document.getElementById("serverStatusText");
const serverUrlBoxEl = document.getElementById("serverUrlBox");
const serverUrlEl = document.getElementById("serverUrl");
const startServerBtn = document.getElementById("startServerBtn");
const stopServerBtn = document.getElementById("stopServerBtn");
const openSiteBtn = document.getElementById("openSiteBtn");
const openProjectBtn = document.getElementById("openProjectBtn");
const serverLogEl = document.getElementById("serverLog");
const serverBadge = document.getElementById("serverBadge");

const LOG_MAX_LINES = 400;

let lastOutDir = null;
let activeServerUrl = null;
let currentScan = null;
let downloadBusy = false;
let serverBusy = false;
let serverRunning = false;

function appendLog(target, text, kind = "") {
  const line = document.createElement("div");
  if (kind) line.className = kind;
  line.textContent = text;
  target.appendChild(line);
  while (target.childElementCount > LOG_MAX_LINES) {
    target.removeChild(target.firstElementChild);
  }
  target.scrollTop = target.scrollHeight;
}

function normalizeUrlInput(raw) {
  const trimmed = raw.trim();
  if (!trimmed) return "";
  if (/^https?:\/\//i.test(trimmed)) return trimmed;
  return `https://${trimmed.replace(/^\/+/, "")}`;
}

function parseServerPort() {
  const raw = Number(serverPortEl.value);
  if (!Number.isFinite(raw) || !Number.isInteger(raw) || raw < 1024 || raw > 65535) {
    return null;
  }
  return raw;
}

async function expectedOutputPath() {
  const saveDir = saveDirEl.value.trim();
  const outName = outNameEl.value.trim() || "cloned-site";
  if (!saveDir) return null;
  try {
    return await invoke("resolve_clone_output_path", { saveDir, outName });
  } catch {
    return null;
  }
}

function setDownloadBusy(busy) {
  downloadBusy = busy;
  downloadBtn.disabled = busy;
  pickDirBtn.disabled = busy;
  openBtn.disabled = busy || !lastOutDir;
  useForServerBtn.disabled = !saveDirEl.value.trim();
  downloadBadge.classList.toggle("hidden", !busy);
  downloadBadge.classList.toggle("downloading", busy);
}

function updateServerControls() {
  const controlsLocked = serverRunning || serverBusy;

  serverStatusTextEl.classList.toggle("running", serverRunning);

  startServerBtn.disabled = controlsLocked;
  stopServerBtn.disabled = !serverRunning || serverBusy;
  openSiteBtn.disabled = !serverRunning;
  pickProjectBtn.disabled = serverBusy;
  serverBackendEl.disabled = controlsLocked;
  serverPortEl.disabled = controlsLocked;
  openProjectBtn.disabled = !projectDirEl.value.trim();

  serverBadge.classList.toggle("hidden", !serverRunning);
}

function backendLabel(backend) {
  if (backend === "static") return "استاتیک";
  if (backend === "php") return "PHP";
  if (backend === "aspNet") return "ASP.NET";
  return backend;
}

function renderScan(scan) {
  currentScan = scan;
  projectScanEl.classList.remove("hidden");

  const tags = [];
  if (scan.hasHtml) tags.push("HTML");
  if (scan.hasPhp) tags.push("PHP");
  if (scan.hasAsp) tags.push("ASP");
  if (scan.hasAspx) tags.push("ASPX");
  if (scan.hasCsproj) tags.push(".NET");

  scanTagsEl.textContent = tags.length
    ? tags.join(" · ")
    : "فایل شناخته‌شده‌ای پیدا نشد — حالت استاتیک پیشنهاد می‌شود";

  serverBackendEl.innerHTML = "";
  scan.backends.forEach((item) => {
    const option = document.createElement("option");
    option.value = item.backend;
    option.textContent = item.available ? item.label : `${item.label} (غیرفعال)`;
    option.disabled = !item.available;
    if (item.backend === scan.recommended && item.available) {
      option.selected = true;
    }
    serverBackendEl.appendChild(option);
  });

  updateBackendNote();
}

function updateBackendNote() {
  if (!currentScan) {
    backendNoteEl.textContent = "";
    return;
  }
  const selected = serverBackendEl.value;
  const item = currentScan.backends.find((b) => b.backend === selected);
  backendNoteEl.textContent = item ? item.note : "";
}

function updateServerUi(status) {
  activeServerUrl = status.running ? status.url : null;
  serverRunning = Boolean(status.running);
  serverBusy = Boolean(status.busy);

  serverStatusTextEl.textContent = serverRunning
    ? `در حال اجرا روی پورت ${status.port ?? "?"} (${backendLabel(status.backend || "static")})`
    : serverBusy
      ? "در حال پردازش..."
      : "متوقف";

  if (serverRunning && status.port) {
    serverPortEl.value = String(status.port);
  }

  if (serverRunning && status.url) {
    serverUrlBoxEl.classList.remove("hidden");
    serverUrlEl.textContent = status.url;
    serverUrlEl.href = status.url;
  } else {
    serverUrlBoxEl.classList.add("hidden");
    serverUrlEl.textContent = "";
    serverUrlEl.href = "#";
  }

  updateServerControls();
}

async function refreshServerStatus() {
  try {
    const status = await invoke("get_local_server_status");
    updateServerUi(status);
    if (status.projectDir && !projectDirEl.value) {
      projectDirEl.value = status.projectDir;
      await scanProject(status.projectDir);
    }
  } catch (err) {
    appendLog(serverLogEl, String(err), "err");
  }
}

async function scanProject(dir) {
  if (!dir) return;
  try {
    const scan = await invoke("scan_local_project", { projectDir: dir });
    renderScan(scan);
  } catch (err) {
    projectScanEl.classList.add("hidden");
    appendLog(serverLogEl, String(err), "err");
  }
}

async function applyOutputPathToServer() {
  const path = lastOutDir || (await expectedOutputPath());
  if (!path) {
    appendLog(serverLogEl, "ابتدا محل ذخیره و نام پوشه را مشخص کنید.", "err");
    return;
  }
  projectDirEl.value = path;
  serverLogEl.textContent = "";
  appendLog(serverLogEl, `پوشه سرور: ${path}`);
  await scanProject(path);
}

async function initSaveDir() {
  try {
    const defaultDir = await invoke("get_default_save_dir");
    if (defaultDir && !saveDirEl.value) {
      saveDirEl.value = defaultDir;
    }
  } catch (err) {
    appendLog(logEl, `خطا در خواندن مسیر پیش‌فرض: ${err}`, "err");
  }
}

pickDirBtn.addEventListener("click", async () => {
  try {
    const picked = await invoke("pick_output_folder");
    if (picked) saveDirEl.value = picked;
    useForServerBtn.disabled = !saveDirEl.value.trim();
  } catch (err) {
    appendLog(logEl, String(err), "err");
  }
});

pickProjectBtn.addEventListener("click", async () => {
  try {
    const picked = await invoke("pick_project_folder");
    if (picked) {
      projectDirEl.value = picked;
      serverLogEl.textContent = "";
      await scanProject(picked);
    }
  } catch (err) {
    appendLog(serverLogEl, String(err), "err");
  }
});

serverBackendEl.addEventListener("change", updateBackendNote);

outNameEl.addEventListener("input", () => {
  useForServerBtn.disabled = !saveDirEl.value.trim();
});

downloadBtn.addEventListener("click", async () => {
  const url = normalizeUrlInput(urlEl.value);
  if (!url) {
    appendLog(logEl, "لطفاً آدرس سایت را وارد کنید.", "err");
    return;
  }

  if (!saveDirEl.value.trim()) {
    await initSaveDir();
  }

  const saveDir = saveDirEl.value.trim();
  if (!saveDir) {
    appendLog(logEl, "لطفاً محل ذخیره‌سازی را انتخاب کنید.", "err");
    return;
  }

  const outName = outNameEl.value.trim() || "cloned-site";
  urlEl.value = url;

  logEl.textContent = "";
  setDownloadBusy(true);
  appendLog(logEl, `در حال دانلود از ${url} ...`);
  appendLog(logEl, "می‌توانید همزمان سرور محلی را هم راه‌اندازی کنید →");

  try {
    const result = await invoke("download_site", {
      options: {
        url,
        saveDir,
        outName,
        maxPages: Number(maxPagesEl.value) || 40,
        maxDepth: Number(maxDepthEl.value) || 3,
        concurrency: Number(concurrencyEl.value) || 8,
        includeExternalAssets: externalAssetsEl.checked,
        followExternalPages: followExternalEl.checked,
        zip: zipEl.checked,
      },
    });
    lastOutDir = result.outDir;
    appendLog(logEl, result.message, "ok");
    openBtn.disabled = false;
  } catch (err) {
    appendLog(logEl, String(err), "err");
  } finally {
    setDownloadBusy(false);
  }
});

openBtn.addEventListener("click", async () => {
  if (!lastOutDir) return;
  try {
    await invoke("open_folder", { path: lastOutDir });
  } catch (err) {
    appendLog(logEl, String(err), "err");
  }
});

useForServerBtn.addEventListener("click", applyOutputPathToServer);

startServerBtn.addEventListener("click", async () => {
  let projectDir = projectDirEl.value.trim();
  if (!projectDir) {
    const predicted = await expectedOutputPath();
    if (predicted) {
      projectDir = predicted;
      projectDirEl.value = predicted;
    }
  }
  if (!projectDir) {
    appendLog(serverLogEl, "لطفاً پوشه پروژه را انتخاب کنید.", "err");
    return;
  }

  const port = parseServerPort();
  if (port === null) {
    appendLog(serverLogEl, "پورت باید عددی بین 1024 تا 65535 باشد.", "err");
    return;
  }

  const backend = serverBackendEl.value;
  serverBusy = true;
  updateServerControls();
  appendLog(serverLogEl, `در حال راه‌اندازی سرور روی پورت ${port}...`);

  try {
    const result = await invoke("start_local_server", {
      options: { projectDir, port, backend },
    });
    appendLog(serverLogEl, result.message, "ok");
    await scanProject(projectDir);
  } catch (err) {
    appendLog(serverLogEl, String(err), "err");
  } finally {
    await refreshServerStatus();
  }
});

stopServerBtn.addEventListener("click", async () => {
  serverBusy = true;
  updateServerControls();
  appendLog(serverLogEl, "در حال توقف سرور...");
  try {
    await invoke("stop_local_server");
    appendLog(serverLogEl, "سرور متوقف شد.", "ok");
  } catch (err) {
    appendLog(serverLogEl, String(err), "err");
  } finally {
    await refreshServerStatus();
  }
});

openSiteBtn.addEventListener("click", async () => {
  if (!activeServerUrl) return;
  try {
    await invoke("open_url", { url: activeServerUrl });
  } catch (err) {
    appendLog(serverLogEl, String(err), "err");
  }
});

openProjectBtn.addEventListener("click", async () => {
  const dir = projectDirEl.value.trim();
  if (!dir) return;
  try {
    await invoke("open_folder", { path: dir });
  } catch (err) {
    appendLog(serverLogEl, String(err), "err");
  }
});

initSaveDir();
refreshServerStatus();
setInterval(refreshServerStatus, 4000);

listen("download-progress", (event) => {
  if (typeof event.payload === "string") {
    appendLog(logEl, event.payload);
  }
});
